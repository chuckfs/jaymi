//! Temporary runtime state owned by an expanded capability workspace.
//!
//! Capability state is ephemeral. It disappears when the workspace closes
//! unless the caller explicitly promotes an entry elsewhere (conversation,
//! project memory, etc.).

use std::collections::{BTreeMap, BTreeSet};

use crate::editor::{
    EditorPaneId, EditorSettings, EditorViewState, EditorWorkspaceSnapshot, FoldedRegion,
    OpenEditors, SplitDirection, RECENTLY_OPENED_CAP,
};
use crate::{Capability, WorkspaceKind};

/// One node in the Project Explorer tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerNode {
    /// Display name (file or folder basename).
    pub name: String,
    /// Absolute path string.
    pub path: String,
    /// True when this node is a directory.
    pub is_dir: bool,
    /// Child nodes (directories first, then files; alphabetical).
    pub children: Vec<ExplorerNode>,
}

/// Status of the Project Explorer load.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExplorerStatus {
    /// Not loaded yet.
    #[default]
    Idle,
    /// Tree is ready to render.
    Ready,
    /// No open project / no root directory.
    NoProject,
    /// Load failed.
    Error(String),
}

/// Inline create / rename draft owned by the explorer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExplorerPending {
    /// No pending create/rename.
    #[default]
    None,
    /// Creating a file under `parent`.
    NewFile {
        /// Absolute parent directory.
        parent: String,
        /// Draft basename.
        draft_name: String,
    },
    /// Creating a folder under `parent`.
    NewFolder {
        /// Absolute parent directory.
        parent: String,
        /// Draft basename.
        draft_name: String,
    },
    /// Renaming an existing path.
    Rename {
        /// Absolute path being renamed.
        path: String,
        /// Draft basename.
        draft_name: String,
    },
}

/// Project Explorer state owned by the Coding workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExplorerState {
    /// Absolute project root path, when known.
    pub project_root: Option<String>,
    /// Root-level explorer nodes (children of the project root).
    pub nodes: Vec<ExplorerNode>,
    /// Paths currently expanded in the tree.
    pub expanded_paths: BTreeSet<String>,
    /// Selected path (file or folder); may differ from the active editor tab.
    pub selected_path: Option<String>,
    /// Explorer load status.
    pub status: ExplorerStatus,
    /// Inline create / rename draft.
    pub pending: ExplorerPending,
}

impl ExplorerState {
    /// Clear the tree and mark no project.
    pub fn clear_no_project(&mut self) {
        self.project_root = None;
        self.nodes.clear();
        self.expanded_paths.clear();
        self.selected_path = None;
        self.status = ExplorerStatus::NoProject;
        self.pending = ExplorerPending::None;
    }

    /// Toggle expand/collapse for a directory path.
    pub fn toggle_expanded(&mut self, path: &str) {
        if !self.expanded_paths.remove(path) {
            self.expanded_paths.insert(path.to_string());
        }
    }

    /// Expand every ancestor directory of `path` under the project root.
    pub fn expand_ancestors_of(&mut self, path: &str) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let mut current = std::path::Path::new(path);
        while let Some(parent) = current.parent() {
            let parent_key = parent.to_string_lossy().into_owned();
            if parent_key == root {
                break;
            }
            if parent_key.starts_with(&root) {
                self.expanded_paths.insert(parent_key);
            } else {
                break;
            }
            current = parent;
        }
    }

    /// Begin an inline new-file draft under `parent`.
    pub fn begin_new_file(&mut self, parent: impl Into<String>) {
        self.pending = ExplorerPending::NewFile {
            parent: parent.into(),
            draft_name: "untitled.txt".into(),
        };
    }

    /// Begin an inline new-folder draft under `parent`.
    pub fn begin_new_folder(&mut self, parent: impl Into<String>) {
        self.pending = ExplorerPending::NewFolder {
            parent: parent.into(),
            draft_name: "new-folder".into(),
        };
    }

    /// Begin an inline rename draft for `path`.
    pub fn begin_rename(&mut self, path: impl Into<String>, current_name: impl Into<String>) {
        self.pending = ExplorerPending::Rename {
            path: path.into(),
            draft_name: current_name.into(),
        };
    }

    /// Update the draft name for the active pending action.
    pub fn set_pending_draft(&mut self, draft_name: String) {
        match &mut self.pending {
            ExplorerPending::NewFile {
                draft_name: draft, ..
            }
            | ExplorerPending::NewFolder {
                draft_name: draft, ..
            }
            | ExplorerPending::Rename {
                draft_name: draft, ..
            } => {
                *draft = draft_name;
            }
            ExplorerPending::None => {}
        }
    }

    /// Clear any pending create/rename.
    pub fn clear_pending(&mut self) {
        self.pending = ExplorerPending::None;
    }
}

/// One open file in a coding workspace (legacy summary; prefer [`EditorSession`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileState {
    /// Filesystem path.
    pub path: String,
    /// True when the buffer has unsaved edits.
    pub dirty: bool,
}

/// One terminal session in a coding workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSessionState {
    /// Stable session id for this workspace lifetime.
    pub id: String,
    /// Display title (tab label).
    pub title: String,
    /// Working directory, when known.
    pub cwd: Option<String>,
    /// Last command preview.
    pub last_command: Option<String>,
    /// Full scrollback buffer rendered by the UI.
    pub output: String,
    /// Command history (oldest first) for Up/Down navigation.
    pub history: Vec<String>,
    /// Current draft input line.
    pub input: String,
    /// Index into history while navigating (`None` = editing a new line).
    pub history_index: Option<usize>,
    /// Vertical scroll offset for the output pane.
    pub scroll_offset: f32,
}

impl Eq for TerminalSessionState {}

impl TerminalSessionState {
    /// Create a new empty session bound to an optional cwd, with a default title.
    pub fn new(id: impl Into<String>, cwd: Option<String>) -> Self {
        let id = id.into();
        Self::with_title(id, None, cwd)
    }

    /// Create a new empty session with an explicit (or default) title.
    pub fn with_title(id: impl Into<String>, title: Option<String>, cwd: Option<String>) -> Self {
        let id = id.into();
        let title = title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Terminal".to_string());
        Self {
            id,
            title,
            cwd,
            last_command: None,
            output: String::new(),
            history: Vec::new(),
            input: String::new(),
            history_index: None,
            scroll_offset: 0.0,
        }
    }

    /// Apply a successful terminal tool result into UI state.
    pub fn apply_result(
        &mut self,
        cwd: Option<String>,
        last_command: Option<String>,
        scrollback: String,
        history: Vec<String>,
    ) {
        if let Some(cwd) = cwd {
            self.cwd = Some(cwd);
        }
        if let Some(command) = last_command {
            self.last_command = Some(command);
        }
        self.output = scrollback;
        self.history = history;
        self.input.clear();
        self.history_index = None;
    }

    /// Move draft input to the previous history entry.
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_index {
            None => self.history.len().saturating_sub(1),
            Some(0) => 0,
            Some(index) => index.saturating_sub(1),
        };
        self.history_index = Some(next);
        self.input = self.history[next].clone();
    }

    /// Move draft input to the next history entry (or clear at the end).
    pub fn history_down(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 >= self.history.len() {
            self.history_index = None;
            self.input.clear();
            return;
        }
        let next = index + 1;
        self.history_index = Some(next);
        self.input = self.history[next].clone();
    }
}

/// One diagnostic entry in a coding workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticState {
    /// Human-readable message.
    pub message: String,
    /// Related path, when any.
    pub path: Option<String>,
    /// Severity label (`error`, `warning`, `info`, …).
    pub severity: String,
    /// Source id / label (e.g. `rust-analyzer`, `planner`).
    pub source: String,
    /// Zero-based start line, when known.
    pub line: Option<u32>,
    /// Zero-based start character, when known.
    pub character: Option<u32>,
    /// Zero-based end line, when known.
    pub end_line: Option<u32>,
    /// Zero-based end character, when known.
    pub end_character: Option<u32>,
}

impl DiagnosticState {
    /// Create a message-only diagnostic (no range).
    pub fn simple(
        message: impl Into<String>,
        path: Option<String>,
        severity: impl Into<String>,
    ) -> Self {
        Self {
            message: message.into(),
            path,
            severity: severity.into(),
            source: String::new(),
            line: None,
            character: None,
            end_line: None,
            end_character: None,
        }
    }
}

/// Live Git status for the coding workspace shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitFileEntry {
    /// Repository-relative path.
    pub path: String,
    /// Short status label (`M`, `A`, `??`, …).
    pub status: String,
}

/// Live Git status for the coding workspace shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatusState {
    /// Whether the project root is inside a Git work tree.
    pub is_repository: bool,
    /// Absolute repository toplevel, when detected.
    pub repo_root: Option<String>,
    /// Current branch name, when known.
    pub branch: Option<String>,
    /// Short status summary (e.g. "clean", "2 modified").
    pub summary: String,
    /// Unstaged worktree modifications (not deletes).
    pub modified: Vec<GitFileEntry>,
    /// Newly staged (added) files.
    pub added: Vec<GitFileEntry>,
    /// Deleted files (worktree and/or index).
    pub deleted: Vec<GitFileEntry>,
    /// Staged index changes.
    pub staged: Vec<GitFileEntry>,
    /// Untracked paths.
    pub untracked: Vec<GitFileEntry>,
    /// Draft commit message for the panel.
    pub commit_message: String,
    /// Paths awaiting discard confirmation (UI).
    pub pending_discard: Option<Vec<String>>,
    /// Last error from a Git operation, when any.
    pub last_error: Option<String>,
}

impl GitStatusState {
    /// Apply a refreshed status snapshot from the Git tool.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_snapshot(
        &mut self,
        is_repository: bool,
        repo_root: Option<String>,
        branch: Option<String>,
        summary: String,
        modified: Vec<GitFileEntry>,
        added: Vec<GitFileEntry>,
        deleted: Vec<GitFileEntry>,
        staged: Vec<GitFileEntry>,
        untracked: Vec<GitFileEntry>,
    ) {
        self.is_repository = is_repository;
        self.repo_root = repo_root;
        self.branch = branch;
        self.summary = summary;
        self.modified = modified;
        self.added = added;
        self.deleted = deleted;
        self.staged = staged;
        self.untracked = untracked;
        self.pending_discard = None;
        self.last_error = None;
    }
}

/// Which bottom auxiliary panel is visible in the Coding shell (VS Code-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CodingBottomTab {
    /// Bottom panel collapsed — tab strip remains (≈32px); editor fills the rest.
    #[default]
    Hidden,
    /// Integrated terminal.
    Terminal,
    /// Aggregated Problems (LSP / Planner / workspace / …).
    Problems,
    /// Find in Files / project search + replace.
    Search,
    /// Git status / stage / commit.
    Git,
    /// Workspace operational diagnostics (read-only status).
    Diagnostics,
}

impl CodingBottomTab {
    /// Short tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hidden => "",
            Self::Terminal => "Terminal",
            Self::Problems => "Problems",
            Self::Search => "Search",
            Self::Git => "Git",
            Self::Diagnostics => "Diagnostics",
        }
    }

    /// Stable id for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Terminal => "terminal",
            Self::Problems => "problems",
            Self::Search => "search",
            Self::Git => "git",
            Self::Diagnostics => "diagnostics",
        }
    }

    /// Parse a persisted tab id.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "hidden" | "" => Some(Self::Hidden),
            "terminal" => Some(Self::Terminal),
            "problems" => Some(Self::Problems),
            // Legacy snapshot / command id used before Problems was split out.
            "diagnostics-legacy" => Some(Self::Problems),
            "search" => Some(Self::Search),
            "git" => Some(Self::Git),
            "diagnostics" => Some(Self::Diagnostics),
            _ => None,
        }
    }
}

/// One row in the Find-in-Files results list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResultEntry {
    /// Absolute file path.
    pub path: String,
    /// Display title (filename or document title).
    pub title: String,
    /// Zero-based start line of the match, when known.
    pub line: Option<u32>,
    /// Zero-based start column of the match, when known.
    pub column: Option<u32>,
    /// Zero-based end line of the match, when known.
    pub end_line: Option<u32>,
    /// Zero-based end column of the match, when known.
    pub end_column: Option<u32>,
    /// One-line preview / snippet around the match.
    pub preview: String,
    /// Human-readable explanation of why this result matched.
    pub why_matched: String,
}

/// Find in Files / project search panel state owned by the Coding workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchPanelState {
    /// Current search query.
    pub query: String,
    /// Replacement text for Replace All.
    pub replace_text: String,
    /// Treat `query` as a regular expression.
    pub use_regex: bool,
    /// Case-sensitive matching.
    pub case_sensitive: bool,
    /// Match whole words only.
    pub whole_word: bool,
    /// Restrict matching to filenames only (skip file bodies).
    pub filename_only: bool,
    /// Ranked results from the last run.
    pub results: Vec<SearchResultEntry>,
    /// Status line (result count, error, or progress message).
    pub status: String,
    /// True while a search is in flight.
    pub searching: bool,
}

/// Default Project Explorer column width (points).
pub const DEFAULT_EXPLORER_WIDTH: f32 = 260.0;
/// Minimum Project Explorer column width.
pub const MIN_EXPLORER_WIDTH: f32 = 180.0;
/// Maximum Project Explorer column width.
pub const MAX_EXPLORER_WIDTH: f32 = 400.0;
/// Default bottom auxiliary panel height when expanded (points).
pub const DEFAULT_BOTTOM_PANEL_HEIGHT: f32 = 180.0;
/// Minimum bottom auxiliary panel height when expanded.
pub const MIN_BOTTOM_PANEL_HEIGHT: f32 = 96.0;
/// Maximum bottom auxiliary panel height.
pub const MAX_BOTTOM_PANEL_HEIGHT: f32 = 420.0;
/// Collapsed bottom chrome height (tab strip only).
pub const COLLAPSED_BOTTOM_TAB_HEIGHT: f32 = 32.0;
/// Default Coding side-panel width fallback when window size is unknown.
pub const DEFAULT_WORKSPACE_PANEL_WIDTH: f32 = 770.0;
/// Soft floor for Coding side-panel width (also clamped by conversation max 45%).
pub const MIN_WORKSPACE_PANEL_WIDTH: f32 = 280.0;
/// Absolute ceiling for Coding side-panel width (also clamped by conversation min).
pub const MAX_WORKSPACE_PANEL_WIDTH: f32 = 1400.0;
/// Minimum conversation (central) column width — Coding cannot steal past this.
pub const MIN_CONVERSATION_WIDTH: f32 = 340.0;
/// Default conversation fraction of the window (workspace takes the rest).
pub const DEFAULT_CONVERSATION_FRACTION: f32 = 0.30;
/// Maximum conversation fraction of the window.
pub const MAX_CONVERSATION_FRACTION: f32 = 0.45;

/// Temporary state for the Coding workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct CodingState {
    /// Project Explorer (tree, selection, expand, pending create/rename).
    pub explorer: ExplorerState,
    /// Open editors / tab strip (workspace-owned sessions).
    pub editors: OpenEditors,
    /// Editor chrome preferences (minimap, wrap, font) — workspace-owned.
    pub editor_settings: EditorSettings,
    /// Whether the Project Explorer column is visible.
    pub explorer_visible: bool,
    /// Project Explorer column width in points (user-resizable).
    pub explorer_width: f32,
    /// Bottom auxiliary panel height in points when a tab is open (user-resizable).
    pub bottom_panel_height: f32,
    /// Remembered Coding side-panel width (conversation ↔ workspace divider).
    pub workspace_panel_width: f32,
    /// Active terminal sessions.
    pub terminal_sessions: Vec<TerminalSessionState>,
    /// Session id of the terminal tab currently in focus, when any.
    pub active_terminal_id: Option<String>,
    /// Live Git status for the shell, when set.
    pub git: Option<GitStatusState>,
    /// LSP working-set diagnostics (fed into the Problems registry as the `lsp` source).
    pub diagnostics: Vec<DiagnosticState>,
    /// Aggregated Problems panel issues from [`crate::ProblemsRegistry`].
    pub problems: Vec<crate::ProblemIssue>,
    /// Bottom auxiliary panel tab (Terminal / Git / Problems / Search), or hidden.
    pub bottom_tab: CodingBottomTab,
    /// Find in Files / project search panel state.
    pub search: SearchPanelState,
}

impl Default for CodingState {
    fn default() -> Self {
        Self {
            explorer: ExplorerState::default(),
            editors: OpenEditors::default(),
            editor_settings: EditorSettings::default(),
            explorer_visible: true,
            explorer_width: DEFAULT_EXPLORER_WIDTH,
            bottom_panel_height: DEFAULT_BOTTOM_PANEL_HEIGHT,
            workspace_panel_width: DEFAULT_WORKSPACE_PANEL_WIDTH,
            terminal_sessions: Vec::new(),
            active_terminal_id: None,
            git: None,
            diagnostics: Vec::new(),
            problems: Vec::new(),
            bottom_tab: CodingBottomTab::default(),
            search: SearchPanelState::default(),
        }
    }
}

impl Eq for CodingState {}

impl CodingState {
    /// Number of tracked entries across explorer, tabs, terminals, git, and diagnostics.
    pub fn entry_count(&self) -> usize {
        count_explorer_nodes(&self.explorer.nodes)
            + self.editors.len()
            + self.terminal_sessions.len()
            + self.diagnostics.len()
            + self.problems.len()
            + usize::from(self.git.is_some())
            + usize::from(self.explorer.selected_path.is_some())
    }

    /// Open editor files as a simple path/dirty list (compatibility helper).
    pub fn open_files(&self) -> Vec<OpenFileState> {
        self.editors.open_files()
    }

    /// Active editor path in the focused pane, when any.
    pub fn active_tab_path(&self) -> Option<&str> {
        self.editors
            .panes
            .get(self.editors.focused_pane.as_str())
            .and_then(|pane| pane.active_path.as_deref())
    }

    /// Focus an existing session by path in the focused pane (returns false when missing).
    pub fn focus_tab(&mut self, path: &str) -> bool {
        if !self.editors.activate_path(path) {
            // Try other panes — activate wherever the path is open.
            let pane_id = self.editors.panes.values().find_map(|pane| {
                pane.tabs
                    .iter()
                    .find(|tab| tab.path == path)
                    .map(|_| pane.id.clone())
            });
            let Some(pane_id) = pane_id else {
                return false;
            };
            if !self.editors.activate_path_in_pane(&pane_id, path) {
                return false;
            }
        }
        self.explorer.selected_path = Some(path.to_string());
        self.explorer.expand_ancestors_of(path);
        true
    }

    /// Open a permanent session (or refresh an existing one) and make it active.
    pub fn open_permanent(&mut self, path: &str, content: String) {
        self.editors.open_permanent(path, content);
        self.explorer.selected_path = Some(path.to_string());
        self.explorer.expand_ancestors_of(path);
    }

    /// Open a preview session (replaces any other preview) and make it active.
    pub fn open_preview(&mut self, path: &str, content: String) {
        self.editors.open_preview(path, content);
        self.explorer.selected_path = Some(path.to_string());
        self.explorer.expand_ancestors_of(path);
    }

    /// Insert or replace a permanent tab from legacy callers.
    ///
    /// Prefer [`Self::open_permanent`]. `scroll_offset` seeds view state.
    pub fn upsert_tab(&mut self, path: &str, name: &str, content: String, scroll_offset: f32) {
        self.open_permanent(path, content);
        if let Some(buffer) = self.editors.buffers.get_mut(path) {
            buffer.name = name.to_string();
        }
        let _ = self.editors.set_scroll_top(path, scroll_offset);
    }

    /// Close a session by path in the focused pane; activates a neighbor when needed.
    pub fn close_tab(&mut self, path: &str) -> bool {
        if !self.editors.close_path(path) {
            return false;
        }
        if let Some(active) = self.active_tab_path().map(str::to_string) {
            self.explorer.selected_path = Some(active.clone());
            self.explorer.expand_ancestors_of(&active);
        }
        true
    }

    /// Toggle whether a directory path is expanded.
    pub fn toggle_expanded(&mut self, path: &str) {
        self.explorer.toggle_expanded(path);
    }

    /// Update scroll offset for a session path in the focused pane.
    pub fn set_scroll_offset(&mut self, path: &str, offset: f32) {
        let _ = self.editors.set_scroll_top(path, offset);
    }

    /// Update cursor for a session path in the focused pane.
    pub fn set_cursor(&mut self, path: &str, line: u32, column: u32) {
        let _ = self.editors.set_cursor(path, line, column);
    }

    /// Update folded regions for a session path in the focused pane.
    pub fn set_folded_regions(&mut self, path: &str, folded_regions: Vec<FoldedRegion>) {
        let _ = self.editors.set_folded_regions(path, folded_regions);
    }

    /// Apply full view state for a session path in the focused pane (no content).
    pub fn set_view_state(&mut self, path: &str, view: EditorViewState) {
        let _ = self.editors.set_view_state(path, view);
    }

    /// Update editable content for a session path (promotes preview).
    pub fn set_tab_content(&mut self, path: &str, content: String) {
        let _ = self.editors.set_content(path, content);
    }

    /// Clear dirty after a successful save.
    pub fn mark_tab_clean(&mut self, path: &str) {
        let _ = self.editors.mark_clean(path);
    }

    /// Snapshot editor UI state for persistence (paths + view + settings; never contents).
    pub fn editor_workspace_snapshot(&self) -> EditorWorkspaceSnapshot {
        let mut snapshot = self.editors.snapshot(self.editor_settings.clone());
        snapshot.explorer_width = Some(self.explorer_width);
        snapshot.bottom_panel_height = Some(self.bottom_panel_height);
        snapshot.workspace_panel_width = Some(self.workspace_panel_width);
        snapshot.bottom_tab = Some(self.bottom_tab.as_str().to_string());
        snapshot
    }

    /// Apply preferences and MRU from a snapshot without opening buffers.
    pub fn apply_editor_workspace_meta(&mut self, snapshot: &EditorWorkspaceSnapshot) {
        self.editor_settings = snapshot.settings.clone();
        self.editors.recently_opened = snapshot.recently_opened.clone();
        self.editors.recently_opened.truncate(RECENTLY_OPENED_CAP);
        self.apply_shell_chrome_sizes(snapshot);
    }

    /// Apply full pane/layout structure from a snapshot (buffers still empty).
    pub fn apply_editor_workspace_structure(&mut self, snapshot: &EditorWorkspaceSnapshot) {
        self.editor_settings = snapshot.settings.clone();
        self.editors.apply_snapshot_structure(snapshot);
        self.apply_shell_chrome_sizes(snapshot);
    }

    fn apply_shell_chrome_sizes(&mut self, snapshot: &EditorWorkspaceSnapshot) {
        if let Some(width) = snapshot.explorer_width {
            self.explorer_width = width.clamp(MIN_EXPLORER_WIDTH, MAX_EXPLORER_WIDTH);
        }
        if let Some(height) = snapshot.bottom_panel_height {
            self.bottom_panel_height =
                height.clamp(MIN_BOTTOM_PANEL_HEIGHT, MAX_BOTTOM_PANEL_HEIGHT);
        }
        if let Some(width) = snapshot.workspace_panel_width {
            self.workspace_panel_width =
                width.clamp(MIN_WORKSPACE_PANEL_WIDTH, MAX_WORKSPACE_PANEL_WIDTH);
        }
        if let Some(tab) = snapshot
            .bottom_tab
            .as_deref()
            .and_then(CodingBottomTab::parse)
        {
            self.bottom_tab = tab;
        }
    }

    /// Clamp and store the Project Explorer column width.
    pub fn set_explorer_width(&mut self, width: f32) {
        self.explorer_width = width.clamp(MIN_EXPLORER_WIDTH, MAX_EXPLORER_WIDTH);
    }

    /// Clamp and store the bottom auxiliary panel height.
    pub fn set_bottom_panel_height(&mut self, height: f32) {
        self.bottom_panel_height = height.clamp(MIN_BOTTOM_PANEL_HEIGHT, MAX_BOTTOM_PANEL_HEIGHT);
    }

    /// Clamp and store the Coding side-panel width.
    pub fn set_workspace_panel_width(&mut self, width: f32) {
        self.workspace_panel_width =
            width.clamp(MIN_WORKSPACE_PANEL_WIDTH, MAX_WORKSPACE_PANEL_WIDTH);
    }

    /// Seed a shared buffer without changing pane tabs (restore helper).
    pub fn seed_editor_buffer(&mut self, path: &str, content: String) {
        use crate::editor::EditorBuffer;
        if let Some(existing) = self.editors.buffers.get_mut(path) {
            if !existing.dirty {
                existing.content = content;
            }
            return;
        }
        self.editors
            .buffers
            .insert(path.to_string(), EditorBuffer::new(path, content));
    }

    /// Drop all open editor sessions (used before restoring from disk).
    pub fn clear_editors(&mut self) {
        self.editors = OpenEditors::default();
    }

    /// Split the focused editor pane.
    pub fn split_editor(&mut self, direction: SplitDirection) -> Option<EditorPaneId> {
        self.editors.split(direction)
    }

    /// Close an editor pane (no-op when it is the only pane).
    pub fn close_editor_pane(&mut self, pane_id: &EditorPaneId) -> bool {
        self.editors.close_pane(pane_id)
    }

    /// Push a new terminal session; marks it active when no session is active yet.
    pub fn push_terminal_session(&mut self, session: TerminalSessionState) {
        let id = session.id.clone();
        self.terminal_sessions.push(session);
        if self.active_terminal_id.is_none() {
            self.active_terminal_id = Some(id);
        }
    }

    /// Remove a terminal session by id; picks a new active session when needed.
    pub fn remove_terminal_session(&mut self, session_id: &str) -> bool {
        let before = self.terminal_sessions.len();
        self.terminal_sessions
            .retain(|session| session.id != session_id);
        let removed = self.terminal_sessions.len() != before;
        if removed && self.active_terminal_id.as_deref() == Some(session_id) {
            self.active_terminal_id = self
                .terminal_sessions
                .first()
                .map(|session| session.id.clone());
        }
        removed
    }

    /// Select an existing terminal session as active (no-op when missing).
    pub fn select_terminal(&mut self, session_id: &str) -> bool {
        if self
            .terminal_sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            self.active_terminal_id = Some(session_id.to_string());
            true
        } else {
            false
        }
    }
}

fn count_explorer_nodes(nodes: &[ExplorerNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + count_explorer_nodes(&node.children))
        .sum()
}

/// File extensions / names the Coding Editor opens as editable text.
pub fn is_editable_coding_extension(path: &str) -> bool {
    let path = std::path::Path::new(path);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "dockerfile"
            | "makefile"
            | "gnumakefile"
            | "cmakelists.txt"
            | "license"
            | "licence"
            | "readme"
            | "gemfile"
            | "rakefile"
            | "procfile"
            | "cargo.lock"
            | "gitignore"
            | "gitattributes"
            | "editorconfig"
    ) {
        return true;
    }
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "txt"
            | "md"
            | "markdown"
            | "rs"
            | "toml"
            | "json"
            | "jsonc"
            | "yaml"
            | "yml"
            | "lock"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "mjs"
            | "cjs"
            | "css"
            | "scss"
            | "html"
            | "htm"
            | "xml"
            | "svg"
            | "py"
            | "rb"
            | "go"
            | "java"
            | "kt"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "cs"
            | "swift"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "ps1"
            | "sql"
            | "graphql"
            | "proto"
            | "env"
            | "ini"
            | "cfg"
            | "conf"
            | "properties"
            | "gitignore"
            | "dockerignore"
            | "editorconfig"
            | "csv"
            | "log"
            | "ron"
            | "nix"
    )
}

/// Build a sorted explorer tree from a flat list of file entries under `root`.
///
/// Entries should already exclude hidden names and `.git`. Directories are
/// sorted before files; siblings are alphabetical.
pub fn build_explorer_tree(root: &str, entries: &[(String, String, bool)]) -> Vec<ExplorerNode> {
    // entries: (absolute_path, name, is_dir)
    let root = root.trim_end_matches('/').trim_end_matches('\\');
    let mut by_parent: BTreeMap<String, Vec<(String, String, bool)>> = BTreeMap::new();

    for (path, name, is_dir) in entries {
        let parent = std::path::Path::new(path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        by_parent
            .entry(parent)
            .or_default()
            .push((path.clone(), name.clone(), *is_dir));
    }

    fn build_children(
        parent: &str,
        by_parent: &BTreeMap<String, Vec<(String, String, bool)>>,
    ) -> Vec<ExplorerNode> {
        let mut children = by_parent.get(parent).cloned().unwrap_or_default();
        children.sort_by(|left, right| match (left.2, right.2) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left
                .1
                .to_ascii_lowercase()
                .cmp(&right.1.to_ascii_lowercase()),
        });
        children
            .into_iter()
            .map(|(path, name, is_dir)| ExplorerNode {
                name,
                path: path.clone(),
                is_dir,
                children: if is_dir {
                    build_children(&path, by_parent)
                } else {
                    Vec::new()
                },
            })
            .collect()
    }

    build_children(root, &by_parent)
}

/// One generated asset in a creation workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedAssetState {
    /// Stable asset id for this workspace lifetime.
    pub id: String,
    /// Asset kind (`image`, `mask`, …).
    pub kind: String,
    /// Optional URI / path.
    pub uri: Option<String>,
}

/// One canvas history step in a creation workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasHistoryState {
    /// Stable step id.
    pub id: String,
    /// Short summary of the canvas change.
    pub summary: String,
}

/// Temporary state for the Creation workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CreationState {
    /// Generated assets.
    pub generated_assets: Vec<GeneratedAssetState>,
    /// Canvas history steps.
    pub canvas_history: Vec<CanvasHistoryState>,
}

impl CreationState {
    /// Number of tracked assets and canvas steps.
    pub fn entry_count(&self) -> usize {
        self.generated_assets.len() + self.canvas_history.len()
    }
}

/// One collected source in a research workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchSourceState {
    /// Stable source id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Optional URI / path.
    pub uri: Option<String>,
}

/// One research note in a research workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchNoteState {
    /// Stable note id.
    pub id: String,
    /// Note body.
    pub content: String,
}

/// Temporary state for the Research workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResearchState {
    /// Collected sources.
    pub sources: Vec<ResearchSourceState>,
    /// Working notes.
    pub notes: Vec<ResearchNoteState>,
}

impl ResearchState {
    /// Number of tracked sources and notes.
    pub fn entry_count(&self) -> usize {
        self.sources.len() + self.notes.len()
    }
}

/// Independent runtime state for one expanded capability workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum CapabilityState {
    /// Coding / IDE temporary state.
    Coding(CodingState),
    /// Creation / canvas temporary state.
    Creation(CreationState),
    /// Research temporary state.
    Research(ResearchState),
}

impl CapabilityState {
    /// Empty state for a workspace kind (conversation has none).
    pub fn empty_for(kind: WorkspaceKind) -> Option<Self> {
        match kind {
            WorkspaceKind::Conversation => None,
            WorkspaceKind::Coding => Some(Self::Coding(CodingState::default())),
            WorkspaceKind::Creation => Some(Self::Creation(CreationState::default())),
            WorkspaceKind::Research => Some(Self::Research(ResearchState::default())),
        }
    }

    /// Empty state for a capability's requested workspace.
    pub fn empty_for_capability(capability: Capability) -> Option<Self> {
        crate::capability_workspace(capability).and_then(Self::empty_for)
    }

    /// Workspace kind this state belongs to.
    pub fn workspace_kind(&self) -> WorkspaceKind {
        match self {
            Self::Coding(_) => WorkspaceKind::Coding,
            Self::Creation(_) => WorkspaceKind::Creation,
            Self::Research(_) => WorkspaceKind::Research,
        }
    }

    /// Total ephemeral entries currently held.
    pub fn entry_count(&self) -> usize {
        match self {
            Self::Coding(state) => state.entry_count(),
            Self::Creation(state) => state.entry_count(),
            Self::Research(state) => state.entry_count(),
        }
    }

    /// Coding state borrow, when this is a coding workspace.
    pub fn coding(&self) -> Option<&CodingState> {
        match self {
            Self::Coding(state) => Some(state),
            _ => None,
        }
    }

    /// Mutable coding state borrow.
    pub fn coding_mut(&mut self) -> Option<&mut CodingState> {
        match self {
            Self::Coding(state) => Some(state),
            _ => None,
        }
    }

    /// Creation state borrow.
    pub fn creation(&self) -> Option<&CreationState> {
        match self {
            Self::Creation(state) => Some(state),
            _ => None,
        }
    }

    /// Mutable creation state borrow.
    pub fn creation_mut(&mut self) -> Option<&mut CreationState> {
        match self {
            Self::Creation(state) => Some(state),
            _ => None,
        }
    }

    /// Research state borrow.
    pub fn research(&self) -> Option<&ResearchState> {
        match self {
            Self::Research(state) => Some(state),
            _ => None,
        }
    }

    /// Mutable research state borrow.
    pub fn research_mut(&mut self) -> Option<&mut ResearchState> {
        match self {
            Self::Research(state) => Some(state),
            _ => None,
        }
    }

    /// Promote a research note (or coding diagnostic / creation asset summary)
    /// into plain text suitable for conversation or memory promotion.
    ///
    /// This does not persist anything — callers decide where to store it.
    pub fn promote_summary(&self, entry_id: &str) -> Option<String> {
        match self {
            Self::Coding(state) => state
                .diagnostics
                .iter()
                .find(|item| {
                    item.path.as_deref() == Some(entry_id) || item.message.contains(entry_id)
                })
                .map(|item| format!("Diagnostic: {}", item.message))
                .or_else(|| {
                    state
                        .editors
                        .buffer_by_path(entry_id)
                        .map(|buffer| format!("Open file: {}", buffer.path))
                }),
            Self::Creation(state) => state
                .generated_assets
                .iter()
                .find(|asset| asset.id == entry_id)
                .map(|asset| {
                    format!(
                        "Generated asset {} ({})",
                        asset.id,
                        asset.uri.as_deref().unwrap_or(asset.kind.as_str())
                    )
                })
                .or_else(|| {
                    state
                        .canvas_history
                        .iter()
                        .find(|step| step.id == entry_id)
                        .map(|step| format!("Canvas: {}", step.summary))
                }),
            Self::Research(state) => state
                .notes
                .iter()
                .find(|note| note.id == entry_id)
                .map(|note| note.content.clone())
                .or_else(|| {
                    state
                        .sources
                        .iter()
                        .find(|source| source.id == entry_id)
                        .map(|source| {
                            format!(
                                "Source: {}{}",
                                source.title,
                                source
                                    .uri
                                    .as_ref()
                                    .map(|uri| format!(" ({uri})"))
                                    .unwrap_or_default()
                            )
                        })
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Capability;

    #[test]
    fn empty_states_are_kind_specific() {
        assert!(CapabilityState::empty_for(WorkspaceKind::Conversation).is_none());
        assert_eq!(
            CapabilityState::empty_for(WorkspaceKind::Coding)
                .unwrap()
                .workspace_kind(),
            WorkspaceKind::Coding
        );
        assert_eq!(
            CapabilityState::empty_for_capability(Capability::Search)
                .unwrap()
                .workspace_kind(),
            WorkspaceKind::Research
        );
    }

    #[test]
    fn build_explorer_tree_sorts_folders_before_files() {
        let root = "/proj";
        let entries = vec![
            ("/proj/z.txt".into(), "z.txt".into(), false),
            ("/proj/src".into(), "src".into(), true),
            ("/proj/a.md".into(), "a.md".into(), false),
            ("/proj/src/lib.rs".into(), "lib.rs".into(), false),
            ("/proj/src/main.rs".into(), "main.rs".into(), false),
        ];
        let tree = build_explorer_tree(root, &entries);
        assert_eq!(tree.len(), 3);
        assert!(tree[0].is_dir);
        assert_eq!(tree[0].name, "src");
        assert_eq!(tree[1].name, "a.md");
        assert_eq!(tree[2].name, "z.txt");
        assert_eq!(tree[0].children[0].name, "lib.rs");
        assert_eq!(tree[0].children[1].name, "main.rs");
    }

    #[test]
    fn expand_ancestors_of_active_file() {
        let mut explorer = ExplorerState {
            project_root: Some("/proj".into()),
            status: ExplorerStatus::Ready,
            ..ExplorerState::default()
        };
        explorer.expand_ancestors_of("/proj/src/app/main.rs");
        assert!(explorer.expanded_paths.contains("/proj/src"));
        assert!(explorer.expanded_paths.contains("/proj/src/app"));
        assert!(!explorer.expanded_paths.contains("/proj"));
        assert!(!explorer.expanded_paths.contains("/proj/src/app/main.rs"));
    }

    #[test]
    fn editor_tabs_focus_reopen_and_close() {
        let mut state = CodingState::default();
        state.upsert_tab("/proj/a.rs", "a.rs", "fn a() {}".into(), 10.0);
        state.upsert_tab("/proj/b.rs", "b.rs", "fn b() {}".into(), 0.0);
        assert_eq!(state.editors.len(), 2);
        assert_eq!(state.active_tab_path(), Some("/proj/b.rs"));

        assert!(state.focus_tab("/proj/a.rs"));
        assert_eq!(state.active_tab_path(), Some("/proj/a.rs"));
        assert_eq!(state.explorer.selected_path.as_deref(), Some("/proj/a.rs"));

        // Re-upsert focuses existing tab without duplicating.
        state.upsert_tab(
            "/proj/a.rs",
            "a.rs",
            "fn a() { /* updated */ }".into(),
            12.0,
        );
        assert_eq!(state.editors.len(), 2);
        assert_eq!(
            state.editors.buffer_by_path("/proj/a.rs").unwrap().content,
            "fn a() { /* updated */ }"
        );

        assert!(state.close_tab("/proj/a.rs"));
        assert_eq!(state.editors.len(), 1);
        assert_eq!(state.active_tab_path(), Some("/proj/b.rs"));
    }

    #[test]
    fn open_editors_preview_and_recent() {
        let mut editors = OpenEditors::default();
        editors.open_preview("/proj/a.rs", "a".into());
        assert!(editors.sessions()[0].preview);
        editors.open_preview("/proj/b.rs", "b".into());
        assert_eq!(editors.len(), 1, "preview replaces previous preview");
        assert_eq!(editors.sessions()[0].path, "/proj/b.rs");
        editors.open_permanent("/proj/b.rs", "b2".into());
        assert!(!editors.sessions()[0].preview);
        editors.open_permanent("/proj/c.rs", "c".into());
        assert_eq!(editors.len(), 2);
        assert_eq!(editors.recently_opened[0], "/proj/c.rs");
    }

    #[test]
    fn editor_workspace_snapshot_omits_contents() {
        let mut state = CodingState::default();
        state.editor_settings.font_size = 16;
        state.editor_settings.word_wrap = true;
        state.editor_settings.minimap = false;
        state.open_permanent("/proj/a.rs", "SECRET_BUFFER_CONTENTS".into());
        state.set_cursor("/proj/a.rs", 3, 4);
        state.set_scroll_offset("/proj/a.rs", 42.0);
        state.set_folded_regions(
            "/proj/a.rs",
            vec![FoldedRegion {
                start_line: 1,
                end_line: 5,
            }],
        );

        let snapshot = state.editor_workspace_snapshot();
        let json = serde_json::to_string(&snapshot).expect("serialize");
        assert!(!json.contains("SECRET_BUFFER_CONTENTS"));
        assert_eq!(snapshot.tabs.len(), 1);
        assert_eq!(snapshot.tabs[0].path, "/proj/a.rs");
        assert_eq!(snapshot.tabs[0].view.cursor.line, 3);
        assert_eq!(snapshot.tabs[0].view.scroll_top, 42.0);
        assert_eq!(snapshot.tabs[0].view.folded_regions.len(), 1);
        assert_eq!(snapshot.settings.font_size, 16);
        assert!(snapshot.settings.word_wrap);
        assert!(!snapshot.settings.minimap);
    }

    #[test]
    fn shell_chrome_setters_clamp_to_min_max() {
        let mut state = CodingState::default();

        state.set_explorer_width(10.0);
        assert_eq!(state.explorer_width, MIN_EXPLORER_WIDTH);
        state.set_explorer_width(10_000.0);
        assert_eq!(state.explorer_width, MAX_EXPLORER_WIDTH);
        state.set_explorer_width(250.0);
        assert_eq!(state.explorer_width, 250.0);

        state.set_bottom_panel_height(1.0);
        assert_eq!(state.bottom_panel_height, MIN_BOTTOM_PANEL_HEIGHT);
        state.set_bottom_panel_height(10_000.0);
        assert_eq!(state.bottom_panel_height, MAX_BOTTOM_PANEL_HEIGHT);
        state.set_bottom_panel_height(200.0);
        assert_eq!(state.bottom_panel_height, 200.0);

        state.set_workspace_panel_width(1.0);
        assert_eq!(state.workspace_panel_width, MIN_WORKSPACE_PANEL_WIDTH);
        state.set_workspace_panel_width(10_000.0);
        assert_eq!(state.workspace_panel_width, MAX_WORKSPACE_PANEL_WIDTH);
        state.set_workspace_panel_width(700.0);
        assert_eq!(state.workspace_panel_width, 700.0);
    }

    #[test]
    fn editor_workspace_snapshot_roundtrips_shell_chrome_sizes() {
        let mut state = CodingState::default();
        state.set_explorer_width(260.0);
        state.set_bottom_panel_height(210.0);
        state.set_workspace_panel_width(720.0);
        state.bottom_tab = CodingBottomTab::Search;

        let snapshot = state.editor_workspace_snapshot();
        assert_eq!(snapshot.explorer_width, Some(260.0));
        assert_eq!(snapshot.bottom_panel_height, Some(210.0));
        assert_eq!(snapshot.workspace_panel_width, Some(720.0));
        assert_eq!(snapshot.bottom_tab.as_deref(), Some("search"));

        let mut restored = CodingState::default();
        restored.apply_editor_workspace_meta(&snapshot);
        assert_eq!(restored.explorer_width, 260.0);
        assert_eq!(restored.bottom_panel_height, 210.0);
        assert_eq!(restored.workspace_panel_width, 720.0);
        assert_eq!(restored.bottom_tab, CodingBottomTab::Search);

        // Out-of-range persisted values are clamped on restore too.
        let mut out_of_range = snapshot.clone();
        out_of_range.explorer_width = Some(-5.0);
        out_of_range.bottom_panel_height = Some(1.0);
        out_of_range.workspace_panel_width = Some(5_000.0);
        let mut restored_clamped = CodingState::default();
        restored_clamped.apply_editor_workspace_structure(&out_of_range);
        assert_eq!(restored_clamped.explorer_width, MIN_EXPLORER_WIDTH);
        assert_eq!(
            restored_clamped.bottom_panel_height,
            MIN_BOTTOM_PANEL_HEIGHT
        );
        assert_eq!(
            restored_clamped.workspace_panel_width,
            MAX_WORKSPACE_PANEL_WIDTH
        );
    }

    #[test]
    fn coding_bottom_tab_parse_roundtrips() {
        for tab in [
            CodingBottomTab::Hidden,
            CodingBottomTab::Terminal,
            CodingBottomTab::Problems,
            CodingBottomTab::Search,
            CodingBottomTab::Git,
            CodingBottomTab::Diagnostics,
        ] {
            assert_eq!(CodingBottomTab::parse(tab.as_str()), Some(tab));
        }
        assert_eq!(
            CodingBottomTab::parse("diagnostics-legacy"),
            Some(CodingBottomTab::Problems)
        );
    }

    #[test]
    fn editable_extensions_match_editor_allowlist() {
        assert!(is_editable_coding_extension("main.rs"));
        assert!(is_editable_coding_extension("Cargo.toml"));
        assert!(is_editable_coding_extension("notes.MD"));
        assert!(is_editable_coding_extension("cfg.yaml"));
        assert!(is_editable_coding_extension("app.ts"));
        assert!(is_editable_coding_extension("Dockerfile"));
        assert!(is_editable_coding_extension("Makefile"));
        assert!(!is_editable_coding_extension("photo.png"));
        assert!(!is_editable_coding_extension("bin"));
    }

    #[test]
    fn promote_summary_reads_research_and_creation_entries() {
        let mut research = CapabilityState::empty_for(WorkspaceKind::Research).unwrap();
        research
            .research_mut()
            .unwrap()
            .notes
            .push(ResearchNoteState {
                id: "n1".into(),
                content: "Finding A".into(),
            });
        assert_eq!(research.promote_summary("n1").as_deref(), Some("Finding A"));

        let mut creation = CapabilityState::empty_for(WorkspaceKind::Creation).unwrap();
        creation
            .creation_mut()
            .unwrap()
            .generated_assets
            .push(GeneratedAssetState {
                id: "asset-1".into(),
                kind: "image".into(),
                uri: Some("blob://1".into()),
            });
        assert!(creation
            .promote_summary("asset-1")
            .unwrap()
            .contains("asset-1"));
    }
}
