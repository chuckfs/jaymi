//! Persistent multi-line message composer.

use eframe::egui::{self, Key, Modifiers, RichText};

use super::theme::ConversationTheme;

/// Composer interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerState {
    /// Ready for input.
    Idle,
    /// Temporarily blocked (reserved).
    Disabled,
    /// Message is being submitted through the Planner.
    Sending,
}

/// Bottom message composer with Enter-to-send semantics.
pub struct MessageComposer;

impl MessageComposer {
    /// Draw the composer. Returns `true` when the user requested send.
    pub fn show(ui: &mut egui::Ui, draft: &mut String, state: ComposerState) -> bool {
        let enabled = matches!(state, ComposerState::Idle);
        let mut send = false;
        let width = (ui.available_width() - 56.0).max(240.0);

        ui.add_enabled_ui(enabled, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 72.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // Consume bare Enter before the TextEdit inserts a newline.
                    let wants_send = ui.input_mut(|input| {
                        if input.modifiers.shift {
                            return false;
                        }
                        if input.key_pressed(Key::Enter) {
                            input.consume_key(Modifiers::NONE, Key::Enter);
                            true
                        } else {
                            false
                        }
                    });

                    let editor_width = (ui.available_width() - 88.0).max(180.0);
                    let response = ui.add_sized(
                        [editor_width, 64.0],
                        egui::TextEdit::multiline(draft)
                            .id_salt("message_composer")
                            .desired_width(editor_width)
                            .desired_rows(2)
                            .hint_text("Message Jaymi…"),
                    );

                    if wants_send && (response.has_focus() || !draft.trim().is_empty()) {
                        send = true;
                    }

                    ui.add_space(8.0);

                    let send_clicked = ui
                        .add_sized(
                            [72.0, 40.0],
                            egui::Button::new(
                                RichText::new(if state == ComposerState::Sending {
                                    "…"
                                } else {
                                    "Send"
                                })
                                .color(ConversationTheme::ON_ACCENT_TEXT),
                            )
                            .fill(ConversationTheme::ACCENT)
                            .corner_radius(12.0),
                        )
                        .clicked();

                    if send_clicked {
                        send = true;
                    }
                },
            );
        });

        send && enabled && !draft.trim().is_empty()
    }
}
