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
            NavTab::Projects,
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
            NavTab::Knowledge,
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
            NavTab::Media,
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
            NavTab::Conversations,
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
    tab: NavTab,
    selected: bool,
    expandable: Option<bool>,
) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
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

    let color = if selected || hovered {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    let pad = space::SM;
    let icon_size = 16.0;
    let icon_center = egui::pos2(rect.left() + pad + icon_size * 0.5, rect.center().y);
    paint_nav_icon(ui.painter(), tab, icon_center, icon_size, color);

    let text_x = rect.left() + pad + icon_size + space::SM;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        tab.label(),
        egui::FontId::proportional(type_size::UI),
        color,
    );

    if let Some(expanded) = expandable {
        let chevron_center = egui::pos2(rect.right() - pad - 5.0, rect.center().y);
        paint_nav_chevron(ui.painter(), chevron_center, expanded, theme.text_secondary);
    }

    response
}

/// Lucide-style monochrome marks — stroke only, theme-colored.
fn paint_nav_icon(
    painter: &egui::Painter,
    tab: NavTab,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.25, color);
    let r = size * 0.5;
    match tab {
        NavTab::Projects => paint_icon_folder(painter, center, r, stroke, color),
        NavTab::Knowledge => paint_icon_book(painter, center, r, stroke),
        NavTab::Media => paint_icon_image(painter, center, r, stroke, color),
        NavTab::Conversations => paint_icon_chat(painter, center, r, stroke, color),
    }
}

fn paint_icon_folder(
    painter: &egui::Painter,
    center: egui::Pos2,
    r: f32,
    stroke: egui::Stroke,
    color: egui::Color32,
) {
    let body = egui::Rect::from_center_size(center + egui::vec2(0.0, 1.0), egui::vec2(r * 1.7, r * 1.2));
    let tab = egui::Rect::from_min_size(
        egui::pos2(body.left(), body.top() - r * 0.35),
        egui::vec2(r * 0.7, r * 0.4),
    );
    painter.rect_stroke(body, egui::CornerRadius::same(1), stroke, egui::StrokeKind::Outside);
    painter.rect_filled(tab, egui::CornerRadius::same(1), color);
}

fn paint_icon_book(painter: &egui::Painter, center: egui::Pos2, r: f32, stroke: egui::Stroke) {
    // Open book — two pages with a spine.
    let left = [
        center + egui::vec2(-r * 0.85, -r * 0.7),
        center + egui::vec2(-0.5, -r * 0.55),
        center + egui::vec2(-0.5, r * 0.75),
        center + egui::vec2(-r * 0.85, r * 0.6),
    ];
    let right = [
        center + egui::vec2(r * 0.85, -r * 0.7),
        center + egui::vec2(0.5, -r * 0.55),
        center + egui::vec2(0.5, r * 0.75),
        center + egui::vec2(r * 0.85, r * 0.6),
    ];
    painter.add(egui::Shape::closed_line(left.to_vec(), stroke));
    painter.add(egui::Shape::closed_line(right.to_vec(), stroke));
    painter.line_segment(
        [center + egui::vec2(0.0, -r * 0.55), center + egui::vec2(0.0, r * 0.75)],
        stroke,
    );
}

fn paint_icon_image(
    painter: &egui::Painter,
    center: egui::Pos2,
    r: f32,
    stroke: egui::Stroke,
    color: egui::Color32,
) {
    let frame = egui::Rect::from_center_size(center, egui::vec2(r * 1.7, r * 1.35));
    painter.rect_stroke(frame, egui::CornerRadius::same(2), stroke, egui::StrokeKind::Outside);
    // Sun / focus point.
    painter.circle_filled(frame.left_top() + egui::vec2(r * 0.45, r * 0.4), 1.4, color);
    // Mountain silhouette.
    painter.add(egui::Shape::closed_line(
        vec![
            egui::pos2(frame.left() + 2.0, frame.bottom() - 2.0),
            egui::pos2(frame.left() + r * 0.7, frame.center().y),
            egui::pos2(frame.center().x + 1.0, frame.bottom() - 2.0),
        ],
        stroke,
    ));
    painter.add(egui::Shape::closed_line(
        vec![
            egui::pos2(frame.center().x - 1.0, frame.bottom() - 2.0),
            egui::pos2(frame.right() - r * 0.55, frame.center().y - 1.0),
            egui::pos2(frame.right() - 2.0, frame.bottom() - 2.0),
        ],
        stroke,
    ));
}

fn paint_icon_chat(
    painter: &egui::Painter,
    center: egui::Pos2,
    r: f32,
    stroke: egui::Stroke,
    _color: egui::Color32,
) {
    let bubble = egui::Rect::from_center_size(center + egui::vec2(0.0, -0.5), egui::vec2(r * 1.7, r * 1.2));
    painter.rect_stroke(bubble, egui::CornerRadius::same(3), stroke, egui::StrokeKind::Outside);
    // Tail.
    painter.add(egui::Shape::closed_line(
        vec![
            egui::pos2(bubble.left() + r * 0.35, bubble.bottom()),
            egui::pos2(bubble.left() + r * 0.15, bubble.bottom() + r * 0.45),
            egui::pos2(bubble.left() + r * 0.7, bubble.bottom()),
        ],
        stroke,
    ));
}

fn paint_nav_chevron(painter: &egui::Painter, center: egui::Pos2, expanded: bool, color: egui::Color32) {
    let s = 3.5;
    let points = if expanded {
        [
            center + egui::vec2(-s, -s * 0.35),
            center + egui::vec2(s, -s * 0.35),
            center + egui::vec2(0.0, s * 0.55),
        ]
    } else {
        [
            center + egui::vec2(-s * 0.35, -s),
            center + egui::vec2(s * 0.55, 0.0),
            center + egui::vec2(-s * 0.35, s),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        egui::Stroke::NONE,
    ));
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
