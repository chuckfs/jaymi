//! Scrollable conversation transcript.

use eframe::egui;

use crate::conversation::ChatMessage;

use super::message_bubble::MessageBubble;

/// Conversation area that owns scrolling and message layout.
pub struct ConversationView;

impl ConversationView {
    /// Render the transcript and optionally stick to the newest message.
    pub fn show(ui: &mut egui::Ui, messages: &[ChatMessage], scroll_to_bottom: &mut bool) {
        let stick = *scroll_to_bottom;
        let mut scrolled = false;

        egui::ScrollArea::vertical()
            .id_salt("conversation_scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(stick)
            .show(ui, |ui| {
                ui.add_space(18.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.available_height()),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let max_width = (ui.available_width() * 0.78).clamp(420.0, 720.0);
                        ui.set_max_width(max_width);

                        for (index, message) in messages.iter().enumerate() {
                            // Light staggered presence for the first few messages.
                            let fade = if index < 3 {
                                1.0
                            } else {
                                1.0
                            };
                            let _ = fade;
                            MessageBubble::show(ui, message);
                            ui.add_space(14.0);
                        }
                        ui.add_space(12.0);
                    },
                );
                scrolled = true;
            });

        if scrolled && stick {
            *scroll_to_bottom = false;
        }
    }
}
