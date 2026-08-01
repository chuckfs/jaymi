//! Reusable message bubble for conversation turns.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Sense};

use crate::conversation::{ChatMessage, MessageRole};

use super::theme::ConversationTheme;

/// Visual presentation of a single [`ChatMessage`].
pub struct MessageBubble;

impl MessageBubble {
    /// Draw one message bubble aligned by role.
    pub fn show(ui: &mut egui::Ui, message: &ChatMessage) {
        let (fill, text_color, align_right, radius) = match message.role {
            MessageRole::User => (
                ConversationTheme::USER_BUBBLE,
                ConversationTheme::ON_ACCENT_TEXT,
                true,
                CornerRadius {
                    nw: 18,
                    ne: 18,
                    sw: 18,
                    se: 6,
                },
            ),
            MessageRole::Assistant => (
                ConversationTheme::ASSISTANT_BUBBLE,
                ConversationTheme::PRIMARY_TEXT,
                false,
                CornerRadius {
                    nw: 18,
                    ne: 18,
                    sw: 6,
                    se: 18,
                },
            ),
            MessageRole::System => (
                ConversationTheme::SYSTEM_BUBBLE,
                ConversationTheme::MUTED_TEXT,
                false,
                CornerRadius::same(14),
            ),
        };

        let available = ui.available_width();
        let bubble_width = (available * 0.86).min(640.0);

        ui.horizontal(|ui| {
            if align_right {
                ui.add_space((available - bubble_width).max(0.0));
            }

            let frame = Frame::NONE
                .fill(fill)
                .corner_radius(radius)
                .inner_margin(Margin::symmetric(16, 12))
                .stroke(egui::Stroke::new(
                    if matches!(message.role, MessageRole::Assistant) {
                        1.0_f32
                    } else {
                        0.0_f32
                    },
                    ConversationTheme::BORDER,
                ));

            frame.show(ui, |ui| {
                ui.set_max_width(bubble_width - 8.0);
                if matches!(message.role, MessageRole::Assistant) {
                    ui.label(
                        RichText::new("Jaymi")
                            .size(12.0)
                            .color(ConversationTheme::ACCENT)
                            .strong(),
                    );
                    ui.add_space(4.0);
                }
                ui.label(RichText::new(&message.text).size(15.5).color(text_color));
            });
        });

        // Soft hover presence without noisy chrome.
        let _ = ui.interact(
            ui.min_rect(),
            ui.id().with(&message.id),
            Sense::hover(),
        );
        let _ = Color32::TRANSPARENT;
    }
}
