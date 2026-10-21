//! 主視窗：排程清單、排程編輯器、額度看板、即時終端機與系統匣。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use aiwake_core::time::{self, Moment, Span};
use aiwake_core::usage::is_counting_down;
use aiwake_core::{
    calculator, catalog, locator, login, usage, CancelToken, CliKind, CliRunner, CliUsageAvailability,
    CliUsageSnapshot, CoreError, CoreResult, Guid, ScheduleRecurrence, ScheduleStatus, ScheduledJob, UsageReader,
};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetMonitorInfoW, MonitorFromWindow, SetBkMode, HBRUSH, HDC, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, TRANSPARENT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::Controls::{
    ImageList_Create, ImageList_Destroy, DRAWITEMSTRUCT, HIMAGELIST, ILC_COLOR32, LIST_VIEW_ITEM_FLAGS,
    LIST_VIEW_ITEM_STATE_FLAGS, LVCOLUMNW, LVCOLUMNW_MASK, LVITEMW, NMHDR, NMLVCUSTOMDRAW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, GetCursorPos, IsIconic, IsWindowVisible, KillTimer,
    LoadCursorW, PostQuitMessage, RegisterWindowMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos,
    ShowWindow, TrackPopupMenu, HICON, IDC_HAND, IDC_SIZENS, MF_SEPARATOR, MF_STRING, MINMAXINFO, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, TPM_RIGHTBUTTON, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU,
    WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM, WM_ERASEBKGND, WM_GETMINMAXINFO,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_NOTIFY, WM_PAINT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SIZE, WM_TIMER, WM_VSCROLL,
};

use crate::board::UsageBoard;
use crate::host::AppHost;
use crate::presenter;
use crate::settings_window;
use crate::startup;
use crate::terminal::Terminal;
use crate::theme::{self, palette, ButtonVariant, Fonts};
use crate::tray;
use crate::ui::{
    self, UiQueue, VScroll, BN_CLICKED, WM_APP_DISPATCH, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE,
    WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use crate::w32::{
    self, draw_text, wide, DT_CENTER, DT_LEFT, DT_LINE, DT_NOPREFIX, DT_RIGHT, DT_WORDBREAK, WM_MOUSELEAVE,
};

pub const WINDOW_CLASS: &str = "AiWakeMainWindow";
const EDITOR_CLASS: &str = "AiWakeEditorPanel";
pub const APP_TITLE: &str = "AI 倒數喚醒";

pub const ID_LIST: i32 = 100;
pub const ID_NEW: i32 = 101;
pub const ID_RUN_NOW: i32 = 102;
pub const ID_SHOW_RESULT: i32 = 103;
pub const ID_DELETE: i32 = 104;
pub const ID_OPEN_LOGS: i32 = 105;
pub const ID_EXPORT_LOGS: i32 = 106;
pub const ID_NAME: i32 = 110;
pub const ID_TIME: i32 = 112;
pub const ID_MESSAGE: i32 = 113;
pub const ID_WORKING_DIRECTORY: i32 = 114;
pub const ID_BROWSE: i32 = 115;
pub const ID_TARGET_BASE: i32 = 120;
pub const ID_ENABLED: i32 = 130;
pub const ID_REFRESH_USAGE: i32 = 131;
pub const ID_SAVE: i32 = 132;
pub const ID_SETTINGS: i32 = 133;
pub const ID_TERMINAL_CLEAR: i32 = 140;
pub const ID_TERMINAL_AUTO_SCROLL: i32 = 141;
pub const ID_TERMINAL_OUTPUT: i32 = 142;
pub const ID_TRAY_OPEN: i32 = 200;
pub const ID_TRAY_LOGS: i32 = 201;
pub const ID_TRAY_EXIT: i32 = 202;
pub const ID_TRAY_EXPORT_LOGS: i32 = 203;

const WM_APP_REFRESH: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;
const TIMER_CLOCK: usize = 1;
const TIMER_TERMINAL: usize = 2;

const COLUMN_COUNTDOWN: usize = 2;
const COLUMN_HEADERS: [&str; 5] = ["名稱", "時間 / 週期", "倒數", "CLI", "狀態"];

const LVM_SETIMAGELIST: u32 = 0x1003;
const LVM_GETITEMCOUNT: u32 = 0x1004;
const LVM_DELETEITEM: u32 = 0x1008;
const LVM_GETNEXTITEM: u32 = 0x100C;
const LVM_SETCOLUMNWIDTH: u32 = 0x101E;
const LVM_SETITEMSTATE: u32 = 0x102B;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_INSERTITEMW: u32 = 0x104D;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const LVM_SETITEMTEXTW: u32 = 0x1074;
const LVN_ITEMCHANGED: u32 = 0xFFFF_FF9B;
const NM_DBLCLK: u32 = 0xFFFF_FFFD;
const NM_CUSTOMDRAW: u32 = 0xFFFF_FFF4;
const CDDS_PREPAINT: u32 = 0x1;
const CDDS_ITEMPREPAINT: u32 = 0x1_0001;
const CDRF_NEWFONT: isize = 0x2;
const CDRF_NOTIFYITEMDRAW: isize = 0x20;

const EMPTY_STATE_TEXT: &str = "目前沒有排程\r\n選擇下方的「新增」，建立每天自動喚醒。";
const HEADER_SUBTITLE: &str =
    "自動模式：開始時間後每 10 分鐘偵測倒數，沒有倒數就向勾選的 CLI 傳送簡短訊息喚醒配額（預設：早安）";
const EDITOR_NOTE: &str = "自動模式從開始時間起每 10 分鐘讀取所有 CLI 的倒數，未倒數即呼叫，倒數中或額度已用盡則等待。節省 Token 模式會停用工具、MCP 伺服器與專案說明檔，並以最低推理量要求最短回覆，把每次喚醒的用量壓到接近下限。";
/// 內容區至少這麼寬時改用三欄（清單｜排程內容｜額度看板）。
const WIDE_LAYOUT_WIDTH: i32 = 940;
/// 編輯面板至少這麼寬時「排程內容」與「額度看板」左右並排。
const EDITOR_TWO_COLUMN_WIDTH: i32 = 680;
/// 內容區至少這麼寬、且高度放得下時改用大螢幕版面：右欄上下堆疊排程內容與額度看板，
/// 左欄上下分配排程清單與終端機。
const LARGE_LAYOUT_WIDTH: i32 = 2000;
/// 介面設計時的基準工作區大小；視窗比這更大時整體放大（最多 1.5 倍），維持相同比例。
const DESIGN_CLIENT_SIZE: (i32, i32) = (1440, 860);
const MAX_ZOOM: f64 = 1.5;
const USAGE_TITLE: &str = "剩餘流量與重置倒數";
const TERMINAL_TITLE: &str = "即時終端輸出";

/// 視窗所在螢幕的工作區。
pub fn work_area(hwnd: HWND) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return info.rcWork;
        }
    }
    w32::rect(0, 0, 1280, 800)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Horizontal,
}

#[derive(Clone, Copy)]
enum FontKind {
    Caption,
    SectionTitle,
    TableHeader,
}

struct Label {
    bounds: RECT,
    text: String,
    font: FontKind,
    color: COLORREF,
    wrap: bool,
}

/// 主視窗自繪區域的位置。
#[derive(Default, Clone, Copy)]
struct FrameLayout {
    header: RECT,
    left_panel: RECT,
    list_title: RECT,
    list_area: RECT,
    horizontal_splitter: RECT,
    terminal_title: RECT,
    status: RECT,
}

/// 編輯面板自繪區域的位置。
#[derive(Default)]
struct EditorLayout {
    labels: Vec<Label>,
    /// 白底卡片（排程內容、額度看板）。
    cards: Vec<RECT>,
    board_box: RECT,
    board_origin: (i32, i32),
    board_width: i32,
}

pub struct MainWindow {
    hwnd: HWND,
    editor: HWND,
    host: Arc<AppHost>,
    fonts: RefCell<Rc<Fonts>>,
    /// 螢幕實際 DPI；`fonts` 的 DPI 為此值乘上依視窗大小決定的放大倍率。
    base_dpi: Cell<i32>,
    queue: UiQueue<MainWindow>,
    icon: HICON,
    title: String,
    start_minimized: bool,
    taskbar_created: u32,

    list: HWND,
    list_buttons: [HWND; 6],
    name_input: HWND,
    time_input: HWND,
    message_input: HWND,
    working_directory_input: HWND,
    browse_button: HWND,
    target_checks: Vec<(CliKind, HWND)>,
    enabled_check: HWND,
    refresh_usage_button: HWND,
    save_button: HWND,
    settings_button: HWND,
    terminal_clear_button: HWND,
    terminal_auto_scroll: HWND,
    terminal: Terminal,
    canvas_brush: HBRUSH,
    panel_brush: HBRUSH,

    jobs: RefCell<Vec<ScheduledJob>>,
    rows: RefCell<Vec<[String; 5]>>,
    usage_snapshots: RefCell<HashMap<CliKind, CliUsageSnapshot>>,
    board: RefCell<UsageBoard>,
    editing_id: Cell<Option<Guid>>,
    really_exit: Cell<bool>,
    refreshing_selection: Cell<bool>,
    refresh_requested: Arc<AtomicBool>,
    refresh_deferred: Cell<bool>,
    last_countdown_end_refresh: RefCell<HashMap<CliKind, Moment>>,
    login_notified: RefCell<HashSet<CliKind>>,
    login_in_progress: RefCell<HashSet<CliKind>>,
    balloon_login_kind: Cell<Option<CliKind>>,
    last_clock_text: RefCell<String>,
    last_usage_refresh_time: Cell<Option<Moment>>,
    usage_refresh_generation: Cell<u64>,
    usage_refresh_cancel: RefCell<Option<CancelToken>>,
    status_text: RefCell<String>,
    status_color: Cell<COLORREF>,

    frame: Cell<FrameLayout>,
    editor_layout: RefCell<EditorLayout>,
    editor_scroll: VScroll,
    in_editor_layout: Cell<bool>,
    /// 編輯面板是否強制上下堆疊（大螢幕版面）。
    editor_stacked: Cell<bool>,
    /// 使用者拖曳後的終端機高度；None 表示使用預設。
    terminal_height: Cell<Option<i32>>,
    drag: Cell<Drag>,
    tray_added: Cell<bool>,
    clock_running: Cell<bool>,
}

fn register_classes(icon: HICON) {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    if REGISTERED.replace(true) {
        return;
    }
    w32::register_class(WINDOW_CLASS, Some(window_proc), 0, icon);
    w32::register_class(EDITOR_CLASS, Some(editor_proc), 0, HICON::default());
}

/// 建立主視窗。`start_minimized` 為 true 時只顯示系統匣圖示。
pub fn create(host: Arc<AppHost>, icon: HICON, start_minimized: bool, title_suffix: &str) -> Option<HWND> {
    register_classes(icon);
    let title = format!("{APP_TITLE}{title_suffix}");

    let hwnd = w32::create_window(
        WS_EX_CONTROLPARENT,
        WINDOW_CLASS,
        &title,
        WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
        w32::rect(0, 0, 1480, 900),
        None,
        0,
        std::ptr::null(),
    );
    if hwnd.is_invalid() {
        return None;
    }

    // 依目前螢幕工作區決定視窗大小並置中，避免在小螢幕或高 DPI 下超出畫面。
    let fonts = Rc::new(Fonts::new(w32::dpi_of(hwnd)));
    let work = work_area(hwnd);
    let width = fonts.scale(1480).min(w32::width_of(&work));
    let height = fonts.scale(900).min(w32::height_of(&work));
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            work.left + (w32::width_of(&work) - width) / 2,
            work.top + (w32::height_of(&work) - height) / 2,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    let list = create_list(hwnd);
    let list_buttons = [
        theme::create_button(hwnd, ID_NEW, "新增", ButtonVariant::Primary),
        theme::create_button(hwnd, ID_RUN_NOW, "立即執行", ButtonVariant::Secondary),
        theme::create_button(hwnd, ID_SHOW_RESULT, "查看結果", ButtonVariant::Secondary),
        theme::create_button(hwnd, ID_DELETE, "刪除", ButtonVariant::Danger),
        theme::create_button(hwnd, ID_OPEN_LOGS, "開啟日誌資料夾", ButtonVariant::Secondary),
        theme::create_button(hwnd, ID_EXPORT_LOGS, "匯出除錯日誌…", ButtonVariant::Secondary),
    ];

    let editor = w32::create_window(
        WS_EX_CONTROLPARENT,
        EDITOR_CLASS,
        "",
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN,
        w32::rect(0, 0, 10, 10),
        Some(hwnd),
        0,
        std::ptr::null(),
    );
    let name_input = ui::create_edit(editor, ID_NAME, 0);
    let time_input = ui::create_time_picker(editor, ID_TIME);
    let message_input = ui::create_edit(editor, ID_MESSAGE, 50);
    let working_directory_input = ui::create_edit(editor, ID_WORKING_DIRECTORY, 0);
    let browse_button = theme::create_button(editor, ID_BROWSE, "瀏覽…", ButtonVariant::Secondary);
    // CLI 勾選項直接由目錄產生，新增 CLI 不需要改動視窗程式碼。
    let target_checks: Vec<(CliKind, HWND)> = catalog::all()
        .iter()
        .enumerate()
        .map(|(index, descriptor)| {
            (descriptor.kind, ui::create_checkbox(editor, ID_TARGET_BASE + index as i32, descriptor.display_name))
        })
        .collect();
    let enabled_check = ui::create_checkbox(editor, ID_ENABLED, "啟用此排程");
    let refresh_usage_button = theme::create_button(editor, ID_REFRESH_USAGE, "重新讀取倒數", ButtonVariant::Secondary);
    let save_button = theme::create_button(editor, ID_SAVE, "建立排程", ButtonVariant::Primary);
    let settings_button = theme::create_button(editor, ID_SETTINGS, "CLI 設定…", ButtonVariant::Secondary);

    let terminal_clear_button = theme::create_button(hwnd, ID_TERMINAL_CLEAR, "清除", ButtonVariant::Secondary);
    let terminal_auto_scroll = ui::create_checkbox(hwnd, ID_TERMINAL_AUTO_SCROLL, "自動捲動");
    ui::set_checked(terminal_auto_scroll, true);
    let terminal = Terminal::create(hwnd, ID_TERMINAL_OUTPUT);

    let taskbar_created = unsafe {
        let name = wide("TaskbarCreated");
        RegisterWindowMessageW(PCWSTR(name.as_ptr()))
    };

    let state = Box::new(MainWindow {
        hwnd,
        editor,
        host,
        base_dpi: Cell::new(fonts.dpi),
        fonts: RefCell::new(fonts),
        queue: UiQueue::new(hwnd),
        icon,
        title,
        start_minimized,
        taskbar_created,
        list,
        list_buttons,
        name_input,
        time_input,
        message_input,
        working_directory_input,
        browse_button,
        target_checks,
        enabled_check,
        refresh_usage_button,
        save_button,
        settings_button,
        terminal_clear_button,
        terminal_auto_scroll,
        terminal,
        canvas_brush: w32::solid_brush(palette().canvas),
        panel_brush: w32::solid_brush(palette().panel),
        jobs: RefCell::new(Vec::new()),
        rows: RefCell::new(Vec::new()),
        usage_snapshots: RefCell::new(HashMap::new()),
        board: RefCell::new(UsageBoard::new()),
        editing_id: Cell::new(None),
        really_exit: Cell::new(false),
        refreshing_selection: Cell::new(false),
        refresh_requested: Arc::new(AtomicBool::new(false)),
        refresh_deferred: Cell::new(false),
        last_countdown_end_refresh: RefCell::new(HashMap::new()),
        login_notified: RefCell::new(HashSet::new()),
        login_in_progress: RefCell::new(HashSet::new()),
        balloon_login_kind: Cell::new(None),
        last_clock_text: RefCell::new(String::new()),
        last_usage_refresh_time: Cell::new(None),
        usage_refresh_generation: Cell::new(0),
        usage_refresh_cancel: RefCell::new(None),
        status_text: RefCell::new("就緒".to_string()),
        status_color: Cell::new(palette().control_text),
        frame: Cell::new(FrameLayout::default()),
        editor_layout: RefCell::new(EditorLayout::default()),
        editor_scroll: VScroll::default(),
        in_editor_layout: Cell::new(false),
        editor_stacked: Cell::new(false),
        terminal_height: Cell::new(None),
        drag: Cell::new(Drag::None),
        tray_added: Cell::new(false),
        clock_running: Cell::new(false),
    });
    let state_pointer = Box::into_raw(state);
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_USERDATA};
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_pointer as isize);
        SetWindowLongPtrW(editor, GWLP_USERDATA, state_pointer as isize);
    }
    let window: &MainWindow = unsafe { &*state_pointer };
    window.start();
    Some(hwnd)
}

fn create_list(parent: HWND) -> HWND {
    const LVS_REPORT: u32 = 0x0001;
    const LVS_SINGLESEL: u32 = 0x0004;
    const LVS_SHOWSELALWAYS: u32 = 0x0008;
    const LVS_EX_FULLROWSELECT: isize = 0x0020;
    const LVS_EX_DOUBLEBUFFER: isize = 0x1_0000;
    const LVCF_FMT: u32 = 0x1;
    const LVCF_WIDTH: u32 = 0x2;
    const LVCF_TEXT: u32 = 0x4;

    let list = w32::create_window(
        WS_EX_CLIENTEDGE,
        "SysListView32",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        ID_LIST,
        std::ptr::null(),
    );
    // 雙緩衝：整列重繪時的閃爍幾乎都來自這裡。
    w32::send(list, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);

    for (index, header) in COLUMN_HEADERS.iter().enumerate() {
        let mut text = w32::WideBuffer::new(header);
        let column = LVCOLUMNW {
            mask: LVCOLUMNW_MASK(LVCF_FMT | LVCF_WIDTH | LVCF_TEXT),
            cx: 100,
            pszText: text.as_pwstr(),
            ..Default::default()
        };
        w32::send(list, LVM_INSERTCOLUMNW, index, &column as *const _ as isize);
    }
    list
}

impl MainWindow {
    fn fonts(&self) -> Rc<Fonts> {
        Rc::clone(&self.fonts.borrow())
    }

    fn manager(&self) -> &aiwake_core::ScheduleManager {
        &self.host.manager
    }

    fn start(&self) {
        self.apply_fonts();
        self.reset_editor();

        // 排程異動可能連續觸發數次（多個 CLI 平行完成），合併成一次實際更新。
        let refresh_requested = Arc::clone(&self.refresh_requested);
        let hwnd = self.hwnd.0 as isize;
        self.manager().on_jobs_changed(move || {
            if !refresh_requested.swap(true, Ordering::SeqCst) {
                w32::post(HWND(hwnd as *mut _), WM_APP_REFRESH, 0, 0);
            }
        });
        let queue = self.queue.clone();
        self.manager().on_background_error(move |error| {
            let message = format!("背景錯誤：{}", error.message());
            queue.post(move |window: &MainWindow| window.set_status(&message, palette().danger));
        });
        // 事件可能來自背景執行緒（子程序輸出讀取迴圈）；這裡只做執行緒安全的排隊。
        let pending = self.terminal.pending.clone();
        self.manager().on_activity_logged(move |activity| pending.push(activity.clone()));

        self.tray_added.set(tray::add(self.hwnd, self.icon, APP_TITLE));
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_TERMINAL, 150, None);
        }

        self.layout_frame();
        self.refresh_grid();

        if self.start_minimized {
            self.trim_working_set();
        } else {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            }
            self.start_clock();
            self.refresh_usage(false);
        }
    }

    fn apply_fonts(&self) {
        let fonts = self.fonts();
        let mut controls = vec![
            self.list,
            self.name_input,
            self.time_input,
            self.message_input,
            self.working_directory_input,
            self.browse_button,
            self.enabled_check,
            self.refresh_usage_button,
            self.save_button,
            self.settings_button,
            self.terminal_clear_button,
            self.terminal_auto_scroll,
        ];
        controls.extend(self.list_buttons);
        controls.extend(self.target_checks.iter().map(|(_, check)| *check));
        for control in controls {
            w32::set_font(control, &fonts.body);
        }
        w32::set_font(self.terminal.output, &fonts.mono);

        // 以高度為列高的影像清單撐開清單列高。
        unsafe {
            let image_list = ImageList_Create(1, fonts.scale(34), ILC_COLOR32, 1, 0);
            let previous = w32::send(self.list, LVM_SETIMAGELIST, 1, image_list.0);
            if previous != 0 {
                let _ = ImageList_Destroy(Some(HIMAGELIST(previous)));
            }
        }
    }

    // ── 版面 ──────────────────────────────────────────────────────

    /// 擺放主視窗的子控制項並記下自繪區域。
    ///
    /// 欄位依寬度自動調整：夠寬時排程清單、排程內容、額度看板排成三欄；
    /// 較窄時清單移到上方，排程內容與額度看板在下方並排。終端機預設只拿
    /// 上方內容用剩的高度，讓所有資訊盡量在同一頁看完。
    fn layout_frame(&self) {
        let fonts = self.fonts();
        let s = |value: i32| fonts.scale(value);
        let client = w32::client_rect(self.hwnd);
        let (width, height) = (w32::width_of(&client), w32::height_of(&client));
        if width <= 0 || height <= 0 {
            return;
        }

        let header_height = fonts.header_title.height() + s(18);
        let status_height = fonts.body.height() + s(12);
        let splitter = s(6);
        let button_height = theme::button_height(&fonts);
        let pad = s(12);
        let gap = s(12);

        let content_top = header_height;
        let content_height = (height - header_height - status_height).max(0);
        let content_width = (width - pad * 2).max(0);

        let toolbar_height = button_height + s(8);
        let minimum_terminal = toolbar_height + s(56);
        let top = content_top + pad;
        let column_bottom = content_top + content_height - s(8);

        let large_editor_width = ((content_width as f64 * 0.3) as i32).clamp(s(560), s(760));
        let large = content_width >= s(LARGE_LAYOUT_WIDTH)
            && self.arrange_editor(large_editor_width, 0, None, true).1 <= column_bottom - top;
        let wide = content_width >= s(WIDE_LAYOUT_WIDTH);
        let editor_width = if large {
            large_editor_width
        } else if wide {
            ((content_width as f64 * 0.64) as i32).clamp(s(EDITOR_TWO_COLUMN_WIDTH), s(1080))
        } else {
            content_width
        };
        let list_width = (content_width - gap - editor_width).max(0);
        let list_minimum = s(180);

        // 終端機：預設取上方內容放完後剩下的高度，但至少保留幾行輸出。
        // 大螢幕版面中終端機只在左欄，預設佔左欄約四成五。
        let available_height = (column_bottom - top - splitter).max(0);
        let editor_needed = if large { 0 } else { self.arrange_editor(editor_width, 0, None, false).1 };
        let needed = if wide { editor_needed } else { editor_needed + gap + list_minimum };
        let default_terminal = if large {
            ((available_height as f64 * 0.45) as i32).max(minimum_terminal)
        } else {
            (available_height - needed).clamp(minimum_terminal, (available_height / 2).max(minimum_terminal))
        };
        let terminal_height = self.terminal_height.get().unwrap_or(default_terminal).clamp(
            minimum_terminal.min(available_height),
            (available_height - s(240)).max(minimum_terminal).min(available_height),
        );
        let top_height = (available_height - terminal_height).max(0);

        let (left_panel, editor_bounds) = if large {
            (
                w32::rect(pad, top, list_width, top_height),
                w32::rect(pad + list_width + gap, top, editor_width, column_bottom - top),
            )
        } else if wide {
            (
                w32::rect(pad, top, list_width, top_height),
                w32::rect(pad + list_width + gap, top, editor_width, top_height),
            )
        } else {
            let list_height = (top_height - gap - editor_needed).max(list_minimum.min(top_height));
            let editor_top = top + list_height + gap;
            (
                w32::rect(pad, top, content_width, list_height),
                w32::rect(pad, editor_top, content_width, (top + top_height - editor_top).max(0)),
            )
        };
        let stacked_changed = self.editor_stacked.replace(large) != large;
        w32::move_window(self.editor, &editor_bounds);
        if stacked_changed {
            self.layout_editor();
        }

        // 左側面板：標題、清單與按鈕列。
        let panel_pad = s(16);
        let panel_inner = RECT {
            left: left_panel.left + panel_pad,
            top: left_panel.top + panel_pad,
            right: left_panel.right - panel_pad,
            bottom: left_panel.bottom - panel_pad,
        };
        let list_title =
            w32::rect(panel_inner.left, panel_inner.top, w32::width_of(&panel_inner), fonts.section_title.height());

        // 按鈕由左而右排列，寬度不夠時換行。
        let button_gap = s(6);
        let mut positions = Vec::with_capacity(self.list_buttons.len());
        let (mut x, mut row) = (0, 0);
        for button in self.list_buttons {
            let button_width = theme::button_width(&fonts, &w32::get_text(button));
            if x > 0 && x + button_width > w32::width_of(&panel_inner) {
                x = 0;
                row += 1;
            }
            positions.push((button, x, row, button_width));
            x += button_width + button_gap;
        }
        let button_rows = row + 1;
        let buttons_height = button_rows * button_height + (button_rows - 1) * button_gap;
        let buttons_top = panel_inner.bottom - buttons_height;
        for (button, x, row, button_width) in positions {
            w32::move_window(
                button,
                &w32::rect(
                    panel_inner.left + x,
                    buttons_top + row * (button_height + button_gap),
                    button_width,
                    button_height,
                ),
            );
        }

        let list_area = RECT {
            left: panel_inner.left,
            top: list_title.bottom + s(12),
            right: panel_inner.right,
            bottom: (buttons_top - s(12)).max(list_title.bottom + s(12)),
        };
        w32::move_window(self.list, &list_area);
        self.resize_columns();

        // 終端機：工具列與輸出。
        let (terminal_left, terminal_width) = if large { (pad, list_width) } else { (pad, content_width) };
        let horizontal_splitter = if large {
            w32::rect(terminal_left, top + top_height, terminal_width, splitter)
        } else {
            w32::rect(0, top + top_height, width, splitter)
        };
        let toolbar_top = horizontal_splitter.bottom;
        let title_width = fonts.table_header.measure(TERMINAL_TITLE).0 + s(16);
        let terminal_title = w32::rect(terminal_left, toolbar_top, title_width, toolbar_height);
        let clear_width = theme::button_width(&fonts, "清除");
        w32::move_window(
            self.terminal_clear_button,
            &w32::rect(terminal_title.right, toolbar_top + s(4), clear_width, button_height),
        );
        let check_width = fonts.body.measure("自動捲動").0 + s(28);
        w32::move_window(
            self.terminal_auto_scroll,
            &w32::rect(terminal_title.right + clear_width + s(12), toolbar_top + s(4), check_width, button_height),
        );
        let output_top = toolbar_top + toolbar_height;
        w32::move_window(
            self.terminal.output,
            &w32::rect(terminal_left, output_top, terminal_width, (column_bottom - output_top).max(0)),
        );

        self.frame.set(FrameLayout {
            header: w32::rect(0, 0, width, header_height),
            left_panel,
            list_title,
            list_area,
            horizontal_splitter,
            terminal_title,
            status: w32::rect(0, height - status_height, width, status_height),
        });
        w32::invalidate(self.hwnd);
    }

    /// 清單欄寬依清單寬度自動分配：多出的寬度依權重分配，寬度不夠時各欄等比例縮小。
    fn resize_columns(&self) {
        let fonts = self.fonts();
        let preferred = [135, 115, 100, 125, 80].map(|value| fonts.scale(value));
        let total: i32 = preferred.iter().sum();
        // 工作區寬度已扣除外框與捲軸。
        let available = w32::width_of(&w32::client_rect(self.list)).max(0);
        let mut widths = preferred;
        if available >= total {
            // 多出的寬度依權重分給各欄，倒數與 CLI 欄拿得最多。
            const WEIGHTS: [i32; 5] = [2, 1, 3, 3, 1];
            let extra = available - total;
            let weight_sum: i32 = WEIGHTS.iter().sum();
            for (width, weight) in widths.iter_mut().zip(WEIGHTS) {
                *width += extra * weight / weight_sum;
            }
        } else {
            let minimum = fonts.scale(48);
            widths = preferred.map(|value| (value * available / total).max(minimum));
        }
        for (index, column_width) in widths.iter().enumerate() {
            w32::send(self.list, LVM_SETCOLUMNWIDTH, index, *column_width as isize);
        }
    }

    /// 重排主視窗與編輯面板；額度看板內容高度改變時使用。
    fn relayout(&self) {
        self.layout_frame();
        self.layout_editor();
    }

    /// 擺放編輯面板的控制項，並記下要自繪的文字、卡片與看板位置。
    fn layout_editor(&self) {
        if self.in_editor_layout.replace(true) {
            return;
        }
        let client = w32::client_rect(self.editor);
        let offset = self.editor_scroll.position();
        let (layout, content_height) = self.arrange_editor(
            w32::width_of(&client),
            offset,
            Some(w32::height_of(&client)),
            self.editor_stacked.get(),
        );
        *self.editor_layout.borrow_mut() = layout;
        let scrolled = self.editor_scroll.update(self.editor, content_height, w32::height_of(&client));
        self.in_editor_layout.set(false);
        if scrolled {
            // 內容變短使捲動位置被拉回時，依新的位置重排一次。
            self.layout_editor();
            return;
        }
        w32::invalidate(self.editor);
    }

    /// 計算編輯面板的版面與內容總高度。`viewport` 為 None 時只量測、不移動控制項。
    ///
    /// 寬度足夠時「排程內容」與「額度看板」左右並排，否則上下堆疊；
    /// CLI 勾選框與額度看板也會依可用寬度自動分欄。
    fn arrange_editor(&self, width: i32, offset: i32, viewport: Option<i32>, stacked: bool) -> (EditorLayout, i32) {
        let fonts = self.fonts();
        let colors = palette();
        let s = |value: i32| fonts.scale(value);
        let apply = viewport.is_some();
        let place = |control: HWND, bounds: RECT| {
            if apply {
                w32::move_window(control, &bounds);
            }
        };

        let gap = s(12);
        let card_pad = s(16);
        // 預留捲軸寬度的容差，避免出現捲軸後又退回堆疊而來回切換。
        let two_columns = !stacked && width >= s(EDITOR_TWO_COLUMN_WIDTH - 24);
        let form_width = if two_columns { (((width - gap) as f64 * 0.46) as i32).clamp(s(330), s(500)) } else { width };
        let input_height = fonts.body.height() + s(10);
        let check_height = fonts.body.height() + s(6);
        let button_height = theme::button_height(&fonts);
        let section_height = fonts.section_title.height();
        let mut labels: Vec<Label> = Vec::new();

        // ── 排程內容 ──
        let top = -offset;
        let left = card_pad;
        let right = (form_width - card_pad).max(left + s(120));
        let mut y = top + card_pad;
        labels.push(Label {
            bounds: w32::rect(left, y, right - left, section_height),
            text: "排程內容".to_string(),
            font: FontKind::SectionTitle,
            color: colors.primary_text,
            wrap: false,
        });
        y += section_height + s(12);

        let label_width = ["名稱", "開始時間", "訊息", "工作目錄", "傳送至"]
            .iter()
            .map(|text| fonts.table_header.measure(text).0)
            .max()
            .unwrap_or(0)
            + s(14);
        let control_left = left + label_width;
        let control_width = (right - control_left).max(s(80));
        let row_label = |labels: &mut Vec<Label>, text: &str, top: i32, height: i32| {
            labels.push(Label {
                bounds: w32::rect(left, top, label_width, height),
                text: text.to_string(),
                font: FontKind::TableHeader,
                color: colors.primary_text,
                wrap: false,
            });
        };

        for (label, control) in [("名稱", self.name_input), ("開始時間", self.time_input), ("訊息", self.message_input)]
        {
            row_label(&mut labels, label, y, input_height);
            place(control, w32::rect(control_left, y, control_width, input_height));
            y += input_height + s(8);
        }

        row_label(&mut labels, "工作目錄", y, input_height);
        let browse_width = theme::button_width(&fonts, "瀏覽…");
        place(
            self.working_directory_input,
            w32::rect(control_left, y, (control_width - browse_width - s(6)).max(s(40)), input_height),
        );
        place(
            self.browse_button,
            w32::rect(control_left + control_width - browse_width, y, browse_width, input_height),
        );
        y += input_height + s(10);

        // CLI 勾選框排成格狀，欄數依寬度決定。
        row_label(&mut labels, "傳送至", y, check_height);
        let check_width = self
            .target_checks
            .iter()
            .map(|(_, check)| fonts.body.measure(&w32::get_text(*check)).0 + s(30))
            .max()
            .unwrap_or(s(120));
        let check_columns = (control_width / check_width.max(1)).clamp(1, self.target_checks.len().max(1) as i32);
        let cell_width = control_width / check_columns;
        let check_rows = (self.target_checks.len() as i32 + check_columns - 1) / check_columns;
        for (index, (_, check)) in self.target_checks.iter().enumerate() {
            let (column, row) = (index as i32 % check_columns, index as i32 / check_columns);
            place(
                *check,
                w32::rect(
                    control_left + column * cell_width,
                    y + row * (check_height + s(2)),
                    cell_width,
                    check_height,
                ),
            );
        }
        y += check_rows * (check_height + s(2)) + s(6);

        // 「啟用此排程」放得下時與按鈕同列，否則自成一列。
        let save_width = theme::button_width(&fonts, "建立排程").max(theme::button_width(&fonts, "儲存修改"));
        let settings_width = theme::button_width(&fonts, "CLI 設定…");
        let enabled_width = fonts.body.measure(&w32::get_text(self.enabled_check)).0 + s(30);
        let enabled_left = left + save_width + s(6) + settings_width + s(16);
        if enabled_left + enabled_width <= right {
            place(
                self.enabled_check,
                w32::rect(enabled_left, y + (button_height - check_height) / 2, enabled_width, check_height),
            );
        } else {
            place(self.enabled_check, w32::rect(control_left, y, control_width, check_height));
            y += check_height + s(12);
        }
        place(self.save_button, w32::rect(left, y, save_width, button_height));
        place(self.settings_button, w32::rect(left + save_width + s(6), y, settings_width, button_height));
        y += button_height + s(12);

        let note_width = right - left;
        let note_height = fonts.caption.measure_wrapped(EDITOR_NOTE, note_width);
        labels.push(Label {
            bounds: w32::rect(left, y, note_width, note_height),
            text: EDITOR_NOTE.to_string(),
            font: FontKind::Caption,
            color: colors.secondary_text,
            wrap: true,
        });
        y += note_height;
        let form_bottom = y + card_pad;

        // ── 額度看板 ──
        let (board_left, board_top, board_card_width) =
            if two_columns { (form_width + gap, top, width - form_width - gap) } else { (0, form_bottom + gap, width) };
        let board_inner_left = board_left + card_pad;
        let board_inner_right = (board_left + board_card_width - card_pad).max(board_inner_left + s(120));
        let mut y = board_top + card_pad;

        let refresh_text = w32::get_text(self.refresh_usage_button);
        let refresh_width = theme::button_width(&fonts, "重新讀取倒數").max(theme::button_width(&fonts, &refresh_text));
        let title_row_height = section_height.max(button_height);
        labels.push(Label {
            bounds: w32::rect(
                board_inner_left,
                y,
                (board_inner_right - board_inner_left - refresh_width - s(8)).max(0),
                title_row_height,
            ),
            text: USAGE_TITLE.to_string(),
            font: FontKind::SectionTitle,
            color: colors.primary_text,
            wrap: false,
        });
        place(self.refresh_usage_button, w32::rect(board_inner_right - refresh_width, y, refresh_width, button_height));
        y += title_row_height + s(12);

        let board_width = board_inner_right - board_inner_left;
        let board_height = self.board.borrow().measure(&fonts, board_width);
        let board_origin = (board_inner_left, y);
        y += board_height;
        let board_bottom = y + card_pad;

        // 卡片延伸到可見區域底部，左右並排時兩張卡片等高。
        let content_bottom = form_bottom.max(board_bottom);
        let fill_bottom = viewport.map_or(content_bottom, |height| content_bottom.max(height));
        let (form_card, board_card) = if two_columns {
            (
                w32::rect(0, top, form_width, fill_bottom - top),
                w32::rect(board_left, top, board_card_width, fill_bottom - top),
            )
        } else {
            (w32::rect(0, top, width, form_bottom - top), w32::rect(0, board_top, width, fill_bottom - board_top))
        };

        let layout = EditorLayout {
            labels,
            cards: vec![form_card, board_card],
            board_box: board_card,
            board_origin,
            board_width,
        };
        (layout, content_bottom + offset)
    }

    // ── 繪製 ──────────────────────────────────────────────────────

    fn paint_frame(&self, dc: HDC, bounds: &RECT) {
        let fonts = self.fonts();
        let colors = palette();
        let s = |value: i32| fonts.scale(value);
        let frame = self.frame.get();
        w32::fill(dc, bounds, colors.canvas);

        // 標題列：主標題與說明並排在同一行。
        w32::fill(dc, &frame.header, colors.banner);
        let title_width = fonts.header_title.measure(APP_TITLE).0;
        let header_inner = RECT { left: s(24), right: frame.header.right - s(24), ..frame.header };
        draw_text(
            dc,
            &fonts.header_title,
            APP_TITLE,
            &RECT { right: header_inner.left + title_width, ..header_inner },
            colors.banner_text,
            DT_LINE | DT_LEFT,
        );
        draw_text(
            dc,
            &fonts.body,
            HEADER_SUBTITLE,
            &RECT { left: header_inner.left + title_width + s(18), ..header_inner },
            colors.banner_subtitle,
            DT_LINE | DT_LEFT,
        );

        // 已儲存排程
        w32::fill(dc, &frame.left_panel, colors.panel);
        draw_text(dc, &fonts.section_title, "已儲存排程", &frame.list_title, colors.primary_text, DT_LINE | DT_LEFT);
        if self.jobs.borrow().is_empty() {
            w32::frame(dc, &frame.list_area, colors.divider);
            let text_height = fonts.body.measure_wrapped(EMPTY_STATE_TEXT, w32::width_of(&frame.list_area));
            let top = frame.list_area.top + (w32::height_of(&frame.list_area) - text_height) / 2;
            draw_text(
                dc,
                &fonts.body,
                EMPTY_STATE_TEXT,
                &w32::rect(frame.list_area.left, top, w32::width_of(&frame.list_area), text_height),
                colors.secondary_text,
                DT_WORDBREAK | DT_NOPREFIX | DT_CENTER,
            );
        }

        draw_text(
            dc,
            &fonts.table_header,
            TERMINAL_TITLE,
            &frame.terminal_title,
            colors.primary_text,
            DT_LINE | DT_LEFT,
        );

        // 狀態列
        w32::fill(dc, &frame.status, colors.panel);
        w32::line(dc, frame.status.left, frame.status.top, frame.status.right, frame.status.top, colors.divider);
        let status_inner = RECT { left: frame.status.left + s(14), right: frame.status.right - s(14), ..frame.status };
        let clock = self.last_clock_text.borrow();
        let clock_width = fonts.body.measure(&clock).0;
        draw_text(dc, &fonts.body, &clock, &status_inner, colors.secondary_text, DT_LINE | DT_RIGHT);
        draw_text(
            dc,
            &fonts.body,
            &self.status_text.borrow(),
            &RECT { right: status_inner.right - clock_width - s(16), ..status_inner },
            self.status_color.get(),
            DT_LINE | DT_LEFT,
        );
    }

    fn paint_editor(&self, dc: HDC, bounds: &RECT) {
        let fonts = self.fonts();
        let colors = palette();
        w32::fill(dc, bounds, colors.canvas);

        let (board_origin, board_width) = {
            let layout = self.editor_layout.borrow();
            for card in &layout.cards {
                w32::fill(dc, card, colors.panel);
            }
            for label in &layout.labels {
                let font = match label.font {
                    FontKind::Caption => &fonts.caption,
                    FontKind::SectionTitle => &fonts.section_title,
                    FontKind::TableHeader => &fonts.table_header,
                };
                let format = if label.wrap { DT_WORDBREAK | DT_NOPREFIX } else { DT_LINE | DT_LEFT };
                draw_text(dc, font, &label.text, &label.bounds, label.color, format);
            }
            (layout.board_origin, layout.board_width)
        };

        self.board.borrow_mut().paint(dc, &fonts, board_origin.0, board_origin.1, board_width);
    }

    fn invalidate_status(&self) {
        w32::invalidate_rect(self.hwnd, &self.frame.get().status);
    }

    fn set_status(&self, text: &str, color: COLORREF) {
        *self.status_text.borrow_mut() = text.to_string();
        self.status_color.set(color);
        self.invalidate_status();
    }

    fn show_error(&self, message: &str) {
        self.set_status(message, palette().danger);
        w32::message_box(Some(self.hwnd), message, &self.title, w32::MB_OK | w32::MB_ICONERROR);
    }

    // ── 排程清單更新 ──────────────────────────────────────────────

    fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() && !IsIconic(self.hwnd).as_bool() }
    }

    fn on_refresh_requested(&self) {
        self.refresh_requested.store(false, Ordering::SeqCst);

        // 縮在系統匣時不需要維護看不見的清單，只記下它已經過期。
        if !self.is_visible() {
            self.refresh_deferred.set(true);
            return;
        }
        self.refresh_grid();
    }

    fn refresh_grid(&self) {
        let Ok(jobs) = self.manager().get_jobs() else {
            return;
        };
        self.usage_snapshots.borrow_mut().extend(self.host.usage_reader.latest_snapshots());

        self.refresh_deferred.set(false);
        self.update_status_summary(&jobs);
        self.apply_jobs(jobs);
        self.update_usage_labels(&time::now());
    }

    fn selected_index(&self) -> Option<usize> {
        const LVNI_SELECTED: isize = 0x2;
        let index = w32::send(self.list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED);
        (index >= 0).then_some(index as usize)
    }

    fn selected_job(&self) -> Option<ScheduledJob> {
        self.selected_index().and_then(|index| self.jobs.borrow().get(index).cloned())
    }

    fn set_cell(&self, row: usize, column: usize, text: &str) {
        let mut buffer = w32::WideBuffer::new(text);
        let item = LVITEMW { iSubItem: column as i32, pszText: buffer.as_pwstr(), ..Default::default() };
        w32::send(self.list, LVM_SETITEMTEXTW, row, &item as *const _ as isize);
    }

    fn set_row_selected(&self, row: Option<usize>, selected: bool) {
        let item = LVITEMW {
            state: LIST_VIEW_ITEM_STATE_FLAGS(if selected { 0x3 } else { 0 }),
            stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0x3),
            ..Default::default()
        };
        w32::send(self.list, LVM_SETITEMSTATE, row.unwrap_or(usize::MAX), &item as *const _ as isize);
    }

    fn job_cells(&self, job: &ScheduledJob, now: &Moment) -> [String; 5] {
        [
            job.name.clone(),
            presenter::time(job),
            presenter::countdown(job, now, Some(&self.usage_snapshots.borrow())),
            presenter::targets(&job.targets),
            presenter::status(job.status).to_string(),
        ]
    }

    /// 就地更新清單內容，只改真正變動的儲存格，避免整片重繪。
    fn apply_jobs(&self, jobs: Vec<ScheduledJob>) {
        let selected_id = self.selected_job().map(|job| job.id).or(self.editing_id.get());
        let now = time::now();
        let new_rows: Vec<[String; 5]> = jobs.iter().map(|job| self.job_cells(job, &now)).collect();
        let previous_rows = self.rows.replace(new_rows.clone());
        let was_empty = self.jobs.borrow().is_empty();

        self.refreshing_selection.set(true);
        let mut count = w32::send(self.list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
        while count > jobs.len() {
            count -= 1;
            w32::send(self.list, LVM_DELETEITEM, count, 0);
        }
        while count < jobs.len() {
            let mut empty = w32::WideBuffer::new("");
            let item = LVITEMW {
                mask: LIST_VIEW_ITEM_FLAGS(0x1),
                iItem: count as i32,
                pszText: empty.as_pwstr(),
                ..Default::default()
            };
            w32::send(self.list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
            count += 1;
        }

        let selected_index = self.selected_index();
        let mut select = None;
        for (index, cells) in new_rows.iter().enumerate() {
            for (column, text) in cells.iter().enumerate() {
                if previous_rows.get(index).map(|previous| &previous[column]) != Some(text) {
                    self.set_cell(index, column, text);
                }
            }
            if Some(jobs[index].id) == selected_id && selected_index != Some(index) {
                select = Some(index);
            }
        }

        let is_empty = jobs.is_empty();
        *self.jobs.borrow_mut() = jobs;
        if let Some(index) = select {
            self.set_row_selected(Some(index), true);
        }
        self.refreshing_selection.set(false);

        w32::show(self.list, !is_empty);
        if is_empty != was_empty {
            w32::invalidate_rect(self.hwnd, &self.frame.get().left_panel);
        }
        w32::invalidate(self.list);
    }

    fn update_status_summary(&self, jobs: &[ScheduledJob]) {
        let pending = jobs.iter().filter(|job| job.enabled && job.status == ScheduleStatus::Pending).count();
        let failed = jobs.iter().filter(|job| job.status == ScheduleStatus::Failed).count();
        let color = if failed > 0 { palette().danger } else { palette().control_text };
        self.set_status(&format!("等待中：{pending}　失敗：{failed}"), color);
    }

    fn on_list_custom_draw(&self, draw: &mut NMLVCUSTOMDRAW) -> isize {
        let colors = palette();
        match draw.nmcd.dwDrawStage.0 {
            CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW,
            CDDS_ITEMPREPAINT => {
                let index = draw.nmcd.dwItemSpec;
                let status = self.jobs.borrow().get(index).map(|job| job.status);
                draw.clrText = match status {
                    Some(ScheduleStatus::Completed) => colors.success,
                    Some(ScheduleStatus::Failed) => colors.danger,
                    Some(ScheduleStatus::Disabled) => colors.disabled_text,
                    _ => colors.control_text,
                };
                draw.clrTextBk = if index % 2 == 1 { colors.canvas } else { colors.panel };
                CDRF_NEWFONT
            }
            _ => 0,
        }
    }

    // ── 額度 ──────────────────────────────────────────────────────

    /// 額度只在開啟主視窗或使用者要求時查詢一次；之後每秒只用已取得的
    /// 重置時間更新本地倒數，不會持續啟動 CLI 或發出網路要求。
    fn refresh_usage(&self, show_status: bool) {
        let generation = self.usage_refresh_generation.get() + 1;
        self.usage_refresh_generation.set(generation);
        let cancel = CancelToken::new();
        if let Some(previous) = self.usage_refresh_cancel.borrow_mut().replace(cancel.clone()) {
            previous.cancel();
        }

        w32::enable(self.refresh_usage_button, false);
        w32::set_text(self.refresh_usage_button, "讀取中…");
        w32::invalidate(self.refresh_usage_button);

        // 設定可能在兩次讀取之間改過，每次都同步一次自動啟動開關。
        let settings = self.host.settings();
        self.host.usage_reader.set_auto_start_antigravity(settings.auto_start_antigravity);

        let reader = Arc::clone(&self.host.usage_reader);
        let working_directory = self.host.wakeup_workspace();
        let queue = self.queue.clone();
        std::thread::spawn(move || {
            let results: Vec<CoreResult<CliUsageSnapshot>> = std::thread::scope(|scope| {
                let handles: Vec<_> = catalog::all()
                    .iter()
                    .map(|descriptor| {
                        let profile = settings.profile(descriptor.kind);
                        let (reader, cancel, working_directory) = (&reader, &cancel, &working_directory);
                        scope.spawn(move || reader.read(descriptor.kind, &profile, working_directory, cancel))
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .unwrap_or_else(|_| Err(CoreError::InvalidOperation("額度讀取意外中止。".to_string())))
                    })
                    .collect()
            });
            queue.post(move |window: &MainWindow| window.on_usage_refreshed(generation, results, show_status));
        });
    }

    fn on_usage_refreshed(&self, generation: u64, results: Vec<CoreResult<CliUsageSnapshot>>, show_status: bool) {
        // 已經有更新的一次讀取在進行，這一次的結果作廢。
        if generation != self.usage_refresh_generation.get() {
            return;
        }
        self.usage_refresh_cancel.borrow_mut().take();
        w32::enable(self.refresh_usage_button, true);
        w32::set_text(self.refresh_usage_button, "重新讀取倒數");
        w32::invalidate(self.refresh_usage_button);

        let mut snapshots = Vec::new();
        for result in results {
            match result {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(CoreError::Canceled) => return,
                Err(error) => {
                    self.set_status(&format!("額度讀取失敗：{}", error.message()), palette().danger);
                    return;
                }
            }
        }

        let available_count =
            snapshots.iter().filter(|snapshot| snapshot.availability == CliUsageAvailability::Available).count();
        {
            let mut current = self.usage_snapshots.borrow_mut();
            current.clear();
            current.extend(snapshots.into_iter().map(|snapshot| (snapshot.cli, snapshot)));
        }
        let now = time::now();
        self.last_usage_refresh_time.set(Some(now));
        self.update_usage_labels(&now);

        if show_status {
            if available_count > 0 {
                self.set_status(&format!("已更新 {available_count} 個 CLI 的即時額度與重置倒數"), palette().success);
            } else {
                self.set_status("目前尚無可用之即時額度資料", palette().control_text);
            }
        }
    }

    fn update_usage_labels(&self, now: &Moment) {
        let changed = self.board.borrow_mut().apply(&self.usage_snapshots.borrow(), *now);
        if changed {
            self.relayout();
        } else {
            w32::invalidate_rect(self.editor, &self.editor_layout.borrow().board_box);
        }
        self.notify_login_requirements();
    }

    /// CLI 由正常轉為登入失效時跳一次系統匣通知；恢復正常後才會再通知，
    /// 不會因為每秒更新或背景重讀而反覆打擾。
    fn notify_login_requirements(&self) {
        let pending: Vec<CliKind> = {
            let snapshots = self.usage_snapshots.borrow();
            let mut notified = self.login_notified.borrow_mut();
            let in_progress = self.login_in_progress.borrow();
            catalog::all()
                .iter()
                .filter_map(|descriptor| snapshots.get(&descriptor.kind))
                .filter_map(|snapshot| {
                    if !snapshot.requires_login {
                        notified.remove(&snapshot.cli);
                        return None;
                    }
                    (!in_progress.contains(&snapshot.cli) && notified.insert(snapshot.cli)).then_some(snapshot.cli)
                })
                .collect()
        };

        for kind in pending {
            self.balloon_login_kind.set(Some(kind));
            tray::balloon(
                self.hwnd,
                &format!("{} 需要重新登入", kind.short_name()),
                "點這則通知開啟登入視窗，或在「剩餘流量與重置倒數」按「重新登入」。",
                true,
            );
        }
    }

    /// 開一個看得見的主控台跑 CLI 自己的登入流程，視窗關閉後重新讀取額度。
    /// 授權一定要使用者在瀏覽器確認，本程式只負責把流程叫起來。
    fn start_login(&self, kind: CliKind) {
        let name = kind.short_name();
        if !self.login_in_progress.borrow_mut().insert(kind) {
            self.set_status(&format!("{name} 的登入視窗已經開啟。"), palette().control_text);
            return;
        }
        self.set_login_in_progress(kind, true);

        let profile = self.host.settings().profile(kind);
        let working_directory = self.host.wakeup_workspace();
        let started = match locator::resolve(kind, Some(&profile.executable), &working_directory) {
            None => Err(format!("找不到 {name} CLI，請先在設定指定正確路徑。")),
            Some(executable) => login::create_start_info(kind, &executable, &working_directory)
                .and_then(|start_info| start_info.spawn())
                .map_err(|error| format!("無法開啟 {name} 登入視窗：{}", error.message())),
        };

        match started {
            Err(message) => {
                self.set_status(&message, palette().danger);
                self.login_in_progress.borrow_mut().remove(&kind);
                self.set_login_in_progress(kind, false);
            }
            Ok(mut child) => {
                self.set_status(
                    &format!("已開啟 {name} 登入視窗，完成授權後關閉視窗即會重新讀取額度。"),
                    palette().control_text,
                );
                let queue = self.queue.clone();
                std::thread::spawn(move || {
                    let _ = child.wait();
                    queue.post(move |window: &MainWindow| window.on_login_finished(kind));
                });
            }
        }
    }

    fn on_login_finished(&self, kind: CliKind) {
        self.login_in_progress.borrow_mut().remove(&kind);
        self.set_login_in_progress(kind, false);
        if matches!(kind, CliKind::Antigravity | CliKind::AntigravityArgon | CliKind::AntigravityClaude) {
            // 登入後 language server 會換一輪連接埠。
            usage::clear_antigravity_connection_cache();
        }
        self.refresh_usage(true);
    }

    fn set_login_in_progress(&self, kind: CliKind, in_progress: bool) {
        let changed = self.board.borrow_mut().set_login_in_progress(kind, in_progress);
        if changed {
            self.relayout();
        }
    }

    // ── 使用者操作 ────────────────────────────────────────────────

    fn selected_targets(&self) -> Vec<CliKind> {
        self.target_checks.iter().filter(|(_, check)| ui::is_checked(*check)).map(|(kind, _)| *kind).collect()
    }

    fn save_schedule(&self) {
        let (hour, minute) = ui::time_picker_get(self.time_input);
        let time_of_day = Span::hours(hour as i64) + Span::minutes(minute as i64);

        let saved = calculator::get_next_daily_occurrence(time_of_day, &time::now()).and_then(|scheduled_at| {
            let job = ScheduledJob {
                id: self.editing_id.get().unwrap_or_else(Guid::new),
                name: w32::get_text(self.name_input).trim().to_string(),
                scheduled_at,
                initial_time_of_day: Some(time_of_day),
                message: w32::get_text(self.message_input).trim().to_string(),
                working_directory: w32::get_text(self.working_directory_input).trim().to_string(),
                targets: self.selected_targets(),
                recurrence: ScheduleRecurrence::Interval,
                enabled: ui::is_checked(self.enabled_check),
                ..ScheduledJob::default()
            };
            self.manager().upsert(&job).map(|_| job.id)
        });

        match saved {
            Ok(id) => {
                self.editing_id.set(Some(id));
                w32::set_text(self.save_button, "儲存修改");
                w32::invalidate(self.save_button);
                self.set_status("排程已儲存", palette().success);
            }
            Err(error) => self.show_error(error.message()),
        }
    }

    fn run_selected_now(&self) {
        let Some(selected) = self.selected_job() else {
            w32::message_box(
                Some(self.hwnd),
                "請先選擇一個已儲存排程。",
                &self.title,
                w32::MB_OK | w32::MB_ICONINFORMATION,
            );
            return;
        };

        let question = format!("現在向 {} 傳送「{}」？", presenter::targets_long(&selected.targets), selected.message);
        if w32::message_box(Some(self.hwnd), &question, "立即執行", w32::MB_YESNO | w32::MB_ICONQUESTION) != w32::IDYES
        {
            return;
        }

        match self.manager().run_now(selected.id) {
            Ok(()) => self.set_status("已交給排程器立即執行", palette().control_text),
            Err(error) => self.show_error(error.message()),
        }
    }

    fn delete_selected(&self) {
        let Some(selected) = self.selected_job() else {
            return;
        };
        let question = format!("刪除排程「{}」？", selected.name);
        if w32::message_box(Some(self.hwnd), &question, &self.title, w32::MB_YESNO | w32::MB_ICONWARNING) != w32::IDYES
        {
            return;
        }

        match self.manager().delete(selected.id) {
            Ok(()) => self.reset_editor(),
            Err(error) => self.show_error(error.message()),
        }
    }

    fn open_settings(&self) {
        let current = self.host.settings();
        let runner: Arc<dyn CliRunner> = self.host.runner.clone();
        let Some(result) = settings_window::show_modal(self.hwnd, &current, runner, self.icon) else {
            return;
        };

        let apply = || -> Result<(), String> {
            // 模型、額度與其他設定的變更不必重寫 Windows 啟動項目。
            if !self.host.is_isolated && result.start_with_windows != current.start_with_windows {
                startup::set_enabled(result.start_with_windows)?;
            }
            self.host.replace_settings(result.clone()).map_err(|error| error.message().to_string())
        };
        match apply() {
            Ok(()) => {
                // 使用者可能改了可執行檔路徑，讓下一次執行重新解析。
                locator::clear_cache();
                self.refresh_usage(false);
                self.set_status("設定已儲存", palette().success);
            }
            Err(message) => self.show_error(&format!("設定無法儲存：{message}")),
        }
    }

    fn on_selection_changed(&self) {
        if self.refreshing_selection.get() {
            return;
        }
        let Some(job) = self.selected_job() else {
            return;
        };

        self.editing_id.set(Some(job.id));
        w32::set_text(self.save_button, "儲存修改");
        w32::invalidate(self.save_button);
        w32::set_text(self.name_input, &job.name);
        let initial_time = job.initial_time_of_day.unwrap_or_else(|| time::local_time_of_day(&job.scheduled_at));
        ui::time_picker_set(
            self.time_input,
            (initial_time.num_hours() % 24) as u32,
            (initial_time.num_minutes() % 60) as u32,
        );
        w32::set_text(self.message_input, &job.message);
        w32::set_text(self.working_directory_input, &job.working_directory);
        for (kind, check) in &self.target_checks {
            ui::set_checked(*check, job.targets.contains(kind));
        }
        ui::set_checked(self.enabled_check, job.enabled);
        self.layout_editor();
    }

    fn reset_editor(&self) {
        use chrono::Timelike;

        self.editing_id.set(None);
        w32::set_text(self.save_button, "建立排程");
        w32::invalidate(self.save_button);
        self.refreshing_selection.set(true);
        self.set_row_selected(None, false);
        self.refreshing_selection.set(false);

        let proposed = chrono::Local::now().naive_local() + Span::minutes(5);
        w32::set_text(self.name_input, aiwake_core::models::DEFAULT_JOB_NAME);
        ui::time_picker_set(self.time_input, proposed.hour(), proposed.minute());
        w32::set_text(self.message_input, aiwake_core::models::DEFAULT_JOB_MESSAGE);
        w32::set_text(self.working_directory_input, &self.host.wakeup_workspace());
        for (_, check) in &self.target_checks {
            ui::set_checked(*check, true);
        }
        ui::set_checked(self.enabled_check, true);
        self.layout_editor();
    }

    fn show_selected_result(&self) {
        let information = w32::MB_OK | w32::MB_ICONINFORMATION;
        let Some(job) = self.selected_job() else {
            w32::message_box(Some(self.hwnd), "請先選擇排程。", &self.title, information);
            return;
        };
        if job.last_results.is_empty() {
            w32::message_box(Some(self.hwnd), "這個排程尚無執行結果。", &self.title, information);
            return;
        }

        let all_succeeded = job.last_results.iter().all(|result| result.succeeded);
        w32::message_box(
            Some(self.hwnd),
            &presenter::results(&job),
            &format!("{}－執行結果", job.name),
            w32::MB_OK | if all_succeeded { w32::MB_ICONINFORMATION } else { w32::MB_ICONWARNING },
        );
    }

    fn browse_working_directory(&self) {
        let current = w32::get_text(self.working_directory_input);
        if let Some(path) = ui::pick_folder(self.hwnd, "選擇 CLI 執行時使用的工作目錄", &current) {
            w32::set_text(self.working_directory_input, &path);
        }
    }

    fn open_logs_folder(&self) {
        let path = self.host.paths.logs_directory();
        let opened = std::fs::create_dir_all(&path).map_err(|error| error.to_string()).and_then(|_| {
            let operation = wide("open");
            let target = wide(&path.to_string_lossy());
            let result = unsafe {
                ShellExecuteW(None, PCWSTR(operation.as_ptr()), PCWSTR(target.as_ptr()), None, None, SW_SHOWNORMAL)
            };
            if result.0 as isize > 32 {
                Ok(())
            } else {
                Err("Windows 檔案總管沒有回應。".to_string())
            }
        });
        if let Err(message) = opened {
            w32::message_box(
                Some(self.hwnd),
                &format!("無法開啟資料夾：{message}"),
                APP_TITLE,
                w32::MB_OK | w32::MB_ICONERROR,
            );
        }
    }

    /// 將設定、排程、目前額度與所有日誌合併成一個文字檔，方便回報問題時附上。
    fn export_debug_logs(&self) {
        let default_name = format!("AI倒數喚醒-除錯日誌-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let Some(path) = ui::save_file(self.hwnd, "匯出除錯日誌", &default_name, "txt", &[("文字檔 (*.txt)", "*.txt")])
        else {
            return;
        };
        match self.host.export_debug_logs(std::path::Path::new(&path)) {
            Ok(count) => {
                self.set_status(&format!("已匯出除錯日誌（{count} 個日誌檔）：{path}"), palette().success);
                let operation = wide("open");
                let explorer = wide("explorer.exe");
                let arguments = wide(&format!("/select,\"{path}\""));
                unsafe {
                    ShellExecuteW(
                        None,
                        PCWSTR(operation.as_ptr()),
                        PCWSTR(explorer.as_ptr()),
                        PCWSTR(arguments.as_ptr()),
                        None,
                        SW_SHOWNORMAL,
                    );
                }
            }
            Err(error) => {
                w32::message_box(
                    Some(self.hwnd),
                    &format!("無法匯出除錯日誌：{error}"),
                    APP_TITLE,
                    w32::MB_OK | w32::MB_ICONERROR,
                );
            }
        }
    }

    // ── 計時器與系統匣 ────────────────────────────────────────────

    fn start_clock(&self) {
        if !self.clock_running.replace(true) {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_CLOCK, 1000, None);
            }
        }
        self.on_clock_tick();
    }

    fn stop_clock(&self) {
        if self.clock_running.replace(false) {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_CLOCK);
            }
        }
    }

    /// 每秒更新時鐘與倒數。視窗看不見時計時器會完全停掉，
    /// 常駐系統匣期間不會有任何每秒喚醒。
    fn on_clock_tick(&self) {
        if !self.is_visible() {
            self.stop_clock();
            return;
        }

        let clock_text = format!("現在：{}", chrono::Local::now().format("%Y/%m/%d %H:%M:%S"));
        if *self.last_clock_text.borrow() != clock_text {
            *self.last_clock_text.borrow_mut() = clock_text;
            self.invalidate_status();
        }

        let now = time::now();
        let countdowns: Vec<String> = self
            .jobs
            .borrow()
            .iter()
            .map(|job| presenter::countdown(job, &now, Some(&self.usage_snapshots.borrow())))
            .collect();
        for (index, text) in countdowns.into_iter().enumerate() {
            let changed = self.rows.borrow().get(index).is_some_and(|row| row[COLUMN_COUNTDOWN] != text);
            if changed {
                self.set_cell(index, COLUMN_COUNTDOWN, &text);
                self.rows.borrow_mut()[index][COLUMN_COUNTDOWN] = text;
            }
        }
        self.update_usage_labels(&now);

        // 1. 倒數結束時自動讀取：檢查是否有任何 CLI 的倒數時間已到
        let mut has_countdown_ended = false;
        {
            let snapshots = self.usage_snapshots.borrow();
            let mut last_refresh = self.last_countdown_end_refresh.borrow_mut();
            for descriptor in catalog::all() {
                let ended = snapshots
                    .get(&descriptor.kind)
                    .filter(|snapshot| snapshot.availability == CliUsageAvailability::Available)
                    .and_then(|snapshot| snapshot.short_window_or_first())
                    .filter(|window| window.is_active_countdown)
                    .and_then(|window| window.resets_at)
                    .is_some_and(|resets_at| now >= resets_at);
                let recently_refreshed =
                    last_refresh.get(&descriptor.kind).is_some_and(|last| now - *last < Span::seconds(15));
                if ended && !recently_refreshed {
                    last_refresh.insert(descriptor.kind, now);
                    has_countdown_ended = true;
                }
            }
        }

        if has_countdown_ended {
            self.refresh_usage(false);
            return;
        }

        // 2. 沒抓到倒數（未在倒數中）時依設定間隔背景探測。
        // 若所有 CLI 皆處於正常有效倒數中，則略過定時查詢，避免產生不必要的連線與 429。
        let has_uncaptured_countdown = {
            let snapshots = self.usage_snapshots.borrow();
            catalog::all().iter().any(|descriptor| !is_counting_down(snapshots.get(&descriptor.kind), &now))
        };
        let interval_minutes = self.host.settings().quota_auto_refresh_minutes;
        if has_uncaptured_countdown
            && interval_minutes > 0
            && self
                .last_usage_refresh_time
                .get()
                .is_some_and(|last| now - last >= Span::minutes(interval_minutes as i64))
        {
            self.last_usage_refresh_time.set(Some(now));
            self.refresh_usage(false);
        }
    }

    fn on_close(&self) {
        if !self.really_exit.get() && self.host.settings().minimize_to_tray {
            self.hide_to_tray();
            self.balloon_login_kind.set(None);
            tray::balloon(self.hwnd, APP_TITLE, "程式仍在系統匣執行，排程會繼續倒數。", false);
            return;
        }
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    /// 進入常駐狀態後，請作業系統把目前用不到的分頁移出工作集。
    fn trim_working_set(&self) {
        unsafe {
            let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
    }

    fn hide_to_tray(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.stop_clock();
        self.trim_working_set();
    }

    fn show_from_tray(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if IsIconic(self.hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
            let _ = SetForegroundWindow(self.hwnd);
        }
        if self.refresh_deferred.get() {
            self.on_refresh_requested();
        }
        self.start_clock();
    }

    fn show_tray_menu(&self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            for (id, text) in
                [(ID_TRAY_OPEN, "開啟主視窗"), (ID_TRAY_LOGS, "開啟日誌資料夾"), (ID_TRAY_EXPORT_LOGS, "匯出除錯日誌…")]
            {
                let text = wide(text);
                let _ = AppendMenuW(menu, MF_STRING, id as usize, PCWSTR(text.as_ptr()));
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let exit = wide("結束程式");
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT as usize, PCWSTR(exit.as_ptr()));

            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            // 選單要在點擊其他地方時自動關閉，擁有視窗必須先成為前景視窗。
            let _ = SetForegroundWindow(self.hwnd);
            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, cursor.x, cursor.y, None, self.hwnd, None);
            let _ = DestroyMenu(menu);
        }
    }

    fn on_tray_message(&self, lparam: LPARAM) {
        match w32::lo_word(lparam.0 as usize) {
            WM_LBUTTONDBLCLK | tray::NIN_KEYSELECT => self.show_from_tray(),
            WM_RBUTTONUP | WM_CONTEXTMENU => self.show_tray_menu(),
            tray::NIN_BALLOONUSERCLICK => {
                if let Some(kind) = self.balloon_login_kind.take() {
                    self.show_from_tray();
                    self.start_login(kind);
                }
            }
            tray::NIN_BALLOONTIMEOUT | tray::NIN_BALLOONHIDE => self.balloon_login_kind.set(None),
            _ => {}
        }
    }

    fn on_command(&self, id: i32, code: u32) {
        match id {
            ID_NEW if code == BN_CLICKED => self.reset_editor(),
            ID_RUN_NOW if code == BN_CLICKED => self.run_selected_now(),
            ID_SHOW_RESULT if code == BN_CLICKED => self.show_selected_result(),
            ID_DELETE if code == BN_CLICKED => self.delete_selected(),
            ID_OPEN_LOGS if code == BN_CLICKED => self.open_logs_folder(),
            ID_EXPORT_LOGS if code == BN_CLICKED => self.export_debug_logs(),
            ID_BROWSE if code == BN_CLICKED => self.browse_working_directory(),
            ID_REFRESH_USAGE if code == BN_CLICKED => self.refresh_usage(true),
            ID_SAVE if code == BN_CLICKED => self.save_schedule(),
            ID_SETTINGS if code == BN_CLICKED => self.open_settings(),
            ID_TERMINAL_CLEAR if code == BN_CLICKED => self.terminal.clear(),
            ID_TRAY_OPEN => self.show_from_tray(),
            ID_TRAY_LOGS => self.open_logs_folder(),
            ID_TRAY_EXPORT_LOGS => {
                self.show_from_tray();
                self.export_debug_logs();
            }
            ID_TRAY_EXIT => {
                self.really_exit.set(true);
                self.on_close();
            }
            _ => {}
        }
    }

    fn on_notify(&self, lparam: LPARAM) -> Option<LRESULT> {
        let header = unsafe { &*(lparam.0 as *const NMHDR) };
        if header.hwndFrom != self.list {
            return None;
        }
        match header.code {
            LVN_ITEMCHANGED => {
                self.on_selection_changed();
                None
            }
            NM_DBLCLK => {
                self.show_selected_result();
                None
            }
            NM_CUSTOMDRAW => {
                let draw = unsafe { &mut *(lparam.0 as *mut NMLVCUSTOMDRAW) };
                Some(LRESULT(self.on_list_custom_draw(draw)))
            }
            _ => None,
        }
    }

    // ── 分隔線拖曳 ────────────────────────────────────────────────

    fn splitter_at(&self, x: i32, y: i32) -> Drag {
        let frame = self.frame.get();
        if w32::contains(&frame.horizontal_splitter, x, y) {
            Drag::Horizontal
        } else {
            Drag::None
        }
    }

    fn drag_to(&self, y: i32) {
        let fonts = self.fonts();
        let frame = self.frame.get();
        match self.drag.get() {
            Drag::Horizontal => {
                self.terminal_height.set(Some((frame.status.top - y - fonts.scale(3)).max(0)));
            }
            Drag::None => return,
        }
        self.layout_frame();
    }

    /// 依工作區大小決定整體放大倍率（以 0.1 為級距），倍率改變時重建字型。回傳是否改變。
    fn update_zoom(&self) -> bool {
        let client = w32::client_rect(self.hwnd);
        let (width, height) = (w32::width_of(&client), w32::height_of(&client));
        if width <= 0 || height <= 0 {
            return false;
        }
        let base_dpi = self.base_dpi.get();
        let ratio = (width as f64 / w32::scale(DESIGN_CLIENT_SIZE.0, base_dpi) as f64)
            .min(height as f64 / w32::scale(DESIGN_CLIENT_SIZE.1, base_dpi) as f64);
        let zoom = ((ratio.clamp(1.0, MAX_ZOOM) * 10.0).floor() / 10.0).max(1.0);
        let dpi = (base_dpi as f64 * zoom).round() as i32;
        if dpi == self.fonts().dpi {
            return false;
        }
        *self.fonts.borrow_mut() = Rc::new(Fonts::new(dpi));
        self.apply_fonts();
        true
    }

    fn on_dpi_changed(&self, dpi: i32, suggested: &RECT) {
        self.base_dpi.set(dpi);
        *self.fonts.borrow_mut() = Rc::new(Fonts::new(dpi));
        self.apply_fonts();
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                suggested.left,
                suggested.top,
                w32::width_of(suggested),
                w32::height_of(suggested),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        self.layout_frame();
        self.layout_editor();
    }
}

fn handle_draw_item(window: &MainWindow, lparam: LPARAM, background: COLORREF) -> LRESULT {
    let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
    theme::draw_button(item, &window.fonts(), background);
    LRESULT(1)
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCDESTROY {
        if let Some(state) = w32::take_state::<MainWindow>(hwnd) {
            let _ = DeleteObject(HGDIOBJ(state.canvas_brush.0));
            let _ = DeleteObject(HGDIOBJ(state.panel_brush.0));
        }
        return w32::def_proc(hwnd, message, wparam, lparam);
    }
    let Some(window) = w32::state::<MainWindow>(hwnd) else {
        return w32::def_proc(hwnd, message, wparam, lparam);
    };

    match message {
        WM_APP_DISPATCH => {
            window.queue.clone().drain(window);
            LRESULT(0)
        }
        WM_APP_REFRESH => {
            window.on_refresh_requested();
            LRESULT(0)
        }
        tray::WM_TRAY => {
            window.on_tray_message(lparam);
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TIMER_CLOCK => window.on_clock_tick(),
                TIMER_TERMINAL => window.terminal.flush(ui::is_checked(window.terminal_auto_scroll)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let zoomed = window.update_zoom();
            window.layout_frame();
            if zoomed {
                window.layout_editor();
            }
            // 還原視窗後補上縮小期間略過的更新。
            if window.is_visible() {
                if window.refresh_deferred.get() {
                    window.on_refresh_requested();
                }
                if !window.clock_running.get() {
                    window.start_clock();
                }
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            // 最小尺寸以螢幕實際 DPI 計算，不受視窗放大倍率影響。
            let dpi = window.base_dpi.get();
            let work = work_area(hwnd);
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT {
                x: w32::scale(980, dpi).min(w32::width_of(&work)),
                y: w32::scale(650, dpi).min(w32::height_of(&work)),
            };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            window.on_dpi_changed(w32::hi_word(wparam.0) as i32, &*(lparam.0 as *const RECT));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            w32::paint_buffered(hwnd, |dc, bounds| window.paint_frame(dc, bounds));
            LRESULT(0)
        }
        WM_DRAWITEM => {
            // 清單按鈕位於白色面板上，終端機工具列的按鈕位於視窗底色上。
            let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
            let on_panel = window.list_buttons.contains(&item.hwndItem);
            handle_draw_item(window, lparam, if on_panel { palette().panel } else { palette().canvas })
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            SetBkMode(HDC(wparam.0 as *mut _), TRANSPARENT);
            LRESULT(window.canvas_brush.0 as isize)
        }
        WM_COMMAND => {
            let (id, code, _) = ui::command_parts(wparam, lparam);
            window.on_command(id, code);
            LRESULT(0)
        }
        WM_NOTIFY => window.on_notify(lparam).unwrap_or_else(|| w32::def_proc(hwnd, message, wparam, lparam)),
        WM_SETCURSOR => {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut cursor);
            let shape = match window.splitter_at(cursor.x, cursor.y) {
                Drag::Horizontal => Some(IDC_SIZENS),
                Drag::None => None,
            };
            match shape {
                Some(shape) if HWND(wparam.0 as *mut _) == hwnd => {
                    SetCursor(LoadCursorW(None, shape).ok());
                    LRESULT(1)
                }
                _ => w32::def_proc(hwnd, message, wparam, lparam),
            }
        }
        WM_LBUTTONDOWN => {
            let drag = window.splitter_at(w32::x_of(lparam), w32::y_of(lparam));
            if drag != Drag::None {
                window.drag.set(drag);
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            window.drag_to(w32::y_of(lparam));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if window.drag.replace(Drag::None) != Drag::None {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            window.on_close();
            LRESULT(0)
        }
        WM_DESTROY => {
            window.stop_clock();
            let _ = KillTimer(Some(hwnd), TIMER_TERMINAL);
            if let Some(cancel) = window.usage_refresh_cancel.borrow_mut().take() {
                cancel.cancel();
            }
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            // 檔案總管重新啟動後，系統匣圖示要重新加入。
            if message == window.taskbar_created && window.taskbar_created != 0 {
                window.tray_added.set(tray::add(hwnd, window.icon, APP_TITLE));
                return LRESULT(0);
            }
            w32::def_proc(hwnd, message, wparam, lparam)
        }
    }
}

unsafe extern "system" fn editor_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(window) = w32::state::<MainWindow>(hwnd) else {
        return w32::def_proc(hwnd, message, wparam, lparam);
    };

    match message {
        WM_SIZE => {
            window.layout_editor();
            LRESULT(0)
        }
        WM_VSCROLL => {
            if window.editor_scroll.on_scroll(wparam, window.fonts().scale(24)) {
                window.layout_editor();
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if window.editor_scroll.on_wheel(wparam, window.fonts().scale(24)) {
                window.layout_editor();
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            w32::paint_buffered(hwnd, |dc, bounds| window.paint_editor(dc, bounds));
            LRESULT(0)
        }
        // 編輯面板的控制項都放在白底卡片上。
        WM_DRAWITEM => handle_draw_item(window, lparam, palette().panel),
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            SetBkMode(HDC(wparam.0 as *mut _), TRANSPARENT);
            LRESULT(window.panel_brush.0 as isize)
        }
        WM_COMMAND => {
            let (id, code, _) = ui::command_parts(wparam, lparam);
            window.on_command(id, code);
            LRESULT(0)
        }
        WM_MOUSEMOVE | WM_MOUSELEAVE | WM_LBUTTONUP => {
            // 看板上的「重新登入」連結：停留時換游標與顏色，點擊時開啟登入流程。
            let origin = window.editor_layout.borrow().board_origin;
            let hit = if message == WM_MOUSELEAVE {
                None
            } else {
                window.board.borrow().hit_login_link(w32::x_of(lparam) - origin.0, w32::y_of(lparam) - origin.1)
            };
            if window.board.borrow_mut().set_hovered(hit) {
                w32::invalidate_rect(hwnd, &window.editor_layout.borrow().board_box);
            }
            if hit.is_some() {
                SetCursor(LoadCursorW(None, IDC_HAND).ok());
            }
            if let (WM_LBUTTONUP, Some(kind)) = (message, hit) {
                window.start_login(kind);
            }
            LRESULT(0)
        }
        _ => w32::def_proc(hwnd, message, wparam, lparam),
    }
}
