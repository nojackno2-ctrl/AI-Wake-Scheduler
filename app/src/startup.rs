//! 以目前使用者 Run key 管理開機啟動，並遷移舊版排程工作與啟動捷徑。

use std::os::windows::process::CommandExt;
use std::process::Command;

use aiwake_core::process::CREATE_NO_WINDOW;
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};

use crate::w32::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "AI倒數喚醒";
const TASK_NAME: &str = "AI倒數喚醒";
const LEGACY_SHORTCUT_NAMES: [&str; 2] = ["AI 倒數喚醒.lnk", "AI倒數喚醒.lnk"];

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// 保留既有啟動偏好；只有成功移除舊排程後才建立 Run key，避免重複啟動。
/// 遷移未完成時回傳錯誤訊息：仍讓程式啟動，但不能悄悄當作成功。
pub fn migrate_legacy_if_needed() -> Option<String> {
    let migrate = || -> Result<(), String> {
        if has_legacy_task()? || is_enabled() {
            set_enabled(true)?;
        }
        Ok(())
    };
    migrate().err()
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if has_legacy_task()? {
        run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
    }

    let subkey = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut handle = HKEY::default();
    let opened = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            None,
            &mut handle,
            None,
        )
    };
    if opened != ERROR_SUCCESS {
        return Err("無法開啟目前使用者的開機啟動設定。".to_string());
    }
    let key = Key(handle);

    if enabled {
        let command = wide(&build_startup_command()?);
        let bytes: Vec<u8> = command.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        let written = unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) };
        if written != ERROR_SUCCESS {
            return Err("無法寫入開機啟動設定。".to_string());
        }
    } else {
        // 值不存在時刪除會失敗，屬正常狀態。
        unsafe {
            let _ = RegDeleteValueW(key.0, PCWSTR(name.as_ptr()));
        }
    }

    cleanup_startup_shortcuts();
    Ok(())
}

/// 讀取 Run key 中目前登記的啟動命令。
pub fn registered_command() -> Option<String> {
    let subkey = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut handle = HKEY::default();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()), None, KEY_QUERY_VALUE, &mut handle)
            != ERROR_SUCCESS
        {
            return None;
        }
        let key = Key(handle);

        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        if RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, Some(&mut kind), None, Some(&mut size)) != ERROR_SUCCESS
            || kind != REG_SZ
            || size == 0
        {
            return None;
        }

        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        if RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(buffer.as_mut_ptr() as *mut u8),
            Some(&mut size),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        Some(crate::w32::from_wide(&buffer))
    }
}

pub fn is_enabled() -> bool {
    registered_command().is_some_and(|command| !command.trim().is_empty())
}

struct ToolOutput {
    exit_code: i32,
    error: String,
}

/// schtasks 以主控台字碼頁輸出，必須依 OEM 字碼頁解碼才讀得到本地化訊息。
fn decode_console(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        let length = MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None);
        if length <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut buffer = vec![0u16; length as usize];
        MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut buffer));
        String::from_utf16_lossy(&buffer)
    }
}

fn schtasks(arguments: &[&str]) -> std::io::Result<ToolOutput> {
    let output = Command::new("schtasks.exe").args(arguments).creation_flags(CREATE_NO_WINDOW).output()?;
    Ok(ToolOutput { exit_code: output.status.code().unwrap_or(-1), error: decode_console(&output.stderr) })
}

fn has_legacy_task() -> Result<bool, String> {
    let output = schtasks(&["/Query", "/TN", TASK_NAME]).map_err(|_| "無法檢查舊版開機啟動工作。".to_string())?;
    if output.exit_code == 0 {
        return Ok(true);
    }
    // 不存在為正常狀態；拒絕存取不能當成不存在，避免建立重複啟動項目。
    if output.error.to_ascii_lowercase().contains("denied") || output.error.contains("拒絕") {
        return Err("無法存取舊版開機啟動工作，請以管理員身分執行一次程式完成遷移。".to_string());
    }
    Ok(false)
}

fn run_schtasks(arguments: &[&str]) -> Result<(), String> {
    let output = schtasks(arguments).map_err(|_| "無法啟動 schtasks.exe 設定自動啟動工作。".to_string())?;
    if output.exit_code != 0 {
        return Err(format!(
            "設定 Windows 工作排程器自動啟動工作失敗（結束碼 {}）：{}",
            output.exit_code,
            output.error.trim()
        ));
    }
    Ok(())
}

fn cleanup_startup_shortcuts() {
    let Ok(app_data) = std::env::var("APPDATA") else {
        return;
    };
    let startup_folder = std::path::Path::new(&app_data).join(r"Microsoft\Windows\Start Menu\Programs\Startup");
    for shortcut_name in LEGACY_SHORTCUT_NAMES {
        // 忽略非關鍵啟動資料夾捷徑清理失敗
        let _ = std::fs::remove_file(startup_folder.join(shortcut_name));
    }
}

/// Run key 的標準 Windows 命令列；路徑含空白時仍正確引用。
pub fn build_startup_command() -> Result<String, String> {
    let path = std::env::current_exe().map_err(|_| "無法取得目前程式路徑。".to_string())?;
    Ok(startup_command_for(&path.to_string_lossy()))
}

pub fn startup_command_for(executable: &str) -> String {
    format!("\"{executable}\" --minimized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_the_path() {
        assert_eq!(
            startup_command_for(r"C:\Program Files\AI 倒數喚醒\AI倒數喚醒.exe"),
            "\"C:\\Program Files\\AI 倒數喚醒\\AI倒數喚醒.exe\" --minimized"
        );
    }

    #[test]
    fn console_output_decoding_handles_ascii_and_empty() {
        assert_eq!(decode_console(b""), "");
        assert_eq!(decode_console(b"ERROR: Access is denied."), "ERROR: Access is denied.");
    }
}
