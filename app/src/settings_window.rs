//! 主視窗內的設定抽屜；取消時直接丟棄獨立的表單副本。
use crate::{
    theme::{self, Palette},
    ui, widgets,
};
use aiwake_core::{catalog, AppSettings};
use eframe::egui::{self, RichText};
use windows::Win32::Foundation::HWND;

pub struct SettingsSheet {
    pub draft: AppSettings,
    pub probe_text: String,
    pub probing: bool,
    pub generation: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SheetAction {
    None,
    Save,
    Cancel,
    Probe,
}
impl SettingsSheet {
    pub fn new(source: AppSettings, generation: u64) -> Self {
        Self { draft: source, probe_text: String::new(), probing: false, generation }
    }
    pub fn validate(&mut self) -> Result<(), String> {
        for descriptor in catalog::all() {
            let profile = self.draft.cli_profiles.entry(descriptor.kind).or_default();
            profile.executable = profile.executable.trim().to_string();
            profile.model = profile.model.trim().to_string();
            profile.thinking_effort = descriptor.normalize_effort(Some(&profile.model), profile.thinking_effort);
            if profile.executable.is_empty() {
                return Err(format!("請填寫 {} 的命令或路徑。", descriptor.display_name));
            }
            aiwake_core::tokenizer::parse(Some(&profile.additional_arguments))
                .map_err(|error| error.message().to_string())?;
        }
        if !(1..=120).contains(&self.draft.execution_timeout_minutes) {
            return Err("執行逾時請輸入 1 至 120 分鐘。".into());
        }
        if !(0..=1440).contains(&self.draft.quota_auto_refresh_minutes) {
            return Err("額度重新整理請輸入 0 至 1440 分鐘。".into());
        }
        Ok(())
    }
    pub fn show(&mut self, ui: &mut egui::Ui, owner: HWND, colors: Palette) -> SheetAction {
        let mut action = SheetAction::None;
        ui.horizontal(|ui| {
            ui.heading("設定");
            ui.label(RichText::new("CLI 與背景執行").color(colors.secondary));
        });
        ui.separator();
        // 操作列固定於底部，內容本身可捲動。
        egui::Panel::bottom("settings-actions").frame(egui::Frame::new().inner_margin(12)).show(ui, |ui| {
            if !self.probe_text.is_empty() {
                ui.label(&self.probe_text);
            }
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("儲存設定").fill(colors.accent).stroke(egui::Stroke::NONE)).clicked() {
                    action = SheetAction::Save;
                }
                if ui
                    .add_enabled(
                        !self.probing,
                        egui::Button::new(if self.probing { "檢查中…" } else { "檢查 CLI 連線" }),
                    )
                    .clicked()
                {
                    action = SheetAction::Probe;
                }
                if ui.button("取消").clicked() {
                    action = SheetAction::Cancel;
                }
            });
        });
        egui::ScrollArea::vertical().id_salt("settings-scroll").show(ui, |ui| {
            for descriptor in catalog::all() {
                let kind = descriptor.kind;
                let (badge, name, color) = theme::brand(kind, colors.dark);
                ui.push_id(kind.name(), |ui| {
                    theme::card(colors).show(ui, |ui| {
                        ui.set_min_width((ui.available_width() - 1.0).max(0.0));
                        ui.label(RichText::new(format!("{badge}  {name}")).strong().color(color));
                        let profile = self.draft.cli_profiles.entry(kind).or_default();
                        let model_label = ui.label("模型 ID（可自訂）");
                        ui.add(egui::TextEdit::singleline(&mut profile.model).desired_width(f32::INFINITY))
                            .labelled_by(model_label.id);
                        egui::ComboBox::from_id_salt("models").selected_text("推薦模型").width(280.0).show_ui(
                            ui,
                            |ui| {
                                for model in descriptor.preset_models {
                                    ui.selectable_value(
                                        &mut profile.model,
                                        (*model).to_string(),
                                        if model.is_empty() { "本工具預設" } else { model },
                                    );
                                }
                            },
                        );
                        // 手動輸入與推薦清單都經過相同的連動規則。
                        profile.thinking_effort =
                            descriptor.normalize_effort(Some(&profile.model), profile.thinking_effort);
                        ui.horizontal(|ui| {
                            ui.label("思考程度");
                            egui::ComboBox::from_id_salt("effort")
                                .selected_text(profile.thinking_effort.display_name())
                                .show_ui(ui, |ui| {
                                    for effort in descriptor.get_supported_efforts(Some(&profile.model)) {
                                        ui.selectable_value(
                                            &mut profile.thinking_effort,
                                            *effort,
                                            effort.display_name(),
                                        );
                                    }
                                });
                        });
                        let path_label = ui.label("執行檔路徑");
                        ui.add(egui::TextEdit::singleline(&mut profile.executable).desired_width(f32::INFINITY))
                            .labelled_by(path_label.id);
                        if ui.button("瀏覽執行檔…").clicked() {
                            if let Some(path) = ui::pick_file(
                                owner,
                                &format!("選擇 {name}"),
                                &[("可執行檔", "*.exe;*.cmd;*.bat"), ("所有檔案", "*.*")],
                            ) {
                                profile.executable = path;
                            }
                        }
                        let arguments_label = ui.label("額外參數");
                        ui.add(
                            egui::TextEdit::singleline(&mut profile.additional_arguments).desired_width(f32::INFINITY),
                        )
                        .labelled_by(arguments_label.id);
                    });
                });
                ui.add_space(8.0);
            }
            ui.separator();
            ui.strong("一般設定");
            for (label, value) in [
                ("開機自動啟動", &mut self.draft.start_with_windows),
                ("關閉視窗時縮到系統匣", &mut self.draft.minimize_to_tray),
                ("節省 Token 模式", &mut self.draft.token_saver_mode),
                ("允許背景查詢 Antigravity 額度", &mut self.draft.auto_start_antigravity),
            ] {
                ui.horizontal(|ui| {
                    widgets::toggle(ui, value, label, colors);
                    ui.label(label);
                });
            }
            ui.horizontal(|ui| {
                let label = ui.label("執行逾時（分）");
                ui.add(egui::DragValue::new(&mut self.draft.execution_timeout_minutes).range(1..=120))
                    .labelled_by(label.id);
            });
            ui.horizontal(|ui| {
                let label = ui.label("額度自動重新整理（分，0 停用）");
                ui.add(egui::DragValue::new(&mut self.draft.quota_auto_refresh_minutes).range(0..=1440))
                    .labelled_by(label.id);
            });
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiwake_core::{CliKind, ThinkingEffort};
    #[test]
    fn model_switch_normalizes_effort_and_draft_does_not_mutate_source() {
        let source = AppSettings::default();
        let mut sheet = SettingsSheet::new(source.clone(), 1);
        let profile = sheet.draft.cli_profiles.get_mut(&CliKind::Codex).expect("Codex 設定");
        profile.model = "gpt-6-luna".into();
        profile.thinking_effort = ThinkingEffort::Ultra;
        sheet.validate().expect("表單有效");
        assert_eq!(sheet.draft.profile(CliKind::Codex).thinking_effort, ThinkingEffort::Max);
        assert_eq!(source.profile(CliKind::Codex).thinking_effort, ThinkingEffort::Default);
        assert_eq!(
            catalog::get(CliKind::Antigravity).get_supported_efforts(Some("gemini-3.8-flash-high")),
            &[ThinkingEffort::Default]
        );
    }
}
