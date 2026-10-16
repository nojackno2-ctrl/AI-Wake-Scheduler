//! 即時終端機面板：把排程管理器的活動事件流顯示成一個會捲動的終端機畫面
//! （開始/完成/失敗以顏色標示，stdout/stderr 逐段輸出）。
//!
//! 事件可能來自任意背景執行緒（子程序讀取迴圈），這裡只把它們塞進執行緒安全佇列，
//! 實際畫面更新一律在 UI 執行緒的計時器批次處理，避免高頻輸出時灌爆 UI 訊息佇列。

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use aiwake_core::{CliActivityEvent, CliActivityKind, CliKind};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::System::LibraryLoader::LoadLibraryW;
use windows::Win32::UI::Controls::RichEdit::{CHARFORMAT2W, CHARRANGE, GETTEXTLENGTHEX, GETTEXTLENGTHEX_FLAGS};

use crate::ui::{WS_BORDER, WS_CHILD, WS_VISIBLE, WS_VSCROLL};
use crate::w32::{self, rgb, wide};

const MAX_RETAINED_CHARACTERS: usize = 200_000;
const TRIM_TO_CHARACTERS: usize = 120_000;
const MAX_ENTRIES_PER_FLUSH: usize = 400;

const BACKGROUND_COLOR: COLORREF = rgb(18, 20, 24);
const DEFAULT_TEXT_COLOR: COLORREF = rgb(214, 218, 224);
const STDERR_COLOR: COLORREF = rgb(240, 128, 118);
const STARTED_COLOR: COLORREF = rgb(120, 180, 240);
const COMPLETED_COLOR: COLORREF = rgb(120, 210, 150);
const FAILED_COLOR: COLORREF = rgb(240, 110, 110);
const TIMESTAMP_COLOR: COLORREF = rgb(120, 128, 140);

const EM_REPLACESEL: u32 = 0x00C2;
const EM_SETREADONLY: u32 = 0x00CF;
const EM_EXSETSEL: u32 = 0x0437;
const EM_SETBKGNDCOLOR: u32 = 0x0443;
const EM_SETCHARFORMAT: u32 = 0x0444;
const EM_GETTEXTLENGTHEX: u32 = 0x045F;
const WM_SETREDRAW: u32 = 0x000B;
const WM_VSCROLL: u32 = 0x0115;
const SB_BOTTOM: usize = 7;
const SCF_SELECTION: usize = 0x0001;
const CFM_COLOR: u32 = 0x4000_0000;

/// 執行緒安全的活動事件佇列；任何執行緒都可以排入。
#[derive(Clone, Default)]
pub struct ActivityQueue(Arc<Mutex<VecDeque<CliActivityEvent>>>);

impl ActivityQueue {
    pub fn push(&self, activity: CliActivityEvent) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push_back(activity);
    }

    fn take(&self, limit: usize) -> Vec<CliActivityEvent> {
        let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let count = limit.min(queue.len());
        queue.drain(..count).collect()
    }

    fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

pub struct Terminal {
    pub output: HWND,
    pub pending: ActivityQueue,
    last_writer: Cell<Option<CliKind>>,
    at_line_start: Cell<bool>,
}

impl Terminal {
    pub fn create(parent: HWND, id: i32) -> Self {
        const ES_MULTILINE: u32 = 0x0004;
        const ES_AUTOVSCROLL: u32 = 0x0040;
        const ES_READONLY: u32 = 0x0800;

        let library = wide("Msftedit.dll");
        unsafe {
            let _ = LoadLibraryW(PCWSTR(library.as_ptr()));
        }
        let output = w32::create_window(
            0,
            "RICHEDIT50W",
            "",
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_BORDER | ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY,
            w32::rect(0, 0, 10, 10),
            Some(parent),
            id,
            std::ptr::null(),
        );
        w32::send(output, EM_SETBKGNDCOLOR, 0, BACKGROUND_COLOR.0 as isize);

        Self { output, pending: ActivityQueue::default(), last_writer: Cell::new(None), at_line_start: Cell::new(true) }
    }

    pub fn clear(&self) {
        self.pending.clear();
        w32::send(self.output, EM_SETREADONLY, 0, 0);
        w32::set_text(self.output, "");
        w32::send(self.output, EM_SETREADONLY, 1, 0);
        self.last_writer.set(None);
        self.at_line_start.set(true);
    }

    /// 目前保留的字元數（RichEdit 內部以單一字元表示換行）。
    pub fn text_length(&self) -> usize {
        let query = GETTEXTLENGTHEX { flags: GETTEXTLENGTHEX_FLAGS(0), codepage: 1200 };
        w32::send(self.output, EM_GETTEXTLENGTHEX, &query as *const _ as usize, 0).max(0) as usize
    }

    pub fn text(&self) -> String {
        w32::get_text(self.output)
    }

    /// UI 執行緒上批次處理排隊事件，避免逐筆更新造成灌爆。
    pub fn flush(&self, auto_scroll: bool) {
        let entries = self.pending.take(MAX_ENTRIES_PER_FLUSH);
        if entries.is_empty() {
            return;
        }

        w32::send(self.output, WM_SETREDRAW, 0, 0);
        for activity in &entries {
            self.append_entry(activity);
        }
        self.trim_if_needed();
        w32::send(self.output, WM_SETREDRAW, 1, 0);

        if auto_scroll {
            self.select(self.text_length(), self.text_length());
            w32::send(self.output, WM_VSCROLL, SB_BOTTOM, 0);
        }
        w32::invalidate(self.output);
    }

    fn append_entry(&self, activity: &CliActivityEvent) {
        let cli_name = activity.cli.short_name();
        match activity.kind {
            CliActivityKind::Started => {
                self.append_line(&format!("▶ {cli_name} {}", activity.text), STARTED_COLOR, activity)
            }
            CliActivityKind::Completed => {
                self.append_line(&format!("✓ {cli_name} {}", activity.text), COMPLETED_COLOR, activity)
            }
            CliActivityKind::Failed => {
                self.append_line(&format!("✗ {cli_name} {}", activity.text), FAILED_COLOR, activity)
            }
            CliActivityKind::Output => self.append_stream(activity.cli, cli_name, &activity.text, DEFAULT_TEXT_COLOR),
            CliActivityKind::ErrorOutput => self.append_stream(activity.cli, cli_name, &activity.text, STDERR_COLOR),
        }
    }

    /// Started/Completed/Failed 一律另起獨立一行，並附上時間戳記。
    fn append_line(&self, text: &str, color: COLORREF, activity: &CliActivityEvent) {
        self.ensure_new_line();
        let timestamp = aiwake_core::time::local_naive(&activity.timestamp).format("%H:%M:%S");
        self.append_text(&format!("[{timestamp}] "), TIMESTAMP_COLOR);
        self.append_text(&format!("{text}\r\n"), color);
        self.last_writer.set(None);
        self.at_line_start.set(true);
    }

    /// stdout/stderr 的原始片段。多個 CLI 可能同時在跑，寫入者換人時才另起一行並標註來源，
    /// 減少每個片段都插入標籤造成的雜訊。
    fn append_stream(&self, writer: CliKind, cli_name: &str, text: &str, color: COLORREF) {
        if text.is_empty() {
            return;
        }

        if self.last_writer.get() != Some(writer) {
            self.ensure_new_line();
            self.append_text(&format!("[{cli_name}] "), TIMESTAMP_COLOR);
            self.last_writer.set(Some(writer));
        }

        self.append_text(text, color);
        self.at_line_start.set(text.ends_with('\n'));
    }

    fn ensure_new_line(&self) {
        if !self.at_line_start.get() && self.text_length() > 0 {
            self.append_text("\r\n", DEFAULT_TEXT_COLOR);
        }
    }

    fn select(&self, start: usize, end: usize) {
        let range = CHARRANGE { cpMin: start as i32, cpMax: end as i32 };
        w32::send(self.output, EM_EXSETSEL, 0, &range as *const _ as isize);
    }

    fn replace_selection(&self, text: &str) {
        // RichEdit 在唯讀狀態下拒絕取代選取範圍；只在這個同步操作期間暫時可寫。
        let text = wide(text);
        w32::send(self.output, EM_SETREADONLY, 0, 0);
        w32::send(self.output, EM_REPLACESEL, 0, text.as_ptr() as isize);
        w32::send(self.output, EM_SETREADONLY, 1, 0);
    }

    fn append_text(&self, text: &str, color: COLORREF) {
        let end = self.text_length();
        self.select(end, end);

        let mut format = CHARFORMAT2W::default();
        format.Base.cbSize = std::mem::size_of::<CHARFORMAT2W>() as u32;
        format.Base.dwMask = windows::Win32::UI::Controls::RichEdit::CFM_MASK(CFM_COLOR);
        format.Base.crTextColor = color;
        w32::send(self.output, EM_SETCHARFORMAT, SCF_SELECTION, &format as *const _ as isize);
        self.replace_selection(text);
    }

    /// 超過保留上限時從頭截掉舊內容。以取代選取範圍的方式刪除而不是重設全文，
    /// 才不會把剩餘內容的顏色格式一併洗掉。
    fn trim_if_needed(&self) {
        let length = self.text_length();
        if length <= MAX_RETAINED_CHARACTERS {
            return;
        }

        self.select(0, length - TRIM_TO_CHARACTERS);
        self.replace_selection("");
        self.at_line_start.set(true);
        self.last_writer.set(None);
    }
}
