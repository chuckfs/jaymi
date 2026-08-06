//! Coding Workspace breadcrumb bar — pure presentation over [`CodingState`].
//!
//! Segments are derived from the active editor path + explorer project root.
//! No duplicate path state is stored; the bar re-reads CodingState each frame.

use std::path::{Component, Path};

use eframe::egui;
use jaymi_capabilities::CodingState;

use crate::theme::{space, stroke, type_size, Theme};

/// Height of the breadcrumb strip under the Coding toolbar.
pub const BREADCRUMB_BAR_HEIGHT: f32 = 24.0;

/// Role of one crumb in the trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreadcrumbKind {
    /// Workspace chrome root (`Coding`) — focuses the editor.
    Coding,
    /// Project root folder — selects/reveals the project in Explorer.
    Project,
    /// Intermediate directory under the project.
    Folder,
    /// The currently open file — focuses the editor.
    File,
    /// Truncation placeholder (`…`) — not interactive.
    Ellipsis,
}

/// One clickable (or ellipsis) segment in the breadcrumb trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbSegment {
    /// Display label (folder/file name, `Coding`, or `…`).
    pub label: String,
    /// Absolute filesystem path when the crumb maps to Explorer / disk.
    pub path: Option<String>,
    /// Segment role.
    pub kind: BreadcrumbKind,
}

/// Action produced when the user activates a breadcrumb segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreadcrumbAction {
    /// Focus the editor (Coding root or current file).
    FocusEditor,
    /// Select + reveal a path in Project Explorer.
    RevealInExplorer {
        /// Absolute path to select.
        path: String,
        /// Whether the path is a directory (expand it after revealing).
        is_dir: bool,
    },
}

/// Build the full breadcrumb trail from live CodingState.
///
/// Shape: `Coding > Project > folders… > file` when an active tab exists under
/// the project root. With no open file, returns `Coding > Project` (or just
/// `Coding` when no project root is bound).
pub fn breadcrumbs_from_coding_state(state: &CodingState) -> Vec<BreadcrumbSegment> {
    let mut segments = vec![BreadcrumbSegment {
        label: "Coding".to_string(),
        path: None,
        kind: BreadcrumbKind::Coding,
    }];

    let Some(root) = state.explorer.project_root.as_deref() else {
        if let Some(active) = state.active_tab_path() {
            let name = file_name(active).unwrap_or_else(|| active.to_string());
            segments.push(BreadcrumbSegment {
                label: name,
                path: Some(active.to_string()),
                kind: BreadcrumbKind::File,
            });
        }
        return segments;
    };

    let project_label = file_name(root).unwrap_or_else(|| root.to_string());
    segments.push(BreadcrumbSegment {
        label: project_label,
        path: Some(root.to_string()),
        kind: BreadcrumbKind::Project,
    });

    let Some(active) = state.active_tab_path() else {
        return segments;
    };

    let Ok(relative) = Path::new(active).strip_prefix(Path::new(root)) else {
        // File outside the project root — show the file name only.
        let name = file_name(active).unwrap_or_else(|| active.to_string());
        segments.push(BreadcrumbSegment {
            label: name,
            path: Some(active.to_string()),
            kind: BreadcrumbKind::File,
        });
        return segments;
    };

    let parts: Vec<&str> = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();

    if parts.is_empty() {
        return segments;
    }

    let mut cumulative = Path::new(root).to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        cumulative.push(part);
        let is_last = index + 1 == parts.len();
        let path = cumulative.to_string_lossy().to_string();
        segments.push(BreadcrumbSegment {
            label: (*part).to_string(),
            path: Some(path),
            kind: if is_last {
                BreadcrumbKind::File
            } else {
                BreadcrumbKind::Folder
            },
        });
    }

    segments
}

/// Truncate a long trail from the left while keeping `Coding` and the current
/// file visible. Inserts a single `…` ellipsis where middle segments were dropped.
///
/// Prefer preserving the project crumb when budget allows:
/// `Coding › Project › … › file`.
///
/// `max_chars` budgets the joined display length using ` › ` separators
/// (approximately what the bar will paint).
pub fn truncate_breadcrumbs(
    segments: &[BreadcrumbSegment],
    max_chars: usize,
) -> Vec<BreadcrumbSegment> {
    if segments.is_empty() || display_len(segments) <= max_chars {
        return segments.to_vec();
    }

    let first = segments[0].clone();
    let last = segments[segments.len() - 1].clone();
    if segments.len() <= 2 {
        return segments.to_vec();
    }

    let ellipsis = BreadcrumbSegment {
        label: "…".to_string(),
        path: None,
        kind: BreadcrumbKind::Ellipsis,
    };

    // Prefer Coding › Project › … › trailing when the second crumb is the project.
    if segments[1].kind == BreadcrumbKind::Project {
        for keep_from in (2..segments.len()).rev() {
            let mut candidate = vec![first.clone(), segments[1].clone(), ellipsis.clone()];
            candidate.extend(segments[keep_from..].iter().cloned());
            if display_len(&candidate) <= max_chars {
                return candidate;
            }
        }
        let minimal = vec![
            first.clone(),
            segments[1].clone(),
            ellipsis.clone(),
            last.clone(),
        ];
        if display_len(&minimal) <= max_chars {
            return minimal;
        }
    }

    // Fall back: Coding › … › trailing (grow trailing from the file leftward).
    for keep_from in (1..segments.len()).rev() {
        let mut candidate = vec![first.clone(), ellipsis.clone()];
        candidate.extend(segments[keep_from..].iter().cloned());
        if display_len(&candidate) <= max_chars {
            return candidate;
        }
    }

    vec![first, ellipsis, last]
}

/// Map a segment click to a navigation action (ellipsis yields `None`).
pub fn breadcrumb_action(segment: &BreadcrumbSegment) -> Option<BreadcrumbAction> {
    match segment.kind {
        BreadcrumbKind::Ellipsis => None,
        BreadcrumbKind::Coding | BreadcrumbKind::File => Some(BreadcrumbAction::FocusEditor),
        BreadcrumbKind::Project | BreadcrumbKind::Folder => {
            let path = segment.path.clone()?;
            Some(BreadcrumbAction::RevealInExplorer {
                path,
                is_dir: true,
            })
        }
    }
}

/// Apply a reveal action to CodingState (select + expand ancestors).
pub fn apply_breadcrumb_reveal(state: &mut CodingState, path: &str, is_dir: bool) {
    state.explorer.selected_path = Some(path.to_string());
    state.explorer.expand_ancestors_of(path);
    if is_dir {
        state.explorer.expanded_paths.insert(path.to_string());
    }
}

/// Paint the breadcrumb bar and return any click actions for the shell to apply.
pub fn render_coding_breadcrumb(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &CodingState,
) -> Vec<BreadcrumbAction> {
    let mut actions = Vec::new();
    let full = breadcrumbs_from_coding_state(state);
    if full.is_empty() {
        return actions;
    }

    let avail = ui.available_width().max(0.0);
    // Approximate proportional META glyph width for the char budget.
    let max_chars = ((avail / 6.5).floor() as usize).clamp(16, 240);
    let crumbs = truncate_breadcrumbs(&full, max_chars);

    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(stroke::HAIRLINE, theme.border),
    );

    egui::Frame::new()
        .fill(theme.surface_alt)
        .inner_margin(egui::Margin::symmetric(space::SM as i8, 0))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), BREADCRUMB_BAR_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_height(BREADCRUMB_BAR_HEIGHT);
                    ui.set_max_height(BREADCRUMB_BAR_HEIGHT);

                    for (index, segment) in crumbs.iter().enumerate() {
                        if index > 0 {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("›")
                                        .size(type_size::META)
                                        .color(theme.text_secondary),
                                )
                                .sense(egui::Sense::hover()),
                            );
                            ui.add_space(2.0);
                        }

                        let is_current = segment.kind == BreadcrumbKind::File;
                        let color = if is_current {
                            theme.text_primary
                        } else {
                            theme.text_secondary
                        };
                        let text = egui::RichText::new(segment.label.clone())
                            .size(type_size::META)
                            .color(color);
                        let interactive = breadcrumb_action(segment).is_some();
                        if interactive {
                            let mut response = ui.add(
                                egui::Label::new(text)
                                    .sense(egui::Sense::click())
                                    .selectable(false),
                            );
                            response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if let Some(path) = segment.path.as_deref() {
                                response = response.on_hover_text(path);
                            }
                            if response.hovered() {
                                ui.painter().hline(
                                    response.rect.x_range(),
                                    response.rect.bottom() - 1.0,
                                    egui::Stroke::new(1.0, theme.text_secondary),
                                );
                            }
                            if response.clicked() {
                                if let Some(action) = breadcrumb_action(segment) {
                                    actions.push(action);
                                }
                            }
                        } else {
                            ui.add(
                                egui::Label::new(text)
                                    .sense(egui::Sense::hover())
                                    .selectable(false),
                            );
                        }
                        ui.add_space(2.0);
                    }
                },
            );
        });

    actions
}

fn display_len(segments: &[BreadcrumbSegment]) -> usize {
    if segments.is_empty() {
        return 0;
    }
    let labels: usize = segments
        .iter()
        .map(|segment| segment.label.chars().count())
        .sum();
    let separators = segments.len().saturating_sub(1) * 3; // " › "
    labels + separators
}

fn file_name(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::CodingState;

    fn state_with(root: &str, active: Option<&str>) -> CodingState {
        let mut state = CodingState::default();
        state.explorer.project_root = Some(root.to_string());
        if let Some(path) = active {
            state.open_permanent(path, String::new());
        }
        state
    }

    #[test]
    fn breadcrumb_generation() {
        let state = state_with(
            "/Users/charlie/jaymi",
            Some("/Users/charlie/jaymi/crates/planner/src/lib.rs"),
        );
        let crumbs = breadcrumbs_from_coding_state(&state);
        let labels: Vec<&str> = crumbs.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Coding", "jaymi", "crates", "planner", "src", "lib.rs"]
        );
        assert_eq!(crumbs[0].kind, BreadcrumbKind::Coding);
        assert_eq!(crumbs[1].kind, BreadcrumbKind::Project);
        assert_eq!(crumbs[2].kind, BreadcrumbKind::Folder);
        assert_eq!(crumbs.last().unwrap().kind, BreadcrumbKind::File);
        assert_eq!(
            crumbs.last().unwrap().path.as_deref(),
            Some("/Users/charlie/jaymi/crates/planner/src/lib.rs")
        );
        assert_eq!(crumbs[1].path.as_deref(), Some("/Users/charlie/jaymi"));
    }

    #[test]
    fn breadcrumb_truncation() {
        let state = state_with(
            "/Users/charlie/jaymi",
            Some("/Users/charlie/jaymi/crates/planner/src/lib.rs"),
        );
        let full = breadcrumbs_from_coding_state(&state);
        let short = truncate_breadcrumbs(&full, 28);
        assert_eq!(short.first().unwrap().kind, BreadcrumbKind::Coding);
        assert_eq!(short.last().unwrap().label, "lib.rs");
        assert!(
            short.iter().any(|s| s.kind == BreadcrumbKind::Ellipsis),
            "expected ellipsis in {short:?}"
        );
        assert!(display_len(&short) <= 28);
        // Prefer keeping the project name when it fits.
        assert_eq!(short[1].kind, BreadcrumbKind::Project);

        let roomy = truncate_breadcrumbs(&full, 200);
        assert_eq!(roomy, full);
    }

    #[test]
    fn breadcrumb_navigation() {
        let state = state_with(
            "/Users/charlie/jaymi",
            Some("/Users/charlie/jaymi/crates/planner/src/lib.rs"),
        );
        let crumbs = breadcrumbs_from_coding_state(&state);

        assert_eq!(
            breadcrumb_action(&crumbs[0]),
            Some(BreadcrumbAction::FocusEditor)
        );
        assert_eq!(
            breadcrumb_action(crumbs.last().unwrap()),
            Some(BreadcrumbAction::FocusEditor)
        );

        let folder = crumbs
            .iter()
            .find(|c| c.label == "planner")
            .expect("planner folder");
        assert_eq!(
            breadcrumb_action(folder),
            Some(BreadcrumbAction::RevealInExplorer {
                path: "/Users/charlie/jaymi/crates/planner".into(),
                is_dir: true,
            })
        );

        let project = &crumbs[1];
        assert_eq!(
            breadcrumb_action(project),
            Some(BreadcrumbAction::RevealInExplorer {
                path: "/Users/charlie/jaymi".into(),
                is_dir: true,
            })
        );

        let mut navigated = state;
        if let Some(BreadcrumbAction::RevealInExplorer { path, is_dir }) =
            breadcrumb_action(folder)
        {
            apply_breadcrumb_reveal(&mut navigated, &path, is_dir);
        }
        assert_eq!(
            navigated.explorer.selected_path.as_deref(),
            Some("/Users/charlie/jaymi/crates/planner")
        );
        assert!(navigated
            .explorer
            .expanded_paths
            .contains("/Users/charlie/jaymi/crates"));
        assert!(navigated
            .explorer
            .expanded_paths
            .contains("/Users/charlie/jaymi/crates/planner"));

        let ellipsis = BreadcrumbSegment {
            label: "…".into(),
            path: None,
            kind: BreadcrumbKind::Ellipsis,
        };
        assert_eq!(breadcrumb_action(&ellipsis), None);
    }
}
