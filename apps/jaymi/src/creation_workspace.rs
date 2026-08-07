//! Creation Workspace — assets generated during a conversation.
//!
//! UI shell only: there is no on-device image-generation backend wired up
//! yet (that's a separate, larger effort — see the redesign plan's
//! non-goals), so this renders whatever real [`CreationState`] exists,
//! honestly empty when nothing has been generated, rather than a fake
//! "Generate" control that would do nothing.

use eframe::egui::{self, RichText};

use crate::theme::{inset, radius, space, type_size, Theme};
use crate::ui::components::tag;
use crate::ui::components::TagStyle;
use crate::ui::icons::{self, Icon};
use jaymi_capabilities::CreationState;

/// Render the Creation workspace surface.
pub fn render_creation_workspace(ui: &mut egui::Ui, theme: &Theme, state: Option<&CreationState>) {
    header(ui, theme);
    ui.add_space(space::MD);

    let assets = state.map(|s| s.generated_assets.as_slice()).unwrap_or(&[]);
    let steps = state.map(|s| s.canvas_history.as_slice()).unwrap_or(&[]);

    if assets.is_empty() && steps.is_empty() {
        empty_state(ui, theme);
        return;
    }

    if !assets.is_empty() {
        section_label(ui, theme, "Generated assets");
        ui.add_space(space::SM);
        egui::Grid::new("creation_asset_grid")
            .num_columns(2)
            .spacing(egui::vec2(space::MD, space::MD))
            .show(ui, |ui| {
                for (index, asset) in assets.iter().enumerate() {
                    asset_card(ui, theme, asset);
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(space::MD);
    }

    if !steps.is_empty() {
        section_label(ui, theme, "Canvas history");
        ui.add_space(space::SM);
        for step in steps {
            ui.label(
                RichText::new(format!("• {}", step.summary))
                    .size(type_size::META)
                    .color(theme.text_secondary),
            );
        }
    }
}

fn header(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius { nw: 16, ne: 13, sw: 15, se: 17 },
            theme.accent_tint,
        );
        icons::paint(ui.painter(), Icon::Creation, rect.center(), 7.5, theme.accent_deep);
        ui.add_space(space::SM);
        ui.label(
            RichText::new("Creation")
                .font(crate::theme::display_font(type_size::TITLE))
                .color(theme.text_primary),
        );
    });
}

fn empty_state(ui: &mut egui::Ui, theme: &Theme) {
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        tag(ui, theme, "On-device generation", TagStyle::Accent);
        ui.add_space(space::MD);
        ui.label(
            RichText::new("Nothing generated yet in this conversation.")
                .size(type_size::BODY)
                .color(theme.text_secondary),
        );
        ui.add_space(space::XS);
        ui.set_max_width((ui.available_width() * 0.7).clamp(240.0, 420.0));
        ui.label(
            RichText::new(
                "On-device image generation isn't wired up yet — assets Jaymi creates for you will collect here once it is.",
            )
            .size(type_size::META)
            .color(theme.text_faint),
        );
    });
}

fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(type_size::META - 1.0)
            .color(theme.text_faint)
            .strong(),
    );
}

fn asset_card(ui: &mut egui::Ui, theme: &Theme, asset: &jaymi_capabilities::GeneratedAssetState) {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::LG)
        .inner_margin(inset(space::SM + space::XS, space::SM + space::XS))
        .shadow(theme.shadow_sm())
        .show(ui, |ui| {
            ui.set_width(180.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, 150.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(radius::MD as u8), theme.accent_tint);
            icons::paint(ui.painter(), Icon::Creation, rect.center(), 22.0, theme.accent);
            ui.add_space(space::XS);
            ui.label(
                RichText::new(&asset.kind)
                    .size(type_size::META)
                    .strong()
                    .color(theme.text_primary),
            );
            if let Some(uri) = &asset.uri {
                ui.label(RichText::new(uri).size(type_size::META - 1.0).color(theme.text_faint));
            }
        });
}
