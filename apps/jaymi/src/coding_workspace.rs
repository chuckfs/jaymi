//! Coding Workspace shell — Project Explorer + Monaco Editor + Terminal + Git + Diagnostics.
//!
//! Conversation stays in the central panel. This module renders the right-side
//! Coding expansion. UI only renders [`CodingState`]; filesystem, terminal, and
//! git access go through Application → Planner → Tool → Provider.
//!
//! The editor body is a Monaco WebView overlay. Buffer text lives in
//! [`CodingState`] so content survives UI remounts / hot reloads.
//!
//! The Diagnostics panel is read-only operational status for development.

use eframe::egui;
use jaymi_capabilities::{
    CodingBottomTab, CodingState, ExplorerNode, ExplorerStatus, TerminalSessionState,
    WorkspaceExpansion, WorkspacePanel,
};

use crate::diagnostics::DiagnosticsSnapshot;
use crate::experience::ExperienceSession;
use crate::monaco_host::{language_for_path, MonacoDocument, MonacoViewport};
use jaymi_project_engine::Project;

/// Surface describing where Monaco should be positioned this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct MonacoEditorSurface {
    /// Screen-space rect for the WebView overlay.
    pub viewport: MonacoViewport,
    /// Document to display (from CodingState).
    pub document: MonacoDocument,
}

/// One read-only section in the Coding Diagnostics panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingDiagnosticsSection {
    /// Section title (e.g. "Active project").
    pub title: String,
    /// Display lines under the title.
    pub lines: Vec<String>,
}

/// Read-only Coding Workspace diagnostics for development.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodingDiagnosticsView {
    /// Ordered sections shown in the Diagnostics panel.
    pub sections: Vec<CodingDiagnosticsSection>,
}

impl CodingDiagnosticsView {
    /// Flatten sections into bullet lines (for text summaries / tests).
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.sections.is_empty() {
            lines.push("No workspace diagnostics.".to_string());
            return lines;
        }
        for section in &self.sections {
            lines.push(format!("{}:", section.title));
            if section.lines.is_empty() {
                lines.push("  —".to_string());
            } else {
                for line in &section.lines {
                    lines.push(format!("  {line}"));
                }
            }
        }
        lines
    }
}

/// Build the Coding Diagnostics view from live Application state.
pub fn build_coding_diagnostics_view(
    snapshot: &DiagnosticsSnapshot,
    experience: &ExperienceSession,
    coding: Option<&CodingState>,
    project: Option<&Project>,
    activity: Option<&LastPlannerActivity>,
) -> CodingDiagnosticsView {
    let mut sections = Vec::new();

    sections.push(CodingDiagnosticsSection {
        title: "Active project".into(),
        lines: match project {
            Some(project) => vec![
                format!("{} ({})", project.name, project.id.as_str()),
                format!(
                    "root={}",
                    project
                        .root_directory
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .or_else(|| coding.and_then(|state| state.project_root.clone()))
                        .unwrap_or_else(|| "—".into())
                ),
                format!("type={} · status={}", project.project_type.as_str(), project.status.as_str()),
            ],
            None => vec![
                "No active project".into(),
                coding
                    .and_then(|state| state.project_root.clone())
                    .map(|root| format!("coding root={root}"))
                    .unwrap_or_else(|| "coding root=—".into()),
            ],
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Workspace state".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(workspace) = experience.active_workspace() {
                lines.push(format!(
                    "kind={} · expanded={} · panels={}",
                    workspace.kind.id(),
                    experience.workspace_expanded(),
                    workspace.panels.len()
                ));
                lines.push(format!("reason={}", workspace.reason));
            } else {
                lines.push("expanded=false".into());
            }
            if let Some(state) = coding {
                lines.push(format!(
                    "explorer={} · tabs={} (dirty={}) · terminals={} · git={}",
                    match &state.explorer_status {
                        ExplorerStatus::Ready => "ready",
                        ExplorerStatus::Idle => "idle",
                        ExplorerStatus::NoProject => "no-project",
                        ExplorerStatus::Error(_) => "error",
                    },
                    state.open_tabs.len(),
                    state.open_tabs.iter().filter(|tab| tab.dirty).count(),
                    state.terminal_sessions.len(),
                    if state.git.is_some() {
                        "connected"
                    } else {
                        "—"
                    }
                ));
                if let Some(active) = &state.active_tab_path {
                    lines.push(format!("active tab={active}"));
                }
            } else {
                lines.push("coding state=—".into());
            }
            lines.push(format!("conversation turns={}", experience.turn_count()));
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Planner activity".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(planner) = snapshot.subsystem("Planner") {
                lines.push(format!("{} · {}", planner.status.label(), planner.detail));
            }
            match activity {
                Some(activity) => {
                    lines.push(format!(
                        "last: {}{}",
                        if activity.blocked { "blocked · " } else { "" },
                        truncate_summary(&activity.summary, 120)
                    ));
                    lines.push(format!(
                        "capability={} · tool={} · provider={}",
                        activity.capability_id.as_deref().unwrap_or("—"),
                        activity.tool_id.as_deref().unwrap_or("—"),
                        activity.provider_id.as_deref().unwrap_or("—")
                    ));
                }
                None => lines.push("last: no planner requests yet".into()),
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Tool execution".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(tools) = snapshot.subsystem("Tools") {
                lines.push(format!("{} · {}", tools.status.label(), tools.detail));
            } else {
                lines.push(format!(
                    "registered={} · {}",
                    snapshot.tool_count,
                    snapshot.tool_ids.join(", ")
                ));
            }
            if let Some(activity) = activity {
                lines.push(format!(
                    "last tool={} · blocked={}",
                    activity.tool_id.as_deref().unwrap_or("—"),
                    activity.blocked
                ));
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Provider status".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(providers) = snapshot.subsystem("Providers") {
                lines.push(format!("{} · {}", providers.status.label(), providers.detail));
            }
            lines.push(format!(
                "ids={}",
                if snapshot.provider_ids.is_empty() {
                    "—".into()
                } else {
                    snapshot.provider_ids.join(", ")
                }
            ));
            for name in ["OCR Provider", "Embedding Provider", "Embedding Queue"] {
                if let Some(row) = snapshot.subsystem(name) {
                    lines.push(format!(
                        "{}: {} · {}",
                        name,
                        row.status.label(),
                        truncate_summary(&row.detail, 100)
                    ));
                }
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Indexing status".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(index) = snapshot.subsystem("Index Status") {
                lines.push(format!("{} · {}", index.status.label(), index.detail));
            }
            if let Some(queries) = snapshot.subsystem("Discovery Queries") {
                lines.push(format!(
                    "queries: {} · {}",
                    queries.status.label(),
                    queries.detail
                ));
            }
            lines.push(format!(
                "config indexing_enabled={}",
                snapshot
                    .config_indexing_enabled
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".into())
            ));
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Memory context".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(memory) = snapshot.subsystem("Memory Status") {
                lines.push(format!("{} · {}", memory.status.label(), memory.detail));
            }
            if let Some(context) = snapshot.subsystem("Context Engine") {
                lines.push(format!(
                    "context: {} · {}",
                    context.status.label(),
                    context.detail
                ));
            }
            lines.push(format!(
                "conversation_id={}",
                experience.conversation_id().unwrap_or("—")
            ));
            if let Some(activity) = activity {
                lines.push(format!("last memory hits={}", activity.memory_hits));
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Permissions".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(permissions) = snapshot.subsystem("Permissions") {
                lines.push(format!(
                    "{} · {}",
                    permissions.status.label(),
                    permissions.detail
                ));
            }
            if let Some(policies) = snapshot.subsystem("Policies") {
                lines.push(format!("{} · {}", policies.status.label(), policies.detail));
            }
            lines.push(format!(
                "mode={}",
                snapshot.permission_mode.as_deref().unwrap_or("—")
            ));
            if !snapshot.active_policies.is_empty() {
                lines.push(format!("policies={}", snapshot.active_policies.join(", ")));
            }
            if let Some(activity) = activity {
                lines.push(format!(
                    "last decision={} · policy={}",
                    activity.permission_decision.as_deref().unwrap_or("—"),
                    activity.policy_summary.as_deref().unwrap_or("—")
                ));
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Current capability".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(workspace) = experience.active_workspace() {
                lines.push(format!(
                    "{} · expands_from={}",
                    workspace.capability.id(),
                    workspace.expands_from.as_str()
                ));
                lines.push(format!("workspace title={}", workspace.title()));
            } else {
                lines.push("none (conversation only)".into());
            }
            lines.push(format!(
                "catalog available={} / {} · unavailable={}",
                snapshot.available_capability_ids.len(),
                snapshot.capability_count,
                snapshot.unavailable_capability_ids.len()
            ));
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Timing metrics".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(activity) = activity {
                lines.push(format!("last planner handle={}ms", activity.duration_ms));
            } else {
                lines.push("last planner handle=—".into());
            }
            if let Some(index) = snapshot.subsystem("Index Status") {
                if let Some(duration) = extract_metric(&index.detail, "duration_ms=") {
                    lines.push(format!("last index scan={duration}ms"));
                }
            }
            if let Some(queries) = snapshot.subsystem("Discovery Queries") {
                if let Some(duration) = extract_metric(&queries.detail, "last_duration_ms=") {
                    lines.push(format!("last discovery query={duration}ms"));
                }
            }
            if let Some(context) = snapshot.subsystem("Context Engine") {
                lines.push(format!("context {}", context.detail));
            }
            lines
        },
    });

    if let Some(state) = coding {
        let mut problem_lines = Vec::new();
        if state.diagnostics.is_empty() {
            problem_lines.push("none".into());
        } else {
            problem_lines.push(format!("{} issue(s)", state.diagnostics.len()));
            for item in state.diagnostics.iter().take(8) {
                problem_lines.push(format!(
                    "[{}] {}{}",
                    item.severity,
                    item.message,
                    item.path
                        .as_ref()
                        .map(|path| format!(" · {path}"))
                        .unwrap_or_default()
                ));
            }
        }
        sections.push(CodingDiagnosticsSection {
            title: "Problems".into(),
            lines: problem_lines,
        });
    }

    CodingDiagnosticsView { sections }
}

fn truncate_summary(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let shortened: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{shortened}…")
}

fn extract_metric(detail: &str, key: &str) -> Option<String> {
    let start = detail.find(key)? + key.len();
    let rest = &detail[start..];
    let end = rest
        .find(|ch: char| ch == ' ' || ch == ',')
        .unwrap_or(rest.len());
    let value = rest[..end].trim();
    if value.is_empty() || value == "-" {
        None
    } else {
        Some(value.to_string())
    }
}

/// Last Planner turn cached for Coding Diagnostics (activity / timing).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LastPlannerActivity {
    /// Short human summary.
    pub summary: String,
    /// Capability id, when any.
    pub capability_id: Option<String>,
    /// Tool id, when any.
    pub tool_id: Option<String>,
    /// Provider id, when any.
    pub provider_id: Option<String>,
    /// Whether policy/permission blocked execution.
    pub blocked: bool,
    /// End-to-end Planner handle duration.
    pub duration_ms: u64,
    /// Permission decision label, when evaluated.
    pub permission_decision: Option<String>,
    /// Policy summary, when evaluated.
    pub policy_summary: Option<String>,
    /// Number of memory records attached to the response.
    pub memory_hits: usize,
}

/// Events emitted by interactive Coding shell panels.
#[derive(Debug, Clone, PartialEq)]
pub enum CodingShellEvent {
    /// Open a project via the native folder picker (UI owns dialog).
    OpenProject,
    /// Toggle expand/collapse for a directory path.
    ToggleExpand(String),
    /// Select a path (`is_dir` distinguishes folders from files).
    SelectPath { path: String, is_dir: bool },
    /// Activate an open editor tab.
    ActivateTab(String),
    /// Close an open editor tab.
    CloseTab(String),
    /// Update editable buffer contents for a tab.
    EditContent { path: String, content: String },
    /// Persist vertical scroll offset for a tab.
    Scroll { path: String, offset: f32 },
    /// Save the active editor tab through Planner → write_file.
    SaveActive,
    /// Save a specific open tab.
    SaveTab(String),
    /// Toggle Monaco minimap.
    SetMinimap(bool),
    /// Show / hide a bottom auxiliary panel (Terminal, Git, Problems).
    SetBottomTab(CodingBottomTab),
    /// Update the draft input for a terminal session.
    TerminalInput { session_id: String, input: String },
    /// Run the current terminal draft (or an explicit command).
    TerminalRun {
        session_id: String,
        command: String,
    },
    /// Navigate terminal history (`-1` older, `+1` newer).
    TerminalHistory { session_id: String, direction: i8 },
    /// Persist terminal output scroll offset.
    TerminalScroll { session_id: String, offset: f32 },
    /// Refresh Git status through Planner → git.
    GitRefresh,
    /// Stage one or more paths.
    GitStage { paths: Vec<String> },
    /// Unstage one or more paths.
    GitUnstage { paths: Vec<String> },
    /// Discard worktree/untracked changes for paths.
    GitDiscard { paths: Vec<String> },
    /// Update the draft commit message.
    GitCommitMessage(String),
    /// Commit staged changes with the draft message.
    GitCommit,
}

/// Pure text summary of the Coding shell for tests and headless checks.
pub fn coding_shell_summary(
    state: &CodingState,
    diagnostics: Option<&CodingDiagnosticsView>,
) -> String {
    let mut lines = Vec::new();
    lines.push("Coding Workspace Shell".to_string());
    for panel in [
        WorkspacePanel::ProjectExplorer,
        WorkspacePanel::Editor,
        WorkspacePanel::Terminal,
        WorkspacePanel::Git,
        WorkspacePanel::Diagnostics,
    ] {
        lines.push(format!("## {}", panel.label()));
        for line in coding_panel_lines(panel, Some(state), diagnostics) {
            lines.push(format!("- {line}"));
        }
    }
    lines.join("\n")
}

/// Lines shown inside one Coding panel, driven by optional [`CodingState`].
pub fn coding_panel_lines(
    panel: WorkspacePanel,
    state: Option<&CodingState>,
    diagnostics: Option<&CodingDiagnosticsView>,
) -> Vec<String> {
    let Some(state) = state else {
        return vec![placeholder_for(panel).to_string()];
    };
    match panel {
        WorkspacePanel::ProjectExplorer => explorer_lines(state),
        WorkspacePanel::Editor => editor_lines(state),
        WorkspacePanel::Terminal => {
            if state.terminal_sessions.is_empty() {
                vec!["No terminal sessions — open Coding to spawn a PTY.".to_string()]
            } else {
                state
                    .terminal_sessions
                    .iter()
                    .map(|session| {
                        format!(
                            "{} · cwd={} · last={} · history={}",
                            session.id,
                            session.cwd.as_deref().unwrap_or("-"),
                            session.last_command.as_deref().unwrap_or("-"),
                            session.history.len()
                        )
                    })
                    .collect()
            }
        }
        WorkspacePanel::Git => match &state.git {
            Some(git) => {
                let mut lines = vec![format!(
                    "branch={} · {}",
                    git.branch.as_deref().unwrap_or("-"),
                    git.summary
                )];
                if !git.staged.is_empty() {
                    lines.push(format!(
                        "staged: {}",
                        git.staged
                            .iter()
                            .map(|entry| entry.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !git.modified.is_empty() {
                    lines.push(format!(
                        "modified: {}",
                        git.modified
                            .iter()
                            .map(|entry| entry.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !git.untracked.is_empty() {
                    lines.push(format!(
                        "untracked: {}",
                        git.untracked
                            .iter()
                            .map(|entry| entry.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                lines
            }
            None => vec!["Git not connected — open Coding on a repository.".to_string()],
        },
        WorkspacePanel::Diagnostics => {
            if let Some(view) = diagnostics {
                view.summary_lines()
            } else {
                diagnostics_fallback_lines(state)
            }
        }
        _ => vec![panel.id().to_string()],
    }
}

fn diagnostics_fallback_lines(state: &CodingState) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(root) = &state.project_root {
        lines.push(format!("project root: {root}"));
    } else {
        lines.push("project root: —".to_string());
    }
    lines.push(format!(
        "workspace: explorer={} · tabs={} · terminals={} · git={}",
        match &state.explorer_status {
            ExplorerStatus::Ready => "ready",
            ExplorerStatus::Idle => "idle",
            ExplorerStatus::NoProject => "no-project",
            ExplorerStatus::Error(_) => "error",
        },
        state.open_tabs.len(),
        state.terminal_sessions.len(),
        if state.git.is_some() {
            "connected"
        } else {
            "—"
        }
    ));
    if state.diagnostics.is_empty() {
        lines.push("problems: none".to_string());
    } else {
        lines.push(format!("problems: {}", state.diagnostics.len()));
        for item in state.diagnostics.iter().take(8) {
            lines.push(format!(
                "  [{}] {}{}",
                item.severity,
                item.message,
                item.path
                    .as_ref()
                    .map(|path| format!(" · {path}"))
                    .unwrap_or_default()
            ));
        }
    }
    lines
}

fn explorer_lines(state: &CodingState) -> Vec<String> {
    let mut lines = Vec::new();
    match &state.explorer_status {
        ExplorerStatus::Idle => lines.push("Loading project tree…".to_string()),
        ExplorerStatus::NoProject => {
            lines.push("No open project — use Open Project… to browse files.".to_string());
        }
        ExplorerStatus::Error(message) => lines.push(format!("Explorer error: {message}")),
        ExplorerStatus::Ready => {
            if let Some(root) = &state.project_root {
                lines.push(format!("root: {root}"));
            }
            if state.explorer_nodes.is_empty() {
                lines.push("(empty project)".to_string());
            } else {
                collect_explorer_lines(&state.explorer_nodes, state, 0, &mut lines);
            }
        }
    }
    if let Some(selected) = &state.selected_path {
        lines.push(format!("selected: {selected}"));
    }
    if let Some(active) = &state.active_tab_path {
        lines.push(format!("active: {active}"));
    }
    lines
}

fn collect_explorer_lines(
    nodes: &[ExplorerNode],
    state: &CodingState,
    depth: usize,
    lines: &mut Vec<String>,
) {
    let indent = "  ".repeat(depth);
    for node in nodes {
        let icon = if node.is_dir { "📁" } else { "📄" };
        let highlight = if state.active_tab_path.as_deref() == Some(node.path.as_str())
            || state.selected_path.as_deref() == Some(node.path.as_str())
        {
            "▸ "
        } else {
            "  "
        };
        let expand = if node.is_dir {
            if state.expanded_paths.contains(&node.path) {
                "▾ "
            } else {
                "▸ "
            }
        } else {
            "  "
        };
        lines.push(format!("{indent}{highlight}{expand}{icon} {}", node.name));
        if node.is_dir && state.expanded_paths.contains(&node.path) {
            collect_explorer_lines(&node.children, state, depth + 1, lines);
        }
    }
}

fn editor_lines(state: &CodingState) -> Vec<String> {
    if state.open_tabs.is_empty() {
        return vec!["No open files — select a file in Project Explorer.".to_string()];
    }
    let mut lines = Vec::new();
    let tabs: Vec<String> = state
        .open_tabs
        .iter()
        .map(|tab| {
            let marker = if state.active_tab_path.as_deref() == Some(tab.path.as_str()) {
                "*"
            } else {
                " "
            };
            format!(
                "{marker}{}{}",
                tab.name,
                if tab.dirty { " · dirty" } else { "" }
            )
        })
        .collect();
    lines.push(format!("tabs: {}", tabs.join(" | ")));
    if let Some(active) = state
        .open_tabs
        .iter()
        .find(|tab| Some(tab.path.as_str()) == state.active_tab_path.as_deref())
    {
        let preview: String = active.content.chars().take(120).collect();
        lines.push(format!("buffer: {preview}"));
        lines.push(format!("scroll: {}", active.scroll_offset));
    }
    lines
}

fn placeholder_for(panel: WorkspacePanel) -> &'static str {
    match panel {
        WorkspacePanel::ProjectExplorer => "No open project — use Open Project… to browse files.",
        WorkspacePanel::Editor => "No open files — select a file in Project Explorer.",
        WorkspacePanel::Terminal => "No terminal sessions — open Coding to spawn a PTY.",
        WorkspacePanel::Git => "Git not connected — open Coding on a repository.",
        WorkspacePanel::Diagnostics => "No workspace diagnostics.",
        _ => "panel",
    }
}

/// Render the Coding Workspace shell into the right-side expansion panel.
///
/// Chat-forward layout (VS Code-inspired within the side expansion):
/// - Editor fills the code space
/// - Interactive Project Explorer sits to the **right** of the editor
/// - Terminal / Git / Problems are bottom tabs (toggle to show)
///
/// Monaco may overlay the editor body when ready; egui [`TextEdit`] always
/// shows the buffer so content is never blank.
pub fn render_coding_shell(
    ui: &mut egui::Ui,
    expansion: &WorkspaceExpansion,
    state: Option<&CodingState>,
    diagnostics: Option<&CodingDiagnosticsView>,
    events: &mut Vec<CodingShellEvent>,
    minimap: bool,
    monaco_out: &mut Option<MonacoEditorSurface>,
    open_error: Option<&str>,
) {
    *monaco_out = None;
    let _ = expansion;

    ui.horizontal(|ui| {
        ui.strong("Coding");
        if let Some(root) = state.and_then(|coding| coding.project_root.as_deref()) {
            ui.weak(truncate_path(root, 42));
        }
    });
    if let Some(error) = open_error {
        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
    }
    ui.add_space(4.0);
    ui.separator();

    let bottom_tab = state
        .map(|coding| coding.bottom_tab)
        .unwrap_or(CodingBottomTab::Hidden);
    let bottom_open = !matches!(bottom_tab, CodingBottomTab::Hidden);
    let tab_bar_h = 26.0_f32;
    let bottom_h = if bottom_open { 168.0_f32 } else { 0.0 };
    let chrome = 8.0_f32;
    let main_h = (ui.available_height() - tab_bar_h - bottom_h - chrome).max(180.0);
    let explorer_w = 168.0_f32;
    let gap = 6.0_f32;
    let editor_w = (ui.available_width() - explorer_w - gap).max(220.0);

    ui.horizontal(|ui| {
        ui.set_min_height(main_h);
        ui.set_max_height(main_h);

        // Editor column — fills the code space.
        ui.vertical(|ui| {
            ui.set_min_width(editor_w);
            ui.set_max_width(editor_w);
            ui.set_min_height(main_h);
            ui.set_max_height(main_h);
            if let Some(state) = state {
                render_editor(ui, state, events, minimap, monaco_out, main_h);
            } else {
                ui.weak(placeholder_for(WorkspacePanel::Editor));
            }
        });

        ui.add_space(gap);

        // Explorer — interactive tree on the right of the editor.
        ui.vertical(|ui| {
            ui.set_min_width(explorer_w);
            ui.set_max_width(explorer_w);
            ui.set_min_height(main_h);
            ui.set_max_height(main_h);
            ui.strong("Explorer");
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt("coding_explorer_scroll")
                .max_height((main_h - 22.0).max(80.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(explorer_w - 8.0);
                    if let Some(state) = state {
                        render_explorer(ui, state, events);
                    } else {
                        ui.weak(placeholder_for(WorkspacePanel::ProjectExplorer));
                    }
                });
        });
    });

    ui.add_space(4.0);

    // Bottom tab strip — click active tab again to collapse.
    ui.horizontal(|ui| {
        ui.set_min_height(tab_bar_h);
        for tab in [
            CodingBottomTab::Terminal,
            CodingBottomTab::Git,
            CodingBottomTab::Diagnostics,
        ] {
            let selected = bottom_tab == tab;
            if ui.selectable_label(selected, tab.label()).clicked() {
                let next = if selected {
                    CodingBottomTab::Hidden
                } else {
                    tab
                };
                events.push(CodingShellEvent::SetBottomTab(next));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if bottom_open && ui.small_button("▾").on_hover_text("Hide panel").clicked() {
                events.push(CodingShellEvent::SetBottomTab(CodingBottomTab::Hidden));
            }
        });
    });

    if bottom_open {
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("coding_bottom_scroll")
            .max_height(bottom_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match bottom_tab {
                    CodingBottomTab::Terminal => {
                        if let Some(state) = state {
                            render_terminal(ui, state, events);
                        } else {
                            ui.weak(placeholder_for(WorkspacePanel::Terminal));
                        }
                    }
                    CodingBottomTab::Git => {
                        if let Some(state) = state {
                            render_git(ui, state, events);
                        } else {
                            ui.weak(placeholder_for(WorkspacePanel::Git));
                        }
                    }
                    CodingBottomTab::Diagnostics => {
                        render_diagnostics_panel(ui, state, diagnostics);
                    }
                    CodingBottomTab::Hidden => {}
                }
            });
    }
}

fn truncate_path(path: &str, max_chars: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_chars {
        return path.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("…{}", chars[chars.len() - keep..].iter().collect::<String>())
}

/// Read-only Coding Diagnostics panel — operational status for development.
fn render_diagnostics_panel(
    ui: &mut egui::Ui,
    state: Option<&CodingState>,
    diagnostics: Option<&CodingDiagnosticsView>,
) {
    ui.weak("Read-only · refresh follows workspace activity");
    ui.add_space(4.0);

    let fallback = state.map(|state| CodingDiagnosticsView {
        sections: vec![CodingDiagnosticsSection {
            title: "Workspace state".into(),
            lines: diagnostics_fallback_lines(state),
        }],
    });
    let sections = diagnostics
        .or(fallback.as_ref())
        .map(|view| view.sections.as_slice())
        .unwrap_or(&[]);

    if sections.is_empty() {
        ui.weak(placeholder_for(WorkspacePanel::Diagnostics));
        return;
    }

    for section in sections {
        ui.strong(&section.title);
        if section.lines.is_empty() {
            ui.weak("—");
        } else {
            for line in &section.lines {
                ui.label(line);
            }
        }
        ui.add_space(6.0);
    }
}

fn render_explorer(ui: &mut egui::Ui, state: &CodingState, events: &mut Vec<CodingShellEvent>) {
    match &state.explorer_status {
        ExplorerStatus::Idle => {
            ui.weak("Loading project tree…");
        }
        ExplorerStatus::NoProject => {
            ui.weak("No open project — choose a folder to browse files.");
            if ui.button("Open Project…").clicked() {
                events.push(CodingShellEvent::OpenProject);
            }
        }
        ExplorerStatus::Error(message) => {
            ui.colored_label(egui::Color32::from_rgb(180, 60, 60), message);
        }
        ExplorerStatus::Ready => {
            if let Some(root) = &state.project_root {
                ui.weak(root);
            }
            if state.explorer_nodes.is_empty() {
                ui.weak("(empty project)");
            } else {
                render_explorer_nodes(ui, &state.explorer_nodes, state, events);
            }
        }
    }
}

fn render_explorer_nodes(
    ui: &mut egui::Ui,
    nodes: &[ExplorerNode],
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
) {
    for node in nodes {
        let is_active = state.active_tab_path.as_deref() == Some(node.path.as_str());
        let is_selected = state.selected_path.as_deref() == Some(node.path.as_str());
        let expanded = state.expanded_paths.contains(&node.path);

        ui.horizontal(|ui| {
            if node.is_dir {
                let toggle = if expanded { "▾" } else { "▸" };
                if ui.small_button(toggle).clicked() {
                    events.push(CodingShellEvent::ToggleExpand(node.path.clone()));
                }
                // Labels default to hover-only; selectable_label is required for clicks.
                let label = format!("📁 {}", node.name);
                if ui
                    .selectable_label(is_selected || is_active, label)
                    .clicked()
                {
                    events.push(CodingShellEvent::SelectPath {
                        path: node.path.clone(),
                        is_dir: true,
                    });
                    events.push(CodingShellEvent::ToggleExpand(node.path.clone()));
                }
            } else {
                ui.add_space(18.0);
                let label = format!("📄 {}", node.name);
                // Use a frame-less button so clicks register inside ScrollArea.
                if ui
                    .add(egui::Button::new(label).frame(is_active || is_selected))
                    .clicked()
                {
                    events.push(CodingShellEvent::SelectPath {
                        path: node.path.clone(),
                        is_dir: false,
                    });
                }
            }
        });

        if node.is_dir && expanded && !node.children.is_empty() {
            ui.indent(format!("explorer_{}", node.path), |ui| {
                render_explorer_nodes(ui, &node.children, state, events);
            });
        }
    }
}

fn render_editor(
    ui: &mut egui::Ui,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    minimap: bool,
    monaco_out: &mut Option<MonacoEditorSurface>,
    available_height: f32,
) {
    if state.open_tabs.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space((available_height * 0.28).clamp(24.0, 80.0));
            ui.weak("No open files");
            ui.weak("Select a file in Explorer →");
        });
        return;
    }

    if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::S)) {
        events.push(CodingShellEvent::SaveActive);
    }

    ui.horizontal_wrapped(|ui| {
        for tab in &state.open_tabs {
            let active = state.active_tab_path.as_deref() == Some(tab.path.as_str());
            let title = if tab.dirty {
                format!("{}*", tab.name)
            } else {
                tab.name.clone()
            };
            let tab_response = ui.selectable_label(active, &title);
            if tab_response.clicked() {
                events.push(CodingShellEvent::ActivateTab(tab.path.clone()));
            }
            if ui.small_button(format!("✕##close_{}", tab.path)).clicked() {
                events.push(CodingShellEvent::CloseTab(tab.path.clone()));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut minimap_enabled = minimap;
            if ui
                .checkbox(&mut minimap_enabled, "Minimap")
                .on_hover_text("Toggle Monaco minimap")
                .changed()
            {
                events.push(CodingShellEvent::SetMinimap(minimap_enabled));
            }
            let can_save = state
                .open_tabs
                .iter()
                .any(|tab| Some(tab.path.as_str()) == state.active_tab_path.as_deref() && tab.dirty);
            if ui
                .add_enabled(can_save, egui::Button::new("Save"))
                .on_hover_text("Save active file (⌘S)")
                .clicked()
            {
                events.push(CodingShellEvent::SaveActive);
            }
        });
    });
    ui.separator();

    let Some(active) = state
        .open_tabs
        .iter()
        .find(|tab| Some(tab.path.as_str()) == state.active_tab_path.as_deref())
    else {
        ui.weak("No active tab.");
        return;
    };

    let path = active.path.clone();
    let mut content = active.content.clone();
    let scroll_offset = active.scroll_offset;
    let language = language_for_path(&path).to_string();

    ui.weak(format!("{} · {}", active.name, language));
    ui.add_space(2.0);

    // Fill remaining code space; Monaco overlays this rect when ready.
    let chrome = 52.0_f32;
    let editor_height = (available_height - chrome).max(120.0);
    let scroll = egui::ScrollArea::vertical()
        .id_salt(("editor_scroll", &path))
        .max_height(editor_height)
        .auto_shrink([false, false])
        .vertical_scroll_offset(scroll_offset)
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut content)
                    .id_salt(("editor_body", &path))
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .code_editor()
                    .frame(false),
            );
            if response.changed() {
                events.push(CodingShellEvent::EditContent {
                    path: path.clone(),
                    content: content.clone(),
                });
            }
            response.rect
        });

    let new_offset = scroll.state.offset.y;
    if (new_offset - scroll_offset).abs() > f32::EPSILON {
        events.push(CodingShellEvent::Scroll {
            path: path.clone(),
            offset: new_offset,
        });
    }

    let rect = scroll.inner;
    *monaco_out = Some(MonacoEditorSurface {
        viewport: MonacoViewport { rect },
        document: MonacoDocument {
            path,
            content,
            language,
            scroll_top: scroll_offset,
            minimap,
        },
    });
}

fn render_terminal(ui: &mut egui::Ui, state: &CodingState, events: &mut Vec<CodingShellEvent>) {
    if state.terminal_sessions.is_empty() {
        ui.weak("No terminal sessions — open Coding to spawn a PTY.");
        return;
    }

    for session in &state.terminal_sessions {
        render_terminal_session(ui, session, events);
        ui.add_space(6.0);
    }
}

fn render_terminal_session(
    ui: &mut egui::Ui,
    session: &TerminalSessionState,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.horizontal(|ui| {
        ui.strong(&session.id);
        ui.weak(format!(
            "cwd={}",
            session.cwd.as_deref().unwrap_or("-")
        ));
    });

    let scroll = egui::ScrollArea::vertical()
        .id_salt(("terminal_scroll", &session.id))
        .vertical_scroll_offset(session.scroll_offset)
        .auto_shrink([false, false])
        .min_scrolled_height(160.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&session.output).monospace())
                    .wrap()
                    .selectable(true),
            );
        });
    let new_offset = scroll.state.offset.y;
    if (new_offset - session.scroll_offset).abs() > f32::EPSILON {
        events.push(CodingShellEvent::TerminalScroll {
            session_id: session.id.clone(),
            offset: new_offset,
        });
    }

    ui.horizontal(|ui| {
        ui.label("$");
        let mut draft = session.input.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut draft)
                .id_salt(("terminal_input", &session.id))
                .desired_width(f32::INFINITY)
                .hint_text("cargo test · git status · ls · pwd"),
        );
        if response.changed() {
            events.push(CodingShellEvent::TerminalInput {
                session_id: session.id.clone(),
                input: draft.clone(),
            });
        }
        if response.has_focus() {
            if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                events.push(CodingShellEvent::TerminalHistory {
                    session_id: session.id.clone(),
                    direction: -1,
                });
            }
            if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                events.push(CodingShellEvent::TerminalHistory {
                    session_id: session.id.clone(),
                    direction: 1,
                });
            }
        }
        let submit = response.lost_focus()
            && ui.input(|input| input.key_pressed(egui::Key::Enter))
            || ui.button("Run").clicked();
        if submit {
            let command = if draft.trim().is_empty() {
                session.input.clone()
            } else {
                draft
            };
            if !command.trim().is_empty() {
                events.push(CodingShellEvent::TerminalRun {
                    session_id: session.id.clone(),
                    command,
                });
            }
        }
    });
}

fn render_git(ui: &mut egui::Ui, state: &CodingState, events: &mut Vec<CodingShellEvent>) {
    let Some(git) = &state.git else {
        ui.weak("Git not connected — open Coding on a repository.");
        return;
    };

    ui.horizontal(|ui| {
        ui.strong(format!(
            "branch {}",
            git.branch.as_deref().unwrap_or("(unknown)")
        ));
        ui.weak(&git.summary);
        if ui.small_button("Refresh").clicked() {
            events.push(CodingShellEvent::GitRefresh);
        }
    });
    if let Some(error) = &git.last_error {
        ui.colored_label(egui::Color32::from_rgb(180, 60, 60), error);
    }

    render_git_section(ui, "Staged", &git.staged, true, false, events);
    render_git_section(ui, "Modified", &git.modified, false, true, events);
    render_git_section(ui, "Untracked", &git.untracked, false, true, events);

    ui.separator();
    ui.label("Commit message");
    let mut message = git.commit_message.clone();
    let response = ui.add(
        egui::TextEdit::multiline(&mut message)
            .desired_width(f32::INFINITY)
            .desired_rows(2)
            .hint_text("Commit message"),
    );
    if response.changed() {
        events.push(CodingShellEvent::GitCommitMessage(message.clone()));
    }
    let can_commit = !git.staged.is_empty() && !git.commit_message.trim().is_empty();
    if ui
        .add_enabled(can_commit, egui::Button::new("Commit"))
        .clicked()
    {
        events.push(CodingShellEvent::GitCommit);
    }
}

fn render_git_section(
    ui: &mut egui::Ui,
    title: &str,
    entries: &[jaymi_capabilities::GitFileEntry],
    unstage: bool,
    stage_or_discard: bool,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.add_space(4.0);
    ui.strong(format!("{title} ({})", entries.len()));
    if entries.is_empty() {
        ui.weak("(none)");
        return;
    }
    for entry in entries {
        ui.horizontal(|ui| {
            ui.monospace(format!("{:>2} {}", entry.status, entry.path));
            if unstage && ui.small_button(format!("Unstage##unstage_{}", entry.path)).clicked()
            {
                events.push(CodingShellEvent::GitUnstage {
                    paths: vec![entry.path.clone()],
                });
            }
            if stage_or_discard {
                if ui.small_button(format!("Stage##stage_{}", entry.path)).clicked() {
                    events.push(CodingShellEvent::GitStage {
                        paths: vec![entry.path.clone()],
                    });
                }
                if ui
                    .small_button(format!("Discard##discard_{}", entry.path))
                    .clicked()
                {
                    events.push(CodingShellEvent::GitDiscard {
                        paths: vec![entry.path.clone()],
                    });
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::{
        DiagnosticState, EditorTab, ExplorerNode, ExplorerStatus, GitStatusState,
        TerminalSessionState,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn summary_includes_explorer_and_editor_tabs() {
        let state = CodingState {
            project_root: Some("/tmp/app".into()),
            explorer_nodes: vec![
                ExplorerNode {
                    name: "src".into(),
                    path: "/tmp/app/src".into(),
                    is_dir: true,
                    children: vec![ExplorerNode {
                        name: "main.rs".into(),
                        path: "/tmp/app/src/main.rs".into(),
                        is_dir: false,
                        children: Vec::new(),
                    }],
                },
            ],
            expanded_paths: BTreeSet::from(["/tmp/app/src".into()]),
            selected_path: Some("/tmp/app/src/main.rs".into()),
            explorer_status: ExplorerStatus::Ready,
            open_tabs: vec![EditorTab {
                path: "/tmp/app/src/main.rs".into(),
                name: "main.rs".into(),
                content: "fn main() {}".into(),
                dirty: true,
                scroll_offset: 12.0,
            }],
            active_tab_path: Some("/tmp/app/src/main.rs".into()),
            scroll_positions: BTreeMap::from([("/tmp/app/src/main.rs".into(), 12.0)]),
            terminal_sessions: vec![TerminalSessionState {
                id: "term-1".into(),
                cwd: Some("/tmp/app".into()),
                last_command: Some("cargo check".into()),
                output: String::new(),
                history: vec!["cargo check".into()],
                input: String::new(),
                history_index: None,
                scroll_offset: 0.0,
            }],
            git: Some(GitStatusState {
                branch: Some("main".into()),
                summary: "clean".into(),
                ..GitStatusState::default()
            }),
            diagnostics: vec![DiagnosticState::simple(
                "unused import",
                Some("/tmp/app/src/main.rs".into()),
                "warning",
            )],
            bottom_tab: CodingBottomTab::Terminal,
        };
        let summary = coding_shell_summary(&state, None);
        assert!(summary.contains("Project Explorer"));
        assert!(summary.contains("Editor"));
        assert!(summary.contains("Terminal"));
        assert!(summary.contains("Git"));
        assert!(summary.contains("Diagnostics"));
        assert!(summary.contains("main.rs"));
        assert!(summary.contains("cargo check"));
        assert!(summary.contains("branch=main"));
        assert!(summary.contains("unused import"));
        assert!(summary.contains("tabs:"));
        assert!(!summary.contains("stub explorer"));
    }
}
