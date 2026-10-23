//! 固定字級、Windows 中文字型與深淺色主題。
use aiwake_core::CliKind;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: Color32,
    pub surface: Color32,
    pub elevated: Color32,
    pub input: Color32,
    pub border: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub track: Color32,
    pub dark: bool,
}
pub const GOOD: Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);
pub const WARN: Color32 = Color32::from_rgb(0xFB, 0xBF, 0x24);
pub const DANGER: Color32 = Color32::from_rgb(0xF8, 0x71, 0x71);
pub const TERMINAL: Color32 = Color32::from_rgb(0x0B, 0x10, 0x20);
impl Palette {
    pub fn new(dark: bool) -> Self {
        let c = Color32::from_rgb;
        if dark {
            Self {
                canvas: c(11, 16, 32),
                surface: c(21, 28, 46),
                elevated: c(28, 37, 64),
                input: c(15, 22, 38),
                border: c(36, 48, 77),
                text: c(230, 235, 245),
                secondary: c(154, 166, 191),
                muted: c(107, 120, 148),
                accent: c(34, 211, 238),
                track: c(34, 44, 69),
                dark,
            }
        } else {
            Self {
                canvas: c(243, 246, 251),
                surface: Color32::WHITE,
                elevated: c(232, 240, 249),
                input: c(248, 250, 252),
                border: c(226, 232, 240),
                text: c(15, 23, 42),
                secondary: c(71, 85, 105),
                muted: c(100, 116, 139),
                accent: c(14, 116, 144),
                track: c(229, 234, 242),
                dark,
            }
        }
    }
}
pub fn brand(kind: CliKind, dark: bool) -> (&'static str, &'static str, Color32) {
    let (badge, name, dark_rgb, light_rgb) = match kind {
        CliKind::Antigravity => ("Ge", "AGY Gemini", [59, 130, 246], [47, 111, 219]),
        CliKind::AntigravityArgon => ("Ar", "AGY Argon", [181, 133, 0], [154, 110, 0]),
        CliKind::AntigravityClaude => ("AC", "AGY Claude/GPT", [192, 79, 208], [166, 61, 184]),
        CliKind::Codex => ("Cx", "OpenAI Codex", [16, 163, 127], [11, 132, 102]),
        CliKind::Claude => ("CC", "Claude Code", [226, 104, 60], [201, 84, 42]),
    };
    let [r, g, b] = if dark { dark_rgb } else { light_rgb };
    (badge, name, Color32::from_rgb(r, g, b))
}
pub fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, path, family) in [
        ("正黑體", "C:/Windows/Fonts/msjh.ttc", FontFamily::Proportional),
        ("正黑體粗體", "C:/Windows/Fonts/msjhbd.ttc", FontFamily::Name("粗體".into())),
        ("Consolas", "C:/Windows/Fonts/consola.ttf", FontFamily::Monospace),
        ("符號", "C:/Windows/Fonts/seguisym.ttf", FontFamily::Proportional),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.into(), FontData::from_owned(bytes).into());
            let list = fonts.families.entry(family).or_default();
            if name == "符號" {
                list.insert(1.min(list.len()), name.into());
            } else {
                list.insert(0, name.into());
            }
        }
    }
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(FontFamily::Monospace).or_default().extend(fallback.clone());
    fonts.families.entry(FontFamily::Name("粗體".into())).or_default().extend(fallback);
    ctx.set_fonts(fonts);
}
pub fn apply(ctx: &egui::Context, dark: bool) {
    let colors = Palette::new(dark);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut style = (*ctx.style_of(if dark { egui::Theme::Dark } else { egui::Theme::Light })).clone();
    style.visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    style.visuals.panel_fill = colors.canvas;
    style.visuals.window_fill = colors.surface;
    style.visuals.extreme_bg_color = colors.input;
    style.visuals.faint_bg_color = colors.surface;
    style.visuals.override_text_color = Some(colors.text);
    style.visuals.selection.bg_fill = colors.accent.gamma_multiply(0.3);
    style.visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    for widget in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.noninteractive] {
        widget.bg_fill = colors.surface;
        widget.weak_bg_fill = colors.surface;
        widget.bg_stroke = Stroke::new(1.0, colors.border);
        widget.corner_radius = 8.into();
        widget.fg_stroke = Stroke::new(1.0, colors.text);
    }
    style.visuals.widgets.hovered.bg_fill = colors.elevated;
    style.visuals.widgets.hovered.weak_bg_fill = colors.elevated;
    style.visuals.widgets.hovered.corner_radius = 8.into();
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, colors.accent);
    style.visuals.widgets.active.corner_radius = 8.into();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 28.0;
    style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(21.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.0));
    ctx.set_style_of(if dark { egui::Theme::Dark } else { egui::Theme::Light }, style);
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
}
pub fn card(colors: Palette) -> egui::Frame {
    egui::Frame::new().fill(colors.surface).stroke(Stroke::new(1.0, colors.border)).corner_radius(13).inner_margin(16)
}
