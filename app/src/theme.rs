//! 全應用程式共用的字型、色彩與按鈕外觀。

use std::sync::OnceLock;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetSysColor, SYS_COLOR_INDEX};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SystemParametersInfoW, GWLP_USERDATA, SPI_GETHIGHCONTRAST,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_MOUSEMOVE,
};

use crate::w32::{self, rgb, Font, WM_MOUSELEAVE};

const FAMILY_NAME: &str = "Microsoft JhengHei UI";
const MONO_FAMILY_NAME: &str = "Consolas";

/// 單一冷色中性色盤，僅以藍色作為互動重點。紅、綠只表達語意狀態。
/// 高對比模式下改用系統色彩。
pub struct Palette {
    pub high_contrast: bool,
    pub canvas: COLORREF,
    pub panel: COLORREF,
    pub panel_subtle: COLORREF,
    pub divider: COLORREF,
    pub primary_text: COLORREF,
    pub secondary_text: COLORREF,
    pub selected: COLORREF,
    pub banner: COLORREF,
    pub banner_text: COLORREF,
    pub banner_subtitle: COLORREF,
    pub accent: COLORREF,
    pub accent_hover: COLORREF,
    pub accent_pressed: COLORREF,
    pub success: COLORREF,
    pub danger: COLORREF,
    pub danger_subtle: COLORREF,
    /// 額度水位介於安全與危險之間時的提醒色。
    pub caution: COLORREF,
    /// 額度長條的底槽色。
    pub meter_track: COLORREF,
    pub control_text: COLORREF,
    pub disabled_text: COLORREF,
}

fn system_color(index: i32) -> COLORREF {
    COLORREF(unsafe { GetSysColor(SYS_COLOR_INDEX(index)) })
}

fn is_high_contrast() -> bool {
    let mut info = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some(&mut info as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && info.dwFlags.contains(HCF_HIGHCONTRASTON)
    }
}

pub fn palette() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(|| {
        const COLOR_WINDOW: i32 = 5;
        const COLOR_WINDOWTEXT: i32 = 8;
        const COLOR_HIGHLIGHT: i32 = 13;
        const COLOR_HIGHLIGHTTEXT: i32 = 14;
        const COLOR_BTNFACE: i32 = 15;
        const COLOR_BTNSHADOW: i32 = 16;
        const COLOR_GRAYTEXT: i32 = 17;

        if is_high_contrast() {
            let window_text = system_color(COLOR_WINDOWTEXT);
            Palette {
                high_contrast: true,
                canvas: system_color(COLOR_BTNFACE),
                panel: system_color(COLOR_WINDOW),
                panel_subtle: system_color(COLOR_BTNFACE),
                divider: window_text,
                primary_text: window_text,
                secondary_text: system_color(COLOR_GRAYTEXT),
                selected: system_color(COLOR_HIGHLIGHT),
                banner: system_color(COLOR_HIGHLIGHT),
                banner_text: system_color(COLOR_HIGHLIGHTTEXT),
                banner_subtitle: system_color(COLOR_HIGHLIGHTTEXT),
                accent: system_color(COLOR_HIGHLIGHT),
                accent_hover: system_color(COLOR_HIGHLIGHT),
                accent_pressed: system_color(COLOR_HIGHLIGHT),
                success: window_text,
                danger: window_text,
                danger_subtle: system_color(COLOR_WINDOW),
                caution: window_text,
                meter_track: system_color(COLOR_BTNSHADOW),
                control_text: window_text,
                disabled_text: system_color(COLOR_GRAYTEXT),
            }
        } else {
            Palette {
                high_contrast: false,
                canvas: rgb(245, 247, 250),
                panel: rgb(255, 255, 255),
                panel_subtle: rgb(238, 243, 248),
                divider: rgb(207, 216, 228),
                primary_text: rgb(28, 39, 57),
                secondary_text: rgb(76, 91, 112),
                selected: rgb(218, 235, 250),
                banner: rgb(23, 37, 61),
                banner_text: rgb(255, 255, 255),
                banner_subtitle: rgb(195, 207, 224),
                accent: rgb(15, 108, 189),
                accent_hover: rgb(12, 90, 158),
                accent_pressed: rgb(10, 74, 130),
                success: rgb(22, 101, 72),
                danger: rgb(176, 45, 45),
                danger_subtle: rgb(252, 235, 235),
                caution: rgb(173, 108, 12),
                meter_track: rgb(226, 232, 240),
                control_text: rgb(28, 39, 57),
                disabled_text: rgb(150, 160, 174),
            }
        }
    })
}

/// 依 DPI 建立的一組字型；DPI 改變時整組重建。
pub struct Fonts {
    pub dpi: i32,
    pub body: Font,
    pub caption: Font,
    pub section_title: Font,
    pub header_title: Font,
    pub table_header: Font,
    /// 倒數專用的等寬字型：每秒重畫時數字不會左右跳動。
    pub mono: Font,
}

impl Fonts {
    pub fn new(dpi: i32) -> Self {
        Self {
            dpi,
            body: Font::new(FAMILY_NAME, 9.5, false, dpi),
            caption: Font::new(FAMILY_NAME, 8.5, false, dpi),
            section_title: Font::new(FAMILY_NAME, 12.5, true, dpi),
            header_title: Font::new(FAMILY_NAME, 20.0, true, dpi),
            table_header: Font::new(FAMILY_NAME, 9.5, true, dpi),
            mono: Font::new(MONO_FAMILY_NAME, 9.5, false, dpi),
        }
    }

    pub fn scale(&self, value: i32) -> i32 {
        w32::scale(value, self.dpi)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary = 0,
    Secondary = 1,
    Danger = 2,
}

const BS_OWNERDRAW: u32 = 0x0B;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const HOVER_FLAG: isize = 0x100;
const ODS_SELECTED: u32 = 0x0001;
const ODS_DISABLED: u32 = 0x0004;
const ODS_FOCUS: u32 = 0x0010;
const ODS_NOFOCUSRECT: u32 = 0x0200;

/// 建立自繪的平面按鈕。外觀由父視窗在 WM_DRAWITEM 呼叫 `draw_button` 繪製。
pub fn create_button(parent: HWND, id: i32, text: &str, variant: ButtonVariant) -> HWND {
    let button = w32::create_window(
        0,
        "BUTTON",
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        w32::rect(0, 0, 10, 10),
        Some(parent),
        id,
        std::ptr::null(),
    );
    unsafe {
        // BUTTON 類別不使用 GWLP_USERDATA，拿來記錄樣式與滑鼠停留狀態。
        SetWindowLongPtrW(button, GWLP_USERDATA, variant as isize);
        let _ = SetWindowSubclass(button, Some(button_subclass), 1, 0);
    }
    button
}

/// 按鈕依文字內容決定的寬度（含左右留白與最小寬度）。
pub fn button_width(fonts: &Fonts, text: &str) -> i32 {
    (fonts.body.measure(text).0 + fonts.scale(26)).max(fonts.scale(86))
}

pub fn button_height(fonts: &Fonts) -> i32 {
    (fonts.body.height() + fonts.scale(12)).max(fonts.scale(36))
}

unsafe extern "system" fn button_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match message {
        WM_MOUSEMOVE => {
            let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if data & HOVER_FLAG == 0 {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, data | HOVER_FLAG);
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                w32::invalidate(hwnd);
            }
        }
        WM_MOUSELEAVE => {
            let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, data & !HOVER_FLAG);
            w32::invalidate(hwnd);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

/// 繪製自繪按鈕；`background` 是按鈕所在容器的底色（用來填圓角外的角落）。
pub fn draw_button(item: &DRAWITEMSTRUCT, fonts: &Fonts, background: COLORREF) {
    let colors = palette();
    let data = unsafe { GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA) };
    let hovered = data & HOVER_FLAG != 0;
    let pressed = item.itemState.0 & ODS_SELECTED != 0;
    let disabled = item.itemState.0 & ODS_DISABLED != 0;
    let variant = match data & 0xff {
        0 => ButtonVariant::Primary,
        2 => ButtonVariant::Danger,
        _ => ButtonVariant::Secondary,
    };

    let (mut fill, mut text, mut border) = if colors.high_contrast {
        let face = if hovered || pressed { colors.selected } else { colors.canvas };
        (face, if hovered || pressed { colors.banner_text } else { colors.control_text }, colors.control_text)
    } else {
        match variant {
            ButtonVariant::Primary => {
                let face = if pressed {
                    colors.accent_pressed
                } else if hovered {
                    colors.accent_hover
                } else {
                    colors.accent
                };
                (face, rgb(255, 255, 255), face)
            }
            ButtonVariant::Danger => {
                let face = if pressed {
                    rgb(247, 218, 218)
                } else if hovered {
                    colors.danger_subtle
                } else {
                    colors.panel
                };
                (face, colors.danger, rgb(224, 170, 170))
            }
            ButtonVariant::Secondary => {
                let face = if pressed {
                    rgb(220, 228, 238)
                } else if hovered {
                    colors.panel_subtle
                } else {
                    colors.panel
                };
                (face, colors.primary_text, colors.divider)
            }
        }
    };
    if disabled && !colors.high_contrast {
        fill = colors.panel_subtle;
        text = colors.disabled_text;
        border = colors.divider;
    }

    let bounds = item.rcItem;
    let radius = fonts.scale(6);
    w32::fill(item.hDC, &bounds, background);
    w32::fill_round(item.hDC, &bounds, radius, border);
    let inner = windows::Win32::Foundation::RECT {
        left: bounds.left + 1,
        top: bounds.top + 1,
        right: bounds.right - 1,
        bottom: bounds.bottom - 1,
    };
    w32::fill_round(item.hDC, &inner, radius, fill);

    let label = w32::get_text(item.hwndItem);
    w32::draw_text(item.hDC, &fonts.body, &label, &bounds, text, w32::DT_LINE | w32::DT_CENTER);

    if item.itemState.0 & ODS_FOCUS != 0 && item.itemState.0 & ODS_NOFOCUSRECT == 0 {
        let focus = windows::Win32::Foundation::RECT {
            left: bounds.left + 3,
            top: bounds.top + 3,
            right: bounds.right - 3,
            bottom: bounds.bottom - 3,
        };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::DrawFocusRect(item.hDC, &focus);
        }
    }
}
