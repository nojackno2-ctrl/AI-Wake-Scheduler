//! 有界的即時終端輸出；保留來源、時間與事件種類。
use crate::{
    theme::{self, Palette},
    widgets,
};
use aiwake_core::{CliActivityEvent, CliActivityKind};
use eframe::egui::{self, RichText};
use std::collections::VecDeque;
pub struct Terminal {
    pub lines: VecDeque<CliActivityEvent>,
    pub auto_scroll: bool,
}
impl Default for Terminal {
    fn default() -> Self {
        Self { lines: VecDeque::new(), auto_scroll: true }
    }
}
impl Terminal {
    pub fn push(&mut self, event: CliActivityEvent) {
        if self.lines.len() >= 2000 {
            self.lines.pop_front();
        }
        self.lines.push_back(event);
    }
    pub fn text(&self) -> String {
        self.lines.iter().map(|e| format!("{} {}", e.cli.short_name(), e.text)).collect::<Vec<_>>().join("\n")
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::new(true);
        ui.set_style(ui.ctx().style_of(egui::Theme::Dark));
        ui.visuals_mut().override_text_color = Some(colors.text);
        ui.horizontal_wrapped(|ui| {
            ui.strong("即時終端輸出");
            widgets::toggle(ui, &mut self.auto_scroll, "自動捲動", colors);
            ui.small("自動捲動");
            if ui.button("清除").clicked() {
                self.lines.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::both()
            .id_salt("terminal-scroll")
            .stick_to_bottom(self.auto_scroll)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.lines.is_empty() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("等待下一次喚醒…").monospace().color(colors.muted));
                }
                for event in &self.lines {
                    let (badge, _, brand) = theme::brand(event.cli, true);
                    let (symbol, color) = match event.kind {
                        CliActivityKind::Started => ("▶", colors.accent),
                        CliActivityKind::Completed => ("✓", theme::GOOD),
                        CliActivityKind::Failed | CliActivityKind::ErrorOutput => ("✗", theme::DANGER),
                        CliActivityKind::Output => ("│", colors.secondary),
                    };
                    ui.horizontal_top(|ui| {
                        ui.label(
                            RichText::new(event.timestamp.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
                                .monospace()
                                .size(10.0)
                                .color(colors.muted),
                        );
                        ui.label(RichText::new(badge).monospace().color(brand));
                        ui.label(RichText::new(format!("{symbol} {}", event.text.trim_end())).monospace().color(color));
                    });
                }
            });
    }
}
