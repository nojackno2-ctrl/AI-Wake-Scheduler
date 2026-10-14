//! 視窗之間共用的 UI 基礎設施：跨執行緒排回 UI 執行緒的工作佇列、訊息迴圈、
//! 標準控制項的建立與存取、垂直捲動，以及檔案／資料夾選擇對話框。

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, SYSTEMTIME, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FILEOPENDIALOGOPTIONS, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetAncestor, GetMessageW, IsDialogMessageW, PostQuitMessage, TranslateMessage, GA_ROOT, MSG,
    SB_VERT, SCROLLINFO, SCROLLINFO_MASK, WM_APP,
};

use crate::w32::{self, wide};

/// 有背景工作排回 UI 執行緒時送出的訊息。
pub const WM_APP_DISPATCH: u32 = WM_APP + 1;

pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_GROUP: u32 = 0x0002_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
pub const WS_EX_CONTROLPARENT: u32 = 0x0001_0000;

pub const BN_CLICKED: u32 = 0;
pub const EN_CHANGE: u32 = 0x0300;
pub const CBN_SELCHANGE: u32 = 1;
pub const CBN_EDITCHANGE: u32 = 5;

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCOUNT: u32 = 0x0146;
const CB_GETCURSEL: u32 = 0x0147;
const CB_GETLBTEXT: u32 = 0x0148;
const CB_GETLBTEXTLEN: u32 = 0x0149;
const CB_RESETCONTENT: u32 = 0x014B;
const CB_SETCURSEL: u32 = 0x014E;
const EM_SETLIMITTEXT: u32 = 0x00C5;
const DTM_GETSYSTEMTIME: u32 = 0x1001;
const DTM_SETSYSTEMTIME: u32 = 0x1002;
const DTM_SETFORMATW: u32 = 0x1032;
const UDM_SETBUDDY: u32 = 0x0469;
const UDM_SETRANGE32: u32 = 0x046F;
const UDM_SETPOS32: u32 = 0x0471;
const UDM_GETPOS32: u32 = 0x0472;

type Task<T> = Box<dyn FnOnce(&T) + Send>;

/// 把背景執行緒的結果排回 UI 執行緒處理。
/// 背景工作只碰這個佇列，實際的畫面更新一律在 UI 執行緒上發生。
pub struct UiQueue<T> {
    hwnd: isize,
    tasks: Arc<Mutex<Vec<Task<T>>>>,
}

impl<T> Clone for UiQueue<T> {
    fn clone(&self) -> Self {
        Self { hwnd: self.hwnd, tasks: Arc::clone(&self.tasks) }
    }
}

impl<T> UiQueue<T> {
    pub fn new(hwnd: HWND) -> Self {
        Self { hwnd: hwnd.0 as isize, tasks: Arc::new(Mutex::new(Vec::new())) }
    }

    /// 任何執行緒都可以呼叫；視窗已關閉時工作會被直接丟棄。
    pub fn post(&self, task: impl FnOnce(&T) + Send + 'static) {
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(task));
        w32::post(HWND(self.hwnd as *mut _), WM_APP_DISPATCH, 0, 0);
    }

    /// 在 UI 執行緒上執行所有排隊中的工作。
    pub fn drain(&self, state: &T) {
        let tasks = std::mem::take(&mut *self.tasks.lock().unwrap_or_else(|e| e.into_inner()));
        for task in tasks {
            task(state);
        }
    }
}

/// 執行訊息迴圈直到 `done` 成立或收到 WM_QUIT；回傳是否因 WM_QUIT 結束。
pub fn pump_until(done: impl Fn() -> bool) -> bool {
    let mut message = MSG::default();
    loop {
        if done() {
            return false;
        }
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 <= 0 {
            if result.0 == 0 {
                // 巢狀迴圈收到結束訊息時要重新送出，讓外層迴圈也能結束。
                unsafe { PostQuitMessage(message.wParam.0 as i32) };
            }
            return true;
        }
        unsafe {
            // 讓 Tab／方向鍵／Enter／Esc 在控制項之間以對話框慣例運作。
            let root = GetAncestor(message.hwnd, GA_ROOT);
            if !root.is_invalid() && IsDialogMessageW(root, &message).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

pub fn create_edit(parent: HWND, id: i32, max_length: usize) -> HWND {
    const ES_AUTOHSCROLL: u32 = 0x0080;
    let edit = w32::create_window(
        WS_EX_CLIENTEDGE,
        "EDIT",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        id,
        std::ptr::null(),
    );
    if max_length > 0 {
        w32::send(edit, EM_SETLIMITTEXT, max_length, 0);
    }
    edit
}

pub fn create_combo(parent: HWND, id: i32, editable: bool) -> HWND {
    const CBS_DROPDOWN: u32 = 0x0002;
    const CBS_DROPDOWNLIST: u32 = 0x0003;
    const CBS_AUTOHSCROLL: u32 = 0x0040;
    w32::create_window(
        0,
        "COMBOBOX",
        "",
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            | CBS_AUTOHSCROLL
            | if editable { CBS_DROPDOWN } else { CBS_DROPDOWNLIST },
        w32::rect(0, 0, 10, 300),
        Some(parent),
        id,
        std::ptr::null(),
    )
}

pub fn create_checkbox(parent: HWND, id: i32, text: &str) -> HWND {
    const BS_AUTOCHECKBOX: u32 = 0x0003;
    w32::create_window(
        0,
        "BUTTON",
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        id,
        std::ptr::null(),
    )
}

/// 只顯示時與分、以上下鍵調整的時間選擇器。
pub fn create_time_picker(parent: HWND, id: i32) -> HWND {
    const DTS_UPDOWN: u32 = 0x0001;
    const DTS_TIMEFORMAT: u32 = 0x0009;
    let picker = w32::create_window(
        0,
        "SysDateTimePick32",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | DTS_UPDOWN | DTS_TIMEFORMAT,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        id,
        std::ptr::null(),
    );
    let format = wide("HH:mm");
    w32::send(picker, DTM_SETFORMATW, 0, format.as_ptr() as isize);
    picker
}

pub fn time_picker_get(picker: HWND) -> (u32, u32) {
    let mut value = SYSTEMTIME::default();
    w32::send(picker, DTM_GETSYSTEMTIME, 0, &mut value as *mut _ as isize);
    (value.wHour as u32, value.wMinute as u32)
}

pub fn time_picker_set(picker: HWND, hour: u32, minute: u32) {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let value = SYSTEMTIME {
        wYear: today.year() as u16,
        wMonth: today.month() as u16,
        wDay: today.day() as u16,
        wHour: hour.min(23) as u16,
        wMinute: minute.min(59) as u16,
        ..Default::default()
    };
    w32::send(picker, DTM_SETSYSTEMTIME, 0, &value as *const _ as isize);
}

/// 數字輸入框：一個只接受數字的編輯框加上上下鍵。回傳 `(edit, updown)`。
pub fn create_number(parent: HWND, id: i32, minimum: i32, maximum: i32) -> (HWND, HWND) {
    const ES_NUMBER: u32 = 0x2000;
    const ES_AUTOHSCROLL: u32 = 0x0080;
    const UDS_SETBUDDYINT: u32 = 0x0002;
    const UDS_ALIGNRIGHT: u32 = 0x0004;
    const UDS_ARROWKEYS: u32 = 0x0020;
    const UDS_NOTHOUSANDS: u32 = 0x0080;

    let edit = w32::create_window(
        WS_EX_CLIENTEDGE,
        "EDIT",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_NUMBER | ES_AUTOHSCROLL,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        id,
        std::ptr::null(),
    );
    let updown = w32::create_window(
        0,
        "msctls_updown32",
        "",
        WS_CHILD | WS_VISIBLE | UDS_SETBUDDYINT | UDS_ALIGNRIGHT | UDS_ARROWKEYS | UDS_NOTHOUSANDS,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        0,
        std::ptr::null(),
    );
    w32::send(updown, UDM_SETBUDDY, edit.0 as usize, 0);
    w32::send(updown, UDM_SETRANGE32, minimum as usize, maximum as isize);
    (edit, updown)
}

/// 移動數字輸入框後重新把上下鍵貼回編輯框右側。
pub fn number_move(edit: HWND, updown: HWND, bounds: &windows::Win32::Foundation::RECT) {
    w32::move_window(edit, bounds);
    w32::send(updown, UDM_SETBUDDY, edit.0 as usize, 0);
}

pub fn number_set(updown: HWND, value: i32) {
    w32::send(updown, UDM_SETPOS32, 0, value as isize);
}

pub fn number_get(updown: HWND) -> i32 {
    w32::send(updown, UDM_GETPOS32, 0, 0) as i32
}

pub fn is_checked(checkbox: HWND) -> bool {
    w32::send(checkbox, BM_GETCHECK, 0, 0) == 1
}

pub fn set_checked(checkbox: HWND, checked: bool) {
    w32::send(checkbox, BM_SETCHECK, checked as usize, 0);
}

pub fn combo_clear(combo: HWND) {
    w32::send(combo, CB_RESETCONTENT, 0, 0);
}

pub fn combo_add(combo: HWND, text: &str) {
    let text = wide(text);
    w32::send(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
}

pub fn combo_count(combo: HWND) -> i32 {
    w32::send(combo, CB_GETCOUNT, 0, 0) as i32
}

pub fn combo_selected(combo: HWND) -> i32 {
    w32::send(combo, CB_GETCURSEL, 0, 0) as i32
}

pub fn combo_select(combo: HWND, index: i32) {
    w32::send(combo, CB_SETCURSEL, index as usize, 0);
}

/// 可輸入的下拉選單在調整大小後會自動全選文字；沒有焦點時把選取範圍收回開頭，
/// 文字才不會被捲到尾端或以選取色顯示。
pub fn combo_reset_edit_selection(combo: HWND) {
    const CB_SETEDITSEL: u32 = 0x0142;
    let focused = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
    let has_focus = !focused.is_invalid()
        && unsafe { windows::Win32::UI::WindowsAndMessaging::GetParent(focused) }.is_ok_and(|parent| parent == combo);
    if !has_focus {
        w32::send(combo, CB_SETEDITSEL, 0, 0);
    }
}

pub fn combo_item_text(combo: HWND, index: i32) -> String {
    let length = w32::send(combo, CB_GETLBTEXTLEN, index as usize, 0);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    w32::send(combo, CB_GETLBTEXT, index as usize, buffer.as_mut_ptr() as isize);
    w32::from_wide(&buffer)
}

/// 可捲動面板的垂直捲動狀態。
#[derive(Default)]
pub struct VScroll {
    position: Cell<i32>,
    content: Cell<i32>,
    viewport: Cell<i32>,
}

impl VScroll {
    pub fn position(&self) -> i32 {
        self.position.get()
    }

    fn maximum(&self) -> i32 {
        (self.content.get() - self.viewport.get()).max(0)
    }

    /// 內容或可見高度改變後更新捲軸；回傳捲動位置是否被調整。
    pub fn update(&self, hwnd: HWND, content: i32, viewport: i32) -> bool {
        const SIF_RANGE: u32 = 0x1;
        const SIF_PAGE: u32 = 0x2;
        const SIF_POS: u32 = 0x4;

        self.content.set(content);
        self.viewport.set(viewport);
        let previous = self.position.get();
        let position = previous.clamp(0, self.maximum());
        self.position.set(position);

        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SCROLLINFO_MASK(SIF_RANGE | SIF_PAGE | SIF_POS),
            nMin: 0,
            nMax: (content - 1).max(0),
            nPage: viewport.max(0) as u32,
            nPos: position,
            nTrackPos: 0,
        };
        unsafe {
            SetScrollInfo(hwnd, SB_VERT, &info, true);
        }
        position != previous
    }

    fn scroll_to(&self, position: i32) -> bool {
        let position = position.clamp(0, self.maximum());
        if position == self.position.get() {
            false
        } else {
            self.position.set(position);
            true
        }
    }

    /// 處理 WM_VSCROLL；回傳位置是否改變。
    pub fn on_scroll(&self, wparam: WPARAM, line: i32) -> bool {
        let position = self.position.get();
        let target = match w32::lo_word(wparam.0) {
            0 => position - line,                   // SB_LINEUP
            1 => position + line,                   // SB_LINEDOWN
            2 => position - self.viewport.get(),    // SB_PAGEUP
            3 => position + self.viewport.get(),    // SB_PAGEDOWN
            4 | 5 => w32::hi_word(wparam.0) as i32, // SB_THUMBPOSITION / SB_THUMBTRACK
            6 => 0,                                 // SB_TOP
            7 => self.maximum(),                    // SB_BOTTOM
            _ => position,
        };
        self.scroll_to(target)
    }

    /// 處理 WM_MOUSEWHEEL；回傳位置是否改變。
    pub fn on_wheel(&self, wparam: WPARAM, line: i32) -> bool {
        let delta = w32::hi_word(wparam.0) as u16 as i16 as i32;
        self.scroll_to(self.position.get() - delta * line * 3 / 120)
    }
}

fn shell_item_path(item: &IShellItem) -> Option<String> {
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        path
    }
}

fn open_dialog(
    owner: HWND,
    title: &str,
    options: u32,
    filters: &[(&str, &str)],
    initial_folder: Option<&str>,
) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let title = wide(title);
        let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));
        if let Ok(current) = dialog.GetOptions() {
            let _ = dialog.SetOptions(FILEOPENDIALOGOPTIONS(current.0 | options));
        }

        let buffers: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(name, spec)| (wide(name), wide(spec))).collect();
        if !buffers.is_empty() {
            let specs: Vec<COMDLG_FILTERSPEC> = buffers
                .iter()
                .map(|(name, spec)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            let _ = dialog.SetFileTypes(&specs);
        }

        if let Some(folder) = initial_folder.filter(|folder| std::path::Path::new(folder).is_dir()) {
            let folder = wide(folder);
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(folder.as_ptr()), None) {
                let _ = dialog.SetFolder(&item);
            }
        }

        dialog.Show(Some(owner)).ok()?;
        shell_item_path(&dialog.GetResult().ok()?)
    }
}

/// 選擇資料夾；使用者取消時回傳 None。
pub fn pick_folder(owner: HWND, title: &str, initial_folder: &str) -> Option<String> {
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    open_dialog(owner, title, FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM, &[], Some(initial_folder))
}

/// 選擇既有的檔案；使用者取消時回傳 None。
pub fn pick_file(owner: HWND, title: &str, filters: &[(&str, &str)]) -> Option<String> {
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const FOS_FILEMUSTEXIST: u32 = 0x1000;
    open_dialog(owner, title, FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST, filters, None)
}

/// 取得 WM_COMMAND 的控制項識別碼與通知碼。
pub fn command_parts(wparam: WPARAM, lparam: LPARAM) -> (i32, u32, HWND) {
    (w32::lo_word(wparam.0) as i32, w32::hi_word(wparam.0), HWND(lparam.0 as *mut _))
}
