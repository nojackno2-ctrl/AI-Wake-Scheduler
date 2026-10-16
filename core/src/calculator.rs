//! 提供排程下一次執行時間的計算邏輯。

use chrono::Timelike;

use crate::error::{CoreError, CoreResult};
use crate::models::ScheduleRecurrence;
use crate::time::{self, Moment, Span};

/// 自動模式的循環間隔（5 個小時 + 1 分鐘 = 301 分鐘）。
pub fn auto_interval() -> Span {
    Span::minutes(301)
}

fn validate(time_of_day: Span, message: &str) -> CoreResult<()> {
    if time::is_valid_time_of_day(time_of_day) {
        Ok(())
    } else {
        Err(CoreError::OutOfRange(message.to_string()))
    }
}

/// 計算下一次每日排程的執行時間。
pub fn get_next_daily_occurrence(time_of_day: Span, now: &Moment) -> CoreResult<Moment> {
    validate(time_of_day, "每日時間必須介於 00:00 到 23:59。")?;

    let local_now = time::local_naive(now);
    let mut local_candidate =
        local_now.date().and_time(chrono::NaiveTime::MIN) + time::truncate_to_minutes(time_of_day);
    if local_candidate <= local_now {
        local_candidate += Span::days(1);
    }
    Ok(time::from_local_naive(local_candidate))
}

/// 計算「不晚於現在」的最近一次每日執行時間。
/// 用來判斷程式關閉期間是否錯過了今天的喚醒。
pub fn get_previous_daily_occurrence(time_of_day: Span, now: &Moment) -> CoreResult<Moment> {
    validate(time_of_day, "每日時間必須介於 00:00 到 23:59。")?;

    let local_now = time::local_naive(now);
    let mut local_candidate =
        local_now.date().and_time(chrono::NaiveTime::MIN) + time::truncate_to_minutes(time_of_day);
    if local_candidate > local_now {
        local_candidate -= Span::days(1);
    }
    Ok(time::from_local_naive(local_candidate))
}

/// 計算自動模式（每 5 小時 1 分鐘）在指定執行完成時間後的下一次執行時間。
/// 當日完成後若加 5 小時 1 分鐘仍在當日（24:00 前），排在該時間；
/// 若跨入隔天（超過 24:00），則不排在半夜，直接排定為隔天的首次喚醒時間。
pub fn get_next_auto_interval_occurrence(initial_time_of_day: Span, finished_at: &Moment) -> CoreResult<Moment> {
    validate(initial_time_of_day, "首次喚醒時間必須介於 00:00 到 23:59。")?;

    let local_finished = time::local_naive(finished_at);
    let candidate_local = local_finished + auto_interval();

    // 若推算時間仍在完成當日（未跨過 24:00）
    if candidate_local.date() == local_finished.date() {
        return Ok(time::from_local_naive(candidate_local));
    }

    // 跨日：不於半夜執行，排定為隔日的首次喚醒時間
    let next_day = local_finished.date().and_time(chrono::NaiveTime::MIN) + Span::days(1);
    Ok(time::from_local_naive(next_day + time::truncate_to_minutes(initial_time_of_day)))
}

/// 計算指定週期下一次的執行時間。
pub fn get_next_occurrence(
    previous_occurrence: &Moment,
    recurrence: ScheduleRecurrence,
    now: &Moment,
) -> CoreResult<Moment> {
    let increment_days = match recurrence {
        ScheduleRecurrence::Once => {
            return Err(CoreError::InvalidArgument("單次排程沒有下一次執行時間。".to_string()));
        }
        ScheduleRecurrence::Interval => {
            if previous_occurrence > now {
                return Ok(*previous_occurrence);
            }

            let initial_time = time::local_time_of_day(previous_occurrence);
            let mut next = get_next_auto_interval_occurrence(initial_time, previous_occurrence)?;
            while next <= *now {
                next = get_next_auto_interval_occurrence(initial_time, &next)?;
            }
            return Ok(next);
        }
        ScheduleRecurrence::Daily => 1,
        ScheduleRecurrence::Weekly => 7,
        ScheduleRecurrence::Countdown => {
            return Err(CoreError::OutOfRange("此週期沒有固定的下一次執行時間。".to_string()));
        }
    };

    let previous_local = time::local_naive(previous_occurrence);
    let local_now = time::local_naive(now);

    // 若前次時間處於未來（例如系統時鐘倒退或時區變更），依據目前時間重新計算
    if previous_local > local_now {
        let target_time = Span::hours(previous_local.hour() as i64) + Span::minutes(previous_local.minute() as i64);
        let mut base_candidate = local_now.date().and_time(chrono::NaiveTime::MIN) + target_time;
        if base_candidate <= local_now {
            base_candidate += Span::days(increment_days);
        }
        return Ok(time::from_local_naive(base_candidate));
    }

    // 使用日數差快速推算，避免過多迴圈
    let days_diff = (local_now.date() - previous_local.date()).num_days();
    let periods = days_diff / increment_days;
    let mut next_local = previous_local + Span::days(periods * increment_days);

    while time::from_local_naive(next_local) <= *now {
        next_local += Span::days(increment_days);
    }
    Ok(time::from_local_naive(next_local))
}
