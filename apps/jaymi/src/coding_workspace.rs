//! Coding Workspace shell — Project Explorer + Monaco Editor + Terminal + Git + Diagnostics.
//!
//! Conversation stays in the central panel. This module renders the right-side
//! Coding expansion as an IDE region layout (`TopBottomPanel` / `SidePanel` /
//! `CentralPanel` via `show_inside`). UI only renders [`CodingState`]; filesystem,
//! terminal, and git access go through Application → Planner → Tool → Provider.
//!
//! The editor body is a Monaco WebView overlay. Buffer text lives in
//! [`CodingState`] so content survives UI remounts / hot reloads.
//!
//! The Diagnostics panel is read-only operational status for development.
//! The Output dock page is a placeholder for future build / tool streams.

use eframe::egui;
use jaymi_capabilities::{
    CodingBottomTab, CodingState, EditorLayoutNode, EditorPaneId, EditorSession, ExplorerNode,
    ExplorerStatus, ProblemIssue, ProblemSeverity, SplitDirection, TerminalSessionState,
    WorkspaceExpansion, WorkspacePanel, WorkspacePanels, COLLAPSED_BOTTOM_TAB_HEIGHT,
    DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_EXPLORER_WIDTH, MAX_BOTTOM_PANEL_HEIGHT,
    MAX_EXPLORER_WIDTH, MIN_BOTTOM_PANEL_HEIGHT, MIN_EXPLORER_WIDTH,
};

use crate::coding_breadcrumb::{self, BreadcrumbAction, BREADCRUMB_BAR_HEIGHT};
use crate::coding_quick_actions::{
    self, QuickAction, QuickActionBarEvent, QUICK_ACTION_BAR_HEIGHT, QUICK_ACTION_PAD_X,
};
use crate::diagnostics::DiagnosticsSnapshot;
use crate::experience::ExperienceSession;
use crate::monaco_host::{language_for_path, MonacoDocument, MonacoViewport};
use crate::theme::{inset, radius, space, stroke, type_size, Theme};
use crate::ui::icons::Icon;
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
    inspection: &crate::execution_diagnostics::ExecutionInspection,
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
                format!(
                    "type={} · status={}",
                    project.project_type.as_str(),
                    project.status.as_str()
                ),
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

    // Developer-facing execution inspection (pause / review / approvals).
    sections.extend(crate::execution_diagnostics::execution_inspection_sections(
        inspection,
    ));

    // Performance dashboard — observational only (never conversation UI).
    {
        let performance = snapshot.performance_dashboard();
        if performance.has_content() {
            sections.push(CodingDiagnosticsSection {
                title: "Performance".into(),
                lines: performance.lines(),
            });
        }
    }

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
                    if activity.awaiting_review {
                        lines.push(format!(
                            "awaiting_review=true · plan={}",
                            activity.plan_id.as_deref().unwrap_or("—")
                        ));
                    }
                }
                None => lines.push("last: no planner requests yet".into()),
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Conversational reasoning".into(),
        lines: match &snapshot.reasoning_inspector {
            Some(report) => report
                .labeled_values()
                .into_iter()
                .map(|(label, value)| format!("{label}={value}"))
                .collect(),
            None => vec!["reasoning inspector unavailable".into()],
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
                lines.push(format!(
                    "{} · {}",
                    providers.status.label(),
                    providers.detail
                ));
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
        title: "Permission engine".into(),
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
    let end = rest.find([' ', ',']).unwrap_or(rest.len());
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
    /// True when the response left a plan awaiting review.
    pub awaiting_review: bool,
    /// Execution plan id from the last response, when any.
    pub plan_id: Option<String>,
    /// Plan status label, when any.
    pub plan_status: Option<String>,
    /// Estimated risk from the last plan, when any.
    pub risk: Option<String>,
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
    /// Persist text selection for a tab in a specific pane.
    SetSelection {
        pane: String,
        path: String,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
        text: Option<String>,
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
    /// Open the Command Palette (⌘P / ⌘⇧P).
    OpenCommandPalette,
    /// Open the Command Palette (legacy Quick Open shortcut path).
    OpenQuickOpen,
    /// Open Find in Files (bottom Search dock).
    OpenSearch,
    /// Close the Coding workspace (conversation stays open).
    CloseWorkspace,
    /// Open the workspace Diagnostics dock page.
    OpenSettings,
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
    /// Show or hide the Project Explorer (collapsed state keeps width).
    SetExplorerVisible { visible: bool, commit: bool },
    /// Resize the bottom auxiliary panel height (drag divider above the panel).
    /// `commit` is true once on drag release, telling the caller to persist to disk.
    SetBottomPanelHeight { height: f32, commit: bool },
    /// Show / hide / switch a bottom dock page (Terminal, Git, Problems, …).
    /// [`CodingBottomTab::Hidden`] fully collapses the dock.
    SetBottomTab(CodingBottomTab),
    /// Toggle the bottom dock open/closed (restores last page when reopening).
    ToggleBottomDock,
    /// Update the draft input for a terminal session.
    TerminalInput { session_id: String, input: String },
    /// Run the current terminal draft (or an explicit command).
    TerminalRun { session_id: String, command: String },
    /// egui terminal command field needs keyboard focus (release Monaco WebView).
    TerminalWantsKeyboard,
    /// Focus the terminal command field (e.g. click transcript / open Terminal dock).
    TerminalFocusInput { session_id: String },
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
    /// Select + reveal a path in Project Explorer (breadcrumb / jump targets).
    RevealInExplorer {
        /// Absolute filesystem path to select.
        path: String,
        /// Expand the path itself when it is a directory.
        is_dir: bool,
    },
    /// Quick Action Bar button — shell maps via [`crate::coding_quick_actions::dispatch_quick_action`].
    QuickAction(QuickAction),
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
                vec!["No terminal running.".to_string()]
            } else {
                state
                    .terminal_sessions
                    .iter()
                    .map(|session| {
                        let active =
                            state.active_terminal_id.as_deref() == Some(session.id.as_str());
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
            None => vec!["No repository opened.".to_string()],
        },
        WorkspacePanel::Diagnostics => problems_summary_lines(state, diagnostics),
        _ => vec![panel.id().to_string()],
    }
}

/// Text summary of the aggregated Problems panel (severity/source/path/message),
/// with an optional one-line operational footer from the Coding Diagnostics view.
fn problems_summary_lines(
    state: &CodingState,
    diagnostics: Option<&CodingDiagnosticsView>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if state.problems.is_empty() {
        lines.push("No problems detected.".to_string());
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
        let icon = if node.is_dir { "[dir]" } else { "[file]" };
        let highlight = if state.active_tab_path() == Some(node.path.as_str())
            || state.explorer.selected_path.as_deref() == Some(node.path.as_str())
        {
            "> "
        } else {
            "  "
        };
        let expand = if node.is_dir {
            if state.explorer.expanded_paths.contains(&node.path) {
                "v "
            } else {
                "> "
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
        return vec!["No open files — select a file in Explorer.".to_string()];
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
        WorkspacePanel::Editor => "No open files — select a file in Explorer.",
        WorkspacePanel::Terminal => "No terminal running.",
        WorkspacePanel::Git => "Open a project that contains a Git repository.",
        WorkspacePanel::Diagnostics => "Workspace information.",
        _ => "panel",
    }
}

/// Render the Coding Workspace shell into the right-side expansion panel.
///
/// Layout (Monaco as centerpiece):
/// ```text
/// ┌───────────────────────────────┐
/// │ Toolbar                        │
/// ├───────────────────────────────┤
/// │ Tabs                           │
/// ├──────────────────────┬────────┤
/// │      Monaco          │Explorer│
/// ├──────────────────────┴────────┤
/// │ Terminal / Problems / Git      │
/// └───────────────────────────────┘
/// ```
/// - [`TopBottomPanel`] top — Quick Action Bar (Planner intents)
/// - [`TopBottomPanel`] top — breadcrumb
/// - [`TopBottomPanel`] top — editor tabs (focused pane; full width)
/// - [`TopBottomPanel`] bottom — dock (Terminal / Problems / Search / Git / …)
/// - [`SidePanel`] right — Explorer (beside Monaco only)
/// - [`CentralPanel`] — Monaco / editor body (full-bleed)
///
/// `render_explorer_col` draws the explorer (implemented by `ui::explorer`).
/// Chrome colors come from [`Theme`]; Monaco uses its own editor theme separately.
#[allow(clippy::too_many_arguments)]
pub fn render_coding_shell(
    ui: &mut egui::Ui,
    theme: &Theme,
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

    let explorer_visible = state.map(|coding| coding.explorer_visible).unwrap_or(true);
    let explorer_width = state
        .map(|coding| coding.explorer_width)
        .unwrap_or(DEFAULT_EXPLORER_WIDTH);
    let single_pane = state
        .map(|coding| matches!(coding.editors.layout, EditorLayoutNode::Leaf { .. }))
        .unwrap_or(true);

    // --- Workspace header (icon + title + subtitle) -------------------------
    egui::TopBottomPanel::top("coding_workspace_header")
        .exact_height(WORKSPACE_HEADER_HEIGHT)
        .show_separator_line(false)
        .frame(region_frame(
            theme,
            egui::Margin::symmetric(space::LG as i8, 0),
        ))
        .show_inside(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let subtitle = state.and_then(|coding| {
                    let name = coding
                        .active_tab_path()
                        .and_then(|path| path.rsplit('/').next())
                        .unwrap_or("No open files");
                    let dirty = coding
                        .editors
                        .sessions()
                        .into_iter()
                        .filter(|session| session.dirty)
                        .count();
                    Some(if dirty > 0 {
                        format!("{name} · {dirty} edit{} pending", if dirty == 1 { "" } else { "s" })
                    } else {
                        name.to_string()
                    })
                });
                crate::ui::components::render_workspace_header(
                    ui,
                    theme,
                    Icon::Coding,
                    theme.accent_tint,
                    theme.accent_deep,
                    "Coding",
                    subtitle.as_deref(),
                );
            });
        });

    // --- Quick Action Bar (Planner intents; not a VS Code toolbar) ----------
    egui::TopBottomPanel::top("coding_quick_actions")
        .exact_height(QUICK_ACTION_BAR_HEIGHT)
        .show_separator_line(true)
        .frame(region_frame(
            theme,
            egui::Margin::symmetric(QUICK_ACTION_PAD_X as i8, 0),
        ))
        .show_inside(ui, |ui| {
            for event in coding_quick_actions::render_quick_action_bar(ui, theme, open_error) {
                match event {
                    QuickActionBarEvent::Action(action) => {
                        events.push(CodingShellEvent::QuickAction(action));
                    }
                }
            }
        });

    // --- Breadcrumb (under toolbar; derived from CodingState only) ---------
    if let Some(coding) = state {
        egui::TopBottomPanel::top("coding_breadcrumb")
            .exact_height(BREADCRUMB_BAR_HEIGHT)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme.surface_alt))
            .show_inside(ui, |ui| {
                let actions = coding_breadcrumb::render_coding_breadcrumb(ui, theme, coding);
                for action in actions {
                    match action {
                        BreadcrumbAction::FocusEditor => {
                            events.push(CodingShellEvent::FocusPane(
                                coding.editors.focused_pane.0.clone(),
                            ));
                        }
                        BreadcrumbAction::RevealInExplorer { path, is_dir } => {
                            events.push(CodingShellEvent::RevealInExplorer { path, is_dir });
                        }
                    }
                }
            });
    }

    // --- Editor tabs (full width, above Monaco | Explorer) -----------------
    // Single-pane: hoist tabs here so Monaco is the uninterrupted centerpiece.
    // Multi-pane splits keep per-pane tab strips inside the editor column.
    if single_pane {
        egui::TopBottomPanel::top("coding_editor_tabs")
            .exact_height(EDITOR_TAB_STRIP_HEIGHT)
            .show_separator_line(true)
            .frame(region_frame(
                theme,
                egui::Margin::symmetric(space::SM as i8, 0),
            ))
            .show_inside(ui, |ui| {
                render_workspace_tab_strip(ui, theme, state, events);
            });
    }

    // --- Bottom Dock (full width under Monaco + Explorer) ------------------
    let bottom_tab = state
        .map(|coding| coding.bottom_tab())
        .unwrap_or(CodingBottomTab::Hidden);
    let bottom_open = bottom_tab.is_page();
    let bottom_panel_height = state
        .map(|coding| coding.bottom_panel_height())
        .unwrap_or(DEFAULT_BOTTOM_PANEL_HEIGHT);

    if bottom_open {
        let tab_bar_h = DOCK_TAB_BAR_HEIGHT;
        let dock_height = tab_bar_h + bottom_panel_height;
        let panel_id = egui::Id::new(DOCK_PANEL_ID);
        sync_dock_panel_height(ui.ctx(), panel_id, dock_height);

        let bottom_response = egui::TopBottomPanel::bottom(panel_id)
            .default_height(dock_height)
            .height_range(
                (tab_bar_h + MIN_BOTTOM_PANEL_HEIGHT)..=(tab_bar_h + MAX_BOTTOM_PANEL_HEIGHT),
            )
            .resizable(true)
            .show_separator_line(true)
            .frame(region_frame(theme, egui::Margin::ZERO))
            .show_inside(ui, |ui| {
                egui::Frame::new()
                    .fill(theme.surface_alt)
                    .inner_margin(inset(space::SM, 0.0))
                    .show(ui, |ui| {
                        render_bottom_dock_tabs(ui, theme, bottom_tab, events);
                    });
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(stroke::HAIRLINE, theme.border),
                );

                let content_h = ui.available_height().max(MIN_BOTTOM_PANEL_HEIGHT);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), content_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_height(content_h);
                        ui.set_max_height(content_h);
                        egui::Frame::new()
                            .inner_margin(inset(space::MD, space::SM))
                            .show(ui, |ui| {
                                render_bottom_dock_page(
                                    ui,
                                    theme,
                                    bottom_tab,
                                    state,
                                    diagnostics,
                                    events,
                                );
                            });
                    },
                );
            });

        let rendered_h = bottom_response.response.rect.height();
        let content_h =
            (rendered_h - tab_bar_h).clamp(MIN_BOTTOM_PANEL_HEIGHT, MAX_BOTTOM_PANEL_HEIGHT);

        // Double-click the egui resize divider to reset to the default height.
        if dock_divider_double_clicked(ui, bottom_response.response.rect) {
            events.push(CodingShellEvent::SetBottomPanelHeight {
                height: DEFAULT_BOTTOM_PANEL_HEIGHT,
                commit: true,
            });
            clear_dock_panel_state(ui.ctx(), panel_id);
        } else if (content_h - bottom_panel_height).abs() > 1.0 {
            let commit = !ui.ctx().input(|input| input.pointer.primary_down());
            events.push(CodingShellEvent::SetBottomPanelHeight {
                height: content_h,
                commit,
            });
        }
    }

    // --- Right Explorer (beside Monaco only) -------------------------------
    if explorer_visible {
        let panel_id = egui::Id::new(EXPLORER_PANEL_ID);
        sync_explorer_panel_width(ui.ctx(), panel_id, explorer_width);

        let explorer_response = egui::SidePanel::right(panel_id)
            .default_width(explorer_width)
            .width_range(MIN_EXPLORER_WIDTH..=MAX_EXPLORER_WIDTH)
            .resizable(true)
            .show_separator_line(true)
            .frame(region_frame(theme, inset(space::SM, space::SM)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Explorer")
                            .strong()
                            .size(type_size::UI)
                            .color(theme.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let collapse = ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("›")
                                        .size(type_size::UI)
                                        .color(theme.text_secondary),
                                )
                                .frame(false)
                                .min_size(egui::vec2(18.0, 18.0)),
                            )
                            .on_hover_text("Collapse Explorer");
                        if collapse.clicked() {
                            events.push(CodingShellEvent::SetExplorerVisible {
                                visible: false,
                                commit: true,
                            });
                        }
                    });
                });
                ui.add_space(space::SM);
                egui::ScrollArea::vertical()
                    .id_salt("coding_explorer_scroll")
                    .animated(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width());
                        if let Some(state) = state {
                            render_explorer_col(ui, state);
                        } else {
                            ui.label(
                                egui::RichText::new(placeholder_for(
                                    WorkspacePanel::ProjectExplorer,
                                ))
                                .color(theme.text_secondary),
                            );
                        }
                    });
            });

        let panel_rect = explorer_response.response.rect;
        let rendered_w = panel_rect.width();

        // Double-click the egui resize divider to reset to the default width.
        if explorer_divider_double_clicked(ui, panel_rect) {
            events.push(CodingShellEvent::SetExplorerWidth {
                width: DEFAULT_EXPLORER_WIDTH,
                commit: true,
            });
            clear_explorer_panel_state(ui.ctx(), panel_id);
        } else if (rendered_w - explorer_width).abs() > 1.0 {
            let commit = !ui.ctx().input(|input| input.pointer.primary_down());
            events.push(CodingShellEvent::SetExplorerWidth {
                width: rendered_w.clamp(MIN_EXPLORER_WIDTH, MAX_EXPLORER_WIDTH),
                commit,
            });
        }
    }

    // --- Center: Monaco / editor body (full-bleed centerpiece) -------------
    egui::CentralPanel::default()
        .frame(region_frame(theme, egui::Margin::ZERO))
        .show_inside(ui, |ui| {
            let available_height = ui.available_height();
            if let Some(state) = state {
                render_editor(
                    ui,
                    theme,
                    state,
                    events,
                    monaco_out,
                    available_height,
                    !single_pane, // per-pane tabs only when split
                );
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space((available_height * 0.28).clamp(24.0, 72.0));
                    ui.label(
                        egui::RichText::new(placeholder_for(WorkspacePanel::Editor))
                            .color(theme.text_secondary),
                    );
                });
            }
        });
}

/// Full-width tab strip for the focused editor pane (single-pane layout).
fn render_workspace_tab_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: Option<&CodingState>,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.set_min_height(EDITOR_TAB_STRIP_HEIGHT);
    ui.set_max_height(EDITOR_TAB_STRIP_HEIGHT);
    let Some(state) = state else {
        ui.label(
            egui::RichText::new("No open files")
                .size(type_size::UI)
                .color(theme.text_secondary),
        );
        return;
    };
    if state.editors.is_empty() {
        ui.label(
            egui::RichText::new("No open files")
                .size(type_size::UI)
                .color(theme.text_secondary),
        );
        return;
    }

    let pane_id = state.editors.focused_pane.clone();
    let pane_str = pane_id.as_str().to_string();
    let sessions = state.editors.sessions_in_pane(&pane_id);
    let active_path = state
        .editors
        .active_session_in_pane(&pane_id)
        .map(|session| session.path);

    let strip_response = ui
        .scope(|ui| {
            egui::ScrollArea::horizontal()
                .id_salt("coding_workspace_tab_strip")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::XS;
                        ui.set_min_height(EDITOR_TAB_STRIP_HEIGHT);
                        if sessions.is_empty() {
                            ui.label(
                                egui::RichText::new("Select a file in Explorer")
                                    .size(type_size::UI)
                                    .color(theme.text_secondary),
                            );
                        }
                        for session in &sessions {
                            render_pane_tab(ui, theme, &pane_str, &active_path, session, events);
                        }
                    });
                });
        })
        .response;

    let drop = ui.interact(
        strip_response.rect,
        ui.id().with("coding_workspace_tab_drop"),
        egui::Sense::hover(),
    );
    if let Some(payload) = drop.dnd_release_payload::<TabDragPayload>() {
        if payload.pane != pane_str {
            events.push(CodingShellEvent::MoveTab {
                from_pane: payload.pane.clone(),
                path: payload.path.clone(),
                to_pane: pane_str,
                index: None,
            });
        }
    }
}

/// Flat region chrome — fill + margin only (no nested card Frames). The
/// Coding shell sits on `--panel`, one step deeper than the conversation's
/// cream ground, so the workspace reads as its own surface.
fn region_frame(theme: &Theme, margin: egui::Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.panel)
        .inner_margin(margin)
        .stroke(egui::Stroke::NONE)
}

/// Bottom dock tab strip — one page visible; active tab is highlighted.
/// Clicking the active tab collapses the dock (VS Code panel behavior).
fn render_bottom_dock_tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    bottom_tab: CodingBottomTab,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.horizontal(|ui| {
        ui.set_min_height(DOCK_TAB_BAR_HEIGHT - 4.0);
        ui.spacing_mut().item_spacing.x = space::XS;
        for &tab in WorkspacePanels::dock_tabs() {
            let selected = bottom_tab == tab;
            let galley = ui.painter().layout_no_wrap(
                tab.label().to_string(),
                egui::FontId::proportional(type_size::META),
                egui::Color32::PLACEHOLDER,
            );
            let size = egui::vec2(galley.size().x + space::SM * 2.0, 24.0);
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let response = response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("Show {} panel", tab.label()));
            let hovered = response.hovered();
            if selected {
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(radius::PILL as u8),
                    theme.surface,
                );
            } else if hovered {
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(radius::PILL as u8),
                    theme.selection(),
                );
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tab.label(),
                egui::FontId::proportional(type_size::META),
                if selected || hovered {
                    theme.text_primary
                } else {
                    theme.text_secondary
                },
            );
            if response.clicked() {
                // Clicking the active tab collapses the dock completely.
                let next = if selected {
                    CodingBottomTab::Hidden
                } else {
                    tab
                };
                events.push(CodingShellEvent::SetBottomTab(next));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Hide")
                            .size(type_size::META)
                            .color(theme.text_secondary),
                    )
                    .frame(false),
                )
                .on_hover_text("Hide Panel")
                .clicked()
            {
                events.push(CodingShellEvent::SetBottomTab(CodingBottomTab::Hidden));
            }
        });
    });
}

/// Active dock page body. Each page fills the dock content area; state is owned
/// by [`CodingState`] so switching tabs does not reset Terminal / Search / Git.
fn render_bottom_dock_page(
    ui: &mut egui::Ui,
    theme: &Theme,
    bottom_tab: CodingBottomTab,
    state: Option<&CodingState>,
    diagnostics: Option<&CodingDiagnosticsView>,
    events: &mut Vec<CodingShellEvent>,
) {
    match bottom_tab {
        CodingBottomTab::Terminal => {
            if let Some(state) = state {
                render_terminal(ui, theme, state, events);
            } else {
                ui.label(
                    egui::RichText::new(placeholder_for(WorkspacePanel::Terminal))
                        .color(theme.text_secondary),
                );
            }
        }
        CodingBottomTab::Problems => {
            render_problems_panel(ui, theme, state, events);
        }
        CodingBottomTab::Search => {
            if let Some(state) = state {
                render_search_panel(ui, theme, state, events);
            } else {
                ui.label(egui::RichText::new("Search project files…").color(theme.text_secondary));
            }
        }
        CodingBottomTab::Git => {
            if let Some(state) = state {
                render_git(ui, theme, state, events);
            } else {
                ui.label(
                    egui::RichText::new(placeholder_for(WorkspacePanel::Git))
                        .color(theme.text_secondary),
                );
            }
        }
        CodingBottomTab::Diagnostics => {
            render_workspace_diagnostics_panel(ui, theme, diagnostics);
        }
        CodingBottomTab::Output => {
            render_output_panel(ui, theme);
        }
        CodingBottomTab::Hidden => {}
    }
}

/// Output dock page — placeholder surface for future build / tool streams.
fn render_output_panel(ui: &mut egui::Ui, theme: &Theme) {
    egui::ScrollArea::vertical()
        .id_salt("coding_dock_output")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Output")
                    .strong()
                    .size(type_size::UI)
                    .color(theme.text_primary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("No output yet. Build and tool streams will appear here.")
                    .size(type_size::BODY)
                    .color(theme.text_secondary),
            );
        });
}

/// Workspace header (icon + title + subtitle) — spec: `height:56px`.
const WORKSPACE_HEADER_HEIGHT: f32 = 56.0;
/// Full-width editor tab strip under the toolbar.
const EDITOR_TAB_STRIP_HEIGHT: f32 = 32.0;
/// Dock tab strip height when the bottom dock is open.
const DOCK_TAB_BAR_HEIGHT: f32 = COLLAPSED_BOTTOM_TAB_HEIGHT;
/// Stable egui SidePanel id for the Project Explorer (right).
const EXPLORER_PANEL_ID: &str = "coding_explorer";
/// Stable egui TopBottomPanel id for the bottom dock.
const DOCK_PANEL_ID: &str = "coding_dock";

/// When CodingState dock height diverges from egui's persisted panel size
/// (restore / double-click reset) and the user is not dragging, drop PanelState
/// so `default_height` from CodingState wins on the next layout.
fn sync_dock_panel_height(ctx: &egui::Context, panel_id: egui::Id, height: f32) {
    let resizing = ctx
        .read_response(panel_id.with("__resize"))
        .is_some_and(|response| response.dragged());
    if resizing {
        return;
    }
    if let Some(panel) = egui::containers::panel::PanelState::load(ctx, panel_id) {
        if (panel.rect.height() - height).abs() > 1.0 {
            clear_dock_panel_state(ctx, panel_id);
        }
    }
}

fn clear_dock_panel_state(ctx: &egui::Context, panel_id: egui::Id) {
    ctx.data_mut(|data| {
        data.remove::<egui::containers::panel::PanelState>(panel_id);
    });
}

/// Double-click on the TopBottomPanel resize divider (egui grab strip).
fn dock_divider_double_clicked(ui: &egui::Ui, panel_rect: egui::Rect) -> bool {
    let grab = ui.style().interaction.resize_grab_radius_side;
    let resize_y = panel_rect.top();
    let resize_rect = egui::Rect::from_x_y_ranges(
        panel_rect.x_range(),
        (resize_y - grab)..=(resize_y + grab),
    );
    ui.input(|input| {
        input
            .pointer
            .button_double_clicked(egui::PointerButton::Primary)
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| resize_rect.contains(pos))
    })
}

/// When CodingState width diverges from egui's persisted panel size (restore /
/// double-click reset) and the user is not dragging, drop PanelState so
/// `default_width` from CodingState wins on the next layout.
fn sync_explorer_panel_width(ctx: &egui::Context, panel_id: egui::Id, width: f32) {
    let resizing = ctx
        .read_response(panel_id.with("__resize"))
        .is_some_and(|response| response.dragged());
    if resizing {
        return;
    }
    if let Some(panel) = egui::containers::panel::PanelState::load(ctx, panel_id) {
        if (panel.rect.width() - width).abs() > 1.0 {
            clear_explorer_panel_state(ctx, panel_id);
        }
    }
}

fn clear_explorer_panel_state(ctx: &egui::Context, panel_id: egui::Id) {
    ctx.data_mut(|data| {
        data.remove::<egui::containers::panel::PanelState>(panel_id);
    });
}

/// Double-click on the SidePanel resize divider (egui grab strip).
fn explorer_divider_double_clicked(ui: &egui::Ui, panel_rect: egui::Rect) -> bool {
    let grab = ui.style().interaction.resize_grab_radius_side;
    let resize_x = panel_rect.left();
    let resize_rect = egui::Rect::from_x_y_ranges(
        (resize_x - grab)..=(resize_x + grab),
        panel_rect.y_range(),
    );
    ui.input(|input| {
        input
            .pointer
            .button_double_clicked(egui::PointerButton::Primary)
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| resize_rect.contains(pos))
    })
}

fn truncate_path(path: &str, max_chars: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_chars {
        return path.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!(
        "…{}",
        chars[chars.len() - keep..].iter().collect::<String>()
    )
}

/// Problems panel — clickable, aggregated issues from `CodingState.problems`.
///
/// Built by [`crate::coding_workspace`] rendering aggregated
/// [`jaymi_capabilities::ProblemsRegistry`] output only — this panel never
/// talks to individual sources (LSP, Planner, Workspace, Permissions, Search,
/// Memory) directly.
fn render_problems_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: Option<&CodingState>,
    events: &mut Vec<CodingShellEvent>,
) {
    let count = state.map(|state| state.problems.len()).unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{count} problem{}",
                if count == 1 { "" } else { "s" }
            ))
            .strong()
            .color(theme.text_primary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Refresh")
                .on_hover_text("Recompute Problems")
                .clicked()
            {
                events.push(CodingShellEvent::ProblemsRefresh);
            }
        });
    });
    ui.add_space(space::XS);

    let Some(state) = state else {
        ui.label(egui::RichText::new("No problems detected.").color(theme.text_secondary));
        return;
    };

    if state.problems.is_empty() {
        ui.label(egui::RichText::new("No problems detected.").color(theme.text_secondary));
    } else {
        for issue in &state.problems {
            render_problem_row(ui, theme, issue, events);
        }
    }
}

/// Workspace Diagnostics tab — read-only operational status sections.
fn render_workspace_diagnostics_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    diagnostics: Option<&CodingDiagnosticsView>,
) {
    let Some(view) = diagnostics else {
        ui.label(egui::RichText::new("Workspace information.").color(theme.text_secondary));
        return;
    };
    if view.sections.is_empty() {
        ui.label(egui::RichText::new("Workspace information.").color(theme.text_secondary));
        return;
    }
    for section in &view.sections {
        ui.label(
            egui::RichText::new(&section.title)
                .strong()
                .color(theme.text_primary),
        );
        if section.lines.is_empty() {
            ui.label(egui::RichText::new("—").color(theme.text_secondary));
        } else {
            for line in &section.lines {
                ui.label(egui::RichText::new(line).color(theme.text_primary));
            }
        }
        ui.add_space(space::SM);
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

fn severity_color(theme: &Theme, severity: ProblemSeverity) -> egui::Color32 {
    match severity {
        ProblemSeverity::Error => theme.error,
        ProblemSeverity::Warning => theme.warning,
        ProblemSeverity::Info => theme.accent,
        ProblemSeverity::Hint => theme.text_secondary,
    }
}

/// One clickable Problems row: severity · source_label · file:line · message.
fn render_problem_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    issue: &ProblemIssue,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            severity_color(theme, issue.severity),
            severity_icon(issue.severity),
        );
        ui.label(egui::RichText::new(&issue.source_label).color(theme.text_secondary));
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
            ui.label(egui::RichText::new(text).color(theme.text_primary));
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
    theme: &Theme,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    available_height: f32,
    show_pane_tabs: bool,
) {
    if state.editors.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space((available_height * 0.28).clamp(space::LG, space::XL * 2.0 + space::SM));
            ui.label(
                egui::RichText::new("No open files")
                    .size(type_size::BODY)
                    .color(theme.text_primary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("Select a file in Explorer.")
                    .size(type_size::UI)
                    .color(theme.text_secondary),
            );
        });
        return;
    }

    if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::S)) {
        events.push(CodingShellEvent::SaveActive);
    }

    // Hierarchy: (optional per-pane Tabs) → Monaco → Status Bar.
    // Single-pane tabs live in the workspace strip above this column.
    render_layout_node(
        ui,
        theme,
        &state.editors.layout,
        state,
        events,
        monaco_out,
        available_height,
        &[],
        show_pane_tabs,
    );
}

/// Render one node of the split layout tree (leaf pane or nested split).
#[allow(clippy::too_many_arguments)]
fn render_layout_node(
    ui: &mut egui::Ui,
    theme: &Theme,
    node: &EditorLayoutNode,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    height: f32,
    node_path: &[usize],
    show_pane_tabs: bool,
) {
    match node {
        EditorLayoutNode::Leaf { pane } => {
            render_pane(
                ui,
                theme,
                pane,
                state,
                events,
                monaco_out,
                height,
                show_pane_tabs,
            );
        }
        EditorLayoutNode::Split {
            direction,
            sizes,
            children,
        } => {
            let side_by_side = matches!(direction, SplitDirection::Vertical);
            render_split(
                ui,
                theme,
                children,
                sizes,
                state,
                events,
                monaco_out,
                height,
                node_path,
                side_by_side,
                show_pane_tabs,
            );
        }
    }
}

/// Render a split's children with a draggable resize divider between each pair.
#[allow(clippy::too_many_arguments)]
fn render_split(
    ui: &mut egui::Ui,
    theme: &Theme,
    children: &[EditorLayoutNode],
    sizes: &[f32],
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    height: f32,
    node_path: &[usize],
    side_by_side: bool,
    show_pane_tabs: bool,
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
                        theme,
                        child,
                        state,
                        events,
                        monaco_out,
                        child_height,
                        &child_path,
                        show_pane_tabs,
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
                    theme.selection()
                } else {
                    theme.border
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

/// Render a single editor pane: optional tab strip + Monaco/body + status.
#[allow(clippy::too_many_arguments)]
fn render_pane(
    ui: &mut egui::Ui,
    theme: &Theme,
    pane_id: &EditorPaneId,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
    monaco_out: &mut Option<MonacoEditorSurface>,
    height: f32,
    show_tabs: bool,
) {
    let pane_str = pane_id.as_str().to_string();
    let is_focused = state.editors.focused_pane == *pane_id;
    let sessions = state.editors.sessions_in_pane(pane_id);
    let active_path = state
        .editors
        .active_session_in_pane(pane_id)
        .map(|session| session.path);

    ui.set_min_height(height.max(20.0));
    ui.set_max_height(height.max(20.0));
    ui.set_min_width(ui.available_width());

    let mut strip_height = 0.0_f32;
    if show_tabs {
        // Per-pane tabs only when the editor is split into multiple panes.
        let strip_response = ui
            .scope(|ui| {
                egui::ScrollArea::horizontal()
                    .id_salt(("editor_tab_strip", &pane_str))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::XS;
                            if sessions.is_empty() {
                                ui.label(
                                    egui::RichText::new("(empty pane)").color(theme.text_secondary),
                                );
                            }
                            for session in &sessions {
                                render_pane_tab(
                                    ui,
                                    theme,
                                    &pane_str,
                                    &active_path,
                                    session,
                                    events,
                                );
                            }
                        });
                    });
            })
            .response;
        strip_height = strip_response.rect.height();

        if is_focused {
            ui.painter().line_segment(
                [
                    strip_response.rect.left_bottom(),
                    strip_response.rect.right_bottom(),
                ],
                egui::Stroke::new(stroke::HAIRLINE, theme.accent),
            );
        }

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
    }

    let status_h = 22.0_f32;
    let body_h = (height - strip_height - status_h - stroke::HAIRLINE).max(40.0);

    match active_path {
        None => {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), body_h),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add_space(space::MD);
                    ui.label(
                        egui::RichText::new("No open tabs in this pane")
                            .size(type_size::BODY)
                            .color(theme.text_secondary),
                    );
                },
            );
            render_editor_status_bar(ui, theme, status_h, |ui| {
                ui.label(
                    egui::RichText::new("Ready")
                        .size(type_size::META)
                        .color(theme.text_secondary),
                );
            });
        }
        Some(path) => {
            if let Some(session) = sessions.into_iter().find(|session| session.path == path) {
                render_pane_body(
                    ui, &pane_str, is_focused, &session, state, events, monaco_out, body_h,
                );
                render_editor_status_bar(ui, theme, status_h, |ui| {
                    let language = language_for_path(&session.path);
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  ·  Ln {}, Col {}  ·  {}",
                            session.name,
                            session.view.cursor.line + 1,
                            session.view.cursor.column + 1,
                            language
                        ))
                        .size(type_size::META)
                        .color(theme.text_secondary),
                    );
                    if session.dirty {
                        ui.label(
                            egui::RichText::new("· modified")
                                .size(type_size::META)
                                .color(theme.warning),
                        );
                    }
                    if session.preview {
                        ui.label(
                            egui::RichText::new("· preview")
                                .size(type_size::META)
                                .color(theme.text_secondary),
                        );
                    }
                });
            }
        }
    }
}

fn render_editor_status_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    height: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(stroke::HAIRLINE, theme.border),
    );
    egui::Frame::new()
        .fill(theme.surface_alt)
        .inner_margin(inset(space::SM, 0.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(height);
                add_contents(ui);
            });
        });
}

/// Render one editor tab: click to activate, middle-click / hover ✕ to close.
fn render_pane_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
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

    let close_w = 18.0;
    let pad_x = space::SM;
    let measure = ui.fonts(|f| {
        f.layout_no_wrap(
            label.clone(),
            egui::FontId::proportional(type_size::UI),
            theme.text_primary,
        )
    });
    // Reserve close-button width so the tab doesn't jump when ✕ appears on hover.
    let tab_w = (pad_x + measure.size().x + space::XS + close_w + pad_x).max(72.0);
    let tab_h = EDITOR_TAB_STRIP_HEIGHT - 2.0;

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(tab_w, tab_h), egui::Sense::click_and_drag());

    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - pad_x - close_w * 0.5, rect.center().y),
        egui::vec2(close_w, close_w),
    );
    let close_id = ui.id().with(("tab_close", pane_str, session.path.as_str()));
    let close_response = ui.interact(close_rect, close_id, egui::Sense::click());
    let hovered = response.hovered() || close_response.hovered();
    let show_close = hovered;

    let bg = if active {
        theme.surface
    } else if hovered {
        theme.surface_alt
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), bg);
    }

    let text_color = if active || hovered {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    let galley = ui.fonts(|f| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &label,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(type_size::UI),
                color: text_color,
                italics: session.preview,
                ..Default::default()
            },
        );
        f.layout_job(job)
    });
    let text_pos = egui::pos2(rect.left() + pad_x, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, text_color);

    if show_close {
        let close_hovered = close_response.hovered();
        if close_hovered {
            ui.painter().rect_filled(
                close_rect,
                egui::CornerRadius::same(radius::PILL as u8),
                theme.selection(),
            );
        }
        ui.painter().text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(type_size::BODY),
            if close_hovered {
                theme.text_primary
            } else {
                theme.text_secondary
            },
        );
        close_response.clone().on_hover_text("Close tab");
    }

    response.dnd_set_drag_payload(TabDragPayload {
        pane: pane_str.to_string(),
        path: session.path.clone(),
    });

    if close_response.clicked() || response.middle_clicked() {
        events.push(CodingShellEvent::CloseTab {
            pane: pane_str.to_string(),
            path: session.path.clone(),
        });
    } else if response.clicked() {
        events.push(CodingShellEvent::ActivateTab {
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
    let settings = state.editor_settings.clone();

    // Monaco fills available space — no per-file header chrome above the buffer.
    // Allocate a *stable* pane rect first. Never bind the WebView to the TextEdit
    // content rect (`scroll.inner`): that grows with the document and jumps on
    // scroll, which makes the overlay flash / shoot to the top of the window.
    let editor_height = available_height.max(80.0);
    let desired = egui::vec2(ui.available_width(), editor_height);
    let (pane_rect, pane_response) =
        ui.allocate_exact_size(desired, egui::Sense::click().union(egui::Sense::hover()));

    if !is_focused && (pane_response.clicked() || pane_response.gained_focus()) {
        events.push(CodingShellEvent::FocusPane(pane_str.to_string()));
    }

    if is_focused {
        // Focused pane: Monaco owns the buffer. Skip the full-document TextEdit so
        // it cannot expand the layout or drag the native WebView bounds.
        let rect = pane_rect
            .intersect(ui.clip_rect())
            .intersect(ui.ctx().screen_rect());
        if rect.width() >= 2.0 && rect.height() >= 2.0 {
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
        return;
    }

    // Unfocused panes: egui TextEdit fallback, clipped to the reserved pane.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pane_rect));
    child.set_clip_rect(pane_rect.intersect(ui.clip_rect()));
    child.set_max_size(pane_rect.size());
    let scroll = egui::ScrollArea::vertical()
        .id_salt(("editor_scroll", pane_str, &path))
        .max_height(pane_rect.height())
        .auto_shrink([false, false])
        .vertical_scroll_offset(scroll_offset)
        .show(&mut child, |ui| {
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
            if response.clicked() || response.gained_focus() {
                events.push(CodingShellEvent::FocusPane(pane_str.to_string()));
            }
        });

    let new_offset = scroll.state.offset.y;
    if (new_offset - scroll_offset).abs() > f32::EPSILON {
        events.push(CodingShellEvent::Scroll {
            pane: pane_str.to_string(),
            path,
            offset: new_offset,
        });
    }
}

fn render_terminal(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
) {
    let active_id = state.active_terminal_id.clone();

    ui.horizontal_wrapped(|ui| {
        for session in &state.terminal_sessions {
            let is_active = active_id.as_deref() == Some(session.id.as_str());
            render_terminal_tab(ui, theme, session, is_active, events);
            ui.add_space(space::XS);
        }
        if ui
            .button("+ New")
            .on_hover_text("Open a new terminal")
            .clicked()
        {
            events.push(CodingShellEvent::TerminalCreate { title: None });
        }
    });
    ui.separator();

    if state.terminal_sessions.is_empty() {
        ui.label(egui::RichText::new("No terminal running.").color(theme.text_secondary));
        return;
    }

    let active_session = active_id
        .as_deref()
        .and_then(|id| {
            state
                .terminal_sessions
                .iter()
                .find(|session| session.id == id)
        })
        .or_else(|| state.terminal_sessions.first());

    if let Some(session) = active_session {
        render_terminal_session(ui, theme, session, events);
    }
}

/// One tab in the terminal tab strip: select / inline rename / close.
fn render_terminal_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    session: &TerminalSessionState,
    is_active: bool,
    events: &mut Vec<CodingShellEvent>,
) {
    let renaming_id = ui.id().with(("terminal_renaming", &session.id));
    let mut renaming = ui
        .data(|data| data.get_temp::<bool>(renaming_id))
        .unwrap_or(false);

    egui::Frame::new()
        .inner_margin(inset(space::SM, space::XS))
        .fill(if is_active {
            theme.selection()
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(radius::XS)
        .show(ui, |ui| {
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
                    let confirmed = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
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
                    let label = egui::RichText::new(&session.title).color(if is_active {
                        theme.text_primary
                    } else {
                        theme.text_secondary
                    });
                    if ui.selectable_label(is_active, label).clicked() {
                        events.push(CodingShellEvent::TerminalSelect {
                            session_id: session.id.clone(),
                        });
                    }
                    if ui
                        .small_button("Rename")
                        .on_hover_text("Rename terminal")
                        .clicked()
                    {
                        renaming = true;
                    }
                    if ui
                        .small_button("Close")
                        .on_hover_text("Close terminal")
                        .clicked()
                    {
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
    theme: &Theme,
    session: &TerminalSessionState,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&session.title)
                .strong()
                .size(type_size::UI)
                .color(theme.text_primary),
        );
        if let Some(cwd) = session.cwd.as_deref().filter(|value| !value.is_empty()) {
            ui.label(
                egui::RichText::new(truncate_path(cwd, 48))
                    .size(type_size::META)
                    .color(theme.text_secondary),
            )
            .on_hover_text(cwd);
        }
    });
    ui.add_space(space::XS);

    // Reserve the command row first so ScrollArea cannot eat the whole dock
    // height (common with min_scrolled_height ≈ panel height → zero-height input).
    const INPUT_ROW_H: f32 = 28.0;
    let gap = space::XS;
    let output_h = (ui.available_height() - INPUT_ROW_H - gap).max(48.0);

    // The terminal is always a dark "ink" surface, like Monaco's chrome —
    // independent of the app's own light/dark mode. Pull text colors from
    // the dark palette regardless of which theme is active.
    let ink_theme = Theme::dark();
    let mut clicked_transcript = false;
    egui::Frame::new()
        .fill(theme.ink)
        .corner_radius(radius::LG)
        .inner_margin(inset(space::SM, space::XS))
        .show(ui, |ui| {
            let scroll = egui::ScrollArea::vertical()
                .id_salt(("terminal_scroll", &session.id))
                .vertical_scroll_offset(session.scroll_offset)
                .auto_shrink([false, false])
                .max_height(output_h)
                .min_scrolled_height(48.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    let output = if session.output.is_empty() {
                        "(no output yet — type a command below and press Enter)"
                    } else {
                        session.output.as_str()
                    };
                    let label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(output)
                                .monospace()
                                .color(if session.output.is_empty() {
                                    ink_theme.text_secondary
                                } else {
                                    ink_theme.text_primary
                                }),
                        )
                        .wrap()
                        .selectable(true)
                        .sense(egui::Sense::click()),
                    );
                    // Clicking the transcript focuses the command field.
                    if label.clicked() {
                        clicked_transcript = true;
                    }
                });
            let new_offset = scroll.state.offset.y;
            if (new_offset - session.scroll_offset).abs() > f32::EPSILON {
                events.push(CodingShellEvent::TerminalScroll {
                    session_id: session.id.clone(),
                    offset: new_offset,
                });
            }
        });
    if clicked_transcript {
        events.push(CodingShellEvent::TerminalFocusInput {
            session_id: session.id.clone(),
        });
    }

    ui.add_space(gap);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), INPUT_ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(egui::RichText::new("$").monospace().color(theme.text_secondary));
            let run_w = 52.0;
            let edit_w = (ui.available_width() - run_w - space::SM).max(80.0);
            let mut draft = session.input.clone();
            let response = ui.add_sized(
                [edit_w, INPUT_ROW_H - 4.0],
                egui::TextEdit::singleline(&mut draft)
                    .id_salt(("terminal_input", &session.id))
                    .font(egui::TextStyle::Monospace)
                    .hint_text("cargo test · git status · npm test · …"),
            );
            if response.changed() {
                events.push(CodingShellEvent::TerminalInput {
                    session_id: session.id.clone(),
                    input: draft.clone(),
                });
            }
            if response.clicked() || response.gained_focus() || response.has_focus() {
                // Signal host to release Monaco WKWebView first-responder so keys
                // reach this TextEdit (native child WebView otherwise eats typing).
                events.push(CodingShellEvent::TerminalWantsKeyboard);
            }
            // External request to focus (click output / open Terminal dock).
            let focus_id = egui::Id::new(("terminal_request_focus", &session.id));
            let wants_focus = ui.data(|data| data.get_temp::<bool>(focus_id).unwrap_or(false));
            if wants_focus {
                response.request_focus();
                ui.data_mut(|data| data.insert_temp(focus_id, false));
                events.push(CodingShellEvent::TerminalWantsKeyboard);
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
            let submit = (response.has_focus()
                && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                || crate::ui::components::mini_pill_button(
                    ui,
                    theme,
                    "Run",
                    crate::ui::components::ButtonStyle::Primary,
                    true,
                )
                .clicked();
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
        },
    );
}

fn render_git(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
) {
    let Some(git) = &state.git else {
        ui.label(
            egui::RichText::new("Open a project that contains a Git repository.")
                .color(theme.text_secondary),
        );
        return;
    };

    ui.horizontal(|ui| {
        if git.is_repository {
            ui.label(
                egui::RichText::new(format!(
                    "branch {}",
                    git.branch.as_deref().unwrap_or("(unknown)")
                ))
                .strong()
                .color(theme.text_primary),
            );
            ui.label(egui::RichText::new(&git.summary).color(theme.text_secondary));
        } else {
            ui.label(
                egui::RichText::new("Not a Git repository")
                    .strong()
                    .color(theme.text_primary),
            );
            ui.label(egui::RichText::new(&git.summary).color(theme.text_secondary));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::components::mini_pill_button(
                ui,
                theme,
                "Refresh",
                crate::ui::components::ButtonStyle::Secondary,
                true,
            )
            .clicked()
            {
                events.push(CodingShellEvent::GitRefresh);
            }
        });
    });
    if let Some(root) = &git.repo_root {
        ui.label(egui::RichText::new(root).color(theme.text_secondary));
    }
    if let Some(error) = &git.last_error {
        ui.colored_label(theme.error, error);
    }

    if let Some(pending) = &git.pending_discard {
        ui.group(|ui| {
            ui.colored_label(
                theme.warning,
                format!(
                    "Discard changes to {}? This cannot be undone.",
                    pending.join(", ")
                ),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                if crate::ui::components::mini_pill_button(
                    ui,
                    theme,
                    "Confirm Discard",
                    crate::ui::components::ButtonStyle::Primary,
                    true,
                )
                .clicked()
                {
                    events.push(CodingShellEvent::GitDiscardConfirm);
                }
                if crate::ui::components::mini_pill_button(
                    ui,
                    theme,
                    "Cancel",
                    crate::ui::components::ButtonStyle::Ghost,
                    true,
                )
                .clicked()
                {
                    events.push(CodingShellEvent::GitDiscardCancel);
                }
            });
        });
        ui.separator();
    }

    if !git.is_repository {
        ui.label(
            egui::RichText::new("This project folder is not a Git repository.")
                .color(theme.text_secondary),
        );
        return;
    }

    render_git_section(
        ui,
        theme,
        "Staged",
        &git.staged,
        GitSectionActions::Unstage,
        events,
    );
    render_git_section(
        ui,
        theme,
        "Modified",
        &git.modified,
        GitSectionActions::StageAndDiscard,
        events,
    );
    render_git_section(
        ui,
        theme,
        "Added",
        &git.added,
        GitSectionActions::Unstage,
        events,
    );
    render_git_section(
        ui,
        theme,
        "Deleted",
        &git.deleted,
        GitSectionActions::StageAndDiscard,
        events,
    );
    render_git_section(
        ui,
        theme,
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
    if crate::ui::components::mini_pill_button(
        ui,
        theme,
        "Commit",
        crate::ui::components::ButtonStyle::Primary,
        can_commit,
    )
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
    theme: &Theme,
    title: &str,
    entries: &[jaymi_capabilities::GitFileEntry],
    actions: GitSectionActions,
    events: &mut Vec<CodingShellEvent>,
) {
    ui.add_space(space::XS);
    ui.label(
        egui::RichText::new(format!("{title} ({})", entries.len()))
            .strong()
            .size(type_size::UI)
            .color(theme.text_primary),
    );
    if entries.is_empty() {
        ui.label(egui::RichText::new("(none)").color(theme.text_secondary));
        return;
    }
    for entry in entries {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{:>2} {}", entry.status, entry.path))
                    .monospace()
                    .color(theme.text_primary),
            );
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
fn render_search_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &CodingState,
    events: &mut Vec<CodingShellEvent>,
) {
    let panel = &state.search;

    ui.horizontal(|ui| {
        let mut query = panel.query.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut query)
                .desired_width(240.0)
                .hint_text("Search project files…"),
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
        let run_clicked = crate::ui::components::mini_pill_button(
            ui,
            theme,
            "Search",
            crate::ui::components::ButtonStyle::Primary,
            true,
        )
        .clicked()
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
        if crate::ui::components::mini_pill_button(
            ui,
            theme,
            "Replace All",
            crate::ui::components::ButtonStyle::Secondary,
            !panel.query.trim().is_empty() && !panel.filename_only,
        )
        .clicked()
        {
            events.push(CodingShellEvent::ReplaceAll);
        }
    });

    if !panel.status.is_empty() {
        ui.label(egui::RichText::new(&panel.status).color(theme.text_secondary));
    }

    ui.separator();
    if !panel.searching && panel.results.is_empty() {
        ui.add_space(space::XS);
        if panel.query.trim().is_empty() {
            ui.label(egui::RichText::new("Type to search").color(theme.text_secondary));
        } else {
            ui.label(egui::RichText::new("No results").color(theme.text_secondary));
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
        ui.label(
            egui::RichText::new(truncate_path(&result.preview, 120)).color(theme.text_secondary),
        );
        ui.add_space(space::XS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::{
        DiagnosticState, ExplorerNode, ExplorerState, ExplorerStatus, GitStatusState, OpenEditors,
        TerminalSessionState, WorkspacePanels,
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
                nodes: vec![ExplorerNode {
                    name: "src".into(),
                    path: "/tmp/app/src".into(),
                    is_dir: true,
                    children: vec![ExplorerNode {
                        name: "main.rs".into(),
                        path: "/tmp/app/src/main.rs".into(),
                        is_dir: false,
                        children: Vec::new(),
                    }],
                }],
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
            panels: WorkspacePanels {
                active: CodingBottomTab::Terminal,
                ..WorkspacePanels::default()
            },
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
