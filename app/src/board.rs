//! 額度看板。每個 CLI 一段標題，每個額度視窗自成一列：
//! 「視窗名稱 ─ 剩餘量長條 ─ 百分比 ─ 重置倒數」。
//!
//! 看板是自繪的：欄位對齊、以長條表達水位、顏色只用來表示剩餘量級距；
//! 而且整塊每秒只要一次重繪，不必維護數十個標籤控制項。

use std::collections::{HashMap, HashSet};

use aiwake_core::catalog::{self, CliDescriptor};
use aiwake_core::time::{self, Moment, Span};
use aiwake_core::{CliKind, CliUsageAvailability, CliUsageSnapshot};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC, HDC};

use crate::theme::{palette, Fonts};
use crate::w32::{self, draw_text, measure_text, DT_LEFT, DT_LINE, DT_NOPREFIX, DT_RIGHT, DT_WORDBREAK};

#[derive(Debug, Clone, PartialEq)]
pub struct BoardRow {
    pub label: String,
    pub remaining_percent: i32,
    pub percent_text: String,
    pub resets_at: Option<Moment>,
    pub is_counting_down: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardGroup {
    pub availability: CliUsageAvailability,
    pub message: String,
    pub rows: Vec<BoardRow>,
    pub observed_at: Moment,
    pub requires_login: bool,
}

pub struct UsageBoard {
    sources: HashMap<CliKind, CliUsageSnapshot>,
    groups: HashMap<CliKind, BoardGroup>,
    /// 上一次繪製時「重新登入」連結的位置（看板座標），供滑鼠命中測試。
    login_links: Vec<(RECT, CliKind)>,
    login_in_progress: HashSet<CliKind>,
    now: Moment,
    hovered_login: Option<CliKind>,
}

impl Default for UsageBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageBoard {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            groups: HashMap::new(),
            login_links: Vec::new(),
            login_in_progress: HashSet::new(),
            now: time::now(),
            hovered_login: None,
        }
    }

    /// 套用最新的快照，回傳內容是否換過（需要重算版面高度）。
    /// 每秒的倒數更新只會重繪，不會牽動外層版面。
    pub fn apply(&mut self, snapshots: &HashMap<CliKind, CliUsageSnapshot>, now: Moment) -> bool {
        self.now = now;

        let mut changed = false;
        for descriptor in catalog::all() {
            let kind = descriptor.kind;
            match snapshots.get(&kind) {
                None => {
                    self.sources.remove(&kind);
                    changed |= self.groups.remove(&kind).is_some();
                }
                Some(snapshot) => {
                    if self.sources.get(&kind) == Some(snapshot) {
                        continue;
                    }
                    self.sources.insert(kind, snapshot.clone());
                    self.groups.insert(kind, build_group(descriptor, snapshot));
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn group(&self, kind: CliKind) -> Option<&BoardGroup> {
        self.groups.get(&kind)
    }

    /// 標示某個 CLI 的登入視窗是否已開啟，開啟中連結改為提示文字、不可再點。
    /// 回傳狀態是否改變。
    pub fn set_login_in_progress(&mut self, kind: CliKind, in_progress: bool) -> bool {
        if in_progress {
            self.login_in_progress.insert(kind)
        } else {
            self.login_in_progress.remove(&kind)
        }
    }

    /// 命中「重新登入」連結時回傳對應的 CLI（座標為看板座標）。
    pub fn hit_login_link(&self, x: i32, y: i32) -> Option<CliKind> {
        self.login_links
            .iter()
            .find(|(bounds, kind)| w32::contains(bounds, x, y) && !self.login_in_progress.contains(kind))
            .map(|(_, kind)| *kind)
    }

    /// 更新滑鼠停留的連結，回傳是否需要重繪。
    pub fn set_hovered(&mut self, hovered: Option<CliKind>) -> bool {
        if self.hovered_login == hovered {
            false
        } else {
            self.hovered_login = hovered;
            true
        }
    }

    pub fn login_link_count(&self) -> usize {
        self.login_links.len()
    }

    /// 畫完目前資料所需要的高度。
    pub fn measure(&self, fonts: &Fonts, width: i32) -> i32 {
        unsafe {
            let dc = GetDC(None);
            let height = self.layout(dc, None, fonts, 0, 0, width, &mut Vec::new());
            ReleaseDC(None, dc);
            height
        }
    }

    /// 在 `(x, y)` 繪製看板並登記連結的命中範圍。
    pub fn paint(&mut self, dc: HDC, fonts: &Fonts, x: i32, y: i32, width: i32) -> i32 {
        let mut links = Vec::new();
        let height = self.layout(dc, Some(dc), fonts, x, y, width, &mut links);
        for (bounds, _) in &mut links {
            bounds.left -= x;
            bounds.right -= x;
            bounds.top -= y;
            bounds.bottom -= y;
        }
        self.login_links = links;
        height
    }

    /// 量測與繪製共用同一段版面計算：`target` 為 None 時只回傳所需高度。
    #[allow(clippy::too_many_arguments)]
    fn layout(
        &self,
        measure_dc: HDC,
        target: Option<HDC>,
        fonts: &Fonts,
        origin_x: i32,
        origin_y: i32,
        width: i32,
        links: &mut Vec<(RECT, CliKind)>,
    ) -> i32 {
        let colors = palette();
        let scale = |value: i32| fonts.scale(value);
        let gap = scale(10);
        let bar_width = scale(88);
        let bar_height = scale(7);
        let row_height = (fonts.body.height() + scale(9)).max(scale(24));
        let header_height = (fonts.table_header.height() + scale(7)).max(scale(23));

        let mut label_width = scale(44);
        for group in self.groups.values() {
            for row in &group.rows {
                label_width = label_width.max(measure_text(measure_dc, &fonts.body, &row.label).0);
            }
        }
        let label_width = label_width.min(scale(104));
        let percent_width = measure_text(measure_dc, &fonts.table_header, "100%").0;

        let bar_x = label_width + gap;
        let percent_x = bar_x + bar_width + gap;
        let countdown_x = percent_x + percent_width + gap;

        // 寬度足夠時各 CLI 分欄排列（依目錄順序由左而右、由上而下），每一橫列取最高者。
        let columns = columns_for(fonts, width);
        let column_gap = scale(20);
        let column_width = (width - column_gap * (columns as i32 - 1)) / columns as i32;
        let (mut row_top, mut row_bottom) = (0, 0);
        for (index, descriptor) in catalog::all().iter().enumerate() {
            let column = index % columns;
            if column == 0 && index > 0 {
                row_top = row_bottom + scale(9);
            }
            let origin_x = origin_x + column as i32 * (column_width + column_gap);
            let width = column_width;
            let at = |x: i32, y: i32, w: i32, h: i32| w32::rect(origin_x + x, origin_y + y, w, h);
            let mut y = row_top;
            let group = self.groups.get(&descriptor.kind);

            if let Some(dc) = target {
                let bounds = at(0, y, width, header_height);
                draw_text(
                    dc,
                    &fonts.table_header,
                    descriptor.short_name,
                    &bounds,
                    colors.primary_text,
                    DT_LINE | DT_LEFT,
                );
                if let Some(group) = group {
                    let freshness = format_freshness(&self.now, &group.observed_at);
                    draw_text(dc, &fonts.caption, &freshness, &bounds, colors.secondary_text, DT_LINE | DT_RIGHT);
                }
                let line_y = origin_y + y + header_height - 1;
                w32::line(dc, origin_x, line_y, origin_x + (width - 1).max(0), line_y, colors.divider);
            }
            y += header_height + scale(3);

            let Some(group) = group else {
                y += draw_note(
                    measure_dc,
                    target,
                    fonts,
                    "尚未讀取",
                    colors.secondary_text,
                    origin_x,
                    origin_y + y,
                    width,
                );
                row_bottom = row_bottom.max(y);
                continue;
            };

            if group.rows.is_empty() {
                let color = if group.availability == CliUsageAvailability::Unavailable {
                    colors.danger
                } else {
                    colors.secondary_text
                };
                y += draw_note(measure_dc, target, fonts, &group.message, color, origin_x, origin_y + y, width);
                if group.requires_login {
                    y += self.draw_login_link(
                        measure_dc,
                        target,
                        fonts,
                        descriptor.kind,
                        origin_x,
                        origin_y + y,
                        width,
                        links,
                    );
                }
                row_bottom = row_bottom.max(y);
                continue;
            }

            for row in &group.rows {
                if let Some(dc) = target {
                    draw_text(
                        dc,
                        &fonts.body,
                        &row.label,
                        &at(0, y, label_width, row_height),
                        colors.primary_text,
                        DT_LINE | DT_LEFT,
                    );

                    let level = level_color(row.remaining_percent);
                    draw_meter(
                        dc,
                        &at(bar_x, y + (row_height - bar_height) / 2, bar_width, bar_height),
                        row.remaining_percent,
                        level,
                    );
                    draw_text(
                        dc,
                        &fonts.table_header,
                        &row.percent_text,
                        &at(percent_x, y, percent_width, row_height),
                        level,
                        DT_LINE | DT_RIGHT,
                    );

                    let available = width - countdown_x;
                    if available >= scale(44) {
                        self.draw_countdown(
                            dc,
                            fonts,
                            row,
                            origin_x + countdown_x,
                            origin_y + y,
                            available,
                            row_height,
                            gap,
                        );
                    }
                }
                y += row_height;
            }
            row_bottom = row_bottom.max(y);
        }
        row_bottom
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_countdown(
        &self,
        dc: HDC,
        fonts: &Fonts,
        row: &BoardRow,
        x: i32,
        y: i32,
        available: i32,
        row_height: i32,
        gap: i32,
    ) {
        let colors = palette();
        let caption = |text: &str| {
            draw_text(
                dc,
                &fonts.caption,
                text,
                &w32::rect(x, y, available, row_height),
                colors.secondary_text,
                DT_LINE | DT_LEFT,
            );
        };

        let Some(resets_at) = row.resets_at.filter(|_| row.is_counting_down) else {
            caption("額度充足");
            return;
        };
        let remaining = resets_at - self.now;
        if remaining <= Span::zero() {
            caption("等待伺服器更新");
            return;
        }

        let countdown = format_countdown(remaining);
        let countdown_width = measure_text(dc, &fonts.mono, &countdown).0;
        draw_text(
            dc,
            &fonts.mono,
            &countdown,
            &w32::rect(x, y, countdown_width.min(available), row_height),
            colors.primary_text,
            DT_LINE | DT_LEFT,
        );

        let stamp_x = countdown_width + gap;
        let stamp_width = available - stamp_x;
        if stamp_width >= fonts.scale(56) {
            draw_text(
                dc,
                &fonts.caption,
                &format_reset_stamp(&self.now, &resets_at),
                &w32::rect(x + stamp_x, y, stamp_width, row_height),
                colors.secondary_text,
                DT_LINE | DT_LEFT,
            );
        }
    }

    /// 畫「重新登入」連結並登記命中範圍，回傳佔用高度。
    #[allow(clippy::too_many_arguments)]
    fn draw_login_link(
        &self,
        measure_dc: HDC,
        target: Option<HDC>,
        fonts: &Fonts,
        kind: CliKind,
        x: i32,
        y: i32,
        width: i32,
        links: &mut Vec<(RECT, CliKind)>,
    ) -> i32 {
        let colors = palette();
        let in_progress = self.login_in_progress.contains(&kind);
        let (text, font) = if in_progress {
            ("登入視窗已開啟，完成授權後會自動重新讀取", &fonts.caption)
        } else {
            ("重新登入 ›", &fonts.table_header)
        };
        let (text_width, text_height) = measure_text(measure_dc, font, text);
        let height = text_height + fonts.scale(6);

        if let Some(dc) = target {
            let bounds = w32::rect(x, y, (text_width + fonts.scale(4)).min(width.max(0)), height);
            let color = if in_progress {
                colors.secondary_text
            } else if self.hovered_login == Some(kind) {
                colors.accent_hover
            } else {
                colors.accent
            };
            draw_text(dc, font, text, &bounds, color, DT_LINE | DT_LEFT);
            if !in_progress {
                links.push((bounds, kind));
            }
        }
        height + fonts.scale(4)
    }
}

/// 看板依寬度自動決定的欄數：每欄至少約 320px，最多 3 欄。
pub fn columns_for(fonts: &Fonts, width: i32) -> usize {
    let minimum = fonts.scale(320);
    let gap = fonts.scale(20);
    (((width + gap) / (minimum + gap)).max(1) as usize).min(3)
}

/// 畫一段說明文字（尚未讀取、不支援或錯誤訊息），回傳佔用高度。
#[allow(clippy::too_many_arguments)]
fn draw_note(
    _measure_dc: HDC,
    target: Option<HDC>,
    fonts: &Fonts,
    text: &str,
    color: COLORREF,
    x: i32,
    y: i32,
    width: i32,
) -> i32 {
    let available = width.max(fonts.scale(120));
    let height = fonts.caption.measure_wrapped(text, available);
    if let Some(dc) = target {
        draw_text(
            dc,
            &fonts.caption,
            text,
            &w32::rect(x, y + fonts.scale(3), available, height),
            color,
            DT_WORDBREAK | DT_NOPREFIX,
        );
    }
    height + fonts.scale(8)
}

/// 畫剩餘量長條。填滿的比例就是剩餘百分比。
fn draw_meter(dc: HDC, bounds: &RECT, remaining_percent: i32, color: COLORREF) {
    let (width, height) = (w32::width_of(bounds), w32::height_of(bounds));
    if width <= 0 || height <= 0 {
        return;
    }
    w32::fill_round(dc, bounds, height, palette().meter_track);

    let percent = remaining_percent.clamp(0, 100);
    if percent <= 0 {
        return;
    }
    // 只剩 1% 時也要看得見，至少保留一個圓角的寬度。
    let fill_width = ((width as f64 * percent as f64 / 100.0).round() as i32).clamp(height.min(width), width);
    w32::fill_round(dc, &w32::rect(bounds.left, bounds.top, fill_width, height), height, color);
}

fn level_color(remaining_percent: i32) -> COLORREF {
    let colors = palette();
    match remaining_percent {
        50.. => colors.success,
        20..=49 => colors.caution,
        _ => colors.danger,
    }
}

// ── 文字格式 ──────────────────────────────────────────────────

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
        })
        .collect()
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
