//! egui rendering for the Project Explorer.

use std::collections::BTreeSet;

use eframe::egui;

use jaymi_capabilities::{ExplorerNode, ExplorerPending, ExplorerState, ExplorerStatus};

use super::events::ExplorerEvent;
use super::icons::{file_icon, folder_icon};

/// Render the Project Explorer into `ui`, appending interaction events.
///
/// `active_file` is the editor's active tab path (highlight + auto-expand cue).
pub fn render_explorer(
    ui: &mut egui::Ui,
    state: &ExplorerState,
    active_file: Option<&str>,
    events: &mut Vec<ExplorerEvent>,
) {
    match &state.status {
        ExplorerStatus::Idle => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Loading project tree…");
            });
        }
        ExplorerStatus::NoProject => {
            ui.weak("No open project");
            if ui.button("Open Project…").clicked() {
                events.push(ExplorerEvent::OpenProject);
            }
        }
        ExplorerStatus::Error(message) => {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        ExplorerStatus::Ready => {
            if let Some(root) = &state.project_root {
                ui.weak(truncate_middle(root, 36));
                ui.add_space(4.0);
            }
            render_pending_banner(ui, state, events);
            if state.nodes.is_empty() && matches!(state.pending, ExplorerPending::None) {
                ui.weak("(empty project)");
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
                render_nodes(ui, &state.nodes, state, active_file, events);
                // Keyboard navigation only while there is no inline create/rename
                // draft in progress and no other widget (search box, terminal
                // input, rename field, …) holds keyboard focus.
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
/// whenever another widget (search box, rename draft, terminal input, …)
/// holds keyboard focus, so navigation never steals keystrokes from typing.
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
    state: &ExplorerState,
    events: &mut Vec<ExplorerEvent>,
) {
    let Some((label, draft)) = pending_label_and_draft(&state.pending) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(label);
        let mut name = draft.to_string();
        let response = ui.add(
            egui::TextEdit::singleline(&mut name)
                .desired_width(120.0)
                .hint_text("name"),
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
    ui.add_space(4.0);
}

fn pending_label_and_draft(pending: &ExplorerPending) -> Option<(&'static str, &str)> {
    match pending {
        ExplorerPending::None => None,
        ExplorerPending::NewFile { draft_name, .. } => Some(("New file", draft_name.as_str())),
        ExplorerPending::NewFolder { draft_name, .. } => Some(("New folder", draft_name.as_str())),
        ExplorerPending::Rename { draft_name, .. } => Some(("Rename", draft_name.as_str())),
    }
}

fn render_nodes(
    ui: &mut egui::Ui,
    nodes: &[ExplorerNode],
    state: &ExplorerState,
    active_file: Option<&str>,
    events: &mut Vec<ExplorerEvent>,
) {
    for node in nodes {
        render_node_row(ui, node, state, active_file, events);
        let expanded = node.is_dir && state.expanded_paths.contains(&node.path);
        if expanded {
            ui.indent(format!("explorer_{}", node.path), |ui| {
                render_nodes(ui, &node.children, state, active_file, events);
                if let ExplorerPending::NewFile { parent, .. }
                | ExplorerPending::NewFolder { parent, .. } = &state.pending
                {
                    if parent == &node.path {
                        // Pending banner is global; keep tree readable.
                    }
                }
            });
        }
    }
}

fn render_node_row(
    ui: &mut egui::Ui,
    node: &ExplorerNode,
    state: &ExplorerState,
    active_file: Option<&str>,
    events: &mut Vec<ExplorerEvent>,
) {
    let is_active = active_file == Some(node.path.as_str());
    let is_selected = state.selected_path.as_deref() == Some(node.path.as_str());
    let expanded = state.expanded_paths.contains(&node.path);
    let highlighted = is_selected || is_active;

    ui.horizontal(|ui| {
        let label = if node.is_dir {
            format!("{} {}", folder_icon(expanded), node.name)
        } else {
            format!("{} {}", file_icon(node), node.name)
        };

        let row_height = ui.spacing().interact_size.y;
        let desired_size = egui::vec2(ui.available_width(), row_height);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            // Hover feedback for unselected rows; selection keeps its own fill
            // regardless of hover so the "current" row always stays legible.
            let bg_fill = if highlighted {
                ui.visuals().selection.bg_fill
            } else if response.hovered() {
                ui.visuals().widgets.hovered.bg_fill
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 3.0, bg_fill);

            let text_color = if highlighted {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            };
            let font_id = egui::TextStyle::Button.resolve(ui.style());
            ui.painter().text(
                rect.left_center() + egui::vec2(4.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &label,
                font_id,
                text_color,
            );

            // Focus ring: a visible stroke around the selected/active row so
            // keyboard navigation (ArrowUp/Down) is easy to track visually.
            if highlighted {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    ui.visuals().selection.stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }

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
                events.push(ExplorerEvent::ToggleExpand(node.path.clone()));
            } else {
                events.push(ExplorerEvent::Open(node.path.clone()));
            }
        }

        response.context_menu(|ui| {
            render_context_menu(ui, node, state, events);
        });
    });
}

fn render_context_menu(
    ui: &mut egui::Ui,
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

    // TODO: drag-and-drop reordering / move within the tree.
    let _ = state;
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(1) / 2;
    let start: String = chars.iter().take(keep).collect();
    let end: String = chars.iter().rev().take(keep).cloned().rev().collect();
    format!("{start}…{end}")
}
