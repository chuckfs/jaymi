//! Three-dot action menu for future workspace expansion.

use eframe::egui::{self, Align2, Area, Frame, Order, RichText, Sense};

use super::theme::ConversationTheme;

/// Placeholder / functional actions exposed beside the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    StartCodingProject,
    StartCreationWorkspace,
    StartResearchWorkspace,
    SaveConversation,
    Settings,
}

impl MenuAction {
    fn label(self) -> &'static str {
        match self {
            Self::StartCodingProject => "Start Coding Project",
            Self::StartCreationWorkspace => "Start Creation Workspace",
            Self::StartResearchWorkspace => "Start Research Workspace",
            Self::SaveConversation => "Save Conversation",
            Self::Settings => "Settings",
        }
    }

    fn all() -> [MenuAction; 5] {
        [
            Self::StartCodingProject,
            Self::StartCreationWorkspace,
            Self::StartResearchWorkspace,
            Self::SaveConversation,
            Self::Settings,
        ]
    }
}

/// Three-dot button and popup menu.
pub struct ActionMenu;

impl ActionMenu {
    /// Draw the ⋯ button. Returns the button response.
    pub fn show_button(ui: &mut egui::Ui) -> egui::Response {
        ui.add_sized(
            [44.0, 44.0],
            egui::Button::new(RichText::new("⋯").size(22.0).color(ConversationTheme::BRAND))
                .fill(ConversationTheme::COMPOSER_FILL)
                .stroke(egui::Stroke::new(1.0_f32, ConversationTheme::BORDER))
                .corner_radius(14.0),
        )
    }

    /// Draw the popup near `anchor` (bottom-right of composer). Returns a chosen action.
    pub fn show_popup(ctx: &egui::Context, anchor: egui::Pos2) -> Option<MenuAction> {
        let mut chosen = None;
        let mut close = false;

        Area::new(egui::Id::new("jaymi_action_menu"))
            .order(Order::Foreground)
            .fixed_pos(anchor - egui::vec2(260.0, 220.0))
            .pivot(Align2::LEFT_TOP)
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .fill(ConversationTheme::COMPOSER_FILL)
                    .stroke(egui::Stroke::new(1.0_f32, ConversationTheme::BORDER))
                    .corner_radius(14.0)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_min_width(240.0);
                        ui.label(
                            RichText::new("Actions")
                                .size(12.0)
                                .color(ConversationTheme::MUTED_TEXT),
                        );
                        ui.add_space(6.0);
                        for action in MenuAction::all() {
                            let response = ui.add_sized(
                                [220.0, 32.0],
                                egui::Button::new(
                                    RichText::new(action.label())
                                        .size(14.5)
                                        .color(ConversationTheme::PRIMARY_TEXT),
                                )
                                .fill(if action == MenuAction::Settings {
                                    egui::Color32::from_rgb(236, 245, 244)
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .corner_radius(8.0),
                            );
                            if response.clicked() {
                                chosen = Some(action);
                                close = true;
                            }
                        }
                    });

                // Absorb clicks inside the menu.
                let _ = ui.interact(ui.max_rect(), ui.id().with("menu_catch"), Sense::click());
            });

        if close {
            chosen
        } else {
            None
        }
    }
}
