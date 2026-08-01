//! Settings panel — the only functional three-dot action in Slice 4.

use eframe::egui::{self, Align2, Area, Frame, Order, RichText, Window};

use crate::diagnostics::DiagnosticsSnapshot;

use super::theme::ConversationTheme;

/// Lightweight settings overlay.
pub struct SettingsPanel;

impl SettingsPanel {
    /// Draw settings. Returns `Some(true)` when the user closes the panel.
    pub fn show(ctx: &egui::Context, snapshot: Option<&DiagnosticsSnapshot>) -> Option<bool> {
        let mut close = false;
        let mut open = true;

        // Soft dimming layer behind the settings window.
        Area::new(egui::Id::new("settings_backdrop"))
            .order(Order::Middle)
            .fixed_pos(egui::pos2(0.0, 0.0))
            .interactable(true)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    screen,
                    0.0,
                    egui::Color32::from_black_alpha(45),
                );
                if response.clicked() {
                    close = true;
                }
            });

        Window::new("Settings")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .open(&mut open)
            .frame(
                Frame::popup(&ctx.style())
                    .fill(ConversationTheme::COMPOSER_FILL)
                    .stroke(egui::Stroke::new(1.0_f32, ConversationTheme::BORDER))
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(24, 20))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(40),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label(
                    RichText::new("Settings")
                        .size(22.0)
                        .color(ConversationTheme::BRAND)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Conversation is Jaymi’s home. Workspaces will expand from here in future slices.",
                    )
                    .size(13.5)
                    .color(ConversationTheme::MUTED_TEXT),
                );
                ui.add_space(16.0);

                if let Some(snapshot) = snapshot {
                    setting_row(ui, "Status", snapshot.app_state.label());
                    setting_row(ui, "Planner", snapshot.planner_label());
                    setting_row(ui, "Providers", &snapshot.provider_count.to_string());
                    setting_row(ui, "Tools", &snapshot.tool_count.to_string());
                    setting_row(
                        ui,
                        "Capabilities",
                        &snapshot.capability_count.to_string(),
                    );
                    setting_row(ui, "Database", snapshot.database_label());
                } else {
                    ui.label("Unable to load runtime settings.");
                }

                ui.add_space(18.0);
                if ui
                    .add_sized(
                        [120.0, 36.0],
                        egui::Button::new(
                            RichText::new("Close").color(ConversationTheme::ON_ACCENT_TEXT),
                        )
                        .fill(ConversationTheme::ACCENT)
                        .corner_radius(10.0),
                    )
                    .clicked()
                {
                    close = true;
                }
            });

        if !open {
            close = true;
        }

        Some(close).filter(|value| *value)
    }
}

fn setting_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(14.0)
                .color(ConversationTheme::MUTED_TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .size(14.5)
                    .color(ConversationTheme::PRIMARY_TEXT)
                    .strong(),
            );
        });
    });
    ui.add_space(6.0);
}
