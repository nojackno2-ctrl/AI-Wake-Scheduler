//! 把排程模型轉成畫面上的文字與色彩。
//! 與視窗分開後，格式化規則可以獨立驗證，視窗只負責擺放控制項。

use std::collections::HashMap;

use aiwake_core::time::{self, Moment, Span};
use aiwake_core::usage::exhausted_until;
use aiwake_core::{CliKind, CliUsageAvailability, CliUsageSnapshot, ScheduleStatus, ScheduledJob};

/// `hh:mm:ss` 形式的時分秒（不含天數）。
fn hours_minutes_seconds(span: Span) -> String {
    format!("{:02}:{:02}:{:02}", span.num_hours() % 24, span.num_minutes() % 60, span.num_seconds() % 60)
}

/// 距離某個時間點的剩餘時間；超過一天時加上天數。
fn remaining_text(remaining: Span) -> String {
    if remaining.num_days() >= 1 {
        format!("{}天 {}", remaining.num_days(), hours_minutes_seconds(remaining))
    } else {
        hours_minutes_seconds(remaining)
    }
}

pub fn time_of_day(value: Span) -> String {
    format!("{:02}:{:02}", value.num_hours(), value.num_minutes() % 60)
}

pub fn time(job: &ScheduledJob) -> String {
    let start = job.initial_time_of_day.unwrap_or_else(|| time::local_time_of_day(&job.scheduled_at));
    format!(
        "{}–{}{}",
        time_of_day(start),
        time_of_day(job.end_time_of_day),
        if job.end_time_of_day < start { "（隔日）" } else { "" }
    )
}

pub fn countdown(
    job: &ScheduledJob,
    now: &Moment,
    usage_snapshots: Option<&HashMap<CliKind, CliUsageSnapshot>>,
) -> String {
    match job.status {
        ScheduleStatus::Running => return "執行中".to_string(),
        ScheduleStatus::Completed | ScheduleStatus::Failed => return "已執行".to_string(),
        _ => {}
    }
    if !job.enabled {
        return "已停用".to_string();
    }

    if let (Some(snapshots), false) = (usage_snapshots, job.targets.is_empty()) {
        let parts: Vec<String> = job
            .targets
            .iter()
            .map(|target| {
                let snapshot =
                    snapshots.get(target).filter(|snapshot| snapshot.availability == CliUsageAvailability::Available);
                let state = match snapshot.and_then(|snapshot| snapshot.short_window_or_first()) {
                    None => "尚未讀取".to_string(),
                    // 例如每週額度已用完：排程在重置前不會呼叫。
                    Some(_) if exhausted_until(snapshot, now).is_some() => {
                        format!("額度用盡 {}", remaining_text(exhausted_until(snapshot, now).unwrap_or(*now) - *now))
                    }
                    Some(window) => match window.resets_at.filter(|_| window.is_active_countdown) {
                        None => "未倒數".to_string(),
                        Some(resets_at) if resets_at <= *now => "即將重置".to_string(),
                        Some(resets_at) => remaining_text(resets_at - *now),
                    },
                };
                format!("{} {state}", target.short_name())
            })
            .collect();
        return parts.join("；");
    }

    let general_remaining = job.scheduled_at - *now;
    if general_remaining <= Span::zero() {
        "即將執行".to_string()
    } else {
        remaining_text(general_remaining)
    }
}

pub fn status(status: ScheduleStatus) -> &'static str {
    match status {
        ScheduleStatus::Pending => "等待中",
        ScheduleStatus::Running => "執行中",
        ScheduleStatus::Completed => "成功",
        ScheduleStatus::Failed => "失敗",
        ScheduleStatus::Disabled => "已停用",
    }
}

pub fn targets(targets: &[CliKind]) -> String {
    targets.iter().map(|kind| kind.short_name()).collect::<Vec<_>>().join("、")
}

pub fn targets_long(targets: &[CliKind]) -> String {
    targets.iter().map(|kind| kind.display_name()).collect::<Vec<_>>().join("、")
}

pub fn results(job: &ScheduledJob) -> String {
    job.last_results
        .iter()
        .map(|result| {
            format!(
                "{}：{}\r\n結束碼：{}\r\n錯誤：{}\r\n日誌：{}",
                result.cli.display_name(),
                if result.succeeded { "成功" } else { "失敗" },
                result.exit_code.map(|code| code.to_string()).unwrap_or_else(|| "無".to_string()),
                if result.error.trim().is_empty() { "無" } else { &result.error },
                result.log_path,
            )
        })
        .collect::<Vec<_>>()
        .join("\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiwake_core::time::local_wall_clock;
    use aiwake_core::{CliRunResult, CliUsageWindow};

    fn job() -> ScheduledJob {
        ScheduledJob {
            scheduled_at: local_wall_clock(2026, 10, 7, 8, 49, 0),
            initial_time_of_day: Some(Span::hours(8) + Span::minutes(49)),
            targets: vec![CliKind::Codex, CliKind::Claude],
            ..ScheduledJob::default()
        }
    }

    #[test]
    fn time_column_shows_detection_window() {
        assert_eq!(time(&job()), "08:49–24:00");
        assert_eq!(time(&ScheduledJob { end_time_of_day: Span::hours(6), ..job() }), "08:49–06:00（隔日）");
    }

    #[test]
    fn countdown_shows_each_cli_independently() {
        let now = local_wall_clock(2026, 10, 7, 9, 0, 0);
        let mut snapshots = HashMap::new();
        snapshots.insert(
            CliKind::Codex,
            CliUsageSnapshot::new(
                CliKind::Codex,
                CliUsageAvailability::Available,
                vec![CliUsageWindow::new("5h", 10, Some(Span::hours(5)), Some(now + Span::minutes(90)), true)],
                "讀取成功",
                now,
            ),
        );

        let interval = job();
        assert_eq!(countdown(&interval, &now, Some(&snapshots)), "Codex 01:30:00；Claude 尚未讀取");

        snapshots.insert(
            CliKind::Claude,
            CliUsageSnapshot::new(
                CliKind::Claude,
                CliUsageAvailability::Available,
                vec![
                    CliUsageWindow::new("7d", 20, Some(Span::days(7)), Some(now + Span::days(2)), true),
                    CliUsageWindow::new("5h", 0, Some(Span::hours(5)), None, false),
                ],
                "讀取成功",
                now,
            ),
        );
        assert_eq!(
            countdown(&interval, &now, Some(&snapshots)),
            "Codex 01:30:00；Claude 未倒數",
            "應優先採用 5 小時視窗而不是第一個視窗。"
        );

        // 每週額度用盡時，即使 5 小時視窗未倒數也顯示用盡與剩餘時間。
        snapshots.insert(
            CliKind::Claude,
            CliUsageSnapshot::new(
                CliKind::Claude,
                CliUsageAvailability::Available,
                vec![
                    CliUsageWindow::new("5h", 0, Some(Span::hours(5)), None, false),
                    CliUsageWindow::new("7d", 100, Some(Span::days(7)), Some(now + Span::days(2)), true),
                ],
                "讀取成功",
                now,
            ),
        );
        assert_eq!(countdown(&interval, &now, Some(&snapshots)), "Codex 01:30:00；Claude 額度用盡 2天 00:00:00");
    }

    #[test]
    fn countdown_without_quota_and_states() {
        let now = local_wall_clock(2026, 10, 7, 8, 0, 0);
        let mut daily = job();
        assert_eq!(countdown(&daily, &now, None), "00:49:00");
        daily.scheduled_at = now + Span::days(1) + Span::hours(2);
        assert_eq!(countdown(&daily, &now, None), "1天 02:00:00");
        daily.scheduled_at = now;
        assert_eq!(countdown(&daily, &now, None), "即將執行");

        daily.enabled = false;
        assert_eq!(countdown(&daily, &now, None), "已停用");
        daily.status = ScheduleStatus::Running;
        assert_eq!(countdown(&daily, &now, None), "執行中");
        daily.status = ScheduleStatus::Failed;
        assert_eq!(countdown(&daily, &now, None), "已執行");
    }

    #[test]
    fn targets_and_results_text() {
        assert_eq!(targets(&[CliKind::Antigravity, CliKind::Claude]), "AGY(Gemini)、Claude");
        assert_eq!(targets(&[]), "");
        assert_eq!(targets_long(&[CliKind::Codex, CliKind::Claude]), "Codex CLI、Claude CLI");

        let mut finished = job();
        let mut ok = CliRunResult::new(CliKind::Codex);
        ok.succeeded = true;
        ok.exit_code = Some(0);
        ok.log_path = "C:\\logs\\a.log".to_string();
        let mut failed = CliRunResult::new(CliKind::Claude);
        failed.error = "CLI 結束碼為 1。".to_string();
        finished.last_results = vec![ok, failed];
        assert_eq!(
            results(&finished),
            "Codex CLI：成功\r\n結束碼：0\r\n錯誤：無\r\n日誌：C:\\logs\\a.log\r\n\r\nClaude CLI：失敗\r\n結束碼：無\r\n錯誤：CLI 結束碼為 1。\r\n日誌："
        );
        assert_eq!(status(ScheduleStatus::Completed), "成功");
    }
}
