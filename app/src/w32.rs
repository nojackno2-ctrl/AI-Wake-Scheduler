//! Win32 的薄包裝：把常用呼叫收斂成簡單、安全的函式，
//! 讓視窗程式碼專注在版面與行為。

use std::ffi::c_void;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, GetStockObject, GetTextExtentPoint32W, GetTextMetricsW,
    InvalidateRect, LineTo, MoveToEx, ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor, DRAW_TEXT_FORMAT,
    FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, HBRUSH, HDC, HFONT, HGDIOBJ, NULL_PEN,
    PAINTSTRUCT, PS_SOLID, SRCCOPY, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    LoadCursorW, MessageBoxW, MoveWindow, PostMessageW, RegisterClassExW, SendMessageW, SetWindowLongPtrW,
    SetWindowTextW, ShowWindow, GWLP_USERDATA, HCURSOR, HICON, HMENU, IDC_ARROW, MESSAGEBOX_STYLE, SW_HIDE, SW_SHOWNA,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WNDCLASSEXW, WNDCLASS_STYLES, WNDPROC,
};

/// 滑鼠離開視窗（需先以 TrackMouseEvent 要求）。
pub const WM_MOUSELEAVE: u32 = 0x02A3;

pub const DT_LEFT: u32 = 0x0000;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_RIGHT: u32 = 0x0002;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_WORDBREAK: u32 = 0x0010;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_CALCRECT: u32 = 0x0400;
pub const DT_NOPREFIX: u32 = 0x0800;
pub const DT_EDITCONTROL: u32 = 0x2000;
pub const DT_END_ELLIPSIS: u32 = 0x8000;

/// 單行、垂直置中、過長時以刪節號結尾。
pub const DT_LINE: u32 = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS;

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buffer: &[u16]) -> String {
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

pub const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | (green as u32) << 8 | (blue as u32) << 16)
}

pub fn lo_word(value: usize) -> u32 {
    (value & 0xffff) as u32
}

pub fn hi_word(value: usize) -> u32 {
    ((value >> 16) & 0xffff) as u32
}

pub fn x_of(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xffff) as u16 as i16 as i32
}

pub fn y_of(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32
}

pub fn rect(left: i32, top: i32, width: i32, height: i32) -> RECT {
    RECT { left, top, right: left + width, bottom: top + height }
}

pub fn width_of(r: &RECT) -> i32 {
    r.right - r.left
}

pub fn height_of(r: &RECT) -> i32 {
    r.bottom - r.top
}

pub fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(PCWSTR::null()).map(|module| HINSTANCE(module.0)).unwrap_or_default() }
}

pub fn dpi_of(hwnd: HWND) -> i32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) } as i32;
    if dpi <= 0 {
        96
    } else {
        dpi
    }
}

pub fn scale(value: i32, dpi: i32) -> i32 {
    (value as f64 * dpi as f64 / 96.0).round() as i32
}

pub fn register_class(name: &str, wndproc: WNDPROC, style: u32, icon: HICON) {
    let class_name = wide(name);
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: WNDCLASS_STYLES(style),
        lpfnWndProc: wndproc,
        hInstance: hinstance(),
        hIcon: icon,
        hIconSm: icon,
        hCursor: arrow_cursor(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&class);
    }
}

pub fn arrow_cursor() -> HCURSOR {
    unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() }
}

#[allow(clippy::too_many_arguments)]
pub fn create_window(
    ex_style: u32,
    class: &str,
    text: &str,
    style: u32,
    bounds: RECT,
    parent: Option<HWND>,
    id: i32,
    param: *const c_void,
) -> HWND {
    let class = wide(class);
    let text = wide(text);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            PCWSTR(class.as_ptr()),
            PCWSTR(text.as_ptr()),
            WINDOW_STYLE(style),
            bounds.left,
            bounds.top,
            width_of(&bounds),
            height_of(&bounds),
            parent,
            if id == 0 { None } else { Some(HMENU(id as isize as *mut c_void)) },
            Some(hinstance()),
            if param.is_null() { None } else { Some(param) },
        )
        .unwrap_or_default()
    }
}

pub fn send(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    unsafe { SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam))).0 }
}

pub fn post(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> bool {
    unsafe { PostMessageW(Some(hwnd), message, WPARAM(wparam), LPARAM(lparam)).is_ok() }
}

pub fn def_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub fn set_text(hwnd: HWND, text: &str) {
    if get_text(hwnd) == text {
        return;
    }
    let text = wide(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    }
}

pub fn get_text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

pub fn move_window(hwnd: HWND, bounds: &RECT) {
    unsafe {
        let _ = MoveWindow(hwnd, bounds.left, bounds.top, width_of(bounds).max(0), height_of(bounds).max(0), true);
    }
}

pub fn show(hwnd: HWND, visible: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
    }
}

pub fn enable(hwnd: HWND, enabled: bool) {
    unsafe {
        let _ = EnableWindow(hwnd, enabled);
    }
}

pub fn client_rect(hwnd: HWND) -> RECT {
    let mut bounds = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut bounds);
    }
    bounds
}

pub fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn invalidate_rect(hwnd: HWND, bounds: &RECT) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), Some(bounds), false);
    }
}

pub fn set_font(hwnd: HWND, font: &Font) {
    send(hwnd, WM_SETFONT, font.0 .0 as usize, 1);
}

pub fn message_box(owner: Option<HWND>, text: &str, caption: &str, style: u32) -> i32 {
    let text = wide(text);
    let caption = wide(caption);
    unsafe { MessageBoxW(owner, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MESSAGEBOX_STYLE(style)).0 }
}

pub const MB_OK: u32 = 0x0;
pub const MB_YESNO: u32 = 0x4;
pub const MB_ICONERROR: u32 = 0x10;
pub const MB_ICONQUESTION: u32 = 0x20;
pub const MB_ICONWARNING: u32 = 0x30;
pub const MB_ICONINFORMATION: u32 = 0x40;
pub const IDYES: i32 = 6;

/// 把視窗狀態存進 GWLP_USERDATA；視窗銷毀時以 `take_state` 取回並釋放。
pub fn set_state<T>(hwnd: HWND, state: Box<T>) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
    }
}

/// 取得視窗狀態的共用參考。狀態只在 UI 執行緒上存取，
/// 而且所有可變欄位都使用內部可變性，所以重入的訊息處理是安全的。
pub fn state<'a, T>(hwnd: HWND) -> Option<&'a T> {
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const T;
        pointer.as_ref()
    }
}

pub fn take_state<T>(hwnd: HWND) -> Option<Box<T>> {
    unsafe {
        let pointer = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut T;
        if pointer.is_null() {
            None
        } else {
            Some(Box::from_raw(pointer))
        }
    }
}

/// GDI 字型，離開作用域時釋放。
pub struct Font(pub HFONT);

impl Font {
    /// 以點數建立字型；找不到字族時 GDI 會自行換成最接近的字族。
    pub fn new(family: &str, points: f32, bold: bool, dpi: i32) -> Self {
        let face = wide(family);
        let height = -((points as f64 * dpi as f64 / 72.0).round() as i32);
        let font = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                if bold { 700 } else { 400 },
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(5),
                0,
                PCWSTR(face.as_ptr()),
            )
        };
        Font(font)
    }

    /// 字型行高（含行距）。
    pub fn height(&self) -> i32 {
        unsafe {
            let dc = GetDC(None);
            let previous = SelectObject(dc, HGDIOBJ(self.0 .0));
            let mut metrics = TEXTMETRICW::default();
            let _ = GetTextMetricsW(dc, &mut metrics);
            SelectObject(dc, previous);
            ReleaseDC(None, dc);
            metrics.tmHeight
        }
    }

    pub fn measure(&self, text: &str) -> (i32, i32) {
        unsafe {
            let dc = GetDC(None);
            let size = measure_text(dc, self, text);
            ReleaseDC(None, dc);
            size
        }
    }

    /// 在指定寬度內自動換行後的高度。
    pub fn measure_wrapped(&self, text: &str, width: i32) -> i32 {
        unsafe {
            let dc = GetDC(None);
            let previous = SelectObject(dc, HGDIOBJ(self.0 .0));
            let mut buffer: Vec<u16> = text.encode_utf16().collect();
            let mut bounds = rect(0, 0, width.max(1), 0);
            DrawTextW(
                dc,
                &mut buffer,
                &mut bounds,
                DRAW_TEXT_FORMAT(DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX),
            );
            SelectObject(dc, previous);
            ReleaseDC(None, dc);
            height_of(&bounds)
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0 .0));
        }
    }
}

pub fn measure_text(dc: HDC, font: &Font, text: &str) -> (i32, i32) {
    unsafe {
        let previous = SelectObject(dc, HGDIOBJ(font.0 .0));
        let buffer: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &buffer, &mut size);
        SelectObject(dc, previous);
        (size.cx, size.cy)
    }
}

pub fn draw_text(dc: HDC, font: &Font, text: &str, bounds: &RECT, color: COLORREF, format: u32) {
    if text.is_empty() || bounds.right <= bounds.left {
        return;
    }
    unsafe {
        let previous = SelectObject(dc, HGDIOBJ(font.0 .0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        let mut buffer: Vec<u16> = text.encode_utf16().collect();
        let mut target = *bounds;
        DrawTextW(dc, &mut buffer, &mut target, DRAW_TEXT_FORMAT(format));
        SelectObject(dc, previous);
    }
}

pub fn fill(dc: HDC, bounds: &RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, bounds, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// 畫一個單像素外框。
pub fn frame(dc: HDC, bounds: &RECT, color: COLORREF) {
    fill(dc, &RECT { bottom: bounds.top + 1, ..*bounds }, color);
    fill(dc, &RECT { top: bounds.bottom - 1, ..*bounds }, color);
    fill(dc, &RECT { right: bounds.left + 1, ..*bounds }, color);
    fill(dc, &RECT { left: bounds.right - 1, ..*bounds }, color);
}

pub fn line(dc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, color: COLORREF) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, color);
        let previous = SelectObject(dc, HGDIOBJ(pen.0));
        let _ = MoveToEx(dc, x1, y1, None);
        let _ = LineTo(dc, x2, y2);
        SelectObject(dc, previous);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

/// 畫實心圓角矩形（無外框）。
pub fn fill_round(dc: HDC, bounds: &RECT, radius: i32, color: COLORREF) {
    if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
        return;
    }
    unsafe {
        let brush = CreateSolidBrush(color);
        let previous_brush = SelectObject(dc, HGDIOBJ(brush.0));
        let previous_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        let _ = RoundRect(dc, bounds.left, bounds.top, bounds.right + 1, bounds.bottom + 1, radius, radius);
        SelectObject(dc, previous_pen);
        SelectObject(dc, previous_brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

pub fn solid_brush(color: COLORREF) -> HBRUSH {
    unsafe { CreateSolidBrush(color) }
}

/// 以記憶體 DC 雙緩衝繪製整個工作區，避免調整大小或每秒更新時閃爍。
pub fn paint_buffered(hwnd: HWND, paint: impl FnOnce(HDC, &RECT)) {
    unsafe {
        let mut paint_struct = PAINTSTRUCT::default();
        let window_dc = BeginPaint(hwnd, &mut paint_struct);
        let bounds = client_rect(hwnd);
        let (width, height) = (width_of(&bounds).max(1), height_of(&bounds).max(1));

        let memory_dc = CreateCompatibleDC(Some(window_dc));
        let bitmap = CreateCompatibleBitmap(window_dc, width, height);
        let previous = SelectObject(memory_dc, HGDIOBJ(bitmap.0));

        paint(memory_dc, &bounds);

        let _ = BitBlt(window_dc, 0, 0, width, height, Some(memory_dc), 0, 0, SRCCOPY);
        SelectObject(memory_dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
        let _ = EndPaint(hwnd, &paint_struct);
    }
}

/// 以 null 結尾的可寫入寬字元緩衝，供需要 `PWSTR` 的結構使用。
pub struct WideBuffer(pub Vec<u16>);

impl WideBuffer {
    pub fn new(text: &str) -> Self {
        WideBuffer(wide(text))
    }

    pub fn as_pwstr(&mut self) -> PWSTR {
        PWSTR(self.0.as_mut_ptr())
    }
}
