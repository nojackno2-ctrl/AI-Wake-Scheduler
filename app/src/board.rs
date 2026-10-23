//! 與繪圖後端無關的額度資料與格式化規則。
use aiwake_core::catalog::{self, CliDescriptor};
use aiwake_core::time::{self, Moment, Span};
use aiwake_core::{CliKind, CliUsageAvailability, CliUsageSnapshot};
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, PartialEq)]
pub struct BoardRow {
    pub label: String,
    pub remaining_percent: i32,
    pub percent_text: String,
    pub resets_at: Option<Moment>,
    pub is_counting_down: bool,
    pub duration: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardGroup {
    pub availability: CliUsageAvailability,
    pub message: String,
    pub rows: Vec<BoardRow>,
    pub observed_at: Moment,
    pub requires_login: bool,
}

#[derive(Default)]
pub struct UsageBoard {
    groups: HashMap<CliKind, BoardGroup>,
    sources: HashMap<CliKind, CliUsageSnapshot>,
    login_in_progress: HashSet<CliKind>,
}
impl UsageBoard {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn apply(&mut self, snapshots: &HashMap<CliKind, CliUsageSnapshot>, _now: Moment) -> bool {
        if self.sources == *snapshots {
            return false;
        }
        self.sources.clone_from(snapshots);
        self.groups =
            snapshots.iter().map(|(kind, snapshot)| (*kind, build_group(catalog::get(*kind), snapshot))).collect();
        true
    }
    pub fn group(&self, kind: CliKind) -> Option<&BoardGroup> {
        self.groups.get(&kind)
    }
    pub fn set_login_in_progress(&mut self, kind: CliKind, in_progress: bool) -> bool {
        if in_progress {
            self.login_in_progress.insert(kind)
        } else {
            self.login_in_progress.remove(&kind)
        }
    }
}
pub fn format_countdown(remaining: Span) -> String {
    if remaining.num_days() >= 1 {
        format!("{}天 {:02}:{:02}", remaining.num_days(), remaining.num_hours() % 24, remaining.num_minutes() % 60)
    } else {
        format!("{:02}:{:02}:{:02}", remaining.num_hours(), remaining.num_minutes() % 60, remaining.num_seconds() % 60)
    }
}

pub fn format_reset_stamp(now: &Moment, resets_at: &Moment) -> String {
    let local = time::local_naive(resets_at);
    if local.date() == time::local_naive(now).date() {
        format!("{} 重置", local.format("%H:%M"))
    } else {
        format!("{} 重置", local.format("%m/%d %H:%M"))
    }
}

pub fn format_freshness(now: &Moment, observed_at: &Moment) -> String {
    let age = *now - *observed_at;
    if age < Span::seconds(45) {
        "剛更新".to_string()
    } else if age < Span::hours(1) {
        format!("{} 分鐘前更新", age.num_minutes())
    } else if age < Span::days(1) {
        format!("{} 小時前更新", age.num_hours())
    } else {
        format!("{} 更新", time::local_naive(observed_at).format("%m/%d %H:%M"))
    }
}

// ── 資料整形 ──────────────────────────────────────────────────

pub fn build_group(descriptor: &CliDescriptor, snapshot: &CliUsageSnapshot) -> BoardGroup {
    let group = |message: &str, rows: Vec<BoardRow>, requires_login: bool| BoardGroup {
        availability: snapshot.availability,
        message: message.to_string(),
        rows,
        observed_at: snapshot.observed_at,
        requires_login,
    };

    if snapshot.availability != CliUsageAvailability::Available {
        return group(&snapshot.message, Vec::new(), snapshot.requires_login);
    }
    if snapshot.windows.is_empty() {
        return group("沒有可顯示的額度視窗", Vec::new(), false);
    }
    group(&snapshot.message, build_rows(descriptor, snapshot), false)
}

fn build_rows(descriptor: &CliDescriptor, snapshot: &CliUsageSnapshot) -> Vec<BoardRow> {
    let windows = &snapshot.windows;
    let mut labels: Vec<String> = windows
        .iter()
        .map(|window| duration_label(window.duration).unwrap_or_else(|| clean_window_name(&window.name, descriptor)))
        .collect();

    // 期間長度相同的視窗（例如 Claude 的多個 7 天額度）光看期間分不出來，改用原始名稱區分。
    for i in 0..labels.len() {
        for j in i + 1..labels.len() {
            if labels[i] != labels[j] {
                continue;
            }
            labels[i] = clean_window_name(&windows[i].name, descriptor);
            labels[j] = clean_window_name(&windows[j].name, descriptor);
        }
    }

    windows
        .iter()
        .zip(labels)
        .map(|(window, label)| BoardRow {
            label,
            remaining_percent: window.remaining_percent(),
            percent_text: format!("{}%", window.remaining_percent()),
            resets_at: window.resets_at,
            is_counting_down: window.is_active_countdown,
            duration: window.duration,
        })
        .collect()
}

/// 選出圓環要顯示的列與固定位置的每週列：圓環固定顯示 5 小時視窗，
/// 沒有 5 小時視窗時才改顯示每週額度。
pub fn primary_and_weekly(rows: &[BoardRow]) -> (Option<usize>, Option<usize>) {
    let weekly = rows.iter().position(is_weekly);
    let short = rows.iter().position(|row| row.duration.is_some_and(|d| d <= Span::hours(6)));
    (short.or(weekly).or((!rows.is_empty()).then_some(0)), weekly)
}

fn is_weekly(row: &BoardRow) -> bool {
    row.duration
        .filter(|d| *d > Span::zero())
        .map_or(row.label == "每週", |d| (d.num_seconds() as f64 / 86_400.0 - 7.0).abs() < 0.05)
}

/// 由額度視窗的週期長度推出短標籤，這是各 CLI 之間最一致的說法。
pub fn duration_label(duration: Option<Span>) -> Option<String> {
    let value = duration.filter(|value| *value > Span::zero())?;
    let total_days = value.num_seconds() as f64 / 86_400.0;

    Some(if value < Span::hours(1) {
        format!("{} 分鐘", (value.num_seconds() as f64 / 60.0).round() as i64)
    } else if value < Span::days(1) {
        format!("{} 小時", (value.num_seconds() as f64 / 3600.0).round() as i64)
    } else if (total_days - 1.0).abs() < 0.05 {
        "每日".to_string()
    } else if (total_days - 7.0).abs() < 0.05 {
        "每週".to_string()
    } else if (total_days - 30.0).abs() < 1.5 {
        "每月".to_string()
    } else {
        format!("{} 天", total_days.round() as i64)
    })
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    (!prefix.is_empty() && text.len() >= prefix.len() && text.is_char_boundary(prefix.len()))
        .then(|| text.split_at(prefix.len()))
        .filter(|(head, _)| head.eq_ignore_ascii_case(prefix))
        .map(|(_, tail)| tail)
}

fn remove_ignore_case(text: &str, pattern: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while let Some(found) = lower[position..].find(&pattern) {
        result.push_str(&text[position..position + found]);
        result.push(' ');
        position += found + pattern.len();
    }
    result.push_str(&text[position..]);
    result
}

/// 把 API 回傳的視窗名稱縮成標籤：去掉與 CLI 重複的前綴、拆掉括號，
/// 再把常見的英文額度名稱換成中文。
pub fn clean_window_name(name: &str, descriptor: &CliDescriptor) -> String {
    let trimmed = name.trim();
    let text = strip_prefix_ignore_case(trimmed, descriptor.display_name)
        .or_else(|| strip_prefix_ignore_case(trimmed, descriptor.short_name))
        .unwrap_or(trimmed);

    let text: String = text.chars().map(|c| if matches!(c, '（' | '）' | '(' | ')') { ' ' } else { c }).collect();
    let text = remove_ignore_case(&remove_ignore_case(&text, "Limit Remaining"), "Remaining");
    let text = text.split(' ').filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");

    let lower = text.to_ascii_lowercase();
    if lower.contains("five hour") {
        "5 小時".to_string()
    } else if lower.contains("weekly") {
        "每週".to_string()
    } else if lower.contains("daily") {
        "每日".to_string()
    } else if lower.contains("monthly") {
        "每月".to_string()
    } else if text.is_empty() {
        "額度".to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiwake_core::time::local_wall_clock;
    use aiwake_core::CliUsageWindow;

    #[test]
    fn labels_come_from_duration_then_name() {
        assert_eq!(duration_label(Some(Span::hours(5))).as_deref(), Some("5 小時"));
        assert_eq!(duration_label(Some(Span::days(7))).as_deref(), Some("每週"));
        assert_eq!(duration_label(Some(Span::days(1))).as_deref(), Some("每日"));
        assert_eq!(duration_label(Some(Span::days(30))).as_deref(), Some("每月"));
        assert_eq!(duration_label(Some(Span::minutes(30))).as_deref(), Some("30 分鐘"));
        assert_eq!(duration_label(Some(Span::days(3))).as_deref(), Some("3 天"));
        assert_eq!(duration_label(None), None);

        let agy = catalog::get(CliKind::Antigravity);
        assert_eq!(clean_window_name("Antigravity (Gemini)（Five Hour Limit Remaining）", agy), "5 小時");
        assert_eq!(clean_window_name("Antigravity (Gemini)（Weekly Limit Remaining）", agy), "每週");
        let argon = catalog::get(CliKind::AntigravityArgon);
        assert_eq!(clean_window_name("Antigravity (Argon)（Gemini 4 Argon Weekly Limit）", argon), "每週");
        assert_eq!(clean_window_name("  ", agy), "額度");
        let claude = catalog::get(CliKind::Claude);
        assert_eq!(clean_window_name("Claude Opus（7 天）", claude), "Opus 7 天");
    }

    #[test]
    fn same_duration_windows_fall_back_to_names() {
        let now = local_wall_clock(2026, 10, 7, 9, 0, 0);
        let snapshot = CliUsageSnapshot::new(
            CliKind::Claude,
            CliUsageAvailability::Available,
            vec![
                CliUsageWindow::new("Claude（5 小時）", 43, Some(Span::hours(5)), Some(now + Span::hours(2)), true),
                CliUsageWindow::new("Claude（7 天）", 87, Some(Span::days(7)), Some(now + Span::days(2)), true),
                CliUsageWindow::new("Claude Opus（7 天）", 10, Some(Span::days(7)), None, false),
            ],
            "讀取成功",
            now,
        );
        let group = build_group(catalog::get(CliKind::Claude), &snapshot);
        let labels: Vec<&str> = group.rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["5 小時", "7 天", "Opus 7 天"]);
        assert_eq!(group.rows[0].percent_text, "57%");
        assert!(group.rows[0].is_counting_down && !group.rows[2].is_counting_down);
        assert_eq!(primary_and_weekly(&group.rows), (Some(0), Some(1)));
    }

    #[test]
    fn weekly_takes_the_dial_only_without_five_hour_window() {
        let now = local_wall_clock(2026, 10, 7, 9, 0, 0);
        let row = |label: &str, duration: Option<Span>, counting: bool| BoardRow {
            label: label.into(),
            remaining_percent: 50,
            percent_text: "50%".into(),
            resets_at: Some(now + Span::hours(1)),
            is_counting_down: counting,
            duration,
        };
        let idle = [row("5 小時", Some(Span::hours(5)), false), row("每週", Some(Span::days(7)), true)];
        assert_eq!(primary_and_weekly(&idle), (Some(0), Some(1)), "5 小時未倒數時圓環仍顯示 5 小時。");
        let weekly_only = [row("每週", Some(Span::days(7)), true)];
        assert_eq!(primary_and_weekly(&weekly_only), (Some(0), Some(0)));
        let named = [row("每週", None, true), row("5 小時", Some(Span::hours(5)), true)];
        assert_eq!(primary_and_weekly(&named), (Some(1), Some(0)));
        let short_only = [row("5 小時", Some(Span::hours(5)), false)];
        assert_eq!(primary_and_weekly(&short_only), (Some(0), None));
        assert_eq!(primary_and_weekly(&[]), (None, None));
    }

    #[test]
    fn unavailable_groups_keep_message_and_login_flag() {
        let now = local_wall_clock(2026, 10, 7, 9, 0, 0);
        let mut snapshot =
            CliUsageSnapshot::new(CliKind::Codex, CliUsageAvailability::Unavailable, Vec::new(), "Not logged in", now);
        snapshot.requires_login = true;

        let mut board = UsageBoard::new();
        let snapshots = HashMap::from([(CliKind::Codex, snapshot.clone())]);
        assert!(board.apply(&snapshots, now), "新的快照應視為內容變更。");
        assert!(!board.apply(&snapshots, now + Span::seconds(1)), "相同快照只需重繪。");
        let group = board.group(CliKind::Codex).unwrap();
        assert!(group.rows.is_empty() && group.requires_login && group.message == "Not logged in");

        assert!(board.set_login_in_progress(CliKind::Codex, true));
        assert!(!board.set_login_in_progress(CliKind::Codex, true), "重複設定不應視為變更。");
        assert!(board.apply(&HashMap::new(), now), "快照被移除時應清掉該組。");
        assert!(board.group(CliKind::Codex).is_none());
    }

    #[test]
    fn countdown_and_freshness_text() {
        assert_eq!(format_countdown(Span::hours(26) + Span::minutes(5)), "1天 02:05");
        assert_eq!(format_countdown(Span::hours(4) + Span::minutes(5) + Span::seconds(9)), "04:05:09");

        let now = local_wall_clock(2026, 10, 7, 9, 0, 0);
        assert_eq!(format_reset_stamp(&now, &local_wall_clock(2026, 10, 7, 13, 30, 0)), "13:30 重置");
        assert_eq!(format_reset_stamp(&now, &local_wall_clock(2026, 10, 9, 13, 30, 0)), "10/09 13:30 重置");
        assert_eq!(format_freshness(&now, &(now - Span::seconds(30))), "剛更新");
        assert_eq!(format_freshness(&now, &(now - Span::minutes(12))), "12 分鐘前更新");
        assert_eq!(format_freshness(&now, &(now - Span::hours(3))), "3 小時前更新");
        assert_eq!(format_freshness(&now, &local_wall_clock(2026, 10, 5, 8, 15, 0)), "10/05 08:15 更新");
    }
}
