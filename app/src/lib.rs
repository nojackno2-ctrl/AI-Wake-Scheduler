//! AI 倒數喚醒桌面程式：egui GPU 儀表板與原生 Windows 系統匣。

pub mod board;
pub mod host;
pub mod main_window;
pub mod presenter;
pub mod settings_window;
pub mod startup;
pub mod terminal;
pub mod theme;
pub mod tray;
pub mod ui;
pub mod w32;
pub mod widgets;

use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    LoadIconW, LoadImageW, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
};

use crate::host::AppHost;
use crate::main_window::APP_TITLE;

const SINGLE_INSTANCE_MUTEX_NAME: &str = r"Local\AiWakeScheduler-9E3CCEC5-7BAC-4DEA-9687-BBB9E4982EB9";

fn has_flag(arguments: &[String], flag: &str) -> bool {
    arguments.iter().any(|argument| argument.eq_ignore_ascii_case(flag))
}

/// 內嵌於執行檔的應用程式圖示；取不到時回退至系統預設圖示。
fn application_icon() -> HICON {
    unsafe {
        // 資源編譯時圖示的識別碼為 1。
        match LoadImageW(
            Some(w32::hinstance()),
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        ) {
            Ok(handle) => HICON(handle.0),
            Err(_) => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
        }
    }
}

/// 程式進入點；回傳行程結束碼。
pub fn run() -> i32 {
    // 必須在啟動任何子程序之前設定：錯誤模式是在建立子程序時繼承下去的。
    aiwake_core::process::suppress_child_process_error_dialogs();

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let is_minimized = has_flag(&arguments, "--minimized");
    let verify_ui = has_flag(&arguments, "--verify-ui");

    // 功能驗證模式使用獨立的互斥名稱與資料目錄，不影響正式執行個體與資料。
    let mutex_name = if verify_ui {
        format!("{SINGLE_INSTANCE_MUTEX_NAME}-QA-{}", aiwake_core::Guid::new().simple())
    } else {
        SINGLE_INSTANCE_MUTEX_NAME.to_string()
    };
    let mutex_name = w32::wide(&mutex_name);
    let mutex = unsafe { CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())) };
    if mutex.is_err() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if !is_minimized {
            w32::message_box(None, "AI 倒數喚醒已經在執行。", APP_TITLE, w32::MB_OK | w32::MB_ICONINFORMATION);
        }
        return 0;
    }

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let startup_migration_error = if verify_ui { None } else { startup::migrate_legacy_if_needed() };
    let root_directory = verify_ui.then(|| {
        // 測試可預先建立隔離設定；只接受暫存目錄直屬的 QA 資料夾。
        let supplied = std::env::var_os("AIWAKE_VERIFY_ROOT")
            .and_then(|path| std::path::PathBuf::from(path).canonicalize().ok())
            .filter(|path| {
                path.parent() == std::env::temp_dir().canonicalize().ok().as_deref()
                    && path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("AiWakeScheduler-QA-"))
            });
        supplied
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("AiWakeScheduler-QA-{}", aiwake_core::Guid::new().simple()))
            })
            .to_string_lossy()
            .into_owned()
    });

    let host = match AppHost::create(root_directory.as_deref()) {
        Ok(host) => Arc::new(host),
        Err(error) => {
            w32::message_box(
                None,
                &format!("程式無法啟動：{}", error.message()),
                APP_TITLE,
                w32::MB_OK | w32::MB_ICONERROR,
            );
            return 1;
        }
    };

    if let Some(error) = startup_migration_error {
        w32::message_box(
            None,
            &format!("開機啟動設定未完成遷移：{error}"),
            APP_TITLE,
            w32::MB_OK | w32::MB_ICONWARNING,
        );
    }
    let result = main_window::run(Arc::clone(&host), application_icon(), is_minimized, verify_ui);
    host.manager.shutdown();
    if let Err(error) = result {
        w32::message_box(None, &format!("GPU 介面無法啟動：{error}"), APP_TITLE, w32::MB_OK | w32::MB_ICONERROR);
        return 1;
    }
    0
}
