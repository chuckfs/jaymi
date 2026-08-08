//! Left sidebar — Project header card, Conversations list, status card.
//!
//! Ported directly from `Jaymi Redesign.dc.html`'s sidebar block (232px,
//! `padding: 8px 12px 16px 16px`). Coding, Research, Knowledge, Creation,
//! and Settings all live behind the top bar's workspace switcher instead
//! (see `ui::mod::render_top_bar`); this rail only ever shows the active
//! project, its conversation history, and local-reasoning status.

use eframe::egui;

use crate::settings_workspace::ReasoningConnectionStatus;
use crate::theme::{radius, space, stroke, type_size, Theme};
use crate::ui::components::{pulse_alpha, ButtonStyle};
use crate::ui::icons::{self, Icon};
use jaymi_memory::ConversationMeta;

/// Default open width of the left sidebar. Spec: `width: 232px`.
pub const DEFAULT_NAV_WIDTH: f32 = 232.0;
/// Soft floor when the rail is user-resized.
pub const MIN_NAV_WIDTH: f32 = 200.0;
/// Soft ceiling when the rail is user-resized.
pub const MAX_NAV_WIDTH: f32 = 320.0;

/// Spec: conversation / new-conversation rows are `height: 34px`.
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
    /// Reasoning connection status, for the bottom "Local · offline" card.
    pub reasoning_status: ReasoningConnectionStatus,
    /// Default reasoning model display name, when known.
    pub reasoning_model_label: Option<&'a str>,
}

/// Render the rail body (caller owns the SidePanel chrome).
pub fn render_nav_rail(ui: &mut egui::Ui, ctx: &NavRailContext<'_>, events: &mut Vec<NavRailEvent>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0; // Spec: container `gap: 2px`.

        // Spec: `padding: 8px 12px 6px 12px`.
        section_label(ui, ctx.theme, "Project", 8.0);
        render_project_card(ui, ctx, events);

        // Spec: `padding: 16px 12px 6px 12px`.
        section_label(ui, ctx.theme, "Conversations", 16.0);

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

        // Bottom-up: first call lands at the true bottom, so the status
        // card (called first) sits under the Diagnostics footer (called
        // second). `Align::Min` keeps `ui.horizontal` inside each from
        // inheriting right-to-left (that only triggers on `Align::Max`).
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            render_status_card(ui, ctx);
            ui.add_space(space::SM);
            render_footer(ui, ctx, events);
        });
    });
}

/// Spec: `font-size:11px; font-weight:700; letter-spacing:.08em;
/// text-transform:uppercase; color:var(--faint)`. Only the top padding
/// differs between "Project" (8px) and "Conversations" (16px).
fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str, padding_top: f32) {
    let pad = egui::Margin {
        left: 12,
        right: 12,
        top: padding_top as i8,
        bottom: 6,
    };
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .size(11.0)
                .color(theme.text_faint)
                .strong(),
        );
    });
}

/// The project header card. Spec: `gap:10px; background:var(--card);
/// border-radius:16px; padding:10px 12px; box-shadow:var(--sh-sm)`. The
/// badge is an organic blob: `border-radius: 45% 55% 50% 50%` on a 30px box.
fn render_project_card(ui: &mut egui::Ui, ctx: &NavRailContext<'_>, events: &mut Vec<NavRailEvent>) {
    let theme = ctx.theme;
    let response = egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .shadow(theme.shadow_sm())
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (badge_rect, _) =
                    ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    badge_rect,
                    egui::CornerRadius { nw: 13, ne: 17, sw: 15, se: 15 },
                    theme.accent,
                );
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "j",
                    crate::theme::display_font(15.0),
                    theme.on_accent(),
                );
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(ctx.project_label.unwrap_or("Open a project"))
                            .size(13.5)
                            .strong()
                            .color(theme.text_primary),
                    );
                    if let Some(meta) = ctx.project_meta {
                        ui.label(
                            egui::RichText::new(meta)
                                .size(11.5)
                                .color(theme.text_secondary),
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

/// Spec: `height:34px; gap:8px; padding:0 12px; border-radius:999px;
/// font-size:13px`. Selected fill `var(--card)`, hover `var(--panel)`.
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
    let clip = rect.shrink2(egui::vec2(12.0, 0.0));
    ui.painter().with_clip_rect(clip).text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        truncate_middle(label, 42),
        egui::FontId::proportional(13.0),
        text_color,
    );

    response
}

/// Spec: same row geometry, plus a 13x13 plus-icon at stroke-width 2.75.
fn new_conversation_row(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), theme.panel);
    }
    let color = if response.hovered() { theme.text_primary } else { theme.text_secondary };
    let icon_center = egui::pos2(rect.left() + 12.0 + 6.5, rect.center().y);
    icons::paint(ui.painter(), Icon::Plus, icon_center, 6.5, color);
    ui.painter().text(
        egui::pos2(icon_center.x + 6.5 + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "New conversation",
        egui::FontId::proportional(13.0),
        color,
    );
    response
}

fn hint_row(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let pad = egui::Margin {
        left: 12,
        right: 12,
        top: 4,
        bottom: 4,
    };
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(type_size::META)
                .color(theme.text_secondary),
        );
    });
}

/// Bottom status card. Spec: `background:var(--panel); border-radius:16px;
/// padding:10px 12px; gap:8px`, with an 8px sage dot that pulses (`jyPulse
/// 3s ease-in-out infinite`).
fn render_status_card(ui: &mut egui::Ui, ctx: &NavRailContext<'_>) {
    let theme = ctx.theme;
    egui::Frame::new()
        .fill(theme.panel)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                let pulse = pulse_alpha(ui, 3.0);
                ui.painter()
                    .circle_filled(dot_rect.center(), 4.0, theme.accent2.gamma_multiply(pulse));
                ui.ctx().request_repaint();
                ui.vertical(|ui| {
                    let (label, connected) = match ctx.reasoning_status {
                        ReasoningConnectionStatus::Connected => ("Local · connected", true),
                        ReasoningConnectionStatus::Connecting => ("Local · connecting…", false),
                        ReasoningConnectionStatus::Offline => ("Local · offline", false),
                        ReasoningConnectionStatus::Error => ("Local · error", false),
                    };
                    let _ = connected;
                    ui.label(
                        egui::RichText::new(label)
                            .size(12.0)
                            .strong()
                            .color(theme.text_primary),
                    );
                    ui.label(
                        egui::RichText::new(ctx.reasoning_model_label.unwrap_or("No model selected"))
                            .size(11.5)
                            .color(theme.text_secondary),
                    );
                });
            });
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

/// Not part of the prototype (which has no developer-tooling affordance) —
/// kept per "do not remove existing functionality", placed above the status
/// card so it doesn't compete with the spec'd element below it.
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
