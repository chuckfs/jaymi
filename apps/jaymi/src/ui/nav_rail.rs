//! Left sidebar — Project header card, Conversations list, new conversation.
//!
//! Conversation-first: the sidebar no longer carries tabs. Coding, Research,
//! Knowledge, Creation, and Settings all live behind the top bar's workspace
//! switcher instead (see `ui::mod::render_top_bar`); this rail only ever
//! shows the active project and its conversation history.

use eframe::egui;

use crate::theme::{radius, space, stroke, type_size, Theme};
use crate::ui::components::{card_frame, ButtonStyle};
use crate::ui::icons::{self, Icon};
use jaymi_memory::ConversationMeta;

/// Default open width of the left sidebar.
pub const DEFAULT_NAV_WIDTH: f32 = 232.0;
/// Soft floor when the rail is user-resized.
pub const MIN_NAV_WIDTH: f32 = 200.0;
/// Soft ceiling when the rail is user-resized.
pub const MAX_NAV_WIDTH: f32 = 320.0;

const ROW_H: f32 = 34.0;

/// Events emitted by the sidebar (the app applies them).
#[derive(Debug, Clone)]
pub enum NavRailEvent {
    /// Open a folder picker for a new project.
    OpenProject,
    /// Open a known project by id.
    OpenProjectId(String),
    /// Toggle developer diagnostics under conversation.
    ToggleDiagnostics,
    /// Load a persisted conversation into the center chat.
    OpenConversation(String),
    /// Expand Coding from the right (or open a project first).
    OpenCoding,
    /// Start a new conversation in the active project.
    NewConversation,
}

/// Inputs needed to paint the sidebar.
pub struct NavRailContext<'a> {
    /// Theme tokens.
    pub theme: &'a Theme,
    /// Active project's display name, when one is open.
    pub project_label: Option<&'a str>,
    /// Short meta line under the project name (e.g. its root path).
    pub project_meta: Option<&'a str>,
    /// Conversations for the active project (empty when none).
    pub conversations: &'a [ConversationMeta],
    /// Active conversation id, when bound.
    pub active_conversation_id: Option<&'a str>,
    /// Whether a project is currently open.
    pub has_project: bool,
    /// Whether Coding is expanded on the right.
    pub coding_open: bool,
    /// Whether developer diagnostics are visible.
    pub show_diagnostics: bool,
}

/// Render the rail body (caller owns the SidePanel chrome).
pub fn render_nav_rail(ui: &mut egui::Ui, ctx: &NavRailContext<'_>, events: &mut Vec<NavRailEvent>) {
    ui.vertical(|ui| {
        ui.add_space(space::XS);
        section_label(ui, ctx.theme, "Project");
        render_project_card(ui, ctx, events);

        ui.add_space(space::MD);
        section_label(ui, ctx.theme, "Conversations");

        egui::ScrollArea::vertical()
            .id_salt("jaymi_nav_conversations")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !ctx.has_project {
                    hint_row(ui, ctx.theme, "Open a project to see its chats.");
                } else if ctx.conversations.is_empty() {
                    hint_row(ui, ctx.theme, "No conversations yet — send a message to start one.");
                } else {
                    for meta in ctx.conversations.iter().take(24) {
                        let id = meta.id.to_string();
                        let selected = ctx.active_conversation_id == Some(id.as_str());
                        let title = meta
                            .title
                            .as_deref()
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or("Conversation");
                        if conversation_row(ui, ctx.theme, title, selected).clicked() {
                            events.push(NavRailEvent::OpenConversation(id));
                        }
                    }
                }
                if new_conversation_row(ui, ctx.theme).clicked() {
                    events.push(NavRailEvent::NewConversation);
                }
            });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            render_footer(ui, ctx, events);
        });
    });
}

fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let pad = egui::Margin {
        left: (space::SM) as i8,
        right: space::SM as i8,
        top: space::XS as i8,
        bottom: space::XS as i8,
    };
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .size(type_size::META - 1.0)
                .color(theme.text_faint)
                .strong(),
        );
    });
}

/// The project header card — the Organic "signature" small card, clickable
/// to open Coding (or the folder picker, when no project is active yet).
fn render_project_card(ui: &mut egui::Ui, ctx: &NavRailContext<'_>, events: &mut Vec<NavRailEvent>) {
    let response = card_frame(ctx.theme)
        .inner_margin(egui::Margin::symmetric(space::SM as i8, space::SM as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (badge_rect, _) =
                    ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    badge_rect,
                    egui::CornerRadius::same((radius::MD * 0.9) as u8),
                    ctx.theme.accent,
                );
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "j",
                    crate::theme::display_font(15.0),
                    ctx.theme.on_accent(),
                );
                ui.add_space(space::SM);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(ctx.project_label.unwrap_or("Open a project"))
                            .size(type_size::UI)
                            .strong()
                            .color(ctx.theme.text_primary),
                    );
                    if let Some(meta) = ctx.project_meta {
                        ui.label(
                            egui::RichText::new(meta)
                                .size(type_size::META - 0.5)
                                .color(ctx.theme.text_secondary),
                        );
                    }
                });
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.clicked() {
        if ctx.has_project {
            if !ctx.coding_open {
                events.push(NavRailEvent::OpenCoding);
            }
        } else {
            events.push(NavRailEvent::OpenProject);
        }
    }
}

fn conversation_row(ui: &mut egui::Ui, theme: &Theme, label: &str, selected: bool) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    let hovered = response.hovered();
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), theme.surface);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), theme.panel);
    }

    let text_color = if selected || hovered {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    let clip = rect.shrink2(egui::vec2(space::SM, 0.0));
    ui.painter().with_clip_rect(clip).text(
        egui::pos2(rect.left() + space::SM + 2.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        truncate_middle(label, 42),
        egui::FontId::proportional(type_size::UI),
        text_color,
    );

    response
}

fn new_conversation_row(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), theme.panel);
    }
    let color = if response.hovered() { theme.text_primary } else { theme.text_secondary };
    let icon_center = egui::pos2(rect.left() + space::SM + 8.0, rect.center().y);
    icons::paint(ui.painter(), Icon::Plus, icon_center, 7.0, color);
    ui.painter().text(
        egui::pos2(icon_center.x + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "New conversation",
        egui::FontId::proportional(type_size::UI),
        color,
    );
    response
}

fn hint_row(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let pad = egui::Margin {
        left: space::SM as i8,
        right: space::SM as i8,
        top: space::XS as i8,
        bottom: space::XS as i8,
    };
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(type_size::META)
                .color(theme.text_secondary),
        );
    });
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1) / 2;
    let start: String = text.chars().take(keep).collect();
    let end: String = text.chars().rev().take(keep).collect::<String>().chars().rev().collect();
    format!("{start}…{end}")
}

fn render_footer(ui: &mut egui::Ui, ctx: &NavRailContext<'_>, events: &mut Vec<NavRailEvent>) {
    ui.add_space(space::XS);
    let diagnostics_label = if ctx.show_diagnostics {
        "Hide Diagnostics"
    } else {
        "Developer Diagnostics"
    };
    if crate::ui::components::pill_button(ui, ctx.theme, diagnostics_label, ButtonStyle::Ghost)
        .clicked()
    {
        events.push(NavRailEvent::ToggleDiagnostics);
    }
    ui.add_space(space::SM);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(stroke::HAIRLINE, ctx.theme.border),
    );
    ui.add_space(space::SM);
}
