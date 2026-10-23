//! 時間工具：排程以系統本地時區的牆上時間運作，
//! 所有「本地時間」換算都集中在這裡，並妥善處理日光節約時間。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use chrono::{DateTime, FixedOffset, Local, LocalResult, NaiveDate, NaiveDateTime, TimeDelta, TimeZone, Utc};

/// 帶 UTC 位移的時間點。
pub type Moment = DateTime<FixedOffset>;
/// 時間長度（可為負）。
pub type Span = TimeDelta;

/// 時間來源的抽象；測試可用可控時鐘取代。
pub trait Clock: Send + Sync {
    /// 目前時間（以本地時區位移表示）。
    fn now(&self) -> Moment;

    /// 是否為真實系統時鐘。
    fn is_system(&self) -> bool {
        false
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Moment {
        now()
    }

    fn is_system(&self) -> bool {
        true
    }
}

pub fn now() -> Moment {
    Local::now().fixed_offset()
}

pub fn to_local(moment: &Moment) -> Moment {
    moment.with_timezone(&Local).fixed_offset()
}

/// 時間點在本地時區的牆上時間。
pub fn local_naive(moment: &Moment) -> NaiveDateTime {
    moment.with_timezone(&Local).naive_local()
}

pub fn time_of_day(local: &NaiveDateTime) -> Span {
    local.signed_duration_since(local.date().and_time(chrono::NaiveTime::MIN))
}

pub fn local_time_of_day(moment: &Moment) -> Span {
    time_of_day(&local_naive(moment))
}

/// 只保留時與分（捨去秒）。
pub fn truncate_to_minutes(span: Span) -> Span {
    Span::minutes(span.num_minutes())
}

pub fn is_valid_time_of_day(span: Span) -> bool {
    span >= Span::zero() && span < Span::days(1)
}

/// 安全地把本地牆上時間轉為時間點，並處理日光節約時間 (DST) 的無效與重複時段。
pub fn from_local_naive(local: NaiveDateTime) -> Moment {
    match Local.from_local_datetime(&local) {
        LocalResult::Single(value) => value.fixed_offset(),
        // 重複時段取標準時間（位移較小者），與原本以標準位移解讀的行為一致。
        LocalResult::Ambiguous(first, second) => {
            let first = first.fixed_offset();
            let second = second.fixed_offset();
            if first.offset().local_minus_utc() <= second.offset().local_minus_utc() {
                first
            } else {
                second
            }
        }
        // 落在 DST 無效時間，往後平移至有效時間區段（通常為 1 小時）。
        LocalResult::None => match Local.from_local_datetime(&(local + Span::hours(1))) {
            LocalResult::Single(value) => value.fixed_offset(),
            LocalResult::Ambiguous(first, _) => first.fixed_offset(),
            LocalResult::None => Utc.from_utc_datetime(&local).fixed_offset(),
        },
    }
}

/// 以本地時區的牆上時間建立時間點。
pub fn local_wall_clock(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Moment {
    let local = NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, second))
        .expect("無效的日期時間");
    from_local_naive(local)
}

pub fn from_unix_seconds(seconds: i64) -> Option<Moment> {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|value| value.fixed_offset())
}

fn fraction_ticks(nanoseconds: u32) -> String {
    let ticks = (nanoseconds % 1_000_000_000) / 100;
    if ticks == 0 {
        String::new()
    } else {
        format!(".{ticks:07}").trim_end_matches('0').to_string()
    }
}

/// 資料檔使用的 ISO 8601 格式（小數秒最多 7 位並去除尾端的 0），與舊版資料檔相容。
pub fn format_json_moment(moment: &Moment) -> String {
    format!(
        "{}{}{}",
        moment.format("%Y-%m-%dT%H:%M:%S"),
        fraction_ticks(moment.timestamp_subsec_nanos()),
        moment.format("%:z")
    )
}

/// 往返格式（固定 7 位小數秒），用於日誌。
pub fn format_round_trip(moment: &Moment) -> String {
    format!(
        "{}.{:07}{}",
        moment.format("%Y-%m-%dT%H:%M:%S"),
        (moment.timestamp_subsec_nanos() % 1_000_000_000) / 100,
        moment.format("%:z")
    )
}

/// 寬鬆解析日期時間字串；沒有時區資訊時視為本地時間。
pub fn parse_moment(text: &str) -> Option<Moment> {
    let text = text.trim();
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Some(value);
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y/%m/%d %H:%M:%S%.f"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(text, format) {
            return Some(from_local_naive(value));
        }
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f%:z", "%Y-%m-%dT%H:%M:%S%.f%z"] {
        if let Ok(value) = DateTime::parse_from_str(text, format) {
            return Some(value);
        }
    }
    DateTime::parse_from_rfc2822(text).ok()
}

/// 資料檔使用的時間長度格式：`[-][d.]hh:mm:ss[.fffffff]`。
pub fn format_span(span: Span) -> String {
    let negative = span < Span::zero();
    let span = span.abs();
    let total_seconds = span.num_seconds();
    let days = total_seconds / 86_400;
    let mut text = String::new();
    if negative {
        text.push('-');
    }
    if days > 0 {
        text.push_str(&format!("{days}."));
    }
    text.push_str(&format!(
        "{:02}:{:02}:{:02}{}",
        (total_seconds / 3600) % 24,
        (total_seconds / 60) % 60,
        total_seconds % 60,
        fraction_ticks(span.subsec_nanos().unsigned_abs())
    ));
    text
}

pub fn parse_span(text: &str) -> Option<Span> {
    let text = text.trim();
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }

    let (days, hours) = match parts[0].split_once('.') {
        Some((days, hours)) => (days.parse::<i64>().ok()?, hours.parse::<i64>().ok()?),
        None => (0, parts[0].parse::<i64>().ok()?),
    };
    let minutes = parts[1].parse::<i64>().ok()?;
    let (seconds, nanoseconds) = match parts.get(2) {
        None => (0, 0),
        Some(value) => match value.split_once('.') {
            Some((whole, fraction)) => {
                if fraction.is_empty() || fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let padded = format!("{fraction:0<9}");
                (whole.parse::<i64>().ok()?, padded.parse::<i64>().ok()?)
            }
            None => (value.parse::<i64>().ok()?, 0),
        },
    };
    if days < 0 || !(0..24).contains(&hours) || !(0..60).contains(&minutes) || !(0..60).contains(&seconds) {
        return None;
    }

    let span = Span::try_days(days)?
        .checked_add(&Span::hours(hours))?
        .checked_add(&Span::minutes(minutes))?
        .checked_add(&Span::seconds(seconds))?
        .checked_add(&Span::nanoseconds(nanoseconds))?;
    Some(if negative { -span } else { span })
}

/// 把非負的時間長度轉成可用於等待的 `std::time::Duration`。
pub fn to_std(span: Span) -> std::time::Duration {
    span.to_std().unwrap_or(std::time::Duration::ZERO)
}

/// 合作式取消旗標，並可作為可被中斷的等待。
#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

struct CancelInner {
    canceled: AtomicBool,
    lock: Mutex<()>,
    signal: Condvar,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancelInner {
                canceled: AtomicBool::new(false),
                lock: Mutex::new(()),
                signal: Condvar::new(),
            }),
        }
    }

    /// 永遠不會被取消的旗標。
    pub fn none() -> Self {
        Self::new()
    }

    pub fn cancel(&self) {
        self.inner.canceled.store(true, Ordering::SeqCst);
        let _guard = self.inner.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.inner.signal.notify_all();
    }

    pub fn is_canceled(&self) -> bool {
        self.inner.canceled.load(Ordering::SeqCst)
    }

    /// 等待指定時間；期間被取消則提早返回 true。
    pub fn wait(&self, duration: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + duration;
        let mut guard = self.inner.lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if self.is_canceled() {
                return true;
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            guard = self.inner.signal.wait_timeout(guard, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}
