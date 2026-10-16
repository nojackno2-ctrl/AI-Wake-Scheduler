//! 端對端 UI 測試：啟動真正的執行檔（`--verify-ui` 隔離模式），
//! 以視窗訊息操作控制項，並以隔離資料目錄中的 JSON 驗證結果。
//!
//! 設定環境變數 `AIWAKE_UI_SHOTS` 為資料夾路徑時，會在各階段把視窗畫面存成 BMP 供人工檢視。

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use aiwake_app::{main_window as main, settings_window as settings, w32};
use aiwake_core::{
    AppSettings, CliKind, DataStore, JsonFileStore, ScheduleRecurrence, ScheduleStatus, ScheduledJob, ThinkingEffort,
};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetDlgItem, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    WM_CLOSE, WM_COMMAND, WM_GETTEXT, WM_GETTEXTLENGTH, WM_SETTEXT, WM_VSCROLL,
};

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const CB_GETCOUNT: u32 = 0x0146;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const LVM_GETITEMCOUNT: u32 = 0x1004;
const BN_CLICKED: usize = 0;
const CBN_SELCHANGE: usize = 1;
const CBN_EDITCHANGE: usize = 5;

/// 測試結束（包含失敗）時確保受測程式不會殘留在系統匣。
struct App {
    child: Child,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for<T>(what: &str, timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "等待逾時：{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until(what: &str, timeout: Duration, condition: impl Fn() -> bool) {
    wait_for(what, timeout, || condition().then_some(()));
}

fn class_of(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

/// 指定行程中符合類別名稱的最上層視窗。
fn top_window(process_id: u32, class: &str) -> Option<HWND> {
    struct Search {
        process_id: u32,
        found: Vec<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        if owner == search.process_id {
            search.found.push(hwnd);
        }
        BOOL(1)
    }

    let mut search = Search { process_id, found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut _ as isize));
    }
    search.found.into_iter().find(|hwnd| class_of(*hwnd) == class)
}

fn child_of_class(parent: HWND, class: &str) -> HWND {
    let class = w32::wide(class);
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowExW(
            Some(parent),
            None,
            windows::core::PCWSTR(class.as_ptr()),
            windows::core::PCWSTR::null(),
        )
        .expect("找不到子視窗")
    }
}

fn control(parent: HWND, id: i32) -> HWND {
    unsafe { GetDlgItem(Some(parent), id) }.unwrap_or_else(|_| panic!("找不到控制項 {id}"))
}

/// 跨行程讀取控制項文字（WM_GETTEXT 會由系統代為封送）。
fn text_of(hwnd: HWND) -> String {
    let length = w32::send(hwnd, WM_GETTEXTLENGTH, 0, 0).max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let copied = w32::send(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize).max(0) as usize;
    String::from_utf16_lossy(&buffer[..copied.min(length)])
}

fn set_text(hwnd: HWND, text: &str) {
    let text = w32::wide(text);
    w32::send(hwnd, WM_SETTEXT, 0, text.as_ptr() as isize);
}

fn is_checked(hwnd: HWND) -> bool {
    w32::send(hwnd, BM_GETCHECK, 0, 0) == 1
}

fn set_checked(hwnd: HWND, checked: bool) {
    w32::send(hwnd, BM_SETCHECK, checked as usize, 0);
}

/// 以非同步方式送出控制項通知，避免受測程式顯示對話框時把測試卡住。
fn notify(parent: HWND, id: i32, code: usize) {
    let target = control(parent, id);
    assert!(w32::post(parent, WM_COMMAND, (code << 16) | id as usize, target.0 as isize), "無法送出通知");
}

fn select_combo(parent: HWND, id: i32, index: usize) {
    w32::send(control(parent, id), CB_SETCURSEL, index, 0);
    notify(parent, id, CBN_SELCHANGE);
}

/// 受測程式目前顯示中的訊息方塊。
fn message_box(process_id: u32) -> Option<HWND> {
    top_window(process_id, "#32770").filter(|hwnd| is_visible(*hwnd))
}

fn is_enabled(hwnd: HWND) -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(hwnd) }.as_bool()
}

fn is_visible(hwnd: HWND) -> bool {
    unsafe { IsWindowVisible(hwnd).as_bool() }
}

/// 把視窗畫面存成 BMP（僅在設定 AIWAKE_UI_SHOTS 時）。
fn screenshot(hwnd: HWND, name: &str) {
    let Ok(directory) = std::env::var("AIWAKE_UI_SHOTS") else {
        return;
    };
    // 等待非同步的重繪完成。
    std::thread::sleep(Duration::from_millis(400));
    unsafe {
        let mut bounds = RECT::default();
        let _ = GetWindowRect(hwnd, &mut bounds);
        let (width, height) = (w32::width_of(&bounds), w32::height_of(&bounds));
        if width <= 0 || height <= 0 {
            return;
        }

        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let _ = PrintWindow(hwnd, memory, PRINT_WINDOW_FLAGS(2));
        SelectObject(memory, previous);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        GetDIBits(memory, bitmap, 0, height as u32, Some(pixels.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        let mut file = Vec::with_capacity(54 + pixels.len());
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&54u32.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&width.to_le_bytes());
        file.extend_from_slice(&(-height).to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&[0; 24]);
        file.extend_from_slice(&pixels);
        let _ = std::fs::create_dir_all(&directory);
        std::fs::write(Path::new(&directory).join(format!("{name}.bmp")), file).unwrap();
    }
}

fn load_jobs(data_directory: &Path) -> Vec<ScheduledJob> {
    JsonFileStore::<Vec<ScheduledJob>>::new(data_directory.join("schedules.json"), Vec::new).load().unwrap()
}

#[test]
fn main_window_editor_settings_and_tray() {
    let mut app = App {
        child: Command::new(env!("CARGO_BIN_EXE_ai-wake-scheduler"))
            .arg("--verify-ui")
            .spawn()
            .expect("無法啟動受測程式"),
    };
    let process_id = app.child.id();
    let long = Duration::from_secs(60);
    let short = Duration::from_secs(10);

    let window = wait_for("主視窗出現", long, || {
        top_window(process_id, main::WINDOW_CLASS).filter(|hwnd| is_visible(*hwnd))
    });
    assert!(text_of(window).contains("功能驗證"), "隔離模式應在標題標示功能驗證。");
    let editor = child_of_class(window, "AiWakeEditorPanel");
    let list = control(window, main::ID_LIST);
    let save_button = control(editor, main::ID_SAVE);
    let refresh_button = control(editor, main::ID_REFRESH_USAGE);

    // ── 編輯器預設值 ──
    assert_eq!(text_of(control(editor, main::ID_NAME)), "AI 倒數喚醒");
    assert_eq!(text_of(control(editor, main::ID_MESSAGE)), "早安");
    assert_eq!(text_of(save_button), "建立排程");
    assert_eq!(w32::send(control(editor, main::ID_RECURRENCE), CB_GETCOUNT, 0, 0), 3, "應提供三種排程模式。");
    assert_eq!(w32::send(control(editor, main::ID_RECURRENCE), CB_GETCURSEL, 0, 0), 0);
    for index in 0..CliKind::ALL.len() as i32 {
        assert!(is_checked(control(editor, main::ID_TARGET_BASE + index)), "新排程預設應勾選全部 CLI。");
    }
    assert!(is_checked(control(editor, main::ID_ENABLED)), "新排程預設應啟用。");
    assert_eq!(w32::send(list, LVM_GETITEMCOUNT, 0, 0), 0, "隔離資料目錄一開始不應有排程。");

    // 隔離資料目錄：工作目錄預設為資料目錄下的 workspace。
    let workspace = PathBuf::from(text_of(control(editor, main::ID_WORKING_DIRECTORY)));
    let data_directory = workspace.parent().expect("工作目錄應位於資料目錄之下").to_path_buf();
    assert!(
        data_directory.file_name().unwrap().to_string_lossy().starts_with("AiWakeScheduler-QA-"),
        "功能驗證模式必須使用隔離的資料目錄：{}",
        data_directory.display()
    );
    assert!(workspace.is_dir(), "工作目錄應已建立。");

    // 啟動時的額度讀取結束後，按鈕會恢復可用。
    wait_until("啟動時的額度讀取完成", long, || text_of(refresh_button) == "重新讀取倒數");
    screenshot(window, "01-main");

    // ── 建立排程（先取消啟用，避免真的送出訊息）──
    set_checked(control(editor, main::ID_ENABLED), false);
    set_text(control(editor, main::ID_NAME), "測試排程");
    select_combo(editor, main::ID_RECURRENCE, 2);
    notify(editor, main::ID_SAVE, BN_CLICKED);
    wait_until("排程出現在清單", short, || w32::send(list, LVM_GETITEMCOUNT, 0, 0) == 1);
    wait_until("儲存按鈕改為儲存修改", short, || text_of(save_button) == "儲存修改");

    let created = load_jobs(&data_directory);
    assert_eq!(created.len(), 1, "資料檔應有一筆排程。");
    assert_eq!(created[0].name, "測試排程");
    assert_eq!(created[0].recurrence, ScheduleRecurrence::Countdown);
    assert!(created[0].initial_time_of_day.is_some(), "自動倒數模式應保存開始偵測時間。");
    assert!(!created[0].enabled && created[0].status == ScheduleStatus::Disabled, "取消啟用的排程應為已停用。");
    assert_eq!(created[0].targets, CliKind::ALL);
    assert_eq!(created[0].message, "早安");

    // ── 修改同一筆排程 ──
    select_combo(editor, main::ID_RECURRENCE, 1);
    set_checked(control(editor, main::ID_TARGET_BASE), false);
    notify(editor, main::ID_SAVE, BN_CLICKED);
    wait_until("修改寫入資料檔", short, || {
        let jobs = load_jobs(&data_directory);
        jobs.len() == 1 && jobs[0].recurrence == ScheduleRecurrence::Interval
    });
    let modified = load_jobs(&data_directory);
    assert_eq!(modified[0].id, created[0].id, "儲存修改不應建立新排程。");
    assert_eq!(modified[0].targets, &CliKind::ALL[1..], "取消勾選的 CLI 不應保留在目標中。");
    assert!(!modified[0].enabled, "修改後仍應維持停用。");
    w32::send(editor, WM_VSCROLL, 7, 0);
    screenshot(window, "02-editor-bottom");
    w32::send(editor, WM_VSCROLL, 6, 0);

    // ── 關閉主視窗：預設縮到系統匣，程式繼續執行 ──
    w32::post(window, WM_CLOSE, 0, 0);
    wait_until("主視窗縮到系統匣", short, || !is_visible(window));
    assert!(app.child.try_wait().unwrap().is_none(), "縮到系統匣後程式應繼續執行。");
    w32::post(window, WM_COMMAND, main::ID_TRAY_OPEN as usize, 0);
    wait_until("由系統匣還原主視窗", short, || is_visible(window));

    // ── 設定對話框 ──
    notify(editor, main::ID_SETTINGS, BN_CLICKED);
    let dialog = wait_for("設定對話框出現", short, || {
        top_window(process_id, settings::WINDOW_CLASS).filter(|hwnd| is_visible(*hwnd))
    });
    let content = child_of_class(dialog, "AiWakeSettingsContent");
    let codex = CliKind::ALL.iter().position(|kind| *kind == CliKind::Codex).unwrap() as i32;
    let agy = CliKind::ALL.iter().position(|kind| *kind == CliKind::Antigravity).unwrap() as i32;
    let model = control(content, settings::ID_MODEL_BASE + codex);
    let effort = control(content, settings::ID_EFFORT_BASE + codex);
    let effort_count = |combo: HWND| w32::send(combo, CB_GETCOUNT, 0, 0);
    assert_eq!(text_of(model), "gpt-6-luna", "Codex 預設應為低價喚醒模型。");
    assert_eq!(text_of(control(content, settings::ID_PATH_BASE + codex)), "codex");
    assert!(is_checked(control(content, settings::ID_TRAY)) && is_checked(control(content, settings::ID_TOKEN_SAVER)));
    assert_eq!(text_of(control(content, settings::ID_TIMEOUT)), "3");
    assert_eq!(text_of(control(content, settings::ID_QUOTA_REFRESH)), "10");

    // 模型切換時，思考程度選單要跟著該模型支援的範圍調整。
    set_text(model, "gpt-6.1-sol");
    notify(content, settings::ID_MODEL_BASE + codex, CBN_EDITCHANGE);
    wait_until("Sol 提供 Ultra", short, || effort_count(effort) == 7);
    w32::send(effort, CB_SETCURSEL, 6, 0);
    assert!(text_of(effort).contains("Ultra"), "Sol 應可選擇 Ultra。");
    set_text(model, "gpt-6-luna");
    notify(content, settings::ID_MODEL_BASE + codex, CBN_EDITCHANGE);
    wait_until("Luna 不提供 Ultra", short, || effort_count(effort) == 6);
    assert!(text_of(effort).contains("Max"), "由 Sol Ultra 切到 Luna 應降為 Max，實際：{}", text_of(effort));

    set_text(control(content, settings::ID_MODEL_BASE + agy), "gemini-3.8-flash-high");
    notify(content, settings::ID_MODEL_BASE + agy, CBN_EDITCHANGE);
    wait_until("完整模型 ID 只保留預設程度", short, || {
        effort_count(control(content, settings::ID_EFFORT_BASE + agy)) == 1
    });
    screenshot(dialog, "03-settings");

    // 取消：不應改動任何設定。
    set_checked(control(content, settings::ID_TOKEN_SAVER), false);
    notify(dialog, settings::ID_CANCEL, BN_CLICKED);
    wait_until("取消後對話框關閉", short, || !unsafe { IsWindow(Some(dialog)) }.as_bool());
    assert!(!data_directory.join("settings.json").exists(), "取消設定不應寫入設定檔。");

    // 儲存：寫入隔離的設定檔。
    notify(editor, main::ID_SETTINGS, BN_CLICKED);
    let dialog = wait_for("設定對話框再次出現", short, || {
        top_window(process_id, settings::WINDOW_CLASS).filter(|hwnd| is_visible(*hwnd))
    });
    let content = child_of_class(dialog, "AiWakeSettingsContent");
    assert!(is_checked(control(content, settings::ID_TOKEN_SAVER)), "先前取消的變更不應殘留。");
    let model = control(content, settings::ID_MODEL_BASE + codex);
    let effort = control(content, settings::ID_EFFORT_BASE + codex);
    set_text(model, "gpt-6.1-sol");
    notify(content, settings::ID_MODEL_BASE + codex, CBN_EDITCHANGE);
    wait_until("Sol 提供 Ultra", short, || effort_count(effort) == 7);
    w32::send(effort, CB_SETCURSEL, 6, 0);
    set_text(model, "gpt-6-luna");
    notify(content, settings::ID_MODEL_BASE + codex, CBN_EDITCHANGE);
    wait_until("Luna 不提供 Ultra", short, || effort_count(effort) == 6);
    set_checked(control(content, settings::ID_TRAY), false);
    set_text(control(content, settings::ID_TIMEOUT), "7");
    // 把四個 CLI 都指向假的批次檔，之後的「立即執行」才不會呼叫真正的 CLI。
    let fake_cli = data_directory.join("fake-cli.cmd");
    let probe_marker = data_directory.join("fake-cli-probed.txt");
    std::fs::write(
        &fake_cli,
        "@echo off
echo PROBED >> \"%~dp0fake-cli-probed.txt\"
echo FAKE-CLI-OK
",
    )
    .unwrap();
    for index in 0..CliKind::ALL.len() as i32 {
        set_text(control(content, settings::ID_PATH_BASE + index), &fake_cli.to_string_lossy());
    }

    // 連線檢查：對輸入框中的（假）CLI 執行 --version，結束後按鈕恢復可用。
    let probe = control(content, settings::ID_PROBE);
    notify(content, settings::ID_PROBE, BN_CLICKED);
    wait_until("連線檢查完成", long, || probe_marker.is_file() && is_enabled(probe));
    screenshot(dialog, "03b-settings-probed");

    notify(dialog, settings::ID_OK, BN_CLICKED);
    wait_until("儲存後對話框關閉", short, || !unsafe { IsWindow(Some(dialog)) }.as_bool());

    let settings_path = data_directory.join("settings.json");
    wait_until("設定檔寫入", short, || settings_path.is_file());
    let saved = JsonFileStore::<AppSettings>::new(settings_path, AppSettings::create_default).load().unwrap();
    assert_eq!(saved.cli_profiles[&CliKind::Codex].model, "gpt-6-luna");
    assert_eq!(saved.cli_profiles[&CliKind::Codex].thinking_effort, ThinkingEffort::Max);
    assert!(!saved.minimize_to_tray && saved.token_saver_mode, "只有勾選狀態被更改的選項應改變。");
    assert_eq!(saved.execution_timeout_minutes, 7);
    assert!(!saved.start_with_windows, "隔離模式不得啟用正式的開機啟動。");
    assert!(saved.cli_profiles.values().all(|profile| Path::new(&profile.executable) == fake_cli));

    // ── 立即執行：確認對話框 → 平行執行 → 終端機輸出 → 結果 ──
    wait_until("設定儲存後的額度讀取完成", long, || text_of(refresh_button) == "重新讀取倒數");
    notify(window, main::ID_RUN_NOW, BN_CLICKED);
    let confirm = wait_for("立即執行的確認對話框", short, || message_box(process_id));
    let question = text_of(control(confirm, 0xFFFF));
    assert!(question.contains("Codex CLI") && question.contains("早安"), "確認訊息應列出目標與訊息：{question}");
    assert!(!question.contains("Antigravity (Gemini)"), "已取消勾選的 CLI 不應出現在確認訊息：{question}");
    w32::post(confirm, WM_COMMAND, 6, 0);

    wait_until("三個假 CLI 執行完成", long, || {
        let jobs = load_jobs(&data_directory);
        jobs[0].status == ScheduleStatus::Pending && jobs[0].last_results.len() == 3
    });
    let finished = load_jobs(&data_directory).remove(0);
    assert!(finished.enabled, "立即執行會啟用排程。");
    for result in &finished.last_results {
        assert!(result.succeeded && result.exit_code == Some(0), "{} 應成功：{}", result.cli, result.error);
        assert!(Path::new(&result.log_path).is_file(), "每次執行都應留下日誌。");
    }
    let terminal = control(window, main::ID_TERMINAL_OUTPUT);
    wait_until("終端機顯示執行過程", short, || {
        let output = text_of(terminal);
        output.matches("✓").count() == 3 && output.contains("FAKE-CLI-OK")
    });
    let output = text_of(terminal);
    assert!(output.contains("▶ Codex 開始執行，訊息：早安"), "終端機應顯示開始事件：{output}");
    screenshot(window, "04-after-run");

    notify(window, main::ID_SHOW_RESULT, BN_CLICKED);
    let results = wait_for("執行結果對話框", short, || message_box(process_id));
    let summary = text_of(control(results, 0xFFFF));
    assert!(summary.contains("Codex CLI：成功") && summary.contains("結束碼：0"), "結果摘要不正確：{summary}");
    w32::post(results, WM_CLOSE, 0, 0);
    wait_until("結果對話框關閉", short, || message_box(process_id).is_none());

    notify(window, main::ID_TERMINAL_CLEAR, BN_CLICKED);
    wait_until("終端機清除", short, || text_of(terminal).is_empty());

    // ── 新增：回到空白編輯狀態 ──
    notify(window, main::ID_NEW, BN_CLICKED);
    wait_until("編輯器重設", short, || text_of(save_button) == "建立排程");
    assert_eq!(text_of(control(editor, main::ID_NAME)), "AI 倒數喚醒");
    assert!(is_checked(control(editor, main::ID_TARGET_BASE)) && is_checked(control(editor, main::ID_ENABLED)));

    // 沒有選取排程時，刪除不做任何事。
    notify(window, main::ID_DELETE, BN_CLICKED);
    std::thread::sleep(Duration::from_millis(300));
    assert!(message_box(process_id).is_none(), "沒有選取排程時不應跳出刪除確認。");

    // ── 關閉縮到系統匣後，關閉主視窗就是結束程式 ──
    screenshot(window, "05-main-final");
    w32::post(window, WM_CLOSE, 0, 0);
    let status = wait_for("程式結束", Duration::from_secs(20), || app.child.try_wait().unwrap());
    assert!(status.success(), "程式應正常結束：{status:?}");

    let _ = std::fs::remove_dir_all(&data_directory);
}
