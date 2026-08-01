//! Visual tokens for the conversation shell.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, Visuals};

/// Conversation shell palette and typography.
pub struct ConversationTheme;

impl ConversationTheme {
    pub const CANVAS: Color32 = Color32::from_rgb(244, 247, 250);
    pub const ATMOSPHERE_TOP: Color32 = Color32::from_rgb(236, 244, 246);
    pub const ATMOSPHERE_BOTTOM: Color32 = Color32::from_rgb(248, 246, 241);
    pub const HEADER_FILL: Color32 = Color32::from_rgb(252, 253, 254);
    pub const COMPOSER_BAND: Color32 = Color32::from_rgb(255, 255, 255);
    pub const BRAND: Color32 = Color32::from_rgb(18, 52, 58);
    pub const ACCENT: Color32 = Color32::from_rgb(28, 120, 118);
    pub const USER_BUBBLE: Color32 = Color32::from_rgb(28, 120, 118);
    pub const ASSISTANT_BUBBLE: Color32 = Color32::from_rgb(255, 255, 255);
    pub const SYSTEM_BUBBLE: Color32 = Color32::from_rgb(232, 237, 239);
    pub const PRIMARY_TEXT: Color32 = Color32::from_rgb(28, 36, 40);
    pub const ON_ACCENT_TEXT: Color32 = Color32::from_rgb(248, 252, 251);
    pub const MUTED_TEXT: Color32 = Color32::from_rgb(96, 110, 116);
    pub const BORDER: Color32 = Color32::from_rgb(210, 220, 224);
    pub const COMPOSER_FILL: Color32 = Color32::from_rgb(255, 255, 255);

    pub fn apply(ctx: &egui::Context) {
        let mut visuals = Visuals::light();
        visuals.window_fill = Self::CANVAS;
        visuals.panel_fill = Self::CANVAS;
        visuals.override_text_color = Some(Self::PRIMARY_TEXT);
        visuals.widgets.inactive.bg_fill = Self::COMPOSER_FILL;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(236, 245, 244);
        visuals.widgets.active.bg_fill = Color32::from_rgb(220, 236, 234);
        visuals.selection.bg_fill = Color32::from_rgb(200, 228, 226);
        visuals.window_stroke = Stroke::new(1.0_f32, Self::BORDER);
        visuals.window_corner_radius = CornerRadius::same(16);
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(30.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            FontId::new(16.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            FontId::new(12.5, FontFamily::Proportional),
        );
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        ctx.set_style(style);
    }
}
