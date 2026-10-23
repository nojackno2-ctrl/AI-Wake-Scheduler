//! 以 egui Painter 繪製的滑動開關、時間軸與雙環儀表。
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Response, Role, Sense, Stroke, Vec2, WidgetInfo};

use crate::theme::Palette;

pub fn toggle(ui: &mut egui::Ui, value: &mut bool, label: &str, colors: Palette) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| WidgetInfo::selected(Role::Switch, ui.is_enabled(), *value, label));
    let t = ui.ctx().animate_bool(response.id, *value);
    let fill = if *value { colors.accent } else { colors.track };
    ui.painter().rect_filled(rect, 11.0, fill);
    ui.painter().circle_filled(Pos2::new(rect.left() + 11.0 + t * 18.0, rect.center().y), 8.0, Color32::WHITE);
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), 13.0, Stroke::new(1.0, colors.accent), egui::StrokeKind::Outside);
    }
    response.on_hover_text(label)
}

pub fn timeline_segments(start: i64, end: i64) -> Vec<(f32, f32)> {
    if end < start {
        vec![(0.0, end as f32 / 1440.0), (start as f32 / 1440.0, 1.0)]
    } else {
        vec![(start as f32 / 1440.0, end as f32 / 1440.0)]
    }
}

/// 把時間軸上的位置吸附到五分鐘，開始把手不可超過 23:55。
pub fn snap_minute(fraction: f32, end: bool) -> i64 {
    ((fraction.clamp(0.0, 1.0) * 288.0).round() as i64 * 5).clamp(0, if end { 1440 } else { 1435 })
}

pub fn timeline(
    ui: &mut egui::Ui,
    start: &mut i64,
    end: &mut i64,
    now: f32,
    markers: &[(f32, Color32)],
    interactive: bool,
    colors: Palette,
) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), if interactive { 62.0 } else { 42.0 }), Sense::hover());
    let track = Rect::from_min_max(rect.left_top() + egui::vec2(8.0, 12.0), rect.right_top() + egui::vec2(-8.0, 22.0));
    let x = |fraction: f32| track.left() + fraction * track.width();
    ui.painter().rect_filled(track, 5.0, colors.track);
    for (a, b) in timeline_segments(*start, *end) {
        ui.painter().rect_filled(
            Rect::from_min_max(Pos2::new(x(a), track.top()), Pos2::new(x(b), track.bottom())),
            4.0,
            colors.accent.gamma_multiply(0.65),
        );
    }
    ui.painter().line_segment(
        [Pos2::new(x(now), track.top() - 5.0), Pos2::new(x(now), track.bottom() + 5.0)],
        Stroke::new(1.5, colors.text),
    );
    for &(fraction, color) in markers {
        let inside = if *end < *start {
            fraction >= *start as f32 / 1440.0 || fraction < *end as f32 / 1440.0
        } else {
            fraction >= *start as f32 / 1440.0 && fraction < *end as f32 / 1440.0
        };
        let point = Pos2::new(x(fraction), track.center().y);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                point + egui::vec2(0.0, -5.0),
                point + egui::vec2(5.0, 0.0),
                point + egui::vec2(0.0, 5.0),
                point + egui::vec2(-5.0, 0.0),
            ],
            if inside { color } else { color.gamma_multiply(0.3) },
            Stroke::NONE,
        ));
    }
    for hour in [0, 6, 12, 18, 24] {
        ui.painter().text(
            Pos2::new(x(hour as f32 / 24.0), track.bottom() + 12.0),
            egui::Align2::CENTER_CENTER,
            if interactive { hour.to_string() } else { format!("{hour:02}:00") },
            FontId::monospace(10.0),
            colors.muted,
        );
    }
    if interactive {
        for (is_end, minute, label) in [(false, start, "開始時間把手"), (true, end, "結束時間把手")] {
            let center = Pos2::new(x(*minute as f32 / 1440.0), track.center().y);
            let response = ui.interact(
                Rect::from_center_size(center, egui::vec2(16.0, 28.0)),
                ui.scope_id().with(label),
                Sense::drag(),
            );
            response.widget_info(|| WidgetInfo::labeled(Role::Slider, true, label));
            if let Some(pointer) = response.interact_pointer_pos().filter(|_| response.dragged()) {
                *minute = snap_minute((pointer.x - track.left()) / track.width(), is_end);
            }
            ui.painter().circle_filled(center, 7.0, colors.accent);
            ui.painter().circle_filled(center, 3.0, colors.surface);
            response.on_hover_text(label);
        }
    }
}

pub fn arc(ui: &egui::Ui, center: Pos2, radius: f32, progress: f32, width: f32, color: Color32) {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return;
    }
    let steps = (progress * 128.0).ceil().max(2.0) as usize;
    let points: Vec<_> = (0..=steps)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * progress * i as f32 / steps as f32;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    ui.painter().circle_filled(points[0], width / 2.0, color);
    ui.painter().circle_filled(points[steps], width / 2.0, color);
    ui.painter().add(egui::Shape::line(points, Stroke::new(width, color)));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midnight_segments_and_drag_limits() {
        assert_eq!(timeline_segments(1320, 360), vec![(0.0, 0.25), (1320.0 / 1440.0, 1.0)]);
        assert_eq!(snap_minute(1.0, true), 1440);
        assert_eq!(snap_minute(1.0, false), 1435);
        assert_eq!(snap_minute(0.502, false), 725);
    }
}
