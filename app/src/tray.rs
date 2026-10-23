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

/// 原生外殼只處理系統匣；所有畫面由 eframe 視窗繪製。
pub mod shell {
    use crate::w32;
    use eframe::egui;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    };
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub const WINDOW_CLASS: &str = "AiWakeTrayHost";
    pub const OPEN: usize = 200;
    pub const LOGS: usize = 201;
    pub const EXIT: usize = 202;
    pub const EXPORT: usize = 203;
    pub const UPDATES: usize = 204;
    #[derive(Debug, Clone, Copy)]
    pub enum Action {
        Open,
        Logs,
        Exit,
        Export,
        Updates,
    }
    struct State {
        sender: Sender<Action>,
        ctx: egui::Context,
        icon: HICON,
        main_hwnd: HWND,
        exit_requested: Arc<AtomicBool>,
        taskbar_created: u32,
    }

    pub struct Shell {
        hwnd: HWND,
        state: Box<State>,
    }
    impl Shell {
        pub fn new(
            ctx: egui::Context,
            icon: HICON,
            main_hwnd: HWND,
            exit_requested: Arc<AtomicBool>,
        ) -> Option<(Self, Receiver<Action>)> {
            w32::register_class(WINDOW_CLASS, Some(window_proc), 0, icon);
            let hwnd = w32::create_window(
                0,
                WINDOW_CLASS,
                "AI 倒數喚醒系統匣",
                0,
                w32::rect(0, 0, 0, 0),
                None,
                0,
                std::ptr::null(),
            );
            if hwnd.0.is_null() {
                return None;
            }
            let (sender, receiver) = mpsc::channel();
            let state = Box::new(State {
                sender,
                ctx,
                icon,
                main_hwnd,
                exit_requested,
                taskbar_created: unsafe { RegisterWindowMessageW(PCWSTR(w32::wide("TaskbarCreated").as_ptr())) },
            });
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&*state as *const State) as isize);
            }
            let _ = super::add(hwnd, icon, crate::main_window::APP_TITLE);
            Some((Self { hwnd, state }, receiver))
        }
        pub fn notify(&self, text: &str, warning: bool) {
            super::balloon(self.hwnd, crate::main_window::APP_TITLE, text, warning);
        }
        pub fn alive(&self) -> bool {
            unsafe { IsWindow(Some(self.hwnd)).as_bool() }
        }
    }
    impl Drop for Shell {
        fn drop(&mut self) {
            super::remove(self.hwnd);
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.hwnd);
            }
            // state 必須活到視窗銷毀之後。
            let _ = &self.state;
        }
    }
    unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        if !state.is_null() {
            let state = &*state;
            if message == state.taskbar_created {
                super::add(hwnd, state.icon, crate::main_window::APP_TITLE);
                return LRESULT(0);
            }
            let command = if message == WM_COMMAND {
                Some(wparam.0 & 0xffff)
            } else if message == super::WM_TRAY {
                match lparam.0 as u32 & 0xffff {
                    WM_LBUTTONDBLCLK | super::NIN_SELECT | super::NIN_KEYSELECT => Some(OPEN),
                    super::NIN_BALLOONUSERCLICK => Some(OPEN),
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        if let Ok(menu) = CreatePopupMenu() {
                            for (id, label) in [
                                (OPEN, "開啟主視窗"),
                                (LOGS, "開啟日誌資料夾"),
                                (EXPORT, "匯出除錯日誌…"),
                                (UPDATES, "檢查 CLI 更新…"),
                                (EXIT, "結束程式"),
                            ] {
                                let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(w32::wide(label).as_ptr()));
                            }
                            let mut point = POINT::default();
                            let _ = GetCursorPos(&mut point);
                            let _ = SetForegroundWindow(hwnd);
                            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, Some(0), hwnd, None);
                            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                            let _ = DestroyMenu(menu);
                        }
                        None
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(action) = command.and_then(|id| match id {
                OPEN => Some(Action::Open),
                LOGS => Some(Action::Logs),
                EXIT => Some(Action::Exit),
                EXPORT => Some(Action::Export),
                UPDATES => Some(Action::Updates),
                _ => None,
            }) {
                match action {
                    Action::Open | Action::Export | Action::Updates => {
                        let _ = ShowWindow(state.main_hwnd, SW_RESTORE);
                        let _ = ShowWindow(state.main_hwnd, SW_SHOW);
                        let _ = SetForegroundWindow(state.main_hwnd);
                    }
                    Action::Exit => {
                        state.exit_requested.store(true, Ordering::SeqCst);
                        let _ = PostMessageW(Some(state.main_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    Action::Logs => {}
                }
                let _ = state.sender.send(action);
                state.ctx.request_repaint();
                return LRESULT(0);
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}
