//! Temporary runtime state owned by an expanded capability workspace.
//!
//! Capability state is ephemeral. It disappears when the workspace closes
//! unless the caller explicitly promotes an entry elsewhere (conversation,
//! project memory, etc.).

use std::collections::{BTreeMap, BTreeSet};

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

/// One open editor tab in the Coding workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorTab {
    /// Absolute filesystem path.
    pub path: String,
    /// Basename for the tab label.
    pub name: String,
    /// Editable buffer contents (loaded via Planner → read_file).
    pub content: String,
    /// True when the buffer differs from the last loaded content.
    pub dirty: bool,
    /// Vertical scroll offset preserved while the workspace is open.
    pub scroll_offset: f32,
}

impl Eq for EditorTab {}

/// One open file in a coding workspace (legacy summary; prefer [`EditorTab`]).
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
    /// Create a new empty session bound to an optional cwd.
    pub fn new(id: impl Into<String>, cwd: Option<String>) -> Self {
        Self {
            id: id.into(),
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
    /// Current branch name, when known.
    pub branch: Option<String>,
    /// Short status summary (e.g. "clean", "2 modified").
    pub summary: String,
    /// Unstaged worktree modifications.
    pub modified: Vec<GitFileEntry>,
    /// Staged index changes.
    pub staged: Vec<GitFileEntry>,
    /// Untracked paths.
    pub untracked: Vec<GitFileEntry>,
    /// Draft commit message for the panel.
    pub commit_message: String,
    /// Last error from a Git operation, when any.
    pub last_error: Option<String>,
}

impl GitStatusState {
    /// Apply a refreshed status snapshot from the Git tool.
    pub fn apply_snapshot(
        &mut self,
        branch: Option<String>,
        summary: String,
        modified: Vec<GitFileEntry>,
        staged: Vec<GitFileEntry>,
        untracked: Vec<GitFileEntry>,
    ) {
        self.branch = branch;
        self.summary = summary;
        self.modified = modified;
        self.staged = staged;
        self.untracked = untracked;
        self.last_error = None;
    }
}

/// Which bottom auxiliary panel is visible in the Coding shell (VS Code-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CodingBottomTab {
    /// Bottom panel collapsed — editor uses the full code height.
    #[default]
    Hidden,
    /// Integrated terminal.
    Terminal,
    /// Git status / stage / commit.
    Git,
    /// Workspace + LSP problems (Coding Diagnostics).
    Diagnostics,
}

impl CodingBottomTab {
    /// Short tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hidden => "",
            Self::Terminal => "Terminal",
            Self::Git => "Git",
            Self::Diagnostics => "Problems",
        }
    }
}

/// Temporary state for the Coding workspace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodingState {
    /// Absolute project root path, when known.
    pub project_root: Option<String>,
    /// Root-level explorer nodes (children of the project root).
    pub explorer_nodes: Vec<ExplorerNode>,
    /// Paths currently expanded in the Project Explorer.
    pub expanded_paths: BTreeSet<String>,
    /// Selected path in the project explorer (file or folder).
    pub selected_path: Option<String>,
    /// Explorer load status.
    pub explorer_status: ExplorerStatus,
    /// Open editor tabs (multiple files).
    pub open_tabs: Vec<EditorTab>,
    /// Path of the active editor tab, when any.
    pub active_tab_path: Option<String>,
    /// Scroll positions keyed by path (mirrors tab scroll for quick lookup).
    pub scroll_positions: BTreeMap<String, f32>,
    /// Active terminal sessions.
    pub terminal_sessions: Vec<TerminalSessionState>,
    /// Live Git status for the shell, when set.
    pub git: Option<GitStatusState>,
    /// Current diagnostics.
    pub diagnostics: Vec<DiagnosticState>,
    /// Bottom auxiliary panel tab (Terminal / Git / Problems), or hidden.
    pub bottom_tab: CodingBottomTab,
}

impl Eq for CodingState {}

impl CodingState {
    /// Number of tracked entries across explorer, tabs, terminals, git, and diagnostics.
    pub fn entry_count(&self) -> usize {
        count_explorer_nodes(&self.explorer_nodes)
            + self.open_tabs.len()
            + self.terminal_sessions.len()
            + self.diagnostics.len()
            + usize::from(self.git.is_some())
            + usize::from(self.selected_path.is_some())
    }

    /// Open editor files as a simple path/dirty list (compatibility helper).
    pub fn open_files(&self) -> Vec<OpenFileState> {
        self.open_tabs
            .iter()
            .map(|tab| OpenFileState {
                path: tab.path.clone(),
                dirty: tab.dirty,
            })
            .collect()
    }

    /// Focus an existing tab or return false when the path is not open.
    pub fn focus_tab(&mut self, path: &str) -> bool {
        if self.open_tabs.iter().any(|tab| tab.path == path) {
            self.active_tab_path = Some(path.to_string());
            self.selected_path = Some(path.to_string());
            true
        } else {
            false
        }
    }

    /// Insert or replace a tab and make it active.
    pub fn upsert_tab(&mut self, tab: EditorTab) {
        if let Some(existing) = self
            .open_tabs
            .iter_mut()
            .find(|open| open.path == tab.path)
        {
            *existing = tab.clone();
        } else {
            self.open_tabs.push(tab.clone());
        }
        self.scroll_positions
            .insert(tab.path.clone(), tab.scroll_offset);
        self.active_tab_path = Some(tab.path.clone());
        self.selected_path = Some(tab.path);
    }

    /// Close a tab; activates a neighbor when the active tab is closed.
    pub fn close_tab(&mut self, path: &str) -> bool {
        let Some(index) = self.open_tabs.iter().position(|tab| tab.path == path) else {
            return false;
        };
        self.open_tabs.remove(index);
        self.scroll_positions.remove(path);
        if self.active_tab_path.as_deref() == Some(path) {
            self.active_tab_path = self
                .open_tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.open_tabs.get(i)))
                .map(|tab| tab.path.clone());
        }
        true
    }

    /// Toggle whether a directory path is expanded.
    pub fn toggle_expanded(&mut self, path: &str) {
        if !self.expanded_paths.remove(path) {
            self.expanded_paths.insert(path.to_string());
        }
    }

    /// Update scroll offset for a tab path.
    pub fn set_scroll_offset(&mut self, path: &str, offset: f32) {
        if let Some(tab) = self.open_tabs.iter_mut().find(|tab| tab.path == path) {
            tab.scroll_offset = offset;
        }
        self.scroll_positions.insert(path.to_string(), offset);
    }

    /// Update editable content for the active buffer.
    pub fn set_tab_content(&mut self, path: &str, content: String) {
        if let Some(tab) = self.open_tabs.iter_mut().find(|tab| tab.path == path) {
            if tab.content != content {
                tab.content = content;
                tab.dirty = true;
            }
        }
    }

    /// Clear dirty after a successful save.
    pub fn mark_tab_clean(&mut self, path: &str) {
        if let Some(tab) = self.open_tabs.iter_mut().find(|tab| tab.path == path) {
            tab.dirty = false;
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
pub fn build_explorer_tree(
    root: &str,
    entries: &[(String, String, bool)],
) -> Vec<ExplorerNode> {
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
            _ => left.1.to_ascii_lowercase().cmp(&right.1.to_ascii_lowercase()),
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
                        .open_tabs
                        .iter()
                        .find(|tab| tab.path == entry_id)
                        .map(|tab| format!("Open file: {}", tab.path))
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
    fn editor_tabs_focus_reopen_and_close() {
        let mut state = CodingState::default();
        state.upsert_tab(EditorTab {
            path: "/proj/a.rs".into(),
            name: "a.rs".into(),
            content: "fn a() {}".into(),
            dirty: false,
            scroll_offset: 10.0,
        });
        state.upsert_tab(EditorTab {
            path: "/proj/b.rs".into(),
            name: "b.rs".into(),
            content: "fn b() {}".into(),
            dirty: false,
            scroll_offset: 0.0,
        });
        assert_eq!(state.open_tabs.len(), 2);
        assert_eq!(state.active_tab_path.as_deref(), Some("/proj/b.rs"));

        assert!(state.focus_tab("/proj/a.rs"));
        assert_eq!(state.active_tab_path.as_deref(), Some("/proj/a.rs"));
        assert_eq!(state.selected_path.as_deref(), Some("/proj/a.rs"));

        // Re-upsert focuses existing tab without duplicating.
        state.upsert_tab(EditorTab {
            path: "/proj/a.rs".into(),
            name: "a.rs".into(),
            content: "fn a() { /* updated */ }".into(),
            dirty: true,
            scroll_offset: 12.0,
        });
        assert_eq!(state.open_tabs.len(), 2);
        assert_eq!(state.open_tabs[0].content, "fn a() { /* updated */ }");

        assert!(state.close_tab("/proj/a.rs"));
        assert_eq!(state.open_tabs.len(), 1);
        assert_eq!(state.active_tab_path.as_deref(), Some("/proj/b.rs"));
        assert!(!state.scroll_positions.contains_key("/proj/a.rs"));
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
        research.research_mut().unwrap().notes.push(ResearchNoteState {
            id: "n1".into(),
            content: "Finding A".into(),
        });
        assert_eq!(
            research.promote_summary("n1").as_deref(),
            Some("Finding A")
        );

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
