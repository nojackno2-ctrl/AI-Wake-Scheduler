//! 提供排程下一次執行時間的計算邏輯。

use crate::error::{CoreError, CoreResult};
use crate::time::{self, Moment, Span};
use chrono::Timelike;

pub fn validate_interval(minutes: i32) -> CoreResult<()> {
    if !(1..=60).contains(&minutes) {
        return Err(CoreError::OutOfRange("檢查與呼叫間隔必須為 1 至 60 分鐘。".into()));
    }
    Ok(())
}

/// 此分鐘是否為整點起算的刻度；秒數容許背景工作正常啟動的延遲。
pub fn aligned_minute(now: &Moment, minutes: i32) -> bool {
    validate_interval(minutes).is_ok() && time::local_naive(now).minute().is_multiple_of(minutes as u32)
}

/// 下一個有效刻度；inclusive 容許目前刻度所在分鐘，否則取之後的刻度。
pub fn next_aligned_detection(
    start: Span,
    end: Span,
    now: &Moment,
    minutes: i32,
    inclusive: bool,
) -> CoreResult<Moment> {
    validate_interval(minutes)?;
    validate_window(start, end)?;
    let local = time::local_naive(now);
    let mut candidate = local
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .ok_or_else(|| CoreError::InvalidOperation("無法計算分鐘刻度。".into()))?;
    if !inclusive {
        candidate += Span::minutes(1);
    }
    for _ in 0..=4320 {
        let moment = time::from_local_naive(candidate);
        if candidate.minute().is_multiple_of(minutes as u32) && in_detection_window(start, end, &moment) {
            return Ok(if inclusive && moment < *now { *now } else { moment });
        }
        candidate += Span::minutes(1);
    }
    Err(CoreError::InvalidArgument("此時段內沒有符合間隔的分鐘刻度，請調整時間或間隔。".into()))
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
