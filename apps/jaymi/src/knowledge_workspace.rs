//! Knowledge Workspace — search bar, vault cards, hit list + inspector.
//!
//! Backed by the real `KnowledgeStore` (the local SQLite file inventory) —
//! vaults are the store's [`jaymi_knowledge::Collection`]s (Desktop,
//! Documents, Projects, …) and hits are real filesystem paths. There is no
//! Obsidian / Claude / ChatGPT import backend yet, so this workspace only
//! ever shows what the inventory actually knows — no fabricated content.

use eframe::egui::{self, Align, Layout, RichText};

use crate::theme::{inset, radius, space, type_size, Theme};
use crate::ui::components::{card_frame, pill_button, tag, ButtonStyle, TagStyle};
use crate::ui::icons::{self, Icon};
use jaymi_capabilities::{KnowledgeHitState, KnowledgeState, KnowledgeVaultState};

/// UI-only selection state for the Knowledge workspace (not inventory data).
#[derive(Debug, Clone, Default)]
pub struct KnowledgeWorkspaceState {
    /// Current search box contents.
    pub query: String,
    /// Selected vault id, when browsing one collection.
    pub selected_vault: Option<String>,
    /// Selected hit id, shown in the inspector.
    pub selected_hit: Option<String>,
}

/// Events the Knowledge workspace can emit (the app applies them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeWorkspaceEvent {
    /// Search box contents changed — re-query the store.
    QueryChanged(String),
    /// A vault card was toggled.
    SelectVault(Option<String>),
    /// A hit row was selected for the inspector.
    SelectHit(String),
    /// Reveal a path in Finder.
    RevealInFinder(String),
}

/// Inputs required to paint the Knowledge workspace.
pub struct KnowledgeWorkspaceContext<'a> {
    /// Theme tokens.
    pub theme: &'a Theme,
    /// Local UI selection state.
    pub state: &'a KnowledgeWorkspaceState,
    /// Live vaults + hits from the last query, when a workspace is active.
    pub knowledge: Option<&'a KnowledgeState>,
}

/// Render the Knowledge workspace surface.
pub fn render_knowledge_workspace(
    ui: &mut egui::Ui,
    ctx: &KnowledgeWorkspaceContext<'_>,
    events: &mut Vec<KnowledgeWorkspaceEvent>,
) {
    let theme = ctx.theme;

    render_search_bar(ui, ctx, events);
    ui.add_space(space::MD);

    if let Some(knowledge) = ctx.knowledge {
        if !knowledge.vaults.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
                for vault in &knowledge.vaults {
                    let selected = ctx.state.selected_vault.as_deref() == Some(vault.id.as_str());
                    if vault_card(ui, theme, vault, selected).clicked() {
                        let next = if selected { None } else { Some(vault.id.clone()) };
                        events.push(KnowledgeWorkspaceEvent::SelectVault(next));
                    }
                }
            });
            ui.add_space(space::MD);
        }
    }

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;
        let hits_width = ui.available_width() * 0.6;

        ui.vertical(|ui| {
            ui.set_width(hits_width);
            egui::ScrollArea::vertical()
                .id_salt("knowledge_hits")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let hits = ctx.knowledge.map(|k| k.hits.as_slice()).unwrap_or(&[]);
                    if hits.is_empty() {
                        ui.label(
                            RichText::new(if ctx.state.query.trim().is_empty() {
                                "Search above, or pick a vault to browse it."
                            } else {
                                "No matches — try a different search."
                            })
                            .size(type_size::BODY)
                            .color(theme.text_secondary),
                        );
                    } else {
                        for hit in hits {
                            let selected = ctx.state.selected_hit.as_deref() == Some(hit.id.as_str());
                            if hit_row(ui, theme, hit, selected).clicked() {
                                events.push(KnowledgeWorkspaceEvent::SelectHit(hit.id.clone()));
                            }
                            ui.add_space(space::XS);
                        }
                    }
                });
        });

        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width());
            render_inspector(ui, theme, ctx, events);
        });
    });
}

fn render_search_bar(
    ui: &mut egui::Ui,
    ctx: &KnowledgeWorkspaceContext<'_>,
    events: &mut Vec<KnowledgeWorkspaceEvent>,
) {
    let theme = ctx.theme;
    let mut draft = ctx.state.query.clone();
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::PILL)
        .shadow(theme.shadow_sm())
        .inner_margin(egui::Margin::symmetric((space::MD + space::XS) as i8, space::SM as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                icons::paint(ui.painter(), Icon::Search, rect.center(), 7.0, theme.text_secondary);
                ui.add_space(space::XS);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut draft)
                        .desired_width(ui.available_width() - 140.0)
                        .hint_text(
                            RichText::new("Search files across your Mac…").color(theme.text_faint),
                        )
                        .text_color(theme.text_primary)
                        .frame(false),
                );
                if response.changed() {
                    events.push(KnowledgeWorkspaceEvent::QueryChanged(draft));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let count = ctx.knowledge.map(|k| k.hits.len()).unwrap_or(0);
                    ui.label(
                        RichText::new(format!("{count} matches · all local"))
                            .size(type_size::META)
                            .color(theme.text_faint),
                    );
                });
            });
        });
}

fn vault_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    vault: &KnowledgeVaultState,
    selected: bool,
) -> egui::Response {
    let border = if selected {
        theme.accent
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::LG)
        .inner_margin(inset(space::MD, space::SM + space::XS))
        .stroke(egui::Stroke::new(1.5, border))
        .show(ui, |ui| {
            ui.set_width(150.0);
            ui.label(
                RichText::new(&vault.name)
                    .size(type_size::UI)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.label(
                RichText::new(&vault.meta)
                    .size(type_size::META - 0.5)
                    .color(theme.text_secondary),
            );
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn hit_row(ui: &mut egui::Ui, theme: &Theme, hit: &KnowledgeHitState, selected: bool) -> egui::Response {
    let fill = if selected {
        theme.surface
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(radius::LG)
        .inner_margin(inset(space::MD, space::SM))
        .shadow(if selected { theme.shadow_sm() } else { egui::Shadow::NONE })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(&hit.title)
                    .size(type_size::UI)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.label(
                RichText::new(&hit.snippet)
                    .size(type_size::META)
                    .color(theme.text_secondary),
            );
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn render_inspector(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &KnowledgeWorkspaceContext<'_>,
    events: &mut Vec<KnowledgeWorkspaceEvent>,
) {
    let selected_hit = ctx.state.selected_hit.as_deref().and_then(|id| {
        ctx.knowledge
            .and_then(|knowledge| knowledge.hits.iter().find(|hit| hit.id == id))
    });
    card_frame(theme).show(ui, |ui| {
        ui.set_min_height(160.0);
        ui.set_width(ui.available_width());
        match selected_hit {
            Some(hit) => {
                tag(ui, theme, "Local file", TagStyle::Neutral);
                ui.add_space(space::SM);
                ui.label(
                    RichText::new(&hit.title)
                        .size(type_size::TITLE)
                        .strong()
                        .color(theme.text_primary),
                );
                ui.add_space(space::XS);
                ui.label(
                    RichText::new(&hit.snippet)
                        .size(type_size::META)
                        .color(theme.text_secondary),
                );
                ui.add_space(space::MD);
                if pill_button(ui, theme, "Reveal in Finder", ButtonStyle::Secondary).clicked() {
                    events.push(KnowledgeWorkspaceEvent::RevealInFinder(hit.id.clone()));
                }
            }
            None => {
                ui.label(
                    RichText::new("Select a result to inspect it.")
                        .size(type_size::BODY)
                        .color(theme.text_secondary),
                );
            }
        }
    });
}
