//! Research Workspace — sources and notes collected during a conversation.
//!
//! UI shell only: there is no deep-research / citation-synthesis backend yet
//! (that's separate, larger effort — see the redesign plan's non-goals), so
//! this renders whatever real [`ResearchState`] exists, honestly empty when
//! nothing has been collected, rather than fabricating synthesis content.

use eframe::egui::{self, RichText};

use crate::theme::{inset, radius, space, type_size, Theme};
use crate::ui::components::{card_frame, render_workspace_header, tag, TagStyle};
use crate::ui::icons::Icon;
use jaymi_capabilities::ResearchState;

/// Render the Research workspace surface.
pub fn render_research_workspace(ui: &mut egui::Ui, theme: &Theme, state: Option<&ResearchState>) {
    let source_count = state.map(|s| s.sources.len()).unwrap_or(0);
    let subtitle = (source_count > 0).then(|| {
        format!("{source_count} source{}", if source_count == 1 { "" } else { "s" })
    });
    render_workspace_header(
        ui,
        theme,
        Icon::Research,
        theme.accent2_tint,
        theme.accent2_deep,
        "Research",
        subtitle.as_deref(),
    );
    ui.add_space(space::MD);

    let sources = state.map(|s| s.sources.as_slice()).unwrap_or(&[]);
    let notes = state.map(|s| s.notes.as_slice()).unwrap_or(&[]);

    if sources.is_empty() && notes.is_empty() {
        empty_state(ui, theme);
        return;
    }

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;

        ui.vertical(|ui| {
            ui.set_width(ui.available_width() * 0.55);
            section_label(ui, theme, "Sources");
            ui.add_space(space::SM);
            if sources.is_empty() {
                muted_hint(ui, theme, "No sources collected yet.");
            } else {
                for (index, source) in sources.iter().enumerate() {
                    source_card(ui, theme, index + 1, &source.title, source.uri.as_deref());
                    ui.add_space(space::SM);
                }
            }
        });

        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width());
            section_label(ui, theme, "Notes");
            ui.add_space(space::SM);
            if notes.is_empty() {
                muted_hint(ui, theme, "No working notes yet.");
            } else {
                for note in notes {
                    note_card(ui, theme, &note.content);
                    ui.add_space(space::SM);
                }
            }
        });
    });
}

fn empty_state(ui: &mut egui::Ui, theme: &Theme) {
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        tag(ui, theme, "Research before synthesis", TagStyle::Accent2);
        ui.add_space(space::MD);
        ui.label(
            RichText::new("Nothing researched yet in this conversation.")
                .size(type_size::BODY)
                .color(theme.text_secondary),
        );
        ui.add_space(space::XS);
        ui.set_max_width((ui.available_width() * 0.7).clamp(240.0, 420.0));
        ui.label(
            RichText::new(
                "Ask Jaymi a research question — sources and notes it collects along the way will show up here.",
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

fn muted_hint(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(RichText::new(text).size(type_size::META).color(theme.text_faint));
}

fn source_card(ui: &mut egui::Ui, theme: &Theme, index: usize, title: &str, uri: Option<&str>) {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::LG)
        .inner_margin(inset(space::MD, space::SM + space::XS))
        .shadow(theme.shadow_sm())
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 9.0, theme.accent);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    index.to_string(),
                    egui::FontId::proportional(type_size::META - 1.0),
                    theme.on_accent(),
                );
                ui.add_space(space::XS);
                ui.label(
                    RichText::new(title)
                        .size(type_size::UI)
                        .strong()
                        .color(theme.text_primary),
                );
            });
            if let Some(uri) = uri {
                ui.add_space(2.0);
                ui.label(RichText::new(uri).size(type_size::META).color(theme.text_faint));
            }
        });
}

fn note_card(ui: &mut egui::Ui, theme: &Theme, content: &str) {
    card_frame(theme).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(content)
                .size(type_size::META)
                .color(theme.text_secondary),
        );
    });
}
