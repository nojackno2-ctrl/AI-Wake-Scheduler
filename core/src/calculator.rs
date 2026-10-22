//! 提供排程下一次執行時間的計算邏輯。

use crate::error::{CoreError, CoreResult};
use crate::time::{self, Moment, Span};

/// 偵測時段內讀取所有目標倒數狀態的間隔（10 分鐘）。
pub fn auto_probe_interval() -> Span {
    Span::minutes(10)
}

/// 每日偵測時段：開始包含、結束不包含；結束可為 24:00，較早則跨午夜。
pub fn validate_window(start: Span, end: Span) -> CoreResult<()> {
    if !time::is_valid_time_of_day(start) || start != time::truncate_to_minutes(start) {
        return Err(CoreError::OutOfRange("開始時間必須介於 00:00 到 23:59，且只包含時與分。".to_string()));
    }
    if end < Span::zero() || end > Span::days(1) || end != time::truncate_to_minutes(end) {
        return Err(CoreError::OutOfRange("結束時間必須介於 00:00 到 24:00，且只包含時與分。".to_string()));
    }
    if start == end || (start == Span::zero() && end == Span::zero()) {
        return Err(CoreError::InvalidArgument("開始與結束時間不可相同；全天請設定 00:00 至 24:00。".to_string()));
    }
    Ok(())
}

/// 取得目前所在時段；不在時段內時，傳回下一段的開始與結束。
pub fn detection_window(start: Span, end: Span, now: &Moment) -> CoreResult<(Moment, Moment)> {
    validate_window(start, end)?;
    let local = time::local_naive(now);
    let midnight = local.date().and_time(chrono::NaiveTime::MIN);
    let mut begin = midnight + start;
    let mut finish = midnight + end;
    if end < start {
        finish += Span::days(1);
        if local < midnight + end {
            begin -= Span::days(1);
            finish -= Span::days(1);
        }
    }
    if local >= finish {
        begin += Span::days(1);
        finish += Span::days(1);
    }
    Ok((time::from_local_naive(begin), time::from_local_naive(finish)))
}

pub fn in_detection_window(start: Span, end: Span, now: &Moment) -> bool {
    detection_window(start, end, now).is_ok_and(|(begin, finish)| begin <= *now && *now < finish)
}

/// 將下一次偵測限制於有效時段內。
pub fn next_detection(start: Span, end: Span, now: &Moment, delay: Span) -> CoreResult<Moment> {
    let candidate = *now + delay;
    let (begin, finish) = detection_window(start, end, now)?;
    if candidate < begin {
        Ok(begin)
    } else if candidate < finish {
        Ok(candidate)
    } else {
        Ok(detection_window(start, end, &finish)?.0)
    }
}
