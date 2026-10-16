//! 「CLI 與程式設定」對話框。
//!
//! 內容區可捲動，儲存／取消固定在捲動區之外，
//! 視窗變窄、DPI 變高或檢查結果變長都不會把按鈕擠出畫面。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use aiwake_core::catalog;
use aiwake_core::{AppSettings, CancelToken, CliKind, CliProbeResult, CliProfile, CliRunner, ThinkingEffort};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{DeleteObject, SetBkMode, HBRUSH, HDC, HGDIOBJ, TRANSPARENT};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetWindowRect, SetForegroundWindow, SetWindowPos, ShowWindow, HICON, MINMAXINFO, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DPICHANGED, WM_DRAWITEM,
    WM_ERASEBKGND, WM_GETMINMAXINFO, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SIZE, WM_VSCROLL,
};

use crate::theme::{self, palette, ButtonVariant, Fonts};
use crate::ui::{
    self, UiQueue, VScroll, BN_CLICKED, CBN_EDITCHANGE, CBN_SELCHANGE, WM_APP_DISPATCH, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL,
};
use crate::w32::{self, draw_text, DT_LEFT, DT_LINE, DT_NOPREFIX, DT_WORDBREAK};

pub const WINDOW_CLASS: &str = "AiWakeSettingsWindow";
const CONTENT_CLASS: &str = "AiWakeSettingsContent";

pub const ID_OK: i32 = 1;
pub const ID_CANCEL: i32 = 2;
pub const ID_MODEL_BASE: i32 = 300;
pub const ID_EFFORT_BASE: i32 = 310;
pub const ID_PATH_BASE: i32 = 320;
pub const ID_ARGUMENTS_BASE: i32 = 330;
pub const ID_BROWSE_BASE: i32 = 340;
pub const ID_STARTUP: i32 = 350;
pub const ID_TRAY: i32 = 351;
pub const ID_TOKEN_SAVER: i32 = 352;
pub const ID_AUTO_START_AGY: i32 = 353;
pub const ID_TIMEOUT: i32 = 360;
pub const ID_QUOTA_REFRESH: i32 = 361;
pub const ID_PROBE: i32 = 370;

const DM_GETDEFID: u32 = 0x0400;
const DC_HASDEFID: isize = 0x534B;

const TITLE: &str = "CLI 與程式設定";
const DESCRIPTION: &str = "預設使用低價喚醒模型；可為各 CLI 指定其他模型（支援下拉選取或直接輸入任意自訂模型 ID）與思考程度（Reasoning Effort）。可執行檔可填命令名稱（agy / codex / claude）或完整 .exe 路徑。";
const TIMEOUT_LABEL: &str = "單一 CLI 最長執行時間（分鐘）：";
const QUOTA_LABEL: &str = "未倒數時的背景自動讀取間隔（分鐘，0 表示關閉；倒數中將於倒數結束自動讀取）：";

#[derive(Clone, Copy)]
enum FontKind {
    Body,
    SectionTitle,
    HeaderTitle,
    TableHeader,
}

struct Label {
    bounds: RECT,
    text: String,
    font: FontKind,
    color: COLORREF,
    wrap: bool,
}

struct CliRow {
    kind: CliKind,
    model: HWND,
    effort: HWND,
    path: HWND,
    arguments: HWND,
    browse: HWND,
    efforts: RefCell<Vec<ThinkingEffort>>,
}

struct SettingsWindow {
    hwnd: HWND,
    content: HWND,
    fonts: RefCell<Rc<Fonts>>,
    queue: UiQueue<SettingsWindow>,
    runner: Arc<dyn CliRunner>,
    result_settings: RefCell<AppSettings>,

    rows: Vec<CliRow>,
    startup_check: HWND,
    tray_check: HWND,
    token_saver_check: HWND,
    auto_start_agy_check: HWND,
    timeout_input: (HWND, HWND),
    quota_refresh_input: (HWND, HWND),
    probe_button: HWND,
    ok_button: HWND,
    cancel_button: HWND,

    probe_text: RefCell<String>,
    probe_color: Cell<COLORREF>,
    probe_cancel: RefCell<Option<CancelToken>>,

    scroll: VScroll,
    labels: RefCell<Vec<Label>>,
    panels: RefCell<Vec<RECT>>,
    panel_brush: HBRUSH,
    in_layout: Cell<bool>,
    done: Cell<bool>,
    accepted: Cell<bool>,
}

fn register_classes(icon: HICON) {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    if REGISTERED.replace(true) {
        return;
    }
    w32::register_class(WINDOW_CLASS, Some(window_proc), 0, icon);
    w32::register_class(CONTENT_CLASS, Some(content_proc), 0, HICON::default());
}

/// 顯示設定對話框；使用者按「儲存」時回傳新的設定，取消則回傳 None。
pub fn show_modal(owner: HWND, source: &AppSettings, runner: Arc<dyn CliRunner>, icon: HICON) -> Option<AppSettings> {
    register_classes(icon);

    let dpi = w32::dpi_of(owner);
    let fonts = Rc::new(Fonts::new(dpi));

    // 置中於主視窗，並依工作區限制大小，避免在小螢幕或高 DPI 下超出畫面。
    let mut owner_bounds = RECT::default();
    unsafe {
        let _ = GetWindowRect(owner, &mut owner_bounds);
    }
    let work = crate::main_window::work_area(owner);
    let width = fonts.scale(1120).min(w32::width_of(&work));
    let height = fonts.scale(820).min(w32::height_of(&work));
    let x = (owner_bounds.left + (w32::width_of(&owner_bounds) - width) / 2)
        .clamp(work.left, (work.right - width).max(work.left));
    let y = (owner_bounds.top + (w32::height_of(&owner_bounds) - height) / 2)
        .clamp(work.top, (work.bottom - height).max(work.top));

    let hwnd = w32::create_window(
        WS_EX_CONTROLPARENT,
        WINDOW_CLASS,
        TITLE,
        WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
        w32::rect(x, y, width, height),
        Some(owner),
        0,
        std::ptr::null(),
    );
    if hwnd.is_invalid() {
        return None;
    }
    let content = w32::create_window(
        WS_EX_CONTROLPARENT,
        CONTENT_CLASS,
        "",
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN,
        w32::rect(0, 0, 10, 10),
        Some(hwnd),
        0,
        std::ptr::null(),
    );

    let mut rows = Vec::new();
    for (index, descriptor) in catalog::all().iter().enumerate() {
        let index = index as i32;
        let model = ui::create_combo(content, ID_MODEL_BASE + index, true);
        for preset in descriptor.preset_models.iter().filter(|preset| !preset.is_empty()) {
            ui::combo_add(model, preset);
        }
        rows.push(CliRow {
            kind: descriptor.kind,
            model,
            effort: ui::create_combo(content, ID_EFFORT_BASE + index, false),
            path: ui::create_edit(content, ID_PATH_BASE + index, 0),
            arguments: ui::create_edit(content, ID_ARGUMENTS_BASE + index, 0),
            browse: theme::create_button(content, ID_BROWSE_BASE + index, "瀏覽…", ButtonVariant::Secondary),
            efforts: RefCell::new(Vec::new()),
        });
    }

    let state = Box::new(SettingsWindow {
        hwnd,
        content,
        fonts: RefCell::new(fonts),
        queue: UiQueue::new(hwnd),
        runner,
        result_settings: RefCell::new(source.normalized()),
        rows,
        startup_check: ui::create_checkbox(content, ID_STARTUP, "登入 Windows 後自動啟動（最小化到系統匣）"),
        tray_check: ui::create_checkbox(content, ID_TRAY, "關閉主視窗時縮到系統匣，讓排程繼續執行"),
        token_saver_check: ui::create_checkbox(
            content,
            ID_TOKEN_SAVER,
            "節省 Token 模式（停用工具與 MCP、預設低推理、短回覆；建議保持開啟）",
        ),
        auto_start_agy_check: ui::create_checkbox(
            content,
            ID_AUTO_START_AGY,
            "允許在背景無視窗查詢 Antigravity 額度（不需開啟 IDE）",
        ),
        timeout_input: ui::create_number(content, ID_TIMEOUT, 1, 120),
        quota_refresh_input: ui::create_number(content, ID_QUOTA_REFRESH, 0, 1440),
        probe_button: theme::create_button(
            content,
            ID_PROBE,
            "檢查全部 CLI（只執行 --version，不消耗 Token）",
            ButtonVariant::Secondary,
        ),
        ok_button: theme::create_button(hwnd, ID_OK, "儲存", ButtonVariant::Primary),
        cancel_button: theme::create_button(hwnd, ID_CANCEL, "取消", ButtonVariant::Secondary),
        probe_text: RefCell::new(String::new()),
        probe_color: Cell::new(palette().secondary_text),
        probe_cancel: RefCell::new(None),
        scroll: VScroll::default(),
        labels: RefCell::new(Vec::new()),
        panels: RefCell::new(Vec::new()),
        panel_brush: w32::solid_brush(palette().panel),
        in_layout: Cell::new(false),
        done: Cell::new(false),
        accepted: Cell::new(false),
    });
    let state_pointer = Box::into_raw(state);
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_USERDATA};
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_pointer as isize);
        SetWindowLongPtrW(content, GWLP_USERDATA, state_pointer as isize);
    }
    let window: &SettingsWindow = unsafe { &*state_pointer };

    window.apply_fonts();
    window.load_values();
    window.layout();

    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(Some(window.rows[0].model));
    }
    ui::pump_until(|| window.done.get());

    let result = window.accepted.get().then(|| window.result_settings.borrow().clone());
    if let Some(cancel) = window.probe_cancel.borrow_mut().take() {
        cancel.cancel();
    }
    unsafe {
        // 先恢復主視窗再關閉對話框，焦點才會回到主視窗而不是跳到其他應用程式。
        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
        let _ = DestroyWindow(hwnd);
    }
    result
}

impl SettingsWindow {
    fn fonts(&self) -> Rc<Fonts> {
        Rc::clone(&self.fonts.borrow())
    }

    fn apply_fonts(&self) {
        let fonts = self.fonts();
        for row in &self.rows {
            for control in [row.model, row.effort, row.path, row.arguments, row.browse] {
                w32::set_font(control, &fonts.body);
            }
        }
        for control in [
            self.startup_check,
            self.tray_check,
            self.token_saver_check,
            self.auto_start_agy_check,
            self.timeout_input.0,
            self.quota_refresh_input.0,
            self.probe_button,
            self.ok_button,
            self.cancel_button,
        ] {
            w32::set_font(control, &fonts.body);
        }
    }

    fn load_values(&self) {
        let settings = self.result_settings.borrow().clone();
        for row in &self.rows {
            let profile = settings.profile(row.kind);
            w32::set_text(row.path, &profile.executable);
            w32::set_text(row.model, &profile.model);
            w32::set_text(row.arguments, &profile.additional_arguments);
            self.refresh_effort_options(row, &profile.model, Some(profile.thinking_effort));
        }
        ui::set_checked(self.startup_check, settings.start_with_windows);
        ui::set_checked(self.tray_check, settings.minimize_to_tray);
        ui::set_checked(self.token_saver_check, settings.token_saver_mode);
        ui::set_checked(self.auto_start_agy_check, settings.auto_start_antigravity);
        ui::number_set(self.timeout_input.1, settings.execution_timeout_minutes.clamp(1, 120));
        ui::number_set(self.quota_refresh_input.1, settings.quota_auto_refresh_minutes.clamp(0, 1440));
    }

    fn selected_effort(&self, row: &CliRow) -> ThinkingEffort {
        let index = ui::combo_selected(row.effort);
        row.efforts.borrow().get(index.max(0) as usize).copied().unwrap_or(ThinkingEffort::Default)
    }

    /// 依目前的模型重建思考程度選單；不相容的選項降到該模型可接受的最接近等級。
    fn refresh_effort_options(&self, row: &CliRow, model: &str, preferred_effort: Option<ThinkingEffort>) {
        let descriptor = catalog::get(row.kind);
        let current_effort = preferred_effort.unwrap_or_else(|| self.selected_effort(row));
        let normalized_effort = descriptor.normalize_effort(Some(model), current_effort);
        let supported = descriptor.get_supported_efforts(Some(model));

        ui::combo_clear(row.effort);
        let mut selected_index = 0;
        for (index, effort) in supported.iter().enumerate() {
            ui::combo_add(row.effort, effort.display_name());
            if *effort == normalized_effort {
                selected_index = index;
            }
        }
        *row.efforts.borrow_mut() = supported.to_vec();
        ui::combo_select(row.effort, if supported.is_empty() { -1 } else { selected_index as i32 });
    }

    fn profile_from_inputs(&self, row: &CliRow) -> CliProfile {
        CliProfile {
            executable: w32::get_text(row.path).trim().to_string(),
            model: w32::get_text(row.model).trim().to_string(),
            thinking_effort: self.selected_effort(row),
            additional_arguments: w32::get_text(row.arguments).trim().to_string(),
        }
    }

    fn reject_save(&self, message: &str) {
        w32::message_box(Some(self.hwnd), message, TITLE, w32::MB_OK | w32::MB_ICONWARNING);
    }

    fn save_values(&self) {
        let mut settings = self.result_settings.borrow().clone();
        for row in &self.rows {
            let descriptor = catalog::get(row.kind);
            let profile = self.profile_from_inputs(row);

            if profile.executable.is_empty() {
                self.reject_save(&format!("請填寫 {} 的命令或路徑。", descriptor.display_name));
                return;
            }
            if let Err(error) = aiwake_core::tokenizer::parse(Some(&profile.additional_arguments)) {
                self.reject_save(&format!("{}：{}", descriptor.display_name, error.message()));
                return;
            }
            settings.cli_profiles.insert(row.kind, profile);
        }

        settings.start_with_windows = ui::is_checked(self.startup_check);
        settings.minimize_to_tray = ui::is_checked(self.tray_check);
        settings.token_saver_mode = ui::is_checked(self.token_saver_check);
        settings.auto_start_antigravity = ui::is_checked(self.auto_start_agy_check);
        settings.execution_timeout_minutes = ui::number_get(self.timeout_input.1).clamp(1, 120);
        settings.quota_auto_refresh_minutes = ui::number_get(self.quota_refresh_input.1).clamp(0, 1440);

        *self.result_settings.borrow_mut() = settings;
        self.accepted.set(true);
        self.done.set(true);
    }

    fn set_probe_status(&self, text: &str, color: COLORREF) {
        *self.probe_text.borrow_mut() = text.to_string();
        self.probe_color.set(color);
        self.layout();
    }

    fn probe_all(&self) {
        if !unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(self.probe_button) }.as_bool() {
            return;
        }
        w32::enable(self.probe_button, false);
        self.set_probe_status("檢查中…", palette().secondary_text);

        let cancel = CancelToken::new();
        if let Some(previous) = self.probe_cancel.borrow_mut().replace(cancel.clone()) {
            previous.cancel();
        }

        // 使用者可能剛改過路徑，先讓解析快取失效再探測。
        aiwake_core::locator::clear_cache();

        let profiles: Vec<(CliKind, CliProfile)> =
            self.rows.iter().map(|row| (row.kind, self.profile_from_inputs(row))).collect();
        let runner = Arc::clone(&self.runner);
        let queue = self.queue.clone();
        std::thread::spawn(move || {
            let working_directory = aiwake_core::models::user_profile_directory();
            let results: Vec<CliProbeResult> = std::thread::scope(|scope| {
                let handles: Vec<_> = profiles
                    .iter()
                    .map(|(kind, profile)| {
                        let (runner, cancel, working_directory) = (&runner, &cancel, &working_directory);
                        scope.spawn(move || runner.probe(*kind, profile, working_directory, cancel))
                    })
                    .collect();
                handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
            });

            if cancel.is_canceled() {
                return;
            }
            queue.post(move |window: &SettingsWindow| window.on_probe_finished(&results));
        });
    }

    fn on_probe_finished(&self, results: &[CliProbeResult]) {
        let succeeded = results.iter().all(|result| result.succeeded);
        let lines: Vec<String> = results
            .iter()
            .map(|result| {
                format!(
                    "{}：{} {}",
                    result.cli.display_name(),
                    if result.succeeded { "✓" } else { "✗" },
                    result.summary
                )
            })
            .collect();
        self.set_probe_status(&lines.join("\r\n"), if succeeded { palette().success } else { palette().danger });
        w32::enable(self.probe_button, true);
    }

    fn browse_executable(&self, row: &CliRow) {
        let title = format!("選擇 {} 可執行檔", row.kind.display_name());
        let filters = [("可執行檔 (*.exe;*.cmd;*.bat)", "*.exe;*.cmd;*.bat"), ("所有檔案 (*.*)", "*.*")];
        if let Some(path) = ui::pick_file(self.hwnd, &title, &filters) {
            w32::set_text(row.path, &path);
        }
    }

    fn on_command(&self, id: i32, code: u32) {
        match id {
            ID_OK if code == BN_CLICKED => self.save_values(),
            ID_CANCEL if code == BN_CLICKED => self.done.set(true),
            ID_PROBE if code == BN_CLICKED => self.probe_all(),
            _ => {
                let Some(row) = self.rows.get(((id % 10).max(0)) as usize) else {
                    return;
                };
                match id - id % 10 {
                    ID_BROWSE_BASE if code == BN_CLICKED => self.browse_executable(row),
                    ID_MODEL_BASE if code == CBN_EDITCHANGE => {
                        self.refresh_effort_options(row, &w32::get_text(row.model), None);
                    }
                    ID_MODEL_BASE if code == CBN_SELCHANGE => {
                        // 選取當下編輯框文字尚未更新，直接取清單項目文字。
                        let model = ui::combo_item_text(row.model, ui::combo_selected(row.model));
                        self.refresh_effort_options(row, &model, None);
                    }
                    _ => {}
                }
            }
        }
    }

    /// 擺放固定在底部的按鈕列與可捲動的內容區。
    fn layout_frame(&self) {
        let fonts = self.fonts();
        let s = |value: i32| fonts.scale(value);
        let client = w32::client_rect(self.hwnd);
        let button_height = theme::button_height(&fonts);
        let footer_height = s(8) + button_height + s(16);

        let ok_width = theme::button_width(&fonts, "儲存");
        let cancel_width = theme::button_width(&fonts, "取消");
        let top = client.bottom - footer_height + s(8);
        let ok_left = client.right - s(24) - ok_width;
        w32::move_window(self.ok_button, &w32::rect(ok_left, top, ok_width, button_height));
        w32::move_window(
            self.cancel_button,
            &w32::rect(ok_left - s(6) - cancel_width, top, cancel_width, button_height),
        );
        w32::move_window(
            self.content,
            &w32::rect(0, 0, w32::width_of(&client), (w32::height_of(&client) - footer_height).max(0)),
        );
    }

    /// 計算內容區的版面：移動控制項，並記下要自繪的文字與面板位置。
    fn layout(&self) {
        if self.in_layout.replace(true) {
            return;
        }
        let fonts = self.fonts();
        let colors = palette();
        let s = |value: i32| fonts.scale(value);
        let client = w32::client_rect(self.content);
        let width = w32::width_of(&client);
        let offset = self.scroll.position();

        let mut labels: Vec<Label> = Vec::new();
        let mut panels: Vec<RECT> = Vec::new();
        let left = s(24);
        let inner_width = (width - s(48)).max(s(200));
        let input_height = fonts.body.height() + s(10);
        let check_height = fonts.body.height() + s(6);
        let mut y = s(20) - offset;

        let mut push = |bounds: RECT, text: &str, font: FontKind, color: COLORREF, wrap: bool| {
            labels.push(Label { bounds, text: text.to_string(), font, color, wrap });
        };

        let title_height = fonts.header_title.height();
        push(w32::rect(left, y, inner_width, title_height), TITLE, FontKind::HeaderTitle, colors.primary_text, false);
        y += title_height + s(4);

        let description_width = inner_width.min(s(950));
        let description_height = fonts.body.measure_wrapped(DESCRIPTION, description_width);
        push(
            w32::rect(left, y, description_width, description_height),
            DESCRIPTION,
            FontKind::Body,
            colors.secondary_text,
            true,
        );
        y += description_height + s(18);

        // ── CLI 模型與連線設定 ──
        let panel_top = y;
        let pad = s(16);
        let table_left = left + pad;
        let table_width = inner_width - pad * 2;
        y += pad;
        let section_height = fonts.section_title.height();
        push(
            w32::rect(table_left, y, table_width, section_height),
            "CLI 模型與連線設定",
            FontKind::SectionTitle,
            colors.primary_text,
            false,
        );
        y += section_height + s(14);

        let name_width =
            self.rows.iter().map(|row| fonts.body.measure(row.kind.display_name()).0).max().unwrap_or(0) + s(14);
        let effort_width = s(130).max(
            ThinkingEffort::ALL.iter().map(|effort| fonts.body.measure(effort.display_name()).0).max().unwrap_or(0)
                + s(34),
        ) + s(8);
        let browse_width = theme::button_width(&fonts, "瀏覽…");
        let flexible = (table_width - name_width - effort_width - browse_width).max(s(240));
        let model_width = flexible * 32 / 100;
        let path_width = flexible * 38 / 100;
        let arguments_width = flexible - model_width - path_width;

        let model_x = table_left + name_width;
        let effort_x = model_x + model_width;
        let path_x = effort_x + effort_width;
        let arguments_x = path_x + path_width;
        let browse_x = arguments_x + arguments_width;

        let header_height = fonts.table_header.height();
        for (x, column_width, text) in [
            (table_left, name_width, "CLI"),
            (model_x, model_width, "模型 (Model)"),
            (effort_x, effort_width, "思考程度 (Effort)"),
            (path_x, path_width, "可執行檔或命令"),
            (arguments_x, arguments_width, "額外參數（選填）"),
        ] {
            push(
                w32::rect(x, y, column_width - s(8), header_height),
                text,
                FontKind::TableHeader,
                colors.secondary_text,
                false,
            );
        }
        y += header_height + s(8);

        for row in &self.rows {
            let top = y + s(3);
            push(
                w32::rect(table_left, top, name_width - s(8), input_height),
                row.kind.display_name(),
                FontKind::Body,
                colors.primary_text,
                false,
            );
            // 下拉選單的高度同時決定展開清單的高度。
            w32::move_window(row.model, &w32::rect(model_x, top, model_width - s(8), input_height + s(260)));
            ui::combo_reset_edit_selection(row.model);
            w32::move_window(row.effort, &w32::rect(effort_x, top, effort_width - s(8), input_height + s(260)));
            w32::move_window(row.path, &w32::rect(path_x, top, path_width - s(8), input_height));
            w32::move_window(row.arguments, &w32::rect(arguments_x, top, arguments_width - s(8), input_height));
            w32::move_window(row.browse, &w32::rect(browse_x, top, browse_width, input_height));
            y += input_height + s(6);
        }
        y += pad;
        panels.push(RECT { left, top: panel_top, right: left + inner_width, bottom: y });
        y += s(16);

        // ── 啟動與資源 ──
        let panel_top = y;
        y += pad;
        push(
            w32::rect(table_left, y, table_width, section_height),
            "啟動與資源",
            FontKind::SectionTitle,
            colors.primary_text,
            false,
        );
        y += section_height + s(12);
        for (check, bottom_margin) in [
            (self.startup_check, 6),
            (self.tray_check, 6),
            (self.token_saver_check, 6),
            (self.auto_start_agy_check, 10),
        ] {
            y += s(3);
            let check_width = (fonts.body.measure(&w32::get_text(check)).0 + s(28)).min(table_width);
            w32::move_window(check, &w32::rect(table_left, y, check_width, check_height));
            y += check_height + s(bottom_margin);
        }
        for (label, input) in [(TIMEOUT_LABEL, self.timeout_input), (QUOTA_LABEL, self.quota_refresh_input)] {
            y += s(4);
            let label_width = (fonts.body.measure(label).0 + s(5)).min(table_width - s(76));
            push(
                w32::rect(table_left, y, label_width, input_height),
                label,
                FontKind::Body,
                colors.primary_text,
                false,
            );
            ui::number_move(input.0, input.1, &w32::rect(table_left + label_width, y, s(70), input_height));
            y += input_height;
        }
        y += pad;
        panels.push(RECT { left, top: panel_top, right: left + inner_width, bottom: y });
        y += s(16);

        // ── 連線檢查 ──
        let panel_top = y;
        y += pad;
        push(
            w32::rect(table_left, y, table_width, section_height),
            "連線檢查",
            FontKind::SectionTitle,
            colors.primary_text,
            false,
        );
        y += section_height + s(10);
        let probe_text = w32::get_text(self.probe_button);
        let button_height = theme::button_height(&fonts);
        w32::move_window(
            self.probe_button,
            &w32::rect(table_left, y, theme::button_width(&fonts, &probe_text).min(table_width), button_height),
        );
        y += button_height;
        let status = self.probe_text.borrow().clone();
        if !status.is_empty() {
            y += s(6);
            let status_height = fonts.body.measure_wrapped(&status, table_width);
            push(
                w32::rect(table_left, y, table_width, status_height),
                &status,
                FontKind::Body,
                self.probe_color.get(),
                true,
            );
            y += status_height;
        }
        y += pad;
        panels.push(RECT { left, top: panel_top, right: left + inner_width, bottom: y });
        y += s(20);

        *self.labels.borrow_mut() = labels;
        *self.panels.borrow_mut() = panels;
        let scrolled = self.scroll.update(self.content, y + offset, w32::height_of(&client));
        self.in_layout.set(false);
        if scrolled {
            // 內容變短使捲動位置被拉回時，依新的位置重排一次。
            self.layout();
            return;
        }
        w32::invalidate(self.content);
    }

    fn paint_content(&self, dc: HDC, bounds: &RECT) {
        let fonts = self.fonts();
        let colors = palette();
        w32::fill(dc, bounds, colors.canvas);
        for panel in self.panels.borrow().iter() {
            w32::fill(dc, panel, colors.panel);
        }
        for label in self.labels.borrow().iter() {
            let font = match label.font {
                FontKind::Body => &fonts.body,
                FontKind::SectionTitle => &fonts.section_title,
                FontKind::HeaderTitle => &fonts.header_title,
                FontKind::TableHeader => &fonts.table_header,
            };
            let format = if label.wrap { DT_WORDBREAK | DT_NOPREFIX } else { DT_LINE | DT_LEFT };
            draw_text(dc, font, &label.text, &label.bounds, label.color, format);
        }
    }
}

fn handle_draw_item(window: &SettingsWindow, lparam: LPARAM, background: COLORREF) -> LRESULT {
    let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
    theme::draw_button(item, &window.fonts(), background);
    LRESULT(1)
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCDESTROY {
        if let Some(state) = w32::take_state::<SettingsWindow>(hwnd) {
            let _ = DeleteObject(HGDIOBJ(state.panel_brush.0));
        }
        return w32::def_proc(hwnd, message, wparam, lparam);
    }
    let Some(window) = w32::state::<SettingsWindow>(hwnd) else {
        return w32::def_proc(hwnd, message, wparam, lparam);
    };

    match message {
        WM_APP_DISPATCH => {
            window.queue.clone().drain(window);
            LRESULT(0)
        }
        WM_SIZE => {
            window.layout_frame();
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let fonts = window.fonts();
            let work = crate::main_window::work_area(hwnd);
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            info.ptMinTrackSize =
                POINT { x: fonts.scale(980).min(w32::width_of(&work)), y: fonts.scale(640).min(w32::height_of(&work)) };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            *window.fonts.borrow_mut() = Rc::new(Fonts::new(w32::hi_word(wparam.0) as i32));
            window.apply_fonts();
            let suggested = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                suggested.left,
                suggested.top,
                w32::width_of(suggested),
                w32::height_of(suggested),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            window.layout_frame();
            window.layout();
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            w32::paint_buffered(hwnd, |dc, bounds| w32::fill(dc, bounds, palette().canvas));
            LRESULT(0)
        }
        WM_DRAWITEM => handle_draw_item(window, lparam, palette().canvas),
        // Enter 視為按下「儲存」。
        DM_GETDEFID => LRESULT((DC_HASDEFID << 16) | ID_OK as isize),
        WM_COMMAND => {
            let (id, code, _) = ui::command_parts(wparam, lparam);
            window.on_command(id, code);
            LRESULT(0)
        }
        WM_CLOSE => {
            window.done.set(true);
            LRESULT(0)
        }
        _ => w32::def_proc(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn content_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(window) = w32::state::<SettingsWindow>(hwnd) else {
        return w32::def_proc(hwnd, message, wparam, lparam);
    };

    match message {
        WM_SIZE => {
            window.layout();
            LRESULT(0)
        }
        WM_VSCROLL => {
            if window.scroll.on_scroll(wparam, window.fonts().scale(24)) {
                window.layout();
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if window.scroll.on_wheel(wparam, window.fonts().scale(24)) {
                window.layout();
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            w32::paint_buffered(hwnd, |dc, bounds| window.paint_content(dc, bounds));
            LRESULT(0)
        }
        WM_DRAWITEM => handle_draw_item(window, lparam, palette().panel),
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            // 勾選方塊位於白色面板上。
            SetBkMode(HDC(wparam.0 as *mut _), TRANSPARENT);
            LRESULT(window.panel_brush.0 as isize)
        }
        WM_COMMAND => {
            let (id, code, _) = ui::command_parts(wparam, lparam);
            window.on_command(id, code);
            LRESULT(0)
        }
        _ => w32::def_proc(hwnd, message, wparam, lparam),
    }
}
