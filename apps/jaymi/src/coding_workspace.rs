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
    CodingBottomTab, CodingState, EditorLayoutNode, EditorPaneId, EditorSession, ExplorerNode,
    ExplorerStatus, ProblemIssue, ProblemSeverity, SplitDirection, TerminalSessionState,
    WorkspaceExpansion, WorkspacePanel, DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_EXPLORER_WIDTH,
    MAX_BOTTOM_PANEL_HEIGHT, MAX_EXPLORER_WIDTH, MIN_BOTTOM_PANEL_HEIGHT, MIN_EXPLORER_WIDTH,
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
                        .or_else(|| coding.and_then(|state| state.explorer.project_root.clone()))
                        .unwrap_or_else(|| "—".into())
                ),
                format!("type={} · status={}", project.project_type.as_str(), project.status.as_str()),
            ],
            None => vec![
                "No active project".into(),
                coding
                    .and_then(|state| state.explorer.project_root.clone())
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
                    match &state.explorer.status {
                        ExplorerStatus::Ready => "ready",
                        ExplorerStatus::Idle => "idle",
                        ExplorerStatus::NoProject => "no-project",
                        ExplorerStatus::Error(_) => "error",
                    },
                    state.editors.len(),
                    state
                        .editors
                        .buffers
                        .values()
                        .filter(|buffer| buffer.dirty)
                        .count(),
                    state.terminal_sessions.len(),
                    if state.git.is_some() {
                        "connected"
                    } else {
                        "—"
                    }
                ));
                if let Some(active) = state.active_tab_path() {
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
    /// Activate an already-open editor tab in a specific pane.
    ActivateTab { pane: String, path: String },
    /// Close an open editor tab in a specific pane.
    CloseTab { pane: String, path: String },
    /// Update editable buffer contents for a tab (edited from a specific pane).
    EditContent {
        pane: String,
        path: String,
        content: String,
    },
    /// Persist vertical scroll offset for a tab in a specific pane.
    Scroll {
        pane: String,
        path: String,
        offset: f32,
    },
    /// Persist cursor position for a tab in a specific pane.
    SetCursor {
        pane: String,
        path: String,
        line: u32,
        column: u32,
    },
    /// Persist folded regions for a tab in a specific pane.
    SetFolds {
        pane: String,
        path: String,
        regions: Vec<(u32, u32)>,
    },
    /// Save the active editor tab through Planner → write_file.
    SaveActive,
    /// Save a specific open tab.
    SaveTab(String),
    /// Toggle Monaco minimap (workspace-owned setting).
    SetMinimap(bool),
    /// Toggle word wrap (workspace-owned setting).
    SetWordWrap(bool),
    /// Set editor font size (workspace-owned setting).
    SetFontSize(u32),
    /// Split the focused pane side-by-side (VS Code "Split Right").
    SplitVertical,
    /// Split the focused pane stacked (VS Code "Split Down").
    SplitHorizontal,
    /// Close an entire editor pane (no-op when it is the only pane).
    ClosePane(String),
    /// Give keyboard / Monaco focus to a pane.
    FocusPane(String),
    /// Move a tab from one pane to another (drag and drop between splits).
    MoveTab {
        from_pane: String,
        path: String,
        to_pane: String,
        index: Option<usize>,
    },
    /// Resize a split node's relative child sizes (drag divider).
    ResizeSplit {
        node_path: Vec<usize>,
        sizes: Vec<f32>,
    },
    /// Resize the Project Explorer column (drag divider between editor and explorer).
    /// `commit` is true once on drag release, telling the caller to persist to disk.
    SetExplorerWidth { width: f32, commit: bool },
    /// Resize the bottom auxiliary panel height (drag divider above the panel).
    /// `commit` is true once on drag release, telling the caller to persist to disk.
    SetBottomPanelHeight { height: f32, commit: bool },
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
    /// Spawn a new terminal tab (cwd = project root).
    TerminalCreate { title: Option<String> },
    /// Switch the active terminal tab.
    TerminalSelect { session_id: String },
    /// Rename a terminal tab's display title.
    TerminalRename { session_id: String, title: String },
    /// Kill / close a terminal tab.
    TerminalKill { session_id: String },
    /// Refresh Git status through Planner → git.
    GitRefresh,
    /// Stage one or more paths.
    GitStage { paths: Vec<String> },
    /// Unstage one or more paths.
    GitUnstage { paths: Vec<String> },
    /// Request discard confirmation for paths (does not mutate yet).
    GitDiscardRequest { paths: Vec<String> },
    /// Confirm the pending discard.
    GitDiscardConfirm,
    /// Cancel the pending discard.
    GitDiscardCancel,
    /// Update the draft commit message.
    GitCommitMessage(String),
    /// Commit staged changes with the draft message.
    GitCommit,
    /// Update one or more Find in Files panel fields.
    UpdateSearchPanel {
        query: Option<String>,
        replace_text: Option<String>,
        use_regex: Option<bool>,
        case_sensitive: Option<bool>,
        whole_word: Option<bool>,
        filename_only: Option<bool>,
    },
    /// Run Find in Files with the panel's current query/toggles.
    RunSearch,
    /// Replace every located match with the panel's replace text.
    ReplaceAll,
    /// Open a Find in Files / Quick Open result in the editor.
    OpenSearchResult {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    /// Open a Problems panel issue in the editor (jump to file / line / column).
    OpenProblem {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    /// Recompute the Problems panel from every registered source.
    ProblemsRefresh,
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
                        let active = state.active_terminal_id.as_deref() == Some(session.id.as_str());
                        format!(
                            "{}{} ({}) · cwd={} · last={} · history={}",
                            if active { "▶ " } else { "" },
                            session.title,
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
                    "repo={} · branch={} · {}",
                    if git.is_repository {
                        git.repo_root.as_deref().unwrap_or("-")
                    } else {
                        "none"
                    },
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
                if !git.added.is_empty() {
                    lines.push(format!(
                        "added: {}",
                        git.added
                            .iter()
                            .map(|entry| entry.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !git.deleted.is_empty() {
                    lines.push(format!(
                        "deleted: {}",
                        git.deleted
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
                if let Some(pending) = &git.pending_discard {
                    lines.push(format!("pending discard: {}", pending.join(", ")));
                }
                lines
            }
            None => vec!["Git not connected — open Coding on a repository.".to_string()],
        },
        WorkspacePanel::Diagnostics => problems_summary_lines(state, diagnostics),
        _ => vec![panel.id().to_string()],
    }
}

/// Text summary of the aggregated Problems panel (severity/source/path/message),
/// with an optional one-line operational footer from the Coding Diagnostics view.
fn problems_summary_lines(state: &CodingState, diagnostics: Option<&CodingDiagnosticsView>) -> Vec<String> {
    let mut lines = Vec::new();
    if state.problems.is_empty() {
        lines.push("No problems".to_string());
    } else {
        lines.push(format!("{} problem(s)", state.problems.len()));
        for issue in &state.problems {
            lines.push(format!(
                "[{}] {} · {}{}",
                issue.severity,
                issue.source_label,
                problem_location(issue).unwrap_or_else(|| "—".to_string()),
                if issue.message.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", issue.message)
                }
            ));
        }
    }
    if let Some(view) = diagnostics {
        if let Some(section) = view.sections.first() {
            lines.push(format!(
                "{}: {}",
                section.title,
                section.lines.first().cloned().unwrap_or_default()
            ));
        }
    }
    lines
}

/// `path:line` (1-based) for a problem issue, when a path is known.
fn problem_location(issue: &ProblemIssue) -> Option<String> {
    let path = issue.path.as_deref()?;
    match issue.line {
        Some(line) => Some(format!("{path}:{}", line + 1)),
        None => Some(path.to_string()),
    }
}

fn explorer_lines(state: &CodingState) -> Vec<String> {
    let mut lines = Vec::new();
    match &state.explorer.status {
        ExplorerStatus::Idle => lines.push("Loading project tree…".to_string()),
        ExplorerStatus::NoProject => {
            lines.push("No open project — use Open Project… to browse files.".to_string());
        }
        ExplorerStatus::Error(message) => lines.push(format!("Explorer error: {message}")),
        ExplorerStatus::Ready => {
            if let Some(root) = &state.explorer.project_root {
                lines.push(format!("root: {root}"));
            }
            if state.explorer.nodes.is_empty() {
                lines.push("(empty project)".to_string());
            } else {
                collect_explorer_lines(&state.explorer.nodes, state, 0, &mut lines);
            }
        }
    }
    if let Some(selected) = &state.explorer.selected_path {
        lines.push(format!("selected: {selected}"));
    }
    if let Some(active) = state.active_tab_path() {
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
        let highlight = if state.active_tab_path() == Some(node.path.as_str())
            || state.explorer.selected_path.as_deref() == Some(node.path.as_str())
        {
            "▸ "
        } else {
            "  "
        };
        let expand = if node.is_dir {
            if state.explorer.expanded_paths.contains(&node.path) {
                "▾ "
            } else {
                "▸ "
            }
        } else {
            "  "
        };
        lines.push(format!("{indent}{highlight}{expand}{icon} {}", node.name));
        if node.is_dir && state.explorer.expanded_paths.contains(&node.path) {
            collect_explorer_lines(&node.children, state, depth + 1, lines);
        }
    }
}

fn editor_lines(state: &CodingState) -> Vec<String> {
    if state.editors.is_empty() {
        return vec!["No open files — select a file in Project Explorer.".to_string()];
    }
    let mut lines = Vec::new();
    if state.editors.panes.len() > 1 {
        lines.push(format!("panes: {}", state.editors.panes.len()));
    }
    let tabs: Vec<String> = state
        .editors
        .sessions()
        .iter()
        .map(|session| {
            let marker = if state.active_tab_path() == Some(session.path.as_str()) {
                "*"
            } else {
                " "
            };
            format!(
                "{marker}{}{}{}",
                session.name,
                if session.dirty { " · dirty" } else { "" },
                if session.preview { " · preview" } else { "" }
            )
        })
        .collect();
    lines.push(format!("tabs: {}", tabs.join(" | ")));
    if let Some(active) = state.editors.active_session() {
        let preview: String = active.content.chars().take(120).collect();
        lines.push(format!("buffer: {preview}"));
        lines.push(format!("scroll: {}", active.view.scroll_top));
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
/// `render_explorer_col` draws the explorer (implemented by `ui::explorer`).
pub fn render_coding_shell(
    ui: &mut egui::Ui,
    expansion: &WorkspaceExpansion,
    state: Option<&CodingState>,
    diagnostics: Option<&CodingDiagnosticsView>,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    open_error: Option<&str>,
    mut render_explorer_col: impl FnMut(&mut egui::Ui, &CodingState),
) {
    *monaco_out = None;
    let _ = expansion;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Coding").strong().size(15.0));
        if let Some(root) = state.and_then(|coding| coding.explorer.project_root.as_deref()) {
            ui.weak(truncate_path(root, 42));
        }
    });
    if let Some(error) = open_error {
        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
    }
    ui.add_space(SPACE_MD);
    ui.separator();

    let bottom_tab = state
        .map(|coding| coding.bottom_tab)
        .unwrap_or(CodingBottomTab::Hidden);
    let bottom_open = !matches!(bottom_tab, CodingBottomTab::Hidden);
    let explorer_visible = state
        .map(|coding| coding.explorer_visible)
        .unwrap_or(true);
    let explorer_width = state
        .map(|coding| coding.explorer_width)
        .unwrap_or(DEFAULT_EXPLORER_WIDTH);
    let bottom_panel_height = state
        .map(|coding| coding.bottom_panel_height)
        .unwrap_or(DEFAULT_BOTTOM_PANEL_HEIGHT);

    let tab_bar_h = 26.0_f32;
    let bottom_h = if bottom_open { bottom_panel_height } else { 0.0 };
    let chrome = SPACE_MD;
    let main_h = (ui.available_height() - tab_bar_h - bottom_h - chrome).max(180.0);
    let editor_min_width = 220.0_f32;
    let explorer_w = if explorer_visible {
        // Never let the explorer squeeze the editor below its minimum width.
        let max_allowed = (ui.available_width() - editor_min_width - CHROME_DIVIDER)
            .max(MIN_EXPLORER_WIDTH)
            .min(MAX_EXPLORER_WIDTH);
        explorer_width.clamp(MIN_EXPLORER_WIDTH, max_allowed)
    } else {
        0.0
    };
    let gap = if explorer_visible { CHROME_DIVIDER } else { 0.0 };
    let editor_w = (ui.available_width() - explorer_w - gap).max(editor_min_width);

    ui.horizontal(|ui| {
        ui.set_min_height(main_h);
        ui.set_max_height(main_h);

        ui.vertical(|ui| {
            ui.set_min_width(editor_w);
            ui.set_max_width(editor_w);
            ui.set_min_height(main_h);
            ui.set_max_height(main_h);
            if let Some(state) = state {
                render_editor(ui, state, events, monaco_out, main_h);
            } else {
                ui.weak(placeholder_for(WorkspacePanel::Editor));
            }
        });

        if explorer_visible {
            render_vertical_divider(ui, main_h, explorer_w, events);

            ui.vertical(|ui| {
                ui.set_min_width(explorer_w);
                ui.set_max_width(explorer_w);
                ui.set_min_height(main_h);
                ui.set_max_height(main_h);
                ui.label(egui::RichText::new("Explorer").strong().size(15.0));
                ui.add_space(SPACE_SM);
                egui::ScrollArea::vertical()
                    .id_salt("coding_explorer_scroll")
                    .max_height((main_h - 22.0).max(80.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width((explorer_w - 8.0).max(0.0));
                        if let Some(state) = state {
                            render_explorer_col(ui, state);
                        } else {
                            ui.weak(placeholder_for(WorkspacePanel::ProjectExplorer));
                        }
                    });
            });
        }
    });

    ui.add_space(SPACE_MD);

    ui.horizontal(|ui| {
        ui.set_min_height(tab_bar_h);
        for tab in [
            CodingBottomTab::Terminal,
            CodingBottomTab::Git,
            CodingBottomTab::Diagnostics,
            CodingBottomTab::Search,
        ] {
            let selected = bottom_tab == tab;
            let response = ui
                .selectable_label(selected, tab.label())
                .on_hover_text(format!("Show {} panel", tab.label()));
            if selected {
                let stroke = ui.visuals().selection.stroke;
                ui.painter().line_segment(
                    [response.rect.left_bottom(), response.rect.right_bottom()],
                    stroke,
                );
            }
            if response.clicked() {
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
        render_horizontal_divider(ui, bottom_panel_height, events);
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
                        render_diagnostics_panel(ui, state, diagnostics, events);
                    }
                    CodingBottomTab::Search => {
                        if let Some(state) = state {
                            render_search_panel(ui, state, events);
                        } else {
                            ui.weak(placeholder_for(WorkspacePanel::Editor));
                        }
                    }
                    CodingBottomTab::Hidden => {}
                }
            });
    }
}

/// Consistent spacing tokens for the Coding shell (replaces mixed 2/4/6/10 values).
const SPACE_XS: f32 = 4.0;
const SPACE_SM: f32 = 6.0;
const SPACE_MD: f32 = 8.0;
/// Width/height of the drag dividers between editor/explorer and above the bottom panel.
const CHROME_DIVIDER: f32 = 6.0;

/// Vertical drag divider between the editor and the Project Explorer column.
/// Dragging left/right resizes the explorer; state updates every dragged frame,
/// persistence is requested only once the drag is released.
fn render_vertical_divider(
    ui: &mut egui::Ui,
    height: f32,
    explorer_width: f32,
    events: &mut Vec<CodingShellEvent>,
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(CHROME_DIVIDER, height), egui::Sense::drag());
    let color = if response.dragged() || response.hovered() {
        ui.visuals().selection.bg_fill
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    ui.painter().rect_filled(rect, 0.0, color);
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged() {
        let delta = response.drag_delta().x;
        if delta.abs() > f32::EPSILON {
            // Divider sits to the left of Explorer: dragging right shrinks it.
            let width = (explorer_width - delta).clamp(MIN_EXPLORER_WIDTH, MAX_EXPLORER_WIDTH);
            events.push(CodingShellEvent::SetExplorerWidth {
                width,
                commit: false,
            });
        }
    }
    if response.drag_stopped() {
        events.push(CodingShellEvent::SetExplorerWidth {
            width: explorer_width,
            commit: true,
        });
    }
}

/// Horizontal drag divider above the bottom auxiliary panel content.
/// Dragging up/down resizes the panel; same drag/commit split as the explorer divider.
fn render_horizontal_divider(
    ui: &mut egui::Ui,
    bottom_panel_height: f32,
    events: &mut Vec<CodingShellEvent>,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CHROME_DIVIDER),
        egui::Sense::drag(),
    );
    let color = if response.dragged() || response.hovered() {
        ui.visuals().selection.bg_fill
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    ui.painter().rect_filled(rect, 0.0, color);
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if response.dragged() {
        let delta = response.drag_delta().y;
        if delta.abs() > f32::EPSILON {
            // Divider sits above the panel: dragging up (negative delta) grows it.
            let height = (bottom_panel_height - delta)
                .clamp(MIN_BOTTOM_PANEL_HEIGHT, MAX_BOTTOM_PANEL_HEIGHT);
            events.push(CodingShellEvent::SetBottomPanelHeight {
                height,
                commit: false,
            });
        }
    }
    if response.drag_stopped() {
        events.push(CodingShellEvent::SetBottomPanelHeight {
            height: bottom_panel_height,
            commit: true,
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

/// Problems panel — clickable, aggregated issues from `CodingState.problems`.
///
/// Built by [`crate::coding_workspace`] rendering aggregated
/// [`jaymi_capabilities::ProblemsRegistry`] output only — this panel never
/// talks to individual sources (LSP, Planner, Workspace, Permissions, Search,
/// Memory) directly. A weak footer shows one line of operational status from
/// the Coding Diagnostics view, when available, for a lightweight developer
/// signal without displacing the issue list.
fn render_diagnostics_panel(
    ui: &mut egui::Ui,
    state: Option<&CodingState>,
    diagnostics: Option<&CodingDiagnosticsView>,
    events: &mut Vec<CodingShellEvent>,
) {
    let count = state.map(|state| state.problems.len()).unwrap_or(0);
    ui.horizontal(|ui| {
        ui.strong(format!("{count} problem(s)"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Refresh").on_hover_text("Recompute Problems").clicked() {
                events.push(CodingShellEvent::ProblemsRefresh);
            }
        });
    });
    ui.add_space(SPACE_XS);

    let Some(state) = state else {
        ui.weak(placeholder_for(WorkspacePanel::Diagnostics));
        return;
    };

    if state.problems.is_empty() {
        ui.weak("No problems");
    } else {
        for issue in &state.problems {
            render_problem_row(ui, issue, events);
        }
    }

    if let Some(view) = diagnostics {
        if let Some(section) = view.sections.first() {
            ui.add_space(SPACE_SM);
            ui.separator();
            ui.weak(format!(
                "{}: {}",
                section.title,
                section.lines.first().cloned().unwrap_or_default()
            ));
        }
    }
}

/// Severity glyph for a Problems row.
fn severity_icon(severity: ProblemSeverity) -> &'static str {
    match severity {
        ProblemSeverity::Error => "✖",
        ProblemSeverity::Warning => "⚠",
        ProblemSeverity::Info => "ℹ",
        ProblemSeverity::Hint => "·",
    }
}

/// One clickable Problems row: severity · source_label · file:line · message.
fn render_problem_row(ui: &mut egui::Ui, issue: &ProblemIssue, events: &mut Vec<CodingShellEvent>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(severity_icon(issue.severity));
        ui.weak(&issue.source_label);
        let text = match problem_location(issue) {
            Some(location) => format!("{location} · {}", issue.message),
            None => issue.message.clone(),
        };
        if issue.can_jump() {
            if ui.link(text).clicked() {
                events.push(CodingShellEvent::OpenProblem {
                    path: issue.path.clone().unwrap_or_default(),
                    line: issue.line,
                    column: issue.column,
                });
            }
        } else {
            ui.label(text);
        }
    });
}


/// Drag-and-drop payload carried while dragging a tab between panes.
#[derive(Debug, Clone, PartialEq)]
struct TabDragPayload {
    pane: String,
    path: String,
}

/// Divider thickness (points) between split panes.
const SPLIT_DIVIDER: f32 = 6.0;
/// Minimum relative size a pane may be resized down to.
const MIN_SPLIT_FRACTION: f32 = 0.08;

/// Render the editor area — a recursive tree of split panes (VS Code-style).
///
/// Only the **focused** pane's active tab gets a Monaco overlay (`monaco_out`);
/// every other pane falls back to a plain egui `TextEdit` so multiple splits
/// can render without juggling several WebViews.
fn render_editor(
    ui: &mut egui::Ui,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    available_height: f32,
) {
    if state.editors.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space((available_height * 0.3).clamp(32.0, 96.0));
            ui.label(egui::RichText::new("No open files").size(15.0));
            ui.add_space(SPACE_XS);
            ui.weak("Select a file in Explorer →");
        });
        return;
    }

    if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::S)) {
        events.push(CodingShellEvent::SaveActive);
    }

    let header_h = 26.0_f32;
    ui.horizontal(|ui| {
        ui.set_min_height(header_h);
        if ui.button("Split ▥").on_hover_text("Split Right").clicked() {
            events.push(CodingShellEvent::SplitVertical);
        }
        if ui.button("Split ▤").on_hover_text("Split Down").clicked() {
            events.push(CodingShellEvent::SplitHorizontal);
        }
        if state.editors.panes.len() > 1
            && ui
                .button("Close Split")
                .on_hover_text("Close the focused pane")
                .clicked()
        {
            events.push(CodingShellEvent::ClosePane(
                state.editors.focused_pane.as_str().to_string(),
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut minimap_enabled = state.editor_settings.minimap;
            if ui
                .checkbox(&mut minimap_enabled, "Minimap")
                .on_hover_text("Toggle Monaco minimap")
                .changed()
            {
                events.push(CodingShellEvent::SetMinimap(minimap_enabled));
            }
            let mut wrap = state.editor_settings.word_wrap;
            if ui
                .checkbox(&mut wrap, "Wrap")
                .on_hover_text("Toggle word wrap")
                .changed()
            {
                events.push(CodingShellEvent::SetWordWrap(wrap));
            }
            let can_save = state
                .editors
                .active_session()
                .is_some_and(|session| session.dirty);
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

    let remaining = (available_height - header_h - 12.0).max(120.0);
    render_layout_node(ui, &state.editors.layout, state, events, monaco_out, remaining, &[]);
}

/// Render one node of the split layout tree (leaf pane or nested split).
#[allow(clippy::too_many_arguments)]
fn render_layout_node(
    ui: &mut egui::Ui,
    node: &EditorLayoutNode,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    height: f32,
    node_path: &[usize],
) {
    match node {
        EditorLayoutNode::Leaf { pane } => {
            render_pane(ui, pane, state, events, monaco_out, height);
        }
        EditorLayoutNode::Split {
            direction,
            sizes,
            children,
        } => {
            let side_by_side = matches!(direction, SplitDirection::Vertical);
            render_split(
                ui,
                children,
                sizes,
                state,
                events,
                monaco_out,
                height,
                node_path,
                side_by_side,
            );
        }
    }
}

/// Render a split's children with a draggable resize divider between each pair.
#[allow(clippy::too_many_arguments)]
fn render_split(
    ui: &mut egui::Ui,
    children: &[EditorLayoutNode],
    sizes: &[f32],
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    height: f32,
    node_path: &[usize],
    side_by_side: bool,
) {
    if children.is_empty() {
        return;
    }
    let total_span = if side_by_side {
        ui.available_width()
    } else {
        height
    };
    let divider_count = children.len().saturating_sub(1) as f32;
    let usable = (total_span - SPLIT_DIVIDER * divider_count).max(children.len() as f32 * 20.0);
    let mut new_sizes: Vec<f32> = if sizes.len() == children.len() {
        sizes.to_vec()
    } else {
        vec![1.0 / children.len() as f32; children.len()]
    };

    let mut body = |ui: &mut egui::Ui| {
        ui.set_min_height(height);
        ui.set_max_height(height);
        for (index, child) in children.iter().enumerate() {
            let fraction = new_sizes
                .get(index)
                .copied()
                .unwrap_or(1.0 / children.len() as f32);
            let span = (usable * fraction).max(20.0);
            let (child_size, child_height) = if side_by_side {
                (egui::vec2(span, height), height)
            } else {
                (egui::vec2(ui.available_width(), span), span)
            };
            ui.allocate_ui_with_layout(
                child_size,
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let mut child_path = node_path.to_vec();
                    child_path.push(index);
                    render_layout_node(
                        ui,
                        child,
                        state,
                        events,
                        monaco_out,
                        child_height,
                        &child_path,
                    );
                },
            );

            if index + 1 < children.len() {
                let divider_size = if side_by_side {
                    egui::vec2(SPLIT_DIVIDER, height)
                } else {
                    egui::vec2(ui.available_width(), SPLIT_DIVIDER)
                };
                let (rect, response) = ui.allocate_exact_size(divider_size, egui::Sense::drag());
                let color = if response.dragged() || response.hovered() {
                    ui.visuals().selection.bg_fill
                } else {
                    ui.visuals().widgets.noninteractive.bg_stroke.color
                };
                ui.painter().rect_filled(rect, 0.0, color);
                let response = response.on_hover_cursor(if side_by_side {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
                if response.dragged() {
                    let delta = response.drag_delta();
                    let delta_frac = if side_by_side {
                        delta.x / usable.max(1.0)
                    } else {
                        delta.y / usable.max(1.0)
                    };
                    if delta_frac.abs() > f32::EPSILON {
                        let left = new_sizes[index] + delta_frac;
                        let right = new_sizes[index + 1] - delta_frac;
                        if left > MIN_SPLIT_FRACTION && right > MIN_SPLIT_FRACTION {
                            new_sizes[index] = left;
                            new_sizes[index + 1] = right;
                        }
                    }
                }
            }
        }
    };

    if side_by_side {
        ui.horizontal(|ui| body(ui));
    } else {
        ui.vertical(|ui| body(ui));
    }

    let sum: f32 = new_sizes.iter().sum::<f32>().max(f32::EPSILON);
    let normalized: Vec<f32> = new_sizes.iter().map(|value| value / sum).collect();
    let changed = sizes.len() != normalized.len()
        || normalized
            .iter()
            .zip(sizes.iter())
            .any(|(new, old)| (new - old).abs() > 0.001);
    if changed {
        events.push(CodingShellEvent::ResizeSplit {
            node_path: node_path.to_vec(),
            sizes: normalized,
        });
    }
}

/// Render a single editor pane: tab strip (drag source + drop target) and body.
fn render_pane(
    ui: &mut egui::Ui,
    pane_id: &EditorPaneId,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    height: f32,
) {
    let pane_str = pane_id.as_str().to_string();
    let is_focused = state.editors.focused_pane == *pane_id;
    let sessions = state.editors.sessions_in_pane(pane_id);
    let active_path = state
        .editors
        .active_session_in_pane(pane_id)
        .map(|session| session.path);

    egui::Frame::new()
        .stroke(if is_focused {
            egui::Stroke::new(1.0, ui.visuals().selection.bg_fill)
        } else {
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
        })
        .inner_margin(3.0)
        .show(ui, |ui| {
            ui.set_min_height((height - 6.0).max(20.0));
            ui.set_max_height((height - 6.0).max(20.0));
            ui.set_min_width(ui.available_width());

            let strip_response = ui
                .scope(|ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt(("editor_tab_strip", &pane_str))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if sessions.is_empty() {
                                    ui.weak("(empty pane)");
                                }
                                for session in &sessions {
                                    render_pane_tab(ui, &pane_str, &active_path, session, events);
                                }
                            });
                        });
                })
                .response;

            // Drop target: releasing a dragged tab anywhere on this pane's
            // strip moves it here (VS Code-style cross-split drag).
            let drop = ui.interact(
                strip_response.rect,
                ui.id().with(("editor_pane_drop", &pane_str)),
                egui::Sense::hover(),
            );
            if let Some(payload) = drop.dnd_release_payload::<TabDragPayload>() {
                if payload.pane != pane_str {
                    events.push(CodingShellEvent::MoveTab {
                        from_pane: payload.pane.clone(),
                        path: payload.path.clone(),
                        to_pane: pane_str.clone(),
                        index: None,
                    });
                }
            }

            ui.separator();

            match active_path {
                None => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.weak("No open tabs in this pane");
                    });
                }
                Some(path) => {
                    if let Some(session) = sessions.into_iter().find(|session| session.path == path)
                    {
                        render_pane_body(
                            ui,
                            &pane_str,
                            is_focused,
                            &session,
                            state,
                            events,
                            monaco_out,
                            height,
                        );
                    }
                }
            }
        });
}

/// Render one tab label (drag source, click to activate, close button).
fn render_pane_tab(
    ui: &mut egui::Ui,
    pane_str: &str,
    active_path: &Option<String>,
    session: &EditorSession,
    events: &mut Vec<CodingShellEvent>,
) {
    let active = active_path.as_deref() == Some(session.path.as_str());
    let mut label = session.name.clone();
    if session.dirty {
        label.push('*');
    }
    let mut text = egui::RichText::new(&label);
    if session.preview {
        text = text.italics().weak();
    }
    if active {
        text = text.strong();
    }
    let tab_response = ui.add(
        egui::Label::new(text)
            .sense(egui::Sense::click_and_drag())
            .selectable(false),
    );
    if active {
        let stroke = ui.visuals().selection.stroke;
        ui.painter().line_segment(
            [
                tab_response.rect.left_bottom(),
                tab_response.rect.right_bottom(),
            ],
            stroke,
        );
    }
    tab_response.dnd_set_drag_payload(TabDragPayload {
        pane: pane_str.to_string(),
        path: session.path.clone(),
    });
    if tab_response.clicked() {
        events.push(CodingShellEvent::ActivateTab {
            pane: pane_str.to_string(),
            path: session.path.clone(),
        });
    }
    if tab_response.middle_clicked() {
        events.push(CodingShellEvent::CloseTab {
            pane: pane_str.to_string(),
            path: session.path.clone(),
        });
    }
    if ui
        .small_button(format!("✕##close_{}_{}", pane_str, session.path))
        .on_hover_text("Close tab")
        .clicked()
    {
        events.push(CodingShellEvent::CloseTab {
            pane: pane_str.to_string(),
            path: session.path.clone(),
        });
    }
}

/// Render one pane's active-tab body (egui fallback; Monaco overlays the focused pane).
#[allow(clippy::too_many_arguments)]
fn render_pane_body(
    ui: &mut egui::Ui,
    pane_str: &str,
    is_focused: bool,
    session: &EditorSession,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    available_height: f32,
) {
    let path = session.path.clone();
    let mut content = session.content.clone();
    let scroll_offset = session.view.scroll_top;
    let cursor_line = session.view.cursor.line;
    let cursor_column = session.view.cursor.column;
    let folded_regions = session
        .view
        .folded_regions
        .iter()
        .map(|region| (region.start_line, region.end_line))
        .collect::<Vec<_>>();
    let language = language_for_path(&path).to_string();
    let name = session.name.clone();
    let settings = state.editor_settings.clone();

    ui.weak(format!(
        "{} · {}{}",
        name,
        language,
        if session.preview { " · preview" } else { "" }
    ));
    ui.add_space(SPACE_XS);

    // Fill remaining code space; Monaco overlays this rect for the focused pane.
    let chrome = 40.0_f32;
    let editor_height = (available_height - chrome).max(80.0);
    let scroll = egui::ScrollArea::vertical()
        .id_salt(("editor_scroll", pane_str, &path))
        .max_height(editor_height)
        .auto_shrink([false, false])
        .vertical_scroll_offset(scroll_offset)
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut content)
                    .id_salt(("editor_body", pane_str, &path))
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .code_editor()
                    .frame(false),
            );
            if response.changed() {
                events.push(CodingShellEvent::EditContent {
                    pane: pane_str.to_string(),
                    path: path.clone(),
                    content: content.clone(),
                });
            }
            if !is_focused && (response.clicked() || response.gained_focus()) {
                events.push(CodingShellEvent::FocusPane(pane_str.to_string()));
            }
            response.rect
        });

    let new_offset = scroll.state.offset.y;
    if (new_offset - scroll_offset).abs() > f32::EPSILON {
        events.push(CodingShellEvent::Scroll {
            pane: pane_str.to_string(),
            path: path.clone(),
            offset: new_offset,
        });
    }

    if !is_focused {
        return;
    }

    let rect = scroll.inner;
    *monaco_out = Some(MonacoEditorSurface {
        viewport: MonacoViewport { rect },
        document: MonacoDocument {
            path,
            content,
            language,
            scroll_top: scroll_offset,
            cursor_line,
            cursor_column,
            folded_regions,
            minimap: settings.minimap,
            word_wrap: settings.word_wrap,
            font_size: settings.font_size,
        },
    });
}

fn render_terminal(ui: &mut egui::Ui, state: &CodingState, events: &mut Vec<CodingShellEvent>) {
    let active_id = state.active_terminal_id.clone();

    ui.horizontal_wrapped(|ui| {
        for session in &state.terminal_sessions {
            let is_active = active_id.as_deref() == Some(session.id.as_str());
            render_terminal_tab(ui, session, is_active, events);
            ui.add_space(SPACE_XS);
        }
        if ui.button("+ New").on_hover_text("Open a new terminal").clicked() {
            events.push(CodingShellEvent::TerminalCreate { title: None });
        }
    });
    ui.separator();

    if state.terminal_sessions.is_empty() {
        ui.weak("No terminal sessions — open Coding to spawn a PTY.");
        return;
    }

    let active_session = active_id
        .as_deref()
        .and_then(|id| state.terminal_sessions.iter().find(|session| session.id == id))
        .or_else(|| state.terminal_sessions.first());

    if let Some(session) = active_session {
        render_terminal_session(ui, session, events);
    }
}

/// One tab in the terminal tab strip: select / inline rename / close.
fn render_terminal_tab(
    ui: &mut egui::Ui,
    session: &TerminalSessionState,
    is_active: bool,
    events: &mut Vec<CodingShellEvent>,
) {
    let renaming_id = ui.id().with(("terminal_renaming", &session.id));
    let mut renaming = ui
        .data(|data| data.get_temp::<bool>(renaming_id))
        .unwrap_or(false);

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            if renaming {
                let draft_id = ui.id().with(("terminal_rename_draft", &session.id));
                let mut draft = ui
                    .data(|data| data.get_temp::<String>(draft_id))
                    .unwrap_or_else(|| session.title.clone());
                let response = ui.add(
                    egui::TextEdit::singleline(&mut draft)
                        .id_salt(("terminal_rename_input", &session.id))
                        .desired_width(96.0),
                );
                response.request_focus();
                if response.changed() {
                    ui.data_mut(|data| data.insert_temp(draft_id, draft.clone()));
                }
                let confirmed =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if confirmed {
                    let title = draft.trim();
                    if !title.is_empty() && title != session.title {
                        events.push(CodingShellEvent::TerminalRename {
                            session_id: session.id.clone(),
                            title: title.to_string(),
                        });
                    }
                    renaming = false;
                    ui.data_mut(|data| data.remove::<String>(draft_id));
                } else if cancelled || (response.lost_focus() && !confirmed) {
                    renaming = false;
                    ui.data_mut(|data| data.remove::<String>(draft_id));
                }
            } else {
                if ui.selectable_label(is_active, &session.title).clicked() {
                    events.push(CodingShellEvent::TerminalSelect {
                        session_id: session.id.clone(),
                    });
                }
                if ui.small_button("✎").on_hover_text("Rename terminal").clicked() {
                    renaming = true;
                }
                if ui.small_button("×").on_hover_text("Close terminal").clicked() {
                    events.push(CodingShellEvent::TerminalKill {
                        session_id: session.id.clone(),
                    });
                }
            }
        });
    });

    ui.data_mut(|data| data.insert_temp(renaming_id, renaming));
}

fn render_terminal_session(
    ui: &mut egui::Ui,
    session: &TerminalSessionState,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.horizontal(|ui| {
        ui.strong(&session.title);
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
                .hint_text("cargo test · git status · npm test · python …"),
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
        if git.is_repository {
            ui.strong(format!(
                "branch {}",
                git.branch.as_deref().unwrap_or("(unknown)")
            ));
            ui.weak(&git.summary);
        } else {
            ui.strong("Not a Git repository");
            ui.weak(&git.summary);
        }
        if ui.small_button("Refresh").clicked() {
            events.push(CodingShellEvent::GitRefresh);
        }
    });
    if let Some(root) = &git.repo_root {
        ui.weak(root);
    }
    if let Some(error) = &git.last_error {
        ui.colored_label(egui::Color32::from_rgb(180, 60, 60), error);
    }

    if let Some(pending) = &git.pending_discard {
        ui.group(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(160, 90, 20),
                format!(
                    "Discard changes to {}? This cannot be undone.",
                    pending.join(", ")
                ),
            );
            ui.horizontal(|ui| {
                if ui.button("Confirm Discard").clicked() {
                    events.push(CodingShellEvent::GitDiscardConfirm);
                }
                if ui.button("Cancel").clicked() {
                    events.push(CodingShellEvent::GitDiscardCancel);
                }
            });
        });
        ui.separator();
    }

    if !git.is_repository {
        ui.weak("Open a project that is a Git work tree, then Refresh.");
        return;
    }

    render_git_section(ui, "Staged", &git.staged, GitSectionActions::Unstage, events);
    render_git_section(
        ui,
        "Modified",
        &git.modified,
        GitSectionActions::StageAndDiscard,
        events,
    );
    render_git_section(
        ui,
        "Added",
        &git.added,
        GitSectionActions::Unstage,
        events,
    );
    render_git_section(
        ui,
        "Deleted",
        &git.deleted,
        GitSectionActions::StageAndDiscard,
        events,
    );
    render_git_section(
        ui,
        "Untracked",
        &git.untracked,
        GitSectionActions::StageAndDiscard,
        events,
    );

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

#[derive(Clone, Copy)]
enum GitSectionActions {
    Unstage,
    StageAndDiscard,
}

fn render_git_section(
    ui: &mut egui::Ui,
    title: &str,
    entries: &[jaymi_capabilities::GitFileEntry],
    actions: GitSectionActions,
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
            match actions {
                GitSectionActions::Unstage => {
                    if ui
                        .small_button(format!("Unstage##unstage_{title}_{}", entry.path))
                        .clicked()
                    {
                        events.push(CodingShellEvent::GitUnstage {
                            paths: vec![entry.path.clone()],
                        });
                    }
                }
                GitSectionActions::StageAndDiscard => {
                    if ui
                        .small_button(format!("Stage##stage_{title}_{}", entry.path))
                        .clicked()
                    {
                        events.push(CodingShellEvent::GitStage {
                            paths: vec![entry.path.clone()],
                        });
                    }
                    if ui
                        .small_button(format!("Discard##discard_{title}_{}", entry.path))
                        .clicked()
                    {
                        events.push(CodingShellEvent::GitDiscardRequest {
                            paths: vec![entry.path.clone()],
                        });
                    }
                }
            }
        });
    }
}

/// Find in Files / project search + replace panel.
fn render_search_panel(ui: &mut egui::Ui, state: &CodingState, events: &mut Vec<CodingShellEvent>) {
    let panel = &state.search;

    ui.horizontal(|ui| {
        let mut query = panel.query.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut query)
                .desired_width(240.0)
                .hint_text("Search"),
        );
        if response.changed() {
            events.push(CodingShellEvent::UpdateSearchPanel {
                query: Some(query),
                replace_text: None,
                use_regex: None,
                case_sensitive: None,
                whole_word: None,
                filename_only: None,
            });
        }
        let run_clicked = ui.button("Search").clicked()
            || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        if run_clicked {
            events.push(CodingShellEvent::RunSearch);
        }
        if panel.searching {
            ui.spinner();
        }
    });

    ui.horizontal(|ui| {
        let mut regex = panel.use_regex;
        if ui.checkbox(&mut regex, "Regex").changed() {
            events.push(CodingShellEvent::UpdateSearchPanel {
                query: None,
                replace_text: None,
                use_regex: Some(regex),
                case_sensitive: None,
                whole_word: None,
                filename_only: None,
            });
        }
        let mut case_sensitive = panel.case_sensitive;
        if ui.checkbox(&mut case_sensitive, "Case sensitive").changed() {
            events.push(CodingShellEvent::UpdateSearchPanel {
                query: None,
                replace_text: None,
                use_regex: None,
                case_sensitive: Some(case_sensitive),
                whole_word: None,
                filename_only: None,
            });
        }
        let mut whole_word = panel.whole_word;
        if ui.checkbox(&mut whole_word, "Whole word").changed() {
            events.push(CodingShellEvent::UpdateSearchPanel {
                query: None,
                replace_text: None,
                use_regex: None,
                case_sensitive: None,
                whole_word: Some(whole_word),
                filename_only: None,
            });
        }
        let mut filename_only = panel.filename_only;
        if ui.checkbox(&mut filename_only, "Files only").changed() {
            events.push(CodingShellEvent::UpdateSearchPanel {
                query: None,
                replace_text: None,
                use_regex: None,
                case_sensitive: None,
                whole_word: None,
                filename_only: Some(filename_only),
            });
        }
    });

    ui.horizontal(|ui| {
        let mut replace_text = panel.replace_text.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut replace_text)
                .desired_width(240.0)
                .hint_text("Replace"),
        );
        if response.changed() {
            events.push(CodingShellEvent::UpdateSearchPanel {
                query: None,
                replace_text: Some(replace_text),
                use_regex: None,
                case_sensitive: None,
                whole_word: None,
                filename_only: None,
            });
        }
        if ui
            .add_enabled(
                !panel.query.trim().is_empty() && !panel.filename_only,
                egui::Button::new("Replace All"),
            )
            .clicked()
        {
            events.push(CodingShellEvent::ReplaceAll);
        }
    });

    if !panel.status.is_empty() {
        ui.weak(&panel.status);
    }

    ui.separator();
    if !panel.searching && panel.results.is_empty() {
        ui.add_space(SPACE_XS);
        if panel.query.trim().is_empty() {
            ui.weak("Type to search");
        } else {
            ui.weak("No results");
        }
        return;
    }
    for result in &panel.results {
        let location = match (result.line, result.column) {
            (Some(line), Some(column)) => format!("{}:{}:{}", result.path, line + 1, column + 1),
            (Some(line), None) => format!("{}:{}", result.path, line + 1),
            _ => result.path.clone(),
        };
        ui.horizontal(|ui| {
            if ui.link(location).clicked() {
                events.push(CodingShellEvent::OpenSearchResult {
                    path: result.path.clone(),
                    line: result.line,
                    column: result.column,
                });
            }
        });
        ui.weak(truncate_path(&result.preview, 120));
        ui.add_space(SPACE_XS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::{
        DiagnosticState, ExplorerNode, ExplorerState, ExplorerStatus, GitStatusState, OpenEditors,
        TerminalSessionState,
    };
    use std::collections::BTreeSet;

    #[test]
    fn summary_includes_explorer_and_editor_tabs() {
        let mut editors = OpenEditors::default();
        editors.open_permanent("/tmp/app/src/main.rs", "fn main() {}".into());
        editors.set_content("/tmp/app/src/main.rs", "fn main() { /* dirty */ }".into());
        editors.set_scroll_top("/tmp/app/src/main.rs", 12.0);
        let state = CodingState {
            explorer: ExplorerState {
                project_root: Some("/tmp/app".into()),
                nodes: vec![
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
                status: ExplorerStatus::Ready,
                ..ExplorerState::default()
            },
            editors,
            terminal_sessions: vec![TerminalSessionState {
                id: "term-1".into(),
                title: "Terminal".into(),
                cwd: Some("/tmp/app".into()),
                last_command: Some("cargo check".into()),
                output: String::new(),
                history: vec!["cargo check".into()],
                input: String::new(),
                history_index: None,
                scroll_offset: 0.0,
            }],
            active_terminal_id: Some("term-1".into()),
            git: Some(GitStatusState {
                is_repository: true,
                branch: Some("main".into()),
                summary: "clean".into(),
                ..GitStatusState::default()
            }),
            diagnostics: vec![DiagnosticState::simple(
                "unused import",
                Some("/tmp/app/src/main.rs".into()),
                "warning",
            )],
            problems: vec![ProblemIssue {
                id: "lsp:0".into(),
                severity: ProblemSeverity::Warning,
                source: "lsp".into(),
                source_label: "rust-analyzer".into(),
                path: Some("/tmp/app/src/main.rs".into()),
                line: Some(0),
                column: Some(0),
                end_line: Some(0),
                end_column: Some(1),
                message: "unused import".into(),
            }],
            bottom_tab: CodingBottomTab::Terminal,
            ..CodingState::default()
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
