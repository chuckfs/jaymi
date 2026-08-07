//! egui rendering for the Project Explorer — VS Code / Zed-style file tree.

use std::collections::BTreeSet;

use eframe::egui;

use jaymi_capabilities::{ExplorerNode, ExplorerPending, ExplorerState, ExplorerStatus};

use super::events::ExplorerEvent;
use super::icons::{paint_disclosure, paint_file, paint_folder};
use crate::theme::{radius, space, type_size, Theme};

/// Row height for a consistent IDE tree rhythm (3 × 8px).
const ROW_HEIGHT: f32 = 24.0;
/// Indent per directory depth (2 × 8px).
const INDENT_PER_LEVEL: f32 = 16.0;
/// Fixed column for the disclosure chevron.
const CHEVRON_COL: f32 = 16.0;
/// Fixed column for the file/folder icon.
const ICON_COL: f32 = 16.0;
/// Gap between icon column and file name.
const ICON_NAME_GAP: f32 = space::SM;
/// Trailing space reserved for the dirty marker.
const DIRTY_COL: f32 = 16.0;
/// Corner radius for row hover / selection fills — pill rows, like every
/// other selectable list in the shell (sidebar conversations, dock tabs).
const ROW_RADIUS: f32 = radius::PILL;

/// Render the Project Explorer into `ui`, appending interaction events.
///
/// `active_file` is the editor's active tab path (open-file cue).
/// `dirty_paths` are open editor buffers with unsaved changes.
pub fn render_explorer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &ExplorerState,
    active_file: Option<&str>,
    dirty_paths: &BTreeSet<String>,
    events: &mut Vec<ExplorerEvent>,
) {
    // Clip all explorer chrome to the panel so long names never spill out.
    let clip = ui.clip_rect();
    ui.set_clip_rect(clip);

    match &state.status {
        ExplorerStatus::Idle => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new("Loading project tree…").color(theme.text_secondary));
            });
        }
        ExplorerStatus::NoProject => {
            ui.label(egui::RichText::new("No open project").color(theme.text_secondary));
            if ui.button("Open Project").clicked() {
                events.push(ExplorerEvent::OpenProject);
            }
        }
        ExplorerStatus::Error(message) => {
            ui.colored_label(theme.error, message);
            ui.add_space(space::SM);
            if ui
                .button("Retry")
                .on_hover_text("Reload project tree")
                .clicked()
            {
                events.push(ExplorerEvent::Refresh);
            }
        }
        ExplorerStatus::Ready => {
            if let Some(root) = &state.project_root {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(root)
                            .size(type_size::META)
                            .color(theme.text_secondary),
                    )
                    .truncate()
                    .sense(egui::Sense::hover()),
                )
                .on_hover_text(root);
                ui.add_space(space::SM);
            }
            render_pending_banner(ui, theme, state, events);
            if state.nodes.is_empty() && matches!(state.pending, ExplorerPending::None) {
                ui.label(egui::RichText::new("This folder is empty.").color(theme.text_secondary));
                if let Some(root) = &state.project_root {
                    ui.horizontal(|ui| {
                        if ui.small_button("New File").clicked() {
                            events.push(ExplorerEvent::BeginNewFile {
                                parent: root.clone(),
                            });
                        }
                        if ui.small_button("New Folder").clicked() {
                            events.push(ExplorerEvent::BeginNewFolder {
                                parent: root.clone(),
                            });
                        }
                    });
                }
            } else {
                render_nodes(
                    ui,
                    theme,
                    &state.nodes,
                    state,
                    active_file,
                    dirty_paths,
                    0,
                    events,
                );
                if matches!(state.pending, ExplorerPending::None) {
                    let mut flat = Vec::new();
                    flatten_visible(&state.nodes, &state.expanded_paths, &mut flat);
                    handle_keyboard_navigation(ui, state, &flat, events);
                }
            }
        }
    }
}

/// Flatten the visible (respecting `expanded_paths`) tree into the same
/// depth-first order it is drawn in — used for ArrowUp/Down navigation.
fn flatten_visible<'a>(
    nodes: &'a [ExplorerNode],
    expanded: &BTreeSet<String>,
    out: &mut Vec<(&'a str, bool)>,
) {
    for node in nodes {
        out.push((node.path.as_str(), node.is_dir));
        if node.is_dir && expanded.contains(&node.path) {
            flatten_visible(&node.children, expanded, out);
        }
    }
}

/// ArrowUp/Down move selection among visible rows; ArrowRight/Left expand and
/// collapse directories; Enter opens a file or toggles a directory. Skipped
/// whenever another widget holds keyboard focus.
fn handle_keyboard_navigation(
    ui: &egui::Ui,
    state: &ExplorerState,
    flat: &[(&str, bool)],
    events: &mut Vec<ExplorerEvent>,
) {
    if flat.is_empty() || ui.memory(|memory| memory.focused().is_some()) {
        return;
    }
    let current_index = state
        .selected_path
        .as_deref()
        .and_then(|selected| flat.iter().position(|(path, _)| *path == selected));

    ui.input(|input| {
        if input.key_pressed(egui::Key::ArrowDown) {
            let next = current_index
                .map(|index| (index + 1).min(flat.len() - 1))
                .unwrap_or(0);
            let (path, is_dir) = flat[next];
            events.push(ExplorerEvent::Select {
                path: path.to_string(),
                is_dir,
            });
        } else if input.key_pressed(egui::Key::ArrowUp) {
            let next = current_index.map_or(0, |index| index.saturating_sub(1));
            let (path, is_dir) = flat[next];
            events.push(ExplorerEvent::Select {
                path: path.to_string(),
                is_dir,
            });
        } else if input.key_pressed(egui::Key::ArrowRight) {
            if let Some((path, true)) = current_index.map(|index| flat[index]) {
                if !state.expanded_paths.contains(path) {
                    events.push(ExplorerEvent::ToggleExpand(path.to_string()));
                }
            }
        } else if input.key_pressed(egui::Key::ArrowLeft) {
            if let Some((path, true)) = current_index.map(|index| flat[index]) {
                if state.expanded_paths.contains(path) {
                    events.push(ExplorerEvent::ToggleExpand(path.to_string()));
                }
            }
        } else if input.key_pressed(egui::Key::Enter) {
            if let Some((path, is_dir)) = current_index.map(|index| flat[index]) {
                if is_dir {
                    events.push(ExplorerEvent::ToggleExpand(path.to_string()));
                } else {
                    events.push(ExplorerEvent::Open(path.to_string()));
                }
            }
        }
    });
}

fn render_pending_banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &ExplorerState,
    events: &mut Vec<ExplorerEvent>,
) {
    let Some((label, draft)) = pending_label_and_draft(&state.pending) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(theme.text_primary));
        let mut name = draft.to_string();
        let response = ui.add(
            egui::TextEdit::singleline(&mut name)
                .desired_width(120.0)
                .hint_text("name")
                .text_color(theme.text_primary),
        );
        if response.changed() {
            events.push(ExplorerEvent::PendingNameChanged(name.clone()));
        }
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            events.push(ExplorerEvent::ConfirmPending);
        }
        let confirm = match &state.pending {
            ExplorerPending::Rename { .. } => "Rename",
            _ => "Create",
        };
        if ui.small_button(confirm).clicked() {
            events.push(ExplorerEvent::ConfirmPending);
        }
        if ui.small_button("Cancel").clicked() {
            events.push(ExplorerEvent::CancelPending);
        }
    });
    ui.add_space(space::SM);
}

fn pending_label_and_draft(pending: &ExplorerPending) -> Option<(&'static str, &str)> {
    match pending {
        ExplorerPending::None => None,
        ExplorerPending::NewFile { draft_name, .. } => Some(("New file", draft_name.as_str())),
        ExplorerPending::NewFolder { draft_name, .. } => Some(("New folder", draft_name.as_str())),
        ExplorerPending::Rename { draft_name, .. } => Some(("Rename", draft_name.as_str())),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_nodes(
    ui: &mut egui::Ui,
    theme: &Theme,
    nodes: &[ExplorerNode],
    state: &ExplorerState,
    active_file: Option<&str>,
    dirty_paths: &BTreeSet<String>,
    depth: usize,
    events: &mut Vec<ExplorerEvent>,
) {
    for node in nodes {
        render_node_row(
            ui,
            theme,
            node,
            state,
            active_file,
            dirty_paths,
            depth,
            events,
        );
        let expanded = node.is_dir && state.expanded_paths.contains(&node.path);
        if expanded {
            render_nodes(
                ui,
                theme,
                &node.children,
                state,
                active_file,
                dirty_paths,
                depth + 1,
                events,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_node_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    node: &ExplorerNode,
    state: &ExplorerState,
    active_file: Option<&str>,
    dirty_paths: &BTreeSet<String>,
    depth: usize,
    events: &mut Vec<ExplorerEvent>,
) {
    let is_active = active_file == Some(node.path.as_str());
    let is_selected = state.selected_path.as_deref() == Some(node.path.as_str());
    let expanded = state.expanded_paths.contains(&node.path);
    let is_dirty = !node.is_dir && dirty_paths.contains(&node.path);

    let desired_size = egui::vec2(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(
        desired_size,
        egui::Sense::click().union(egui::Sense::hover()),
    );

    if ui.is_rect_visible(rect) {
        // Layered highlights: selection > open file / hover.
        let bg_fill = if is_selected {
            theme.selection()
        } else if is_active || response.hovered() {
            theme.surface_alt
        } else {
            egui::Color32::TRANSPARENT
        };
        if bg_fill != egui::Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(ROW_RADIUS as u8), bg_fill);
        }

        // Open-in-editor cue: thin accent bar on the leading edge (no focus ring).
        if is_selected || is_active {
            let bar = egui::Rect::from_min_size(
                rect.left_top() + egui::vec2(1.0, space::XS),
                egui::vec2(2.0, rect.height() - space::SM),
            );
            ui.painter().rect_filled(
                bar,
                egui::CornerRadius::same(1),
                if is_selected {
                    theme.accent
                } else {
                    theme.text_secondary
                },
            );
        }

        let font_id = egui::FontId::proportional(type_size::BODY);
        let text_color = if is_selected {
            theme.text_primary
        } else if is_dirty {
            theme.warning
        } else {
            theme.text_primary
        };

        let indent = depth as f32 * INDENT_PER_LEVEL;
        let mut x = rect.left() + space::XS + indent;

        // Disclosure column (directories only; spacer for files keeps icons aligned).
        if node.is_dir {
            paint_disclosure(
                ui.painter(),
                egui::pos2(x + CHEVRON_COL * 0.5, rect.center().y),
                expanded,
                theme.text_secondary,
            );
        }
        x += CHEVRON_COL;

        // File / folder icon column — painted shapes (no Unicode tofu).
        let icon_center = egui::pos2(x + ICON_COL * 0.5, rect.center().y);
        if node.is_dir {
            paint_folder(ui.painter(), icon_center, expanded, theme);
        } else {
            paint_file(ui.painter(), icon_center, theme, node);
        }
        x += ICON_COL + ICON_NAME_GAP;

        // Name — truncate so it never escapes the row / panel.
        let dirty_reserve = if is_dirty { DIRTY_COL } else { 0.0 };
        let name_max = (rect.right() - x - dirty_reserve - space::XS).max(12.0);
        let display_name = truncate_to_width(ui, &node.name, &font_id, name_max);
        ui.painter().text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            display_name,
            font_id.clone(),
            text_color,
        );

        if is_dirty {
            ui.painter().circle_filled(
                egui::pos2(rect.right() - space::SM, rect.center().y),
                2.5,
                theme.warning,
            );
        }
    }

    response.clone().on_hover_text(&node.path);

    if response.clicked() {
        events.push(ExplorerEvent::Select {
            path: node.path.clone(),
            is_dir: node.is_dir,
        });
        if node.is_dir {
            events.push(ExplorerEvent::ToggleExpand(node.path.clone()));
        }
    }
    if response.double_clicked() {
        if node.is_dir {
            events.push(ExplorerEvent::Select {
                path: node.path.clone(),
                is_dir: true,
            });
            // Single-click already toggled; double-click keeps expand semantics.
        } else {
            events.push(ExplorerEvent::Open(node.path.clone()));
        }
    }

    response.context_menu(|ui| {
        render_context_menu(ui, theme, node, state, events);
    });
}

/// Ellipsize `name` so its painted width stays within `max_width`.
fn truncate_to_width(ui: &egui::Ui, name: &str, font_id: &egui::FontId, max_width: f32) -> String {
    let fits = |text: &str| {
        ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(text.to_owned(), font_id.clone(), egui::Color32::TRANSPARENT)
                .size()
                .x
        }) <= max_width
    };
    if fits(name) {
        return name.to_string();
    }
    let chars: Vec<char> = name.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&candidate) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let keep = lo.max(1).min(chars.len());
    chars[..keep].iter().collect::<String>() + "…"
}

fn render_context_menu(
    ui: &mut egui::Ui,
    _theme: &Theme,
    node: &ExplorerNode,
    state: &ExplorerState,
    events: &mut Vec<ExplorerEvent>,
) {
    let parent_for_create = if node.is_dir {
        node.path.clone()
    } else {
        std::path::Path::new(&node.path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .or_else(|| state.project_root.clone())
            .unwrap_or_else(|| node.path.clone())
    };

    if ui.button("New File").clicked() {
        events.push(ExplorerEvent::BeginNewFile {
            parent: parent_for_create.clone(),
        });
        ui.close_menu();
    }
    if ui.button("New Folder").clicked() {
        events.push(ExplorerEvent::BeginNewFolder {
            parent: parent_for_create,
        });
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Rename").clicked() {
        events.push(ExplorerEvent::BeginRename {
            path: node.path.clone(),
            name: node.name.clone(),
        });
        ui.close_menu();
    }
    if ui.button("Delete").clicked() {
        events.push(ExplorerEvent::Delete(node.path.clone()));
        ui.close_menu();
    }
    ui.separator();
    let reveal_label = if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Reveal in Explorer"
    } else {
        "Reveal in File Manager"
    };
    if ui.button(reveal_label).clicked() {
        events.push(ExplorerEvent::Reveal(node.path.clone()));
        ui.close_menu();
    }
}
