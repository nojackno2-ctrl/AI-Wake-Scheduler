//! 真正的 egui 操作、隔離假 CLI 與原生系統匣生命週期驗證。
//! AIWAKE_UI_SHOTS 指定資料夾時，透過 wgpu 輸出 PNG 供人工檢視。
use aiwake_app::{host::AppHost, main_window::Dashboard, theme, tray::shell};
use aiwake_core::{
    catalog, AppSettings, CliKind, CliUsageAvailability, CliUsageSnapshot, CliUsageWindow, DataStore, Guid,
    JsonFileStore, ScheduledJob, Span, ThinkingEffort,
};
use eframe::egui;
use egui_kittest::{kittest::Queryable, Harness};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE, WM_COMMAND,
};

fn qa_directory() -> PathBuf {
    let root = std::env::temp_dir().join(format!("AiWakeScheduler-QA-{}", Guid::new().simple()));
    std::fs::create_dir_all(&root).expect("建立隔離資料夾");
    root
}
fn load_jobs(root: &Path) -> Vec<ScheduledJob> {
    JsonFileStore::new(root.join("schedules.json"), Vec::new).load().expect("讀取排程")
}
fn click(harness: &mut Harness<'_, Dashboard>, label: &str) {
    harness.get_by_label(label).scroll_to_me();
    harness.run_steps(2);
    harness.get_by_label(label).click();
    harness.run_steps(3);
}
fn input(harness: &mut Harness<'_, Dashboard>, label: &str, text: &str) {
    harness.get_by_label(label).scroll_to_me();
    harness.run_steps(2);
    harness.get_by_label(label).set_text_value_accesskit(text);
    harness.run_steps(3);
}
fn shot(harness: &mut Harness<'_, Dashboard>, name: &str) {
    if let Some(directory) = std::env::var_os("AIWAKE_UI_SHOTS") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("建立截圖資料夾");
        harness.render().expect("wgpu 畫面渲染").save(directory.join(format!("{name}.png"))).expect("儲存截圖");
    }
}
fn pump_until(harness: &mut Harness<'_, Dashboard>, description: &str, condition: impl Fn(&Dashboard) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        harness.run_steps(2);
        if condition(harness.state()) {
            return;
        }
        assert!(Instant::now() < deadline, "等待逾時：{description}，狀態：{}", harness.state().status);
        std::thread::sleep(Duration::from_millis(30));
    }
}
#[test]
fn egui_editor_settings_fake_cli_and_rendering() {
    let root = qa_directory();
    let host = Arc::new(AppHost::create(Some(&root.to_string_lossy())).expect("隔離 AppHost"));
    let app = Dashboard::new(host.clone(), false, &egui::Context::default());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 1150.0))
        .allow_missing_glyphs()
        .with_context_setup(theme::fonts)
        .build_ui_state(|ui, app: &mut Dashboard| app.draw(ui), app);
    harness.run_steps(3);
    assert_eq!(harness.state().editor.name, "AI 倒數喚醒");
    assert_eq!(harness.state().editor.message, "早安");
    assert_eq!(harness.state().editor.end, "24:00");
    assert_eq!(harness.state().editor.interval, 10);
    assert_eq!(harness.state().editor.targets, catalog::kinds());
    assert!(harness.state().editor.enabled);
    assert!(host
        .paths
        .root_directory()
        .file_name()
        .expect("QA 名稱")
        .to_string_lossy()
        .starts_with("AiWakeScheduler-QA-"));
    shot(&mut harness, "01-default-dark");
    input(&mut harness, "排程名稱", "測試排程");
    // 選用離目前時間 3 小時後的整點一小時時段，涵蓋所有整點刻度且避免測試期間自動觸發真正 CLI。
    let start_hour = (aiwake_core::time::local_time_of_day(&aiwake_core::time::now()).num_hours() + 3).rem_euclid(24);
    let start = format!("{start_hour:02}:00");
    let end = format!("{:02}:00", (start_hour + 1).min(24));
    input(&mut harness, "開始時間 (HH:MM)", &start);
    input(&mut harness, "結束時間 (HH:MM，支援 24:00)", &end);
    click(&mut harness, "建立排程");
    assert_eq!(load_jobs(&root).len(), 1, "{}", harness.state().status);
    click(&mut harness, "啟停 測試排程");
    assert!(!load_jobs(&root)[0].enabled, "卡片停用必須立即寫檔");
    click(&mut harness, "啟停 測試排程");
    assert!(load_jobs(&root)[0].enabled);
    click(&mut harness, "啟停 測試排程");
    input(&mut harness, "結束時間 (HH:MM，支援 24:00)", "24:00");
    click(&mut harness, "✓ Ge AGY Gemini");
    click(&mut harness, "15分");
    click(&mut harness, "儲存修改");
    assert_eq!(load_jobs(&root)[0].targets.len(), 4);
    assert_eq!(load_jobs(&root)[0].end_time_of_day, Span::days(1));
    assert_eq!(load_jobs(&root)[0].probe_interval_minutes, 15);
    click(&mut harness, "＋ 新增");
    assert!(harness.state().editor.id.is_none());
    assert_eq!(harness.state().editor.targets.len(), 5);
    assert_eq!(harness.state().editor.interval, 10);
    click(&mut harness, "測試排程");
    assert_eq!(harness.state().editor.targets.len(), 4);
    assert_eq!(harness.state().editor.interval, 15);
    click(&mut harness, "☀ 淺色");
    assert!(!harness.state().dark);
    shot(&mut harness, "02-light");
    click(&mut harness, "⚙ 設定");
    assert_eq!(
        harness.state().settings_sheet.as_ref().expect("設定抽屜").draft.profile(CliKind::Codex).model,
        "gpt-6-luna"
    );
    // 在畫面中輸入自訂模型並切換推薦模型，驗證相同的連動路徑。
    {
        let sheet = harness.state_mut().settings_sheet.as_mut().expect("設定抽屜");
        let profile = sheet.draft.cli_profiles.get_mut(&CliKind::Codex).expect("Codex");
        profile.model = "gpt-6.1-sol".into();
        profile.thinking_effort = ThinkingEffort::Ultra;
    }
    harness.run_steps(2);
    assert_eq!(
        harness.state().settings_sheet.as_ref().expect("設定抽屜").draft.profile(CliKind::Codex).thinking_effort,
        ThinkingEffort::Ultra
    );
    harness
        .state_mut()
        .settings_sheet
        .as_mut()
        .expect("設定抽屜")
        .draft
        .cli_profiles
        .get_mut(&CliKind::Codex)
        .expect("Codex")
        .model = "gpt-6-luna".into();
    harness.run_steps(2);
    assert_eq!(
        harness.state().settings_sheet.as_ref().expect("設定抽屜").draft.profile(CliKind::Codex).thinking_effort,
        ThinkingEffort::Max
    );
    harness
        .state_mut()
        .settings_sheet
        .as_mut()
        .expect("設定抽屜")
        .draft
        .cli_profiles
        .get_mut(&CliKind::Antigravity)
        .expect("AGY")
        .model = "gemini-3.8-flash-high".into();
    harness.run_steps(2);
    assert_eq!(
        harness.state().settings_sheet.as_ref().expect("設定抽屜").draft.profile(CliKind::Antigravity).thinking_effort,
        ThinkingEffort::Default
    );
    click(&mut harness, "取消");
    assert!(!root.join("settings.json").exists());
    assert_eq!(host.settings().profile(CliKind::Codex).thinking_effort, ThinkingEffort::Default);
    let fake = root.join("fake-cli.cmd");
    std::fs::write(&fake,format!("@echo off\r\nif \"%~1\"==\"--version\" (\r\n echo PROBED>>\"{}\"\r\n echo fake-cli 1.2.3\r\n exit /b 0\r\n)\r\necho FAKE-CLI-OK\r\nexit /b 0\r\n",root.join("fake-cli-probed.txt").display())).expect("假 CLI");
    click(&mut harness, "⚙ 設定");
    {
        let sheet = harness.state_mut().settings_sheet.as_mut().expect("設定抽屜");
        for profile in sheet.draft.cli_profiles.values_mut() {
            profile.executable = fake.to_string_lossy().into_owned();
        }
    }
    harness.run_steps(2);
    click(&mut harness, "檢查 CLI 連線");
    pump_until(&mut harness, "CLI 探針", |app| {
        app.settings_sheet
            .as_ref()
            .is_some_and(|sheet| !sheet.probing && !sheet.probe_text.is_empty() && sheet.probe_text != "檢查中…")
    });
    assert!(root.join("fake-cli-probed.txt").exists());
    assert!(harness.state().settings_sheet.as_ref().expect("設定抽屜").probe_text.contains('✓'));
    shot(&mut harness, "03-settings");
    click(&mut harness, "儲存設定");
    let saved: AppSettings =
        JsonFileStore::new(root.join("settings.json"), AppSettings::default).load().expect("設定 JSON");
    assert_eq!(saved.profile(CliKind::Codex).executable, fake.to_string_lossy());
    click(&mut harness, "清除目標");
    click(&mut harness, "○ Cx OpenAI Codex");
    click(&mut harness, "儲存修改");
    click(&mut harness, "▶ 立即執行");
    click(&mut harness, "確認");
    pump_until(&mut harness, "假 CLI 執行", |app| {
        app.terminal.text().contains("FAKE-CLI-OK") && app.jobs.iter().any(|job| !job.last_results.is_empty())
    });
    let job = &load_jobs(&root)[0];
    assert!(job.last_results[0].succeeded);
    assert!(Path::new(&job.last_results[0].log_path).is_file());
    assert!(harness.state().terminal.text().contains("Codex 開始喚醒"));
    assert!(harness.state().terminal.lines.iter().any(|event| event.kind == aiwake_core::CliActivityKind::Completed));
    shot(&mut harness, "04-fake-cli-terminal");
    // 使用已知額度資料驗證五個儀表的渲染與窄版重排。
    for (index, kind) in catalog::kinds().into_iter().enumerate() {
        harness.state_mut().snapshots.insert(
            kind,
            CliUsageSnapshot::new(
                kind,
                CliUsageAvailability::Available,
                vec![
                    CliUsageWindow::new(
                        "5h",
                        index as i32 * 22,
                        Some(Span::hours(5)),
                        Some(aiwake_core::time::now() + Span::hours(2)),
                        true,
                    ),
                    CliUsageWindow::new("7d", 40, Some(Span::days(7)), None, false),
                ],
                "測試額度",
                aiwake_core::time::now(),
            ),
        );
    }
    click(&mut harness, "☾ 深色");
    harness.run_steps(2);
    shot(&mut harness, "05-ring-dashboard");
    harness.set_size(egui::vec2(800.0, 1000.0));
    harness.run_steps(3);
    shot(&mut harness, "06-compact");
    // 最小支援尺寸下仍須能開啟與取消設定，操作列不能落在視窗外。
    harness.set_size(egui::vec2(700.0, 620.0));
    harness.run_steps(3);
    input(&mut harness, "排程名稱", "最小視窗排程");
    assert_eq!(harness.state().editor.name, "最小視窗排程");
    // 矮內容區需要較長的捲動動畫，先讓儲存按鈕停穩再點擊。
    harness.get_by_label("儲存修改").scroll_to_me();
    harness.run_steps(20);
    click(&mut harness, "儲存修改");
    shot(&mut harness, "07-minimum");
    assert_eq!(load_jobs(&root)[0].name, "最小視窗排程", "狀態：{}", harness.state().status);
    click(&mut harness, "⚙ 設定");
    assert!(harness.state().settings_sheet.is_some());
    shot(&mut harness, "08-minimum-settings");
    click(&mut harness, "取消");
    assert!(harness.state().settings_sheet.is_none());
    // 檢視結果與刪除仍經過正式的畫面操作。
    harness.set_size(egui::vec2(1600.0, 1150.0));
    harness.run_steps(3);
    click(&mut harness, "刪除");
    click(&mut harness, "確認");
    assert!(load_jobs(&root).is_empty());
}

struct App {
    child: Child,
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn wait_for<T>(description: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "等待逾時：{description}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn window(pid: u32, tray: bool) -> Option<HWND> {
    struct Search {
        pid: u32,
        tray: bool,
        found: Option<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, param: LPARAM) -> BOOL {
        let search = &mut *(param.0 as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == search.pid {
            let mut class = [0u16; 128];
            let length = GetClassNameW(hwnd, &mut class);
            let class = String::from_utf16_lossy(&class[..length.max(0) as usize]);
            let matches = if search.tray {
                class == shell::WINDOW_CLASS
            } else {
                aiwake_app::w32::get_text(hwnd).contains("（功能驗證）")
            };
            if matches {
                search.found = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut search = Search { pid, tray, found: None };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM((&mut search as *mut Search) as isize));
    }
    search.found
}
fn post(hwnd: HWND, message: u32, command: usize) {
    unsafe {
        PostMessageW(Some(hwnd), message, WPARAM(command), LPARAM(0)).expect("視窗訊息");
    }
}
#[test]
fn native_wgpu_window_close_restore_and_minimized() {
    for minimize_to_tray in [true, false] {
        let root = qa_directory();
        let settings = AppSettings { minimize_to_tray, ..AppSettings::default() };
        JsonFileStore::new(root.join("settings.json"), AppSettings::default).save(&settings).expect("隔離設定");
        let mut app = App {
            child: Command::new(env!("CARGO_BIN_EXE_ai-wake-scheduler"))
                .arg("--verify-ui")
                .env("AIWAKE_VERIFY_ROOT", &root)
                .spawn()
                .expect("真實 eframe 執行檔"),
        };
        let hwnd = wait_for("eframe 主視窗", || window(app.child.id(), false));
        wait_for("視窗顯示", || unsafe { IsWindowVisible(hwnd).as_bool() }.then_some(()));
        let tray = wait_for("系統匣外殼", || window(app.child.id(), true));
        post(hwnd, WM_CLOSE, 0);
        if minimize_to_tray {
            wait_for("關閉縮到系統匣", || (!unsafe { IsWindowVisible(hwnd).as_bool() }).then_some(()));
            assert!(app.child.try_wait().expect("行程狀態").is_none());
            post(tray, WM_COMMAND, shell::OPEN);
            wait_for("系統匣還原", || unsafe { IsWindowVisible(hwnd).as_bool() }.then_some(()));
            post(tray, WM_COMMAND, shell::EXIT);
        }
        let status = wait_for("正常結束", || app.child.try_wait().expect("行程狀態"));
        assert!(status.success());
    }
    let mut app = App {
        child: Command::new(env!("CARGO_BIN_EXE_ai-wake-scheduler"))
            .args(["--verify-ui", "--minimized"])
            .spawn()
            .expect("縮到系統匣啟動"),
    };
    let tray = wait_for("最小化啟動系統匣", || window(app.child.id(), true));
    let hwnd = wait_for("最小化主視窗", || window(app.child.id(), false));
    wait_for("啟動時隱藏", || (!unsafe { IsWindowVisible(hwnd).as_bool() }).then_some(()));
    assert!(app.child.try_wait().expect("行程狀態").is_none());
    post(tray, WM_COMMAND, shell::OPEN);
    wait_for("最小化啟動後還原", || unsafe { IsWindowVisible(hwnd).as_bool() }.then_some(()));
    post(tray, WM_COMMAND, shell::EXIT);
    assert!(wait_for("最小化啟動後退出", || app.child.try_wait().expect("行程狀態")).success());
}
