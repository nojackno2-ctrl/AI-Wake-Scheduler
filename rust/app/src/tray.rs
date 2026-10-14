//! 系統匣圖示與通知。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4, NOTIFY_ICON_INFOTIP_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{HICON, WM_APP};

/// 系統匣圖示的回呼訊息。
pub const WM_TRAY: u32 = WM_APP + 2;

/// 使用者點了氣球通知。
pub const NIN_BALLOONUSERCLICK: u32 = 0x0405;
/// 氣球通知逾時或被關閉。
pub const NIN_BALLOONTIMEOUT: u32 = 0x0404;
pub const NIN_BALLOONHIDE: u32 = 0x0403;
pub const NIN_SELECT: u32 = 0x0400;
pub const NIN_KEYSELECT: u32 = 0x0401;

const TRAY_ID: u32 = 1;

fn copy_text(target: &mut [u16], text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let count = units.len().min(target.len() - 1);
    target[..count].copy_from_slice(&units[..count]);
    target[count] = 0;
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

pub fn add(hwnd: HWND, icon: HICON, tip: &str) -> bool {
    let mut icon_data = data(hwnd);
    icon_data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    icon_data.uCallbackMessage = WM_TRAY;
    icon_data.hIcon = icon;
    copy_text(&mut icon_data.szTip, tip);
    unsafe {
        if !Shell_NotifyIconW(NIM_ADD, &icon_data).as_bool() {
            return false;
        }
        icon_data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &icon_data);
    }
    true
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd));
    }
}

/// 顯示氣球通知。
pub fn balloon(hwnd: HWND, title: &str, text: &str, warning: bool) {
    let mut icon_data = data(hwnd);
    icon_data.uFlags = NIF_INFO;
    copy_text(&mut icon_data.szInfoTitle, title);
    copy_text(&mut icon_data.szInfo, text);
    icon_data.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO } | NOTIFY_ICON_INFOTIP_FLAGS(0);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data);
    }
}
