//! Application header — brand-first, calm, conversation-home signal.

use eframe::egui::{self, RichText};

use super::theme::ConversationTheme;

/// Top header presenting Jaymi as the primary product signal.
pub struct Header;

impl Header {
    /// Draw the header contents into the provided UI.
    pub fn show(ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Jaymi")
                        .size(28.0)
                        .color(ConversationTheme::BRAND)
                        .strong(),
                );
                ui.label(
                    RichText::new("Your personal AI environment")
                        .size(13.0)
                        .color(ConversationTheme::MUTED_TEXT),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("Conversation")
                        .size(13.0)
                        .color(ConversationTheme::ACCENT),
                );
            });
        });
    }
}
