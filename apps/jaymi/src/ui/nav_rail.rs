//! Left navigation rail — Projects, Knowledge, Media, Conversations.
//!
//! ChatGPT-style: Projects and Conversations expand inline inside the rail
//! (no floating popups). Order is Projects → Knowledge → Media → Conversations,
//! with Developer Diagnostics pinned at the bottom.

use eframe::egui;

use crate::theme::{radius, space, stroke, type_size, Theme};
use jaymi_memory::ConversationMeta;

/// Default open width of the left nav rail.
pub const DEFAULT_NAV_WIDTH: f32 = 240.0;
/// Soft floor when the rail is user-resized.
pub const MIN_NAV_WIDTH: f32 = 200.0;
/// Soft ceiling when the rail is user-resized.
pub const MAX_NAV_WIDTH: f32 = 320.0;

const ROW_H: f32 = 36.0;
const NEST_H: f32 = 32.0;

/// Active highlight in the left nav rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavTab {
    /// Projects section (expandable).
    Projects,
    /// Knowledge base stub.
    Knowledge,
    /// Media library stub.
    Media,
    /// Conversations section (expandable, under Media).
    #[default]
    Conversations,
}

impl NavTab {
    /// Short label for the row.
    pub fn label(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Knowledge => "Knowledge",
            Self::Media => "Media",
            Self::Conversations => "Conversations",
        }
    }
}

/// Events emitted by the nav rail (Application applies them).
#[derive(Debug, Clone)]
pub enum NavRailEvent {
    /// Switch the active rail highlight.
    SelectTab(NavTab),
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
}

/// Inputs needed to paint the rail.
pub struct NavRailContext<'a> {
    /// Theme tokens.
    pub theme: &'a Theme,
    /// Active rail highlight.
    pub tab: NavTab,
    /// Conversations for the active project (empty when none).
    pub conversations: &'a [ConversationMeta],
    /// Active conversation id, when bound.
    pub active_conversation_id: Option<&'a str>,
    /// Whether a project is currently open.
    pub has_project: bool,
    /// Whether Coding is expanded on the right.
    pub coding_open: bool,
    /// Known recent projects `(id, label)`.
    pub recent_projects: &'a [(String, String)],
    /// Whether developer diagnostics are visible.
    pub show_diagnostics: bool,
}

/// Render the rail body (caller owns the SidePanel chrome).
pub fn render_nav_rail(ui: &mut egui::Ui, ctx: &NavRailContext<'_>, events: &mut Vec<NavRailEvent>) {
    let projects_open_id = egui::Id::new("jaymi_nav_projects_open");
    let conversations_open_id = egui::Id::new("jaymi_nav_conversations_open");

    let mut projects_open = ui.ctx().data(|d| d.get_temp::<bool>(projects_open_id).unwrap_or(false));
    let mut conversations_open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(conversations_open_id).unwrap_or(false));

    ui.vertical(|ui| {
        ui.add_space(space::XS);

        // Projects → Knowledge → Media → Conversations (top-down collapses).
        let projects_resp = section_row(
            ui,
            ctx.theme,
            NavTab::Projects.label(),
            ctx.tab == NavTab::Projects,
            Some(projects_open),
        );
        if projects_resp.clicked() {
            projects_open = !projects_open;
            if projects_open {
                conversations_open = false;
            }
            events.push(NavRailEvent::SelectTab(NavTab::Projects));
        }
        if projects_open {
            render_projects_section(ui, ctx, events);
        }

        if section_row(
            ui,
            ctx.theme,
            NavTab::Knowledge.label(),
            ctx.tab == NavTab::Knowledge,
            None,
        )
        .clicked()
        {
            projects_open = false;
            conversations_open = false;
            events.push(NavRailEvent::SelectTab(NavTab::Knowledge));
        }
        if ctx.tab == NavTab::Knowledge {
            nest_hint(
                ui,
                ctx.theme,
                "Knowledge base coming soon — notes, docs, and recall across projects.",
            );
        }

        if section_row(
            ui,
            ctx.theme,
            NavTab::Media.label(),
            ctx.tab == NavTab::Media,
            None,
        )
        .clicked()
        {
            projects_open = false;
            conversations_open = false;
            events.push(NavRailEvent::SelectTab(NavTab::Media));
        }
        if ctx.tab == NavTab::Media {
            nest_hint(
                ui,
                ctx.theme,
                "Media library coming soon — images, audio, and attachments.",
            );
        }

        let conversations_resp = section_row(
            ui,
            ctx.theme,
            NavTab::Conversations.label(),
            ctx.tab == NavTab::Conversations,
            Some(conversations_open),
        );
        if conversations_resp.clicked() {
            conversations_open = !conversations_open;
            if conversations_open {
                projects_open = false;
            }
            events.push(NavRailEvent::SelectTab(NavTab::Conversations));
        }
        if conversations_open {
            render_conversations_section(ui, ctx, events);
        }

        // Diagnostics stay at the bottom of the rail.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            render_footer(ui, ctx, events);
        });
    });

    ui.ctx().data_mut(|d| {
        d.insert_temp(projects_open_id, projects_open);
        d.insert_temp(conversations_open_id, conversations_open);
    });
}

fn section_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    selected: bool,
    expandable: Option<bool>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());

    let hovered = response.hovered();
    let bg = if selected {
        theme.selection()
    } else if hovered {
        theme.surface_alt
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::SM as u8), bg);
    }

    let text_color = if selected || hovered {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(space::SM, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.label(
        egui::RichText::new(label)
            .size(type_size::UI)
            .strong()
            .color(text_color),
    );
    if let Some(expanded) = expandable {
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let chevron = if expanded { "▾" } else { "›" };
            ui.label(
                egui::RichText::new(chevron)
                    .size(type_size::META)
                    .color(theme.text_secondary),
            );
        });
    }

    response
}

fn render_projects_section(
    ui: &mut egui::Ui,
    ctx: &NavRailContext<'_>,
    events: &mut Vec<NavRailEvent>,
) {
    ui.add_space(space::XS);

    if nest_action(ui, ctx.theme, "+", "Open Project").clicked() {
        events.push(NavRailEvent::OpenProject);
    }

    if ctx.has_project && !ctx.coding_open {
        if nest_item(ui, ctx.theme, "Open Coding", false).clicked() {
            events.push(NavRailEvent::OpenCoding);
        }
    } else if ctx.coding_open {
        nest_hint(ui, ctx.theme, "Coding is open beside chat.");
    }

    ui.add_space(space::XS);
    nest_label(ui, ctx.theme, "Recent");

    egui::ScrollArea::vertical()
        .id_salt("jaymi_nav_projects_recent")
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if ctx.recent_projects.is_empty() {
                nest_hint(ui, ctx.theme, "No recent projects yet.");
            } else {
                for (id, label) in ctx.recent_projects.iter().take(12) {
                    if nest_item(ui, ctx.theme, label, false).clicked() {
                        events.push(NavRailEvent::OpenProjectId(id.clone()));
                    }
                }
            }
        });

    ui.add_space(space::SM);
}

fn render_conversations_section(
    ui: &mut egui::Ui,
    ctx: &NavRailContext<'_>,
    events: &mut Vec<NavRailEvent>,
) {
    ui.add_space(space::XS);

    if !ctx.has_project {
        if nest_action(ui, ctx.theme, "+", "Open Project").clicked() {
            events.push(NavRailEvent::OpenProject);
        }
        nest_hint(ui, ctx.theme, "Open a project to see its chats.");
        ui.add_space(space::SM);
        return;
    }

    nest_label(ui, ctx.theme, "Recent");

    egui::ScrollArea::vertical()
        .id_salt("jaymi_nav_conversations_recent")
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if ctx.conversations.is_empty() {
                nest_hint(
                    ui,
                    ctx.theme,
                    "No conversations yet. Send a message to start one.",
                );
            } else {
                for meta in ctx.conversations.iter().take(12) {
                    let id = meta.id.to_string();
                    let selected = ctx.active_conversation_id == Some(id.as_str());
                    let title = meta
                        .title
                        .as_deref()
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or("Conversation");
                    if nest_item(ui, ctx.theme, title, selected).clicked() {
                        events.push(NavRailEvent::OpenConversation(id));
                    }
                }
            }
        });

    ui.add_space(space::SM);
}

fn nest_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let pad = egui::Margin {
        left: (space::MD + space::XS) as i8,
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

fn nest_hint(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let pad = egui::Margin {
        left: (space::MD + space::XS) as i8,
        right: space::SM as i8,
        top: 2,
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

fn nest_action(ui: &mut egui::Ui, theme: &Theme, leading: &str, label: &str) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NEST_H), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(radius::SM as u8),
            theme.selection(),
        );
    }

    let inner = rect.shrink2(egui::vec2(space::MD, 0.0));
    let chip = egui::Rect::from_center_size(
        egui::pos2(inner.left() + 10.0, inner.center().y),
        egui::vec2(20.0, 20.0),
    );
    ui.painter().rect_filled(
        chip,
        egui::CornerRadius::same(radius::SM as u8),
        theme.selection(),
    );
    ui.painter().text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        leading,
        egui::FontId::proportional(type_size::META),
        theme.accent,
    );
    ui.painter().text(
        egui::pos2(chip.right() + space::SM, inner.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(type_size::UI),
        theme.accent,
    );

    response
}

fn nest_item(ui: &mut egui::Ui, theme: &Theme, label: &str, selected: bool) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NEST_H), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    let hovered = response.hovered();
    let bg = if selected {
        theme.selection()
    } else if hovered {
        theme.surface_alt
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::SM as u8), bg);
    }

    let text_color = if selected || hovered {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    ui.painter().text(
        egui::pos2(rect.left() + space::MD + space::XS, rect.center().y),
        egui::Align2::LEFT_CENTER,
        truncate_middle(label, 42),
        egui::FontId::proportional(type_size::UI),
        text_color,
    );

    response
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
    ui.add_space(space::SM);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(stroke::HAIRLINE, ctx.theme.border),
    );
    ui.add_space(space::SM);

    let diagnostics_label = if ctx.show_diagnostics {
        "Hide Diagnostics"
    } else {
        "Developer Diagnostics"
    };
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new(diagnostics_label)
                    .size(type_size::META)
                    .color(ctx.theme.text_secondary),
            )
            .frame(false),
        )
        .clicked()
    {
        events.push(NavRailEvent::ToggleDiagnostics);
    }
    ui.add_space(space::SM);
}
