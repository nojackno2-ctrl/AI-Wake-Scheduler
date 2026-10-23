//! eframe / egui GPU 儀表板；背景排程與原生外殼保持獨立。
use crate::{
    board,
    host::AppHost,
    presenter,
    settings_window::{SettingsSheet, SheetAction},
    startup,
    terminal::Terminal,
    theme::{self, Palette},
    tray::shell::{Action, Shell},
    ui, w32, widgets,
};
use aiwake_core::{
    calculator, catalog,
    cli_update::{self, CliTool, CliUpdateStatus},
    locator, login,
    time::{self, Span},
    usage::{active_reset_at, exhausted_until},
    CancelToken, CliActivityEvent, CliKind, CliRunner, CliUsageSnapshot, Guid, ScheduleStatus, ScheduledJob,
    UsageReader,
};
use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::time::{Duration, Instant};
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOWNORMAL,
};
pub const APP_TITLE: &str = concat!("AI 倒數喚醒 v", env!("CARGO_PKG_VERSION"));
#[derive(Clone)]
pub struct ScheduleEditor {
    pub id: Option<Guid>,
    pub name: String,
    pub start: String,
    pub end: String,
    pub interval: i32,
    pub targets: Vec<CliKind>,
    pub enabled: bool,
    pub message: String,
    pub directory: String,
}
impl ScheduleEditor {
    pub fn new(directory: String) -> Self {
        Self {
            id: None,
            name: "AI 倒數喚醒".into(),
            start: presenter::time_of_day(time::truncate_to_minutes(time::local_time_of_day(&time::now()))),
            end: "24:00".into(),
            interval: 10,
            targets: catalog::kinds(),
            enabled: true,
            message: "早安".into(),
            directory,
        }
    }
    pub fn from_job(job: &ScheduledJob) -> Self {
        Self {
            id: Some(job.id),
            name: job.name.clone(),
            start: presenter::time_of_day(
                job.initial_time_of_day.unwrap_or_else(|| time::local_time_of_day(&job.scheduled_at)),
            ),
            end: presenter::time_of_day(job.end_time_of_day),
            interval: job.probe_interval_minutes,
            targets: job.targets.clone(),
            enabled: job.enabled,
            message: job.message.clone(),
            directory: job.working_directory.clone(),
        }
    }
    pub fn job(&self) -> Result<ScheduledJob, String> {
        let start = parse_time(&self.start, false).ok_or("開始時間請輸入 00:00 至 23:59。")?;
        let end = parse_time(&self.end, true).ok_or("結束時間請輸入 00:00 至 24:00。")?;
        let start = Span::minutes(start);
        let end = Span::minutes(end);
        let scheduled_at = calculator::next_aligned_detection(start, end, &time::now(), self.interval, true)
            .map_err(|e| e.message().to_string())?;
        Ok(ScheduledJob {
            id: self.id.unwrap_or_else(Guid::new),
            name: self.name.trim().into(),
            scheduled_at,
            initial_time_of_day: Some(start),
            end_time_of_day: end,
            probe_interval_minutes: self.interval,
            message: self.message.trim().into(),
            working_directory: self.directory.trim().into(),
            targets: self.targets.clone(),
            enabled: self.enabled,
            ..ScheduledJob::default()
        })
    }
}
pub fn parse_time(text: &str, allow_midnight: bool) -> Option<i64> {
    let (hour, minute) = text.trim().split_once(':')?;
    if hour.len() != 2 || minute.len() != 2 || !hour.bytes().chain(minute.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hour: i64 = hour.parse().ok()?;
    let minute: i64 = minute.parse().ok()?;
    if (0..24).contains(&hour) && (0..60).contains(&minute) {
        Some(hour * 60 + minute)
    } else if allow_midnight && hour == 24 && minute == 0 {
        Some(1440)
    } else {
        None
    }
}
enum Event {
    Activity(CliActivityEvent),
    Error(String),
    Jobs,
    Usage(u64, Vec<CliUsageSnapshot>),
    Probe(u64, String),
    Login(CliKind, Result<(), String>),
    Updates(Vec<CliUpdateStatus>, bool),
}
pub struct Dashboard {
    pub host: Arc<AppHost>,
    pub editor: ScheduleEditor,
    pub jobs: Vec<ScheduledJob>,
    pub snapshots: HashMap<CliKind, CliUsageSnapshot>,
    pub terminal: Terminal,
    pub settings_sheet: Option<SettingsSheet>,
    pub dark: bool,
    pub terminal_open: bool,
    pub status: String,
    initialized: bool,
    background: bool,
    last_tick: Instant,
    last_refresh: Instant,
    last_update: Instant,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    usage_cancel: Option<CancelToken>,
    usage_generation: u64,
    sheet_generation: u64,
    probe_cancel: Option<CancelToken>,
    lifetime: CancelToken,
    refreshing: bool,
    update_running: bool,
    updates: Vec<CliUpdateStatus>,
    show_updates: bool,
    update_notified: HashMap<CliTool, String>,
    login_in_progress: HashSet<CliKind>,
    login_notified: HashSet<CliKind>,
    result_text: Option<String>,
    confirm: Option<(bool, Guid)>,
    shell: Option<Shell>,
    tray_receiver: Option<Receiver<Action>>,
    owner: HWND,
    exit: Arc<AtomicBool>,
    start_minimized: bool,
    window_shown: bool,
}
impl Dashboard {
    /// 隔離測試仍使用真正排程器與 CLI runner，但不查詢本機帳號額度。
    pub fn new(host: Arc<AppHost>, background: bool, ctx: &egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let events = sender.clone();
        let repaint = ctx.clone();
        host.manager.on_activity_logged(move |event| {
            let _ = events.send(Event::Activity(event.clone()));
            repaint.request_repaint();
        });
        let events = sender.clone();
        let repaint = ctx.clone();
        host.manager.on_jobs_changed(move || {
            let _ = events.send(Event::Jobs);
            repaint.request_repaint();
        });
        let events = sender.clone();
        let repaint = ctx.clone();
        host.manager.on_background_error(move |error| {
            let _ = events.send(Event::Error(error.message().into()));
            repaint.request_repaint();
        });
        Self {
            editor: ScheduleEditor::new(host.wakeup_workspace()),
            jobs: host.manager.get_jobs().unwrap_or_default(),
            host,
            snapshots: HashMap::new(),
            terminal: Terminal::default(),
            settings_sheet: None,
            dark: true,
            terminal_open: true,
            status: "已就緒 · 每日自動喚醒".into(),
            initialized: false,
            background,
            last_tick: Instant::now(),
            last_refresh: Instant::now(),
            last_update: Instant::now(),
            sender,
            receiver,
            usage_cancel: None,
            usage_generation: 0,
            sheet_generation: 0,
            probe_cancel: None,
            lifetime: CancelToken::new(),
            refreshing: false,
            update_running: false,
            updates: Vec::new(),
            show_updates: false,
            update_notified: HashMap::new(),
            login_in_progress: HashSet::new(),
            login_notified: HashSet::new(),
            result_text: None,
            confirm: None,
            shell: None,
            tray_receiver: None,
            owner: HWND::default(),
            exit: Arc::new(AtomicBool::new(false)),
            start_minimized: false,
            window_shown: false,
        }
    }
    fn initialize(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        theme::fonts(ctx);
        theme::apply(ctx, self.dark);
        self.initialized = true;
    }
    pub fn open_settings(&mut self) {
        if self.settings_sheet.is_none() {
            self.sheet_generation += 1;
            self.settings_sheet = Some(SettingsSheet::new(self.host.settings(), self.sheet_generation));
        }
    }
    pub fn save_schedule(&mut self) {
        match self
            .editor
            .job()
            .and_then(|job| self.host.manager.upsert(&job).map(|_| job.id).map_err(|e| e.message().to_string()))
        {
            Ok(id) => {
                self.editor.id = Some(id);
                self.status = "排程已儲存".into();
                self.reload_jobs();
            }
            Err(error) => self.status = error,
        }
    }
    fn reload_jobs(&mut self) {
        match self.host.manager.get_jobs() {
            Ok(jobs) => self.jobs = jobs,
            Err(error) => self.status = error.message().into(),
        }
    }
    fn toggle_schedule(&mut self, mut job: ScheduledJob) {
        job.enabled = !job.enabled;
        match self.host.manager.upsert(&job) {
            Ok(()) => {
                if self.editor.id == Some(job.id) {
                    self.editor.enabled = job.enabled;
                }
                self.status = if job.enabled { "排程已啟用" } else { "排程已停用" }.into();
                self.reload_jobs();
            }
            Err(error) => self.status = error.message().into(),
        }
    }
    pub fn tick(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Activity(event) => self.terminal.push(event),
                Event::Jobs => self.reload_jobs(),
                Event::Error(error) => self.status = error,
                Event::Usage(generation, snapshots) if generation == self.usage_generation => {
                    self.refreshing = false;
                    self.usage_cancel = None;
                    for snapshot in snapshots {
                        let kind = snapshot.cli;
                        if snapshot.requires_login && self.login_notified.insert(kind) {
                            if let Some(shell) = &self.shell {
                                shell.notify(&format!("{} 需要重新登入", kind.display_name()), true);
                            }
                        } else if !snapshot.requires_login {
                            self.login_notified.remove(&kind);
                        }
                        self.snapshots.insert(kind, snapshot);
                    }
                    self.status = "倒數已重新讀取".into();
                }
                Event::Usage(_, _) => {}
                Event::Probe(generation, text) => {
                    if let Some(sheet) = &mut self.settings_sheet {
                        if sheet.generation == generation {
                            sheet.probing = false;
                            sheet.probe_text = text;
                        }
                    }
                }
                Event::Login(kind, result) => {
                    self.login_in_progress.remove(&kind);
                    match result {
                        Ok(()) => self.refresh_usage(ctx, true),
                        Err(error) => self.status = error,
                    }
                }
                Event::Updates(updates, interactive) => {
                    self.update_running = false;
                    self.show_updates |= interactive;
                    for update in &updates {
                        if update.update_available() && self.update_notified.get(&update.tool) != update.latest.as_ref()
                        {
                            if let Some(shell) = &self.shell {
                                shell.notify(&format!("{} 有新版本可更新", update.tool.display_name()), false);
                            }
                            if let Some(version) = &update.latest {
                                self.update_notified.insert(update.tool, version.clone());
                            }
                        }
                    }
                    self.status = "CLI 版本檢查完成（僅提示）".into();
                    self.updates = updates;
                }
            }
        }
        let actions: Vec<_> =
            self.tray_receiver.as_ref().map(|receiver| receiver.try_iter().collect()).unwrap_or_default();
        for action in actions {
            match action {
                Action::Open => self.restore(ctx),
                Action::Exit => {
                    self.exit.store(true, Ordering::SeqCst);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::Logs => self.open_logs(),
                Action::Export => {
                    self.restore(ctx);
                    self.export_logs();
                }
                Action::Updates => {
                    self.restore(ctx);
                    self.check_updates(ctx, true);
                }
            }
        }
        if self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.last_tick = Instant::now();
            if self.background {
                self.snapshots.extend(self.host.usage_reader.latest_snapshots());
                let now = time::now();
                let ended = self.snapshots.values().any(|s| {
                    s.windows.iter().any(|w| {
                        w.is_active_countdown
                            && w.resets_at.is_some_and(|reset| reset <= now)
                            && s.observed_at < w.resets_at.unwrap_or(now)
                    })
                });
                let interval = self.host.settings().quota_auto_refresh_minutes;
                if !self.refreshing
                    && (ended
                        || interval > 0 && self.last_refresh.elapsed() >= Duration::from_secs(interval as u64 * 60))
                {
                    self.refresh_usage(ctx, false);
                }
                if !self.update_running
                    && self.last_update.elapsed()
                        >= Duration::from_secs(if self.updates.is_empty() { 60 } else { 12 * 3600 })
                {
                    self.check_updates(ctx, false);
                }
            }
        }
    }
    fn refresh_usage(&mut self, ctx: &egui::Context, force: bool) {
        if !self.background {
            self.status = "功能驗證模式不查詢本機帳號額度".into();
            return;
        }
        if self.refreshing && !force {
            return;
        }
        if let Some(cancel) = self.usage_cancel.take() {
            cancel.cancel();
        }
        self.snapshots.extend(self.host.usage_reader.latest_snapshots());
        let now = time::now();
        let kinds: Vec<_> = catalog::kinds()
            .into_iter()
            .filter(|kind| force || active_reset_at(self.snapshots.get(kind), &now).is_none())
            .collect();
        self.last_refresh = Instant::now();
        if kinds.is_empty() {
            return;
        }
        self.usage_generation += 1;
        let generation = self.usage_generation;
        let cancel = CancelToken::new();
        self.usage_cancel = Some(cancel.clone());
        self.refreshing = true;
        let settings = self.host.settings();
        self.host.usage_reader.set_auto_start_antigravity(settings.auto_start_antigravity);
        let host = self.host.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let snapshots = std::thread::scope(|scope| {
                let handles: Vec<_> = kinds
                    .into_iter()
                    .map(|kind| {
                        let host = &host;
                        let settings = &settings;
                        let cancel = &cancel;
                        scope.spawn(move || {
                            host.usage_reader.read(kind, &settings.profile(kind), &host.wakeup_workspace(), cancel)
                        })
                    })
                    .collect();
                handles.into_iter().filter_map(|handle| handle.join().ok().and_then(Result::ok)).collect()
            });
            if !cancel.is_canceled() {
                let _ = sender.send(Event::Usage(generation, snapshots));
                ctx.request_repaint();
            }
        });
    }
    fn probe(&mut self, ctx: &egui::Context) {
        let Some(sheet) = &mut self.settings_sheet else {
            return;
        };
        if sheet.probing {
            return;
        }
        sheet.probing = true;
        sheet.probe_text = "檢查中…".into();
        if let Some(cancel) = self.probe_cancel.take() {
            cancel.cancel();
        }
        let cancel = CancelToken::new();
        self.probe_cancel = Some(cancel.clone());
        let draft = sheet.draft.clone();
        let generation = sheet.generation;
        let host = self.host.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        locator::clear_cache();
        std::thread::spawn(move || {
            let results: Vec<_> = std::thread::scope(|scope| {
                let handles: Vec<_> = catalog::kinds()
                    .into_iter()
                    .map(|kind| {
                        let host = &host;
                        let draft = &draft;
                        let cancel = &cancel;
                        scope.spawn(move || {
                            host.runner.probe(kind, &draft.profile(kind), &host.wakeup_workspace(), cancel)
                        })
                    })
                    .collect();
                handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
            });
            let text = results
                .iter()
                .map(|r| format!("{} {}：{}", if r.succeeded { "✓" } else { "✗" }, r.cli.short_name(), r.summary))
                .collect::<Vec<_>>()
                .join("\n");
            if !cancel.is_canceled() {
                let _ = sender.send(Event::Probe(generation, text));
                ctx.request_repaint();
            }
        });
    }
    fn apply_settings(&mut self, ctx: &egui::Context) {
        let Some(sheet) = &mut self.settings_sheet else {
            return;
        };
        if let Err(error) = sheet.validate() {
            sheet.probe_text = error;
            return;
        }
        let current = self.host.settings();
        let draft = sheet.draft.clone();
        let result = (|| {
            if !self.host.is_isolated && draft.start_with_windows != current.start_with_windows {
                startup::set_enabled(draft.start_with_windows)?;
            }
            self.host.replace_settings(draft.clone()).map_err(|e| e.message().to_string())
        })();
        match result {
            Ok(()) => {
                self.cancel_settings();
                locator::clear_cache();
                self.host.usage_reader.set_auto_start_antigravity(draft.auto_start_antigravity);
                self.refresh_usage(ctx, false);
                self.status = "設定已儲存".into();
            }
            Err(error) => {
                if let Some(sheet) = &mut self.settings_sheet {
                    sheet.probe_text = error;
                }
            }
        }
    }
    fn cancel_settings(&mut self) {
        if let Some(cancel) = self.probe_cancel.take() {
            cancel.cancel();
        }
        self.settings_sheet = None;
    }
    fn login(&mut self, ctx: &egui::Context, kind: CliKind) {
        if !self.login_in_progress.insert(kind) {
            return;
        }
        let settings = self.host.settings();
        let directory = self.host.wakeup_workspace();
        let result = locator::resolve(kind, Some(&settings.profile(kind).executable), &directory)
            .ok_or_else(|| "找不到 CLI 執行檔。".to_string())
            .and_then(|path| {
                login::create_start_info(kind, &path, &directory)
                    .and_then(|info| info.spawn())
                    .map_err(|e| e.message().to_string())
            });
        match result {
            Ok(mut child) => {
                let sender = self.sender.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = child.wait().map_err(|e| e.to_string()).and_then(|status| {
                        if status.success() {
                            Ok(())
                        } else {
                            Err("登入未完成，請重試。".into())
                        }
                    });
                    let _ = sender.send(Event::Login(kind, result));
                    ctx.request_repaint();
                });
            }
            Err(error) => {
                self.login_in_progress.remove(&kind);
                self.status = error;
            }
        }
    }
    fn check_updates(&mut self, ctx: &egui::Context, interactive: bool) {
        if self.update_running {
            self.show_updates |= interactive;
            return;
        }
        self.update_running = true;
        self.last_update = Instant::now();
        self.status = "正在檢查 CLI 更新…".into();
        let host = self.host.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        let cancel = self.lifetime.clone();
        std::thread::spawn(move || {
            let updates = std::thread::scope(|scope| {
                let handles: Vec<_> = CliTool::ALL
                    .iter()
                    .map(|&tool| {
                        let host = &host;
                        let cancel = &cancel;
                        scope.spawn(move || {
                            let profile = host.settings().profile(tool.kind());
                            let directory = host.wakeup_workspace();
                            let path = locator::resolve(tool.kind(), Some(&profile.executable), &directory);
                            cli_update::check(tool, path, &directory, cancel)
                        })
                    })
                    .collect();
                handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
            });
            let _ = sender.send(Event::Updates(updates, interactive));
            ctx.request_repaint();
        });
    }
    fn open_logs(&self) {
        let path = w32::wide(&self.host.paths.logs_directory().to_string_lossy());
        unsafe {
            ShellExecuteW(
                Some(self.owner),
                PCWSTR(w32::wide("open").as_ptr()),
                PCWSTR(path.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }
    fn export_logs(&mut self) {
        let name = format!("AI倒數喚醒-除錯日誌-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        if let Some(path) = ui::save_file(self.owner, "匯出除錯日誌", &name, "txt", &[("文字檔", "*.txt")]) {
            self.status = match self.host.export_debug_logs(std::path::Path::new(&path)) {
                Ok(count) => format!("已匯出 {count} 個日誌：{path}"),
                Err(error) => format!("匯出失敗：{error}"),
            };
        }
    }
    fn restore(&mut self, ctx: &egui::Context) {
        self.start_minimized = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        unsafe {
            let _ = ShowWindow(self.owner, SW_RESTORE);
            let _ = ShowWindow(self.owner, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(self.owner);
        }
    }
    fn set_native_theme(&self) {
        let dark = BOOL::from(self.dark);
        unsafe {
            let _ = DwmSetWindowAttribute(
                self.owner,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<BOOL>() as u32,
            );
        }
    }
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.initialize(&ctx);
        self.tick(&ctx);
        let colors = Palette::new(self.dark);
        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(colors.surface).inner_margin(egui::Margin::symmetric(22, 16)))
            .show(ui, |ui| self.header(ui, colors));
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(colors.canvas).inner_margin(8)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.small(RichText::new(&self.status).color(colors.secondary));
                ui.separator();
                ui.small(
                    RichText::new(chrono::Local::now().format("%H:%M:%S").to_string()).monospace().color(colors.muted),
                );
            });
        });
        if self.terminal_open {
            let wide = ui.available_width() > 1050.0;
            let panel = if wide { egui::Panel::right("terminal") } else { egui::Panel::bottom("terminal-compact") };
            panel
                .default_size(if wide { 320.0 } else { 190.0 })
                .size_range(if wide { 220.0..=520.0 } else { 100.0..=360.0 })
                .resizable(true)
                .frame(egui::Frame::new().fill(theme::TERMINAL).inner_margin(14))
                .show(ui, |ui| self.terminal.show(ui));
        }
        if ui.available_width() > 850.0 {
            egui::Panel::left("schedules")
                .default_size(280.0)
                .size_range(235.0..=360.0)
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.canvas).inner_margin(14))
                .show(ui, |ui| self.schedules(ui, colors));
        } else {
            egui::Panel::top("schedules-compact")
                .default_size(185.0)
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.canvas).inner_margin(12))
                .show(ui, |ui| self.schedules(ui, colors));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(colors.canvas).inner_margin(16)).show(ui, |ui| {
            let panel_width = ui.available_width();
            egui::ScrollArea::vertical().id_salt("dashboard-scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(panel_width);
                self.gauges(ui, colors);
                ui.add_space(14.0);
                self.editor_card(ui, colors);
            });
        });
        if self.settings_sheet.is_some() {
            let screen = ctx.content_rect();
            let width = 490.0_f32.min(screen.width() - 24.0);
            let modal = egui::Modal::new(egui::Id::unique("settings-sheet"))
                .area(
                    egui::Modal::default_area(egui::Id::unique("settings-sheet"))
                        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0)),
                )
                .frame(theme::card(colors))
                .show(&ctx, |ui| {
                    ui.set_width(width);
                    ui.set_height((screen.height() - 48.0).max(300.0));
                    self.settings_sheet
                        .as_mut()
                        .map(|sheet| sheet.show(ui, self.owner, colors))
                        .unwrap_or(SheetAction::None)
                });
            match modal.inner {
                SheetAction::Save => self.apply_settings(&ctx),
                SheetAction::Cancel => self.cancel_settings(),
                SheetAction::Probe => self.probe(&ctx),
                SheetAction::None => {}
            }
            if modal.should_close() {
                self.cancel_settings();
            }
        }
        self.dialogs(&ctx, colors);
    }
    fn header(&mut self, ui: &mut egui::Ui, colors: Palette) {
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 22.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 7.0, colors.accent.gamma_multiply(0.13));
            ui.painter().circle_filled(rect.center(), 3.5, colors.accent);
            ui.label(RichText::new(APP_TITLE).size(21.0).strong());
            ui.add_space(12.0);
            if ui
                .add_enabled(
                    !self.refreshing,
                    egui::Button::new(if self.refreshing { "讀取中…" } else { "⟳ 重新讀取倒數" }),
                )
                .clicked()
            {
                self.refresh_usage(ui.ctx(), true);
            }
            if ui.button("⚙ 設定").clicked() {
                self.open_settings();
            }
            if ui.button(if self.dark { "☀ 淺色" } else { "☾ 深色" }).clicked() {
                self.dark = !self.dark;
                theme::apply(ui.ctx(), self.dark);
                self.set_native_theme();
            }
            ui.menu_button("更多 ⋯", |ui| {
                if ui.button("開啟日誌").clicked() {
                    self.open_logs();
                    ui.close();
                }
                if ui.button("匯出除錯日誌…").clicked() {
                    self.export_logs();
                    ui.close();
                }
                if ui.button("檢查 CLI 更新…").clicked() {
                    self.check_updates(ui.ctx(), true);
                    ui.close();
                }
                if ui.button(if self.terminal_open { "收合終端機" } else { "展開終端機" }).clicked() {
                    self.terminal_open = !self.terminal_open;
                    ui.close();
                }
            });
        });
        let enabled = self.jobs.iter().filter(|job| job.enabled).count();
        let running = self.jobs.iter().filter(|job| job.status == ScheduleStatus::Running).count();
        let next = self
            .jobs
            .iter()
            .filter(|job| job.enabled && job.status != ScheduleStatus::Running)
            .min_by_key(|job| job.scheduled_at);
        ui.horizontal_wrapped(|ui| {
            pill(ui, &format!("{enabled} 個啟用排程"), colors.accent, colors);
            pill(ui, &format!("{running} 個執行中"), theme::GOOD, colors);
            if let Some(job) = next {
                ui.small(
                    RichText::new(format!(
                        "下次 {} · {}",
                        job.name,
                        presenter::countdown(job, &time::now(), Some(&self.snapshots))
                    ))
                    .color(colors.secondary),
                );
            } else {
                ui.small(RichText::new("準備好迎接下一次靈感").color(colors.secondary));
            }
        });
    }
    fn schedules(&mut self, ui: &mut egui::Ui, colors: Palette) {
        ui.horizontal(|ui| {
            ui.strong("已儲存排程");
            if ui.button("＋ 新增").clicked() {
                self.editor = ScheduleEditor::new(self.host.wakeup_workspace());
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().id_salt("schedule-scroll").show(ui, |ui| {
            if self.jobs.is_empty() {
                ui.label(RichText::new("尚未建立排程\n設定每日喚醒時段即可開始").color(colors.muted));
            }
            for job in self.jobs.clone() {
                let selected = self.editor.id == Some(job.id);
                let mut toggled = false;
                let response = theme::card(colors)
                    .stroke(Stroke::new(1.0, if selected { colors.accent } else { colors.border }))
                    .show(ui, |ui| {
                        ui.set_width((ui.available_width() - 1.0).max(0.0));
                        ui.horizontal(|ui| {
                            let state = if job.status == ScheduleStatus::Running {
                                theme::WARN
                            } else if job.enabled {
                                theme::GOOD
                            } else {
                                colors.muted
                            };
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
                            let pulse = if job.enabled {
                                0.65 + 0.35 * (ui.input(|i| i.time) as f32 * 2.0).sin().abs()
                            } else {
                                1.0
                            };
                            ui.painter().circle_filled(rect.center(), 3.0, state.gamma_multiply(pulse));
                            let title_width = (ui.available_width() - 52.0).max(60.0);
                            if ui
                                .add_sized(
                                    [title_width, 26.0],
                                    egui::Button::new(RichText::new(&job.name).strong()).frame(false),
                                )
                                .clicked()
                            {
                                self.editor = ScheduleEditor::from_job(&job);
                            }
                            let mut enabled = job.enabled;
                            toggled =
                                widgets::toggle(ui, &mut enabled, &format!("啟停 {}", job.name), colors).changed();
                        });
                        ui.small(RichText::new(format!("每日 {}", presenter::time(&job))).color(colors.secondary));
                        let mut start = job.initial_time_of_day.unwrap_or_default().num_minutes();
                        let mut end = job.end_time_of_day.num_minutes();
                        let now = time::now();
                        let markers: Vec<_> = job
                            .targets
                            .iter()
                            .filter_map(|kind| {
                                active_reset_at(self.snapshots.get(kind), &now).map(|reset| {
                                    (
                                        time::local_time_of_day(&reset).num_seconds() as f32 / 86400.0,
                                        theme::brand(*kind, colors.dark).2,
                                    )
                                })
                            })
                            .collect();
                        widgets::timeline(
                            ui,
                            &mut start,
                            &mut end,
                            time::local_time_of_day(&now).num_seconds() as f32 / 86400.0,
                            &markers,
                            false,
                            colors,
                        );
                        ui.horizontal_wrapped(|ui| {
                            for kind in &job.targets {
                                let (badge, _, brand) = theme::brand(*kind, colors.dark);
                                pill(ui, badge, brand, colors);
                            }
                        });
                        ui.small(
                            RichText::new(presenter::countdown(&job, &now, Some(&self.snapshots)))
                                .color(colors.secondary),
                        );
                    })
                    .response;
                if ui.input(|i| {
                    i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| response.rect.contains(p))
                }) && !toggled
                {
                    self.editor = ScheduleEditor::from_job(&job);
                }
                if toggled {
                    self.toggle_schedule(job);
                }
                ui.add_space(10.0);
            }
        });
    }
    fn gauges(&mut self, ui: &mut egui::Ui, colors: Palette) {
        ui.horizontal(|ui| {
            ui.heading("額度觀測站");
            ui.small(RichText::new("剩餘流量與重置倒數").color(colors.secondary));
        });
        ui.add_space(6.0);
        let width = ui.available_width();
        let columns = ((width + 10.0) / 164.0).floor().clamp(1.0, 5.0) as usize;
        let card_width = ((width - (columns - 1) as f32 * 10.0) / columns as f32).max(120.0);
        for chunk in catalog::kinds().chunks(columns) {
            ui.horizontal_top(|ui| {
                for &kind in chunk {
                    self.gauge(ui, kind, card_width, colors);
                }
            });
        }
    }
    fn gauge(&mut self, ui: &mut egui::Ui, kind: CliKind, width: f32, colors: Palette) {
        let (badge, name, brand) = theme::brand(kind, colors.dark);
        let snapshot = self.snapshots.get(&kind).cloned();
        let group = snapshot.as_ref().map(|s| board::build_group(catalog::get(kind), s));
        let primary = group
            .as_ref()
            .and_then(|g| g.rows.iter().find(|r| r.duration.is_some_and(|d| d <= Span::hours(6))).or(g.rows.first()));
        theme::card(colors).inner_margin(12).show(ui, |ui| {
            ui.vertical(|ui| {
                let inner_width = (width - 26.0).max(96.0);
                ui.set_width(inner_width);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 13.0, brand);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        badge,
                        FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                    ui.label(RichText::new(name).size(12.0).strong());
                });
                let size = inner_width.min(104.0);
                let (rect, response) = ui.allocate_exact_size(egui::vec2(inner_width, size), egui::Sense::hover());
                let center = rect.center();
                let radius = size / 2.0 - 8.0;
                ui.painter().circle_stroke(center, radius, Stroke::new(7.5, colors.track));
                ui.painter().circle_stroke(center, radius - 10.5, Stroke::new(3.0, colors.track));
                let now = time::now();
                let exhausted = exhausted_until(snapshot.as_ref(), &now).is_some();
                let mut countdown = "尚未讀取".into();
                let mut value = "—".into();
                if let Some(row) = primary {
                    let percent = row.remaining_percent as f32 / 100.0;
                    let color = if row.remaining_percent == 0 {
                        theme::DANGER
                    } else if row.remaining_percent < 20 {
                        theme::WARN
                    } else {
                        brand
                    };
                    if colors.dark {
                        widgets::arc(ui, center, radius, percent, 13.0, color.gamma_multiply(0.08));
                    }
                    widgets::arc(ui, center, radius, percent, 8.0, color);
                    value = row.percent_text.clone();
                    countdown = if exhausted {
                        "額度用盡".into()
                    } else if row.is_counting_down {
                        row.resets_at
                            .map(|reset| board::format_countdown((reset - now).max(Span::zero())))
                            .unwrap_or_else(|| "未倒數".into())
                    } else {
                        "未倒數".into()
                    };
                    if row.is_counting_down {
                        if let (Some(reset), Some(duration)) =
                            (row.resets_at, row.duration.filter(|d| *d > Span::zero()))
                        {
                            widgets::arc(
                                ui,
                                center,
                                radius - 11.0,
                                (1.0 - (reset - now).num_seconds().max(0) as f32 / duration.num_seconds() as f32)
                                    .clamp(0.0, 1.0),
                                3.5,
                                colors.accent,
                            );
                        }
                    }
                } else if group.as_ref().is_some_and(|g| g.requires_login) {
                    value = "登入".into();
                    countdown = "需要授權".into();
                }
                ui.painter().text(
                    center - egui::vec2(0.0, 6.0),
                    egui::Align2::CENTER_CENTER,
                    &value,
                    FontId::proportional(22.0),
                    colors.text,
                );
                ui.painter().text(
                    center + egui::vec2(0.0, 14.0),
                    egui::Align2::CENTER_CENTER,
                    &countdown,
                    FontId::monospace(10.5),
                    if exhausted { theme::DANGER } else { colors.secondary },
                );
                response.on_hover_text(group.as_ref().map(|g| g.message.as_str()).unwrap_or("等待額度讀取"));
                if let Some(group) = &group {
                    for row in &group.rows {
                        if primary.is_some_and(|p| std::ptr::eq(p, row)) {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.small(&row.label);
                            ui.small(&row.percent_text);
                        });
                        ui.add(
                            egui::ProgressBar::new(row.remaining_percent as f32 / 100.0)
                                .desired_width(inner_width)
                                .desired_height(5.0)
                                .fill(brand),
                        );
                    }
                    if group.requires_login
                        && ui
                            .add_enabled(
                                !self.login_in_progress.contains(&kind),
                                egui::Button::new("重新登入").fill(theme::DANGER.gamma_multiply(0.2)),
                            )
                            .clicked()
                    {
                        self.login(ui.ctx(), kind);
                    }
                    ui.label(
                        RichText::new(board::format_freshness(&now, &group.observed_at)).size(10.0).color(colors.muted),
                    );
                } else {
                    ui.label(RichText::new("等待唯讀額度查詢").size(10.0).color(colors.muted));
                }
            });
        });
    }
    fn editor_card(&mut self, ui: &mut egui::Ui, colors: Palette) {
        theme::card(colors).show(ui, |ui| {
            let card_width = (ui.available_width() - 2.0).max(200.0);
            ui.set_width(card_width);
            ui.set_max_width(card_width);
            ui.horizontal_wrapped(|ui| {
                ui.heading(if self.editor.id.is_some() { "編輯排程" } else { "建立每日喚醒" });
                widgets::toggle(ui, &mut self.editor.enabled, "啟用此排程", colors);
                ui.small("啟用此排程");
            });
            text_field(ui, "排程名稱", &mut self.editor.name);
            ui.columns(2, |columns| {
                text_field(&mut columns[0], "開始時間 (HH:MM)", &mut self.editor.start);
                text_field(&mut columns[1], "結束時間 (HH:MM，支援 24:00)", &mut self.editor.end);
            });
            if let (Some(mut start), Some(mut end)) =
                (parse_time(&self.editor.start, false), parse_time(&self.editor.end, true))
            {
                let before = (start, end);
                widgets::timeline(
                    ui,
                    &mut start,
                    &mut end,
                    time::local_time_of_day(&time::now()).num_seconds() as f32 / 86400.0,
                    &[],
                    true,
                    colors,
                );
                if before != (start, end) {
                    self.editor.start = presenter::time_of_day(Span::minutes(start));
                    self.editor.end = presenter::time_of_day(Span::minutes(end));
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("檢測間隔");
                for minutes in [5, 10, 15, 30, 60] {
                    if ui
                        .add(
                            egui::Button::new(format!("{minutes}分"))
                                .selected(self.editor.interval == minutes)
                                .corner_radius(16),
                        )
                        .clicked()
                    {
                        self.editor.interval = minutes;
                    }
                }
                ui.add(egui::DragValue::new(&mut self.editor.interval).range(1..=60).suffix(" 分"))
                    .on_hover_text("自訂檢測間隔");
            });
            ui.horizontal(|ui| {
                ui.label("目標 CLI");
                if ui.small_button("全選").clicked() {
                    self.editor.targets = catalog::kinds();
                }
                if ui.small_button("清除目標").clicked() {
                    self.editor.targets.clear();
                }
            });
            ui.horizontal_wrapped(|ui| {
                for kind in catalog::kinds() {
                    let (badge, name, brand) = theme::brand(kind, colors.dark);
                    let selected = self.editor.targets.contains(&kind);
                    let label = format!("{} {badge} {name}", if selected { "✓" } else { "○" });
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).color(if selected { brand } else { colors.muted }))
                                .corner_radius(18)
                                .fill(if selected { brand.gamma_multiply(0.12) } else { colors.input })
                                .stroke(Stroke::new(1.0, if selected { brand } else { colors.border })),
                        )
                        .on_hover_text(format!("選取 {}", kind.name()))
                        .clicked()
                    {
                        if selected {
                            self.editor.targets.retain(|target| *target != kind);
                        } else {
                            self.editor.targets.push(kind);
                        }
                    }
                }
            });
            text_field(ui, "喚醒訊息", &mut self.editor.message);
            text_field(ui, "工作目錄", &mut self.editor.directory);
            if ui.button("瀏覽工作目錄…").clicked() {
                if let Some(path) = ui::pick_folder(self.owner, "選擇工作目錄", &self.editor.directory) {
                    self.editor.directory = path;
                }
            }
            ui.small(
                RichText::new("僅在未讀到倒數時依間隔檢查；倒數結束且在偵測時段內時立即重讀與喚醒。")
                    .color(colors.muted),
            );
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(if self.editor.id.is_some() { "儲存修改" } else { "建立排程" })
                                .color(if colors.dark { colors.canvas } else { Color32::WHITE }),
                        )
                        .fill(colors.accent),
                    )
                    .clicked()
                {
                    self.save_schedule();
                }
                let selected = self.editor.id.and_then(|id| self.jobs.iter().find(|job| job.id == id));
                let idle = selected.is_some_and(|job| job.status != ScheduleStatus::Running);
                if ui.add_enabled(idle, egui::Button::new("▶ 立即執行")).clicked() {
                    if let Some(id) = self.editor.id {
                        self.confirm = Some((false, id));
                    }
                }
                if ui.add_enabled(selected.is_some(), egui::Button::new("檢視結果")).clicked() {
                    self.result_text = selected.map(presenter::results).map(|text| {
                        if text.is_empty() {
                            "尚無執行結果".into()
                        } else {
                            text
                        }
                    });
                }
                if ui.add_enabled(idle, egui::Button::new(RichText::new("刪除").color(theme::DANGER))).clicked() {
                    if let Some(id) = self.editor.id {
                        self.confirm = Some((true, id));
                    }
                }
            });
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context, colors: Palette) {
        if let Some((delete, id)) = self.confirm {
            let question = self
                .jobs
                .iter()
                .find(|job| job.id == id)
                .map(|job| {
                    if delete {
                        format!("刪除排程「{}」？", job.name)
                    } else {
                        format!("現在向 {} 傳送「{}」？", presenter::targets_long(&job.targets), job.message)
                    }
                })
                .unwrap_or_default();
            let modal = egui::Modal::new(egui::Id::unique("confirm-action")).show(ctx, |ui| {
                ui.heading(if delete { "刪除排程" } else { "立即執行" });
                ui.label(question);
                let mut answer = None;
                ui.horizontal(|ui| {
                    if ui.button("確認").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("取消操作").clicked() {
                        answer = Some(false);
                    }
                });
                answer
            });
            if modal.inner == Some(true) {
                let result = if delete { self.host.manager.delete(id) } else { self.host.manager.run_now(id) };
                match result {
                    Ok(()) => {
                        if delete {
                            self.editor = ScheduleEditor::new(self.host.wakeup_workspace());
                        }
                        self.reload_jobs();
                        self.status = if delete { "排程已刪除" } else { "已交給排程器立即執行" }.into();
                    }
                    Err(error) => self.status = error.message().into(),
                }
                self.confirm = None;
            } else if modal.inner == Some(false) || modal.should_close() {
                self.confirm = None;
            }
        }
        if let Some(text) = &self.result_text {
            let mut open = true;
            egui::Window::new("執行結果").open(&mut open).resizable(true).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(text);
                });
            });
            if !open {
                self.result_text = None;
            }
        }
        if self.show_updates {
            let mut open = true;
            egui::Window::new("CLI 更新檢查").open(&mut open).show(ctx, |ui| {
                ui.label("版本檢查僅提供提示，不會阻擋喚醒。");
                for update in &self.updates {
                    theme::card(colors).show(ui, |ui| {
                        ui.label(update.describe());
                        if update.update_available()
                            && ui.button(format!("更新 {}", update.tool.display_name())).clicked()
                        {
                            if let Some(path) = &update.executable {
                                if let Err(error) = cli_update::create_update_start_info(
                                    update.tool,
                                    path,
                                    &self.host.wakeup_workspace(),
                                )
                                .and_then(|info| info.spawn())
                                {
                                    self.status = error.message().into();
                                }
                            }
                        }
                    });
                }
            });
            self.show_updates = open;
        }
    }
}
fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    let label = ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY).margin(egui::vec2(10.0, 8.0)))
        .labelled_by(label.id);
}
fn pill(ui: &mut egui::Ui, text: &str, color: Color32, colors: Palette) {
    egui::Frame::new()
        .fill(color.gamma_multiply(if colors.dark { 0.12 } else { 0.08 }))
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        });
}
impl eframe::App for Dashboard {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
        if self.start_minimized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            unsafe {
                let _ = ShowWindow(self.owner, SW_HIDE);
            }
        } else if !self.window_shown {
            self.window_shown = true;
            if !self.owner.0.is_null() {
                unsafe {
                    let _ = ShowWindow(self.owner, SW_SHOWNORMAL);
                }
            }
        }
        if ctx.input(|input| input.viewport().close_requested())
            && !self.exit.load(Ordering::SeqCst)
            && self.host.settings().minimize_to_tray
            && self.shell.as_ref().is_some_and(Shell::alive)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            unsafe {
                let _ = ShowWindow(self.owner, SW_HIDE);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}
impl Drop for Dashboard {
    fn drop(&mut self) {
        self.lifetime.cancel();
        if let Some(cancel) = self.usage_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.probe_cancel.take() {
            cancel.cancel();
        }
        self.host.manager.shutdown();
    }
}
fn native_window(title: &str) -> HWND {
    struct Search<'a> {
        thread: u32,
        title: &'a str,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, param: LPARAM) -> BOOL {
        let search = &mut *(param.0 as *mut Search<'_>);
        // 只檢查目前執行緒建立的視窗，避免對 wgpu 背景執行緒視窗同步送出 WM_GETTEXT 而死鎖。
        if GetWindowThreadProcessId(hwnd, None) == search.thread && w32::get_text(hwnd) == search.title {
            search.found = hwnd;
            return BOOL(0);
        }
        BOOL(1)
    }
    let thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
    let mut search = Search { thread, title, found: HWND::default() };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM((&mut search as *mut Search<'_>) as isize));
    }
    search.found
}
pub fn run(
    host: Arc<AppHost>,
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    minimized: bool,
    verify: bool,
) -> Result<(), String> {
    let title = format!("{APP_TITLE}{}", if verify { "（功能驗證）" } else { "" });
    let window_title = title.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_visible(false)
            .with_inner_size([1540.0, 960.0])
            .with_min_inner_size([700.0, 620.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |creation| {
            let mut app = Dashboard::new(host, !verify, &creation.egui_ctx);
            app.initialize(&creation.egui_ctx);
            app.owner = native_window(&window_title);
            app.set_native_theme();
            if let Some((shell, receiver)) =
                Shell::new(creation.egui_ctx.clone(), icon, app.owner, Arc::clone(&app.exit))
            {
                app.shell = Some(shell);
                app.tray_receiver = Some(receiver);
            } else {
                app.status = "無法建立系統匣，關閉主視窗將直接結束。".into();
            }
            app.start_minimized = minimized && app.shell.is_some();
            unsafe {
                // 第一次呼叫消費背景執行器可能帶入的 STARTUPINFO 隱藏旗標。
                let _ = ShowWindow(app.owner, SW_HIDE);
                if !app.start_minimized {
                    let _ = ShowWindow(app.owner, SW_SHOWNORMAL);
                }
            }
            if app.background {
                app.refresh_usage(&creation.egui_ctx, false);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| error.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn time_parser_rejects_seconds_and_keeps_end_of_day() {
        assert_eq!(parse_time("24:00", true), Some(1440));
        assert_eq!(parse_time("24:00", false), None);
        for text in ["12:99", "-1:00", "10:00:00", "12:30.5", "9:30"] {
            assert_eq!(parse_time(text, true), None);
        }
    }
}
