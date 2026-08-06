//! Split-aware open editors: shared buffers + pane layout tree.
//!
//! Layout is an arbitrary tree of leaves (panes) and splits (horizontal / vertical).
//! Each pane owns its tab strip, active tab, MRU history, and per-tab view state.
//! Buffer contents are shared by path so the same file can appear in multiple panes
//! with independent cursors / scroll / folds.

use std::collections::BTreeMap;

use crate::state::OpenFileState;

/// Cap for the recently-opened path list.
pub const RECENTLY_OPENED_CAP: usize = 20;

/// Current [`EditorWorkspaceSnapshot`] schema version (2 = split layout tree).
pub const EDITOR_WORKSPACE_SNAPSHOT_VERSION: u32 = 2;

/// Cursor position restored when a tab becomes active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct EditorCursor {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based column.
    pub column: u32,
}

/// One collapsed fold range (zero-based inclusive line numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct FoldedRegion {
    /// First line of the fold (inclusive).
    pub start_line: u32,
    /// Last line of the fold (inclusive).
    pub end_line: u32,
}

/// View state restored when activating a session (scroll + cursor + folds).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct EditorViewState {
    /// Vertical scroll in pixels.
    pub scroll_top: f32,
    /// Cursor position.
    pub cursor: EditorCursor,
    /// Collapsed fold ranges for this buffer (not owned by Monaco).
    #[serde(default)]
    pub folded_regions: Vec<FoldedRegion>,
}

impl Eq for EditorViewState {}

/// Editor chrome preferences owned by the Coding workspace (not Monaco).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EditorSettings {
    /// Monaco minimap visibility.
    pub minimap: bool,
    /// Word wrap (`true` → Monaco `"on"`).
    pub word_wrap: bool,
    /// Editor font size in pixels.
    pub font_size: u32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            minimap: true,
            word_wrap: false,
            font_size: 13,
        }
    }
}

/// Tab-strip presentation over an [`EditorSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTab {
    /// Session this tab represents.
    pub session_id: EditorSessionId,
    /// Display title (usually basename).
    pub title: String,
    /// Dirty indicator.
    pub dirty: bool,
    /// Preview styling (italic / transient).
    pub preview: bool,
    /// Reserved — pinning UI is TODO.
    pub pinned: bool,
}

/// Stable identity for an editor pane (split leaf).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct EditorPaneId(pub String);

impl EditorPaneId {
    /// Allocate a new unique pane id for this process.
    pub fn allocate() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(format!("pane-{}", NEXT.fetch_add(1, Ordering::Relaxed)))
    }

    /// Borrow the raw id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EditorPaneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable identity for a shared editor buffer (process-local).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorSessionId(pub String);

impl EditorSessionId {
    /// Allocate a new unique buffer id for this process.
    pub fn allocate() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(format!("ed-{}", NEXT.fetch_add(1, Ordering::Relaxed)))
    }

    /// Borrow the raw id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EditorSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Shared editable buffer (one per filesystem path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorBuffer {
    /// Stable buffer identity.
    pub id: EditorSessionId,
    /// Absolute filesystem path.
    pub path: String,
    /// Basename for tab labels.
    pub name: String,
    /// Editable contents (not persisted in workspace snapshots).
    pub content: String,
    /// True when the buffer differs from the last saved content.
    pub dirty: bool,
}

impl EditorBuffer {
    /// Create a buffer for `path` with `content`.
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        let path = path.into();
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        Self {
            id: EditorSessionId::allocate(),
            path,
            name,
            content: content.into(),
            dirty: false,
        }
    }
}

/// A tab appearance inside one pane (independent view state).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorPaneTab {
    /// Absolute filesystem path (shared buffer key).
    pub path: String,
    /// VS Code-style preview tab within this pane.
    #[serde(default)]
    pub preview: bool,
    /// Reserved — pinning UI is TODO.
    #[serde(default)]
    pub pinned: bool,
    /// Pane-local scroll / cursor / folds.
    #[serde(default)]
    pub view: EditorViewState,
}

impl Eq for EditorPaneTab {}

/// One editor group / pane (leaf of the layout tree).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorPane {
    /// Pane identity.
    pub id: EditorPaneId,
    /// Tabs in strip order.
    #[serde(default)]
    pub tabs: Vec<EditorPaneTab>,
    /// Active tab path within this pane.
    #[serde(default)]
    pub active_path: Option<String>,
    /// Pane-local recently opened history (MRU).
    #[serde(default)]
    pub recently_opened: Vec<String>,
}

impl Eq for EditorPane {}

impl EditorPane {
    /// Create an empty pane with a fresh id.
    pub fn new() -> Self {
        Self {
            id: EditorPaneId::allocate(),
            tabs: Vec::new(),
            active_path: None,
            recently_opened: Vec::new(),
        }
    }

    /// Whether this pane has no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    fn touch_recent(&mut self, path: &str) {
        self.recently_opened.retain(|entry| entry != path);
        self.recently_opened.insert(0, path.to_string());
        self.recently_opened.truncate(RECENTLY_OPENED_CAP);
    }

    fn activate_path(&mut self, path: &str) -> bool {
        if !self.tabs.iter().any(|tab| tab.path == path) {
            return false;
        }
        self.active_path = Some(path.to_string());
        self.touch_recent(path);
        true
    }

    fn tab_mut(&mut self, path: &str) -> Option<&mut EditorPaneTab> {
        self.tabs.iter_mut().find(|tab| tab.path == path)
    }

    fn close_path(&mut self, path: &str) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.path == path) else {
            return false;
        };
        let was_active = self.active_path.as_deref() == Some(path);
        self.tabs.remove(index);
        if was_active {
            self.active_path = self
                .tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.tabs.get(i)))
                .map(|tab| tab.path.clone());
            if let Some(active) = self.active_path.clone() {
                self.touch_recent(&active);
            }
        }
        true
    }
}

impl Default for EditorPane {
    fn default() -> Self {
        Self::new()
    }
}

/// Split axis for an interior layout node.
///
/// - [`Self::Vertical`]: panes side-by-side (vertical divider) — VS Code “Split Right”
/// - [`Self::Horizontal`]: panes stacked (horizontal divider) — VS Code “Split Down”
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    /// Side-by-side panes.
    Vertical,
    /// Stacked panes.
    Horizontal,
}

/// Recursive editor area layout (not limited to two panes).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditorLayoutNode {
    /// A single editor pane.
    Leaf {
        /// Pane shown in this leaf.
        pane: EditorPaneId,
    },
    /// Nested split with relative sizes.
    Split {
        /// Split axis.
        direction: SplitDirection,
        /// Relative sizes (same length as `children`); normalized to sum ≈ 1.
        sizes: Vec<f32>,
        /// Child layout nodes (2+).
        children: Vec<EditorLayoutNode>,
    },
}

impl Eq for EditorLayoutNode {}

impl EditorLayoutNode {
    /// Single-pane layout.
    pub fn leaf(pane: EditorPaneId) -> Self {
        Self::Leaf { pane }
    }

    /// All pane ids in depth-first order.
    pub fn pane_ids(&self) -> Vec<EditorPaneId> {
        let mut out = Vec::new();
        self.collect_pane_ids(&mut out);
        out
    }

    fn collect_pane_ids(&self, out: &mut Vec<EditorPaneId>) {
        match self {
            Self::Leaf { pane } => out.push(pane.clone()),
            Self::Split { children, .. } => {
                for child in children {
                    child.collect_pane_ids(out);
                }
            }
        }
    }

    /// Replace `target` leaf with a split containing `target` and `new_pane`.
    pub fn split_leaf(
        &mut self,
        target: &EditorPaneId,
        direction: SplitDirection,
        new_pane: EditorPaneId,
    ) -> bool {
        match self {
            Self::Leaf { pane } if pane == target => {
                *self = Self::Split {
                    direction,
                    sizes: vec![0.5, 0.5],
                    children: vec![
                        Self::Leaf {
                            pane: target.clone(),
                        },
                        Self::Leaf { pane: new_pane },
                    ],
                };
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { children, .. } => children
                .iter_mut()
                .any(|child| child.split_leaf(target, direction, new_pane.clone())),
        }
    }

    /// Remove a leaf pane and flatten unary splits.
    pub fn remove_pane(&mut self, target: &EditorPaneId) -> bool {
        match self {
            Self::Leaf { pane } => pane != target,
            Self::Split {
                children, sizes, ..
            } => {
                let mut keep = Vec::new();
                let mut keep_sizes = Vec::new();
                for (child, size) in children.drain(..).zip(sizes.drain(..)) {
                    match child {
                        Self::Leaf { pane } if &pane == target => {}
                        mut other => {
                            if other.remove_pane(target) {
                                keep.push(other);
                                keep_sizes.push(size);
                            }
                        }
                    }
                }
                if keep.is_empty() {
                    // Should not happen for a valid tree with ≥1 pane remaining;
                    // caller replaces root.
                    *children = keep;
                    *sizes = keep_sizes;
                    return false;
                }
                if keep.len() == 1 {
                    *self = keep.pop().expect("len == 1");
                    return true;
                }
                let sum: f32 = keep_sizes.iter().sum::<f32>().max(f32::EPSILON);
                for size in &mut keep_sizes {
                    *size /= sum;
                }
                *children = keep;
                *sizes = keep_sizes;
                true
            }
        }
    }

    /// Update relative sizes for the split that directly contains `first_child` leaf/split path.
    /// `node_path` is a sequence of child indices from the root to the split being resized.
    pub fn set_sizes_at(&mut self, node_path: &[usize], sizes: Vec<f32>) -> bool {
        if node_path.is_empty() {
            if let Self::Split {
                sizes: slot,
                children,
                ..
            } = self
            {
                if sizes.len() == children.len() && sizes.len() >= 2 {
                    let sum: f32 = sizes.iter().copied().sum::<f32>().max(f32::EPSILON);
                    *slot = sizes.into_iter().map(|value| value / sum).collect();
                    return true;
                }
            }
            return false;
        }
        let Some((&head, rest)) = node_path.split_first() else {
            return false;
        };
        match self {
            Self::Split { children, .. } => children
                .get_mut(head)
                .is_some_and(|child| child.set_sizes_at(rest, sizes)),
            Self::Leaf { .. } => false,
        }
    }
}

/// Resolved tab+buffer view for UI / Monaco (owned snapshot).
#[derive(Debug, Clone, PartialEq)]
pub struct EditorSession {
    /// Stable buffer identity.
    pub id: EditorSessionId,
    /// Absolute filesystem path.
    pub path: String,
    /// Basename for the tab label.
    pub name: String,
    /// Editable buffer contents.
    pub content: String,
    /// Dirty flag from the shared buffer.
    pub dirty: bool,
    /// Pane-local view state.
    pub view: EditorViewState,
    /// Preview flag from the pane tab.
    pub preview: bool,
    /// Pin flag from the pane tab.
    pub pinned: bool,
    /// Pane that owns this tab appearance.
    pub pane_id: EditorPaneId,
}

impl Eq for EditorSession {}

impl EditorSession {
    /// Tab-strip presentation.
    pub fn as_tab(&self) -> EditorTab {
        EditorTab {
            session_id: self.id.clone(),
            title: self.name.clone(),
            dirty: self.dirty,
            preview: self.preview,
            pinned: self.pinned,
        }
    }
}

/// Persisted open-tab metadata (never includes buffer contents).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedEditorTab {
    /// Absolute filesystem path.
    pub path: String,
    /// Whether this tab was a preview tab.
    #[serde(default)]
    pub preview: bool,
    /// View state (scroll, cursor, folds).
    #[serde(default)]
    pub view: EditorViewState,
}

/// Persisted pane snapshot.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedEditorPane {
    /// Pane id.
    pub id: EditorPaneId,
    /// Tabs in strip order.
    #[serde(default)]
    pub tabs: Vec<PersistedEditorTab>,
    /// Active path within the pane.
    #[serde(default)]
    pub active_path: Option<String>,
    /// Pane-local MRU.
    #[serde(default)]
    pub recently_opened: Vec<String>,
}

/// Disk snapshot of Coding editor workspace UI state (no file contents).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorWorkspaceSnapshot {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Legacy single-pane tabs (v1). Ignored when `panes` is non-empty.
    #[serde(default)]
    pub tabs: Vec<PersistedEditorTab>,
    /// Legacy active path (v1).
    #[serde(default)]
    pub active_path: Option<String>,
    /// Workspace-level recently opened paths (MRU).
    #[serde(default)]
    pub recently_opened: Vec<String>,
    /// Editor chrome preferences.
    #[serde(default)]
    pub settings: EditorSettings,
    /// Multi-pane snapshots (v2+).
    #[serde(default)]
    pub panes: Vec<PersistedEditorPane>,
    /// Layout tree (v2+).
    #[serde(default)]
    pub layout: Option<EditorLayoutNode>,
    /// Focused pane id (v2+).
    #[serde(default)]
    pub focused_pane: Option<EditorPaneId>,
    /// Project Explorer column width (points). Restored with serde defaults when absent.
    #[serde(default)]
    pub explorer_width: Option<f32>,
    /// Whether the Project Explorer column is visible. Absent → leave default / current.
    #[serde(default)]
    pub explorer_visible: Option<bool>,
    /// Bottom auxiliary panel height when open (points).
    #[serde(default)]
    pub bottom_panel_height: Option<f32>,
    /// Coding side-panel width (conversation ↔ workspace divider).
    #[serde(default)]
    pub workspace_panel_width: Option<f32>,
    /// Active bottom panel tab id (`terminal`, `problems`, `search`, `git`, `diagnostics`, `output`, `hidden`).
    #[serde(default)]
    pub bottom_tab: Option<String>,
    /// Last visible bottom dock page (restored when reopening a collapsed dock).
    #[serde(default)]
    pub last_bottom_tab: Option<String>,
}

impl Default for EditorWorkspaceSnapshot {
    fn default() -> Self {
        Self {
            version: EDITOR_WORKSPACE_SNAPSHOT_VERSION,
            tabs: Vec::new(),
            active_path: None,
            recently_opened: Vec::new(),
            settings: EditorSettings::default(),
            panes: Vec::new(),
            layout: None,
            focused_pane: None,
            explorer_width: None,
            explorer_visible: None,
            bottom_panel_height: None,
            workspace_panel_width: None,
            bottom_tab: None,
            last_bottom_tab: None,
        }
    }
}

/// Open editors owned by the Coding workspace (buffers + layout tree).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenEditors {
    /// Shared buffers keyed by absolute path.
    pub buffers: BTreeMap<String, EditorBuffer>,
    /// Panes keyed by pane id string.
    pub panes: BTreeMap<String, EditorPane>,
    /// Recursive split layout.
    pub layout: EditorLayoutNode,
    /// Pane that receives keyboard / Monaco focus.
    pub focused_pane: EditorPaneId,
    /// Workspace-level recently opened paths (MRU).
    pub recently_opened: Vec<String>,
}

impl Eq for OpenEditors {}

impl Default for OpenEditors {
    fn default() -> Self {
        let pane = EditorPane::new();
        let id = pane.id.clone();
        let mut panes = BTreeMap::new();
        panes.insert(id.0.clone(), pane);
        Self {
            buffers: BTreeMap::new(),
            panes,
            layout: EditorLayoutNode::leaf(id.clone()),
            focused_pane: id,
            recently_opened: Vec::new(),
        }
    }
}

impl OpenEditors {
    /// Ensure invariants after mutation (at least one pane; focused exists).
    fn sanitize(&mut self) {
        if self.panes.is_empty() {
            let pane = EditorPane::new();
            let id = pane.id.clone();
            self.panes.insert(id.0.clone(), pane);
            self.layout = EditorLayoutNode::leaf(id.clone());
            self.focused_pane = id;
        }
        if !self.panes.contains_key(self.focused_pane.as_str()) {
            self.focused_pane = self
                .panes
                .values()
                .next()
                .map(|pane| pane.id.clone())
                .expect("sanitized non-empty panes");
        }
        // Drop buffers no longer referenced by any pane.
        let live: std::collections::BTreeSet<_> = self
            .panes
            .values()
            .flat_map(|pane| pane.tabs.iter().map(|tab| tab.path.clone()))
            .collect();
        self.buffers.retain(|path, _| live.contains(path));
    }

    /// Tab-strip entries for the focused pane.
    pub fn tabs(&self) -> Vec<EditorTab> {
        self.sessions_in_pane(&self.focused_pane)
            .into_iter()
            .map(|session| session.as_tab())
            .collect()
    }

    /// Resolve sessions (buffer + pane view) for a pane.
    pub fn sessions_in_pane(&self, pane_id: &EditorPaneId) -> Vec<EditorSession> {
        let Some(pane) = self.panes.get(pane_id.as_str()) else {
            return Vec::new();
        };
        pane.tabs
            .iter()
            .filter_map(|tab| self.resolve_tab(pane_id, tab))
            .collect()
    }

    fn resolve_tab(&self, pane_id: &EditorPaneId, tab: &EditorPaneTab) -> Option<EditorSession> {
        let buffer = self.buffers.get(&tab.path)?;
        Some(EditorSession {
            id: buffer.id.clone(),
            path: buffer.path.clone(),
            name: buffer.name.clone(),
            content: buffer.content.clone(),
            dirty: buffer.dirty,
            view: tab.view.clone(),
            preview: tab.preview,
            pinned: tab.pinned,
            pane_id: pane_id.clone(),
        })
    }

    /// Active session in the focused pane.
    pub fn active_session(&self) -> Option<EditorSession> {
        self.active_session_in_pane(&self.focused_pane)
    }

    /// Active session in a specific pane.
    pub fn active_session_in_pane(&self, pane_id: &EditorPaneId) -> Option<EditorSession> {
        let pane = self.panes.get(pane_id.as_str())?;
        let path = pane.active_path.as_deref()?;
        let tab = pane.tabs.iter().find(|tab| tab.path == path)?;
        self.resolve_tab(pane_id, tab)
    }

    /// Find a buffer by filesystem path.
    pub fn buffer_by_path(&self, path: &str) -> Option<&EditorBuffer> {
        self.buffers.get(path)
    }

    /// Compatibility: session in the focused pane for `path`, else any pane.
    pub fn session_by_path(&self, path: &str) -> Option<EditorSession> {
        if let Some(pane) = self.panes.get(self.focused_pane.as_str()) {
            if let Some(tab) = pane.tabs.iter().find(|tab| tab.path == path) {
                return self.resolve_tab(&self.focused_pane, tab);
            }
        }
        for pane in self.panes.values() {
            if let Some(tab) = pane.tabs.iter().find(|tab| tab.path == path) {
                return self.resolve_tab(&pane.id, tab);
            }
        }
        None
    }

    /// Whether no tabs are open in any pane.
    pub fn is_empty(&self) -> bool {
        self.panes.values().all(|pane| pane.is_empty())
    }

    /// Number of unique open buffers.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Focused-pane sessions in tab order (compatibility helper).
    pub fn sessions(&self) -> Vec<EditorSession> {
        self.sessions_in_pane(&self.focused_pane)
    }

    /// Workspace MRU touch.
    pub fn touch_recent(&mut self, path: &str) {
        self.recently_opened.retain(|entry| entry != path);
        self.recently_opened.insert(0, path.to_string());
        self.recently_opened.truncate(RECENTLY_OPENED_CAP);
    }

    /// Focus a pane.
    pub fn focus_pane(&mut self, pane_id: &EditorPaneId) -> bool {
        if !self.panes.contains_key(pane_id.as_str()) {
            return false;
        }
        self.focused_pane = pane_id.clone();
        true
    }

    /// Activate `path` in the focused pane (must already be open there).
    pub fn activate_path(&mut self, path: &str) -> bool {
        self.activate_path_in_pane(&self.focused_pane.clone(), path)
    }

    /// Activate `path` in a pane.
    pub fn activate_path_in_pane(&mut self, pane_id: &EditorPaneId, path: &str) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id.as_str()) else {
            return false;
        };
        if !pane.activate_path(path) {
            return false;
        }
        self.focused_pane = pane_id.clone();
        self.touch_recent(path);
        true
    }

    fn ensure_buffer(&mut self, path: &str, content: String) -> &EditorBuffer {
        if let Some(existing) = self.buffers.get_mut(path) {
            // Fresh open from disk replaces clean buffers; keep dirty content.
            if !existing.dirty {
                existing.content = content;
            }
            return self.buffers.get(path).expect("just inserted");
        }
        let buffer = EditorBuffer::new(path, content);
        self.buffers.insert(path.to_string(), buffer);
        self.buffers.get(path).expect("just inserted")
    }

    /// Open or focus a permanent tab in the focused pane.
    pub fn open_permanent(&mut self, path: &str, content: String) -> EditorSession {
        self.open_permanent_in_pane(&self.focused_pane.clone(), path, content)
    }

    /// Open or focus a permanent tab in `pane_id`.
    pub fn open_permanent_in_pane(
        &mut self,
        pane_id: &EditorPaneId,
        path: &str,
        content: String,
    ) -> EditorSession {
        self.ensure_buffer(path, content);
        let pane_key = pane_id.0.clone();
        {
            let pane = self.panes.get_mut(&pane_key).expect("pane must exist");
            if let Some(tab) = pane.tab_mut(path) {
                tab.preview = false;
            } else {
                // Opening permanent elsewhere replaces this pane's preview tab.
                pane.tabs.retain(|tab| !tab.preview || tab.path == path);
                pane.tabs.push(EditorPaneTab {
                    path: path.to_string(),
                    preview: false,
                    pinned: false,
                    view: EditorViewState::default(),
                });
            }
            pane.activate_path(path);
        }
        self.focused_pane = pane_id.clone();
        self.touch_recent(path);
        self.session_by_path(path).expect("just opened")
    }

    /// Open or replace the preview tab in the focused pane.
    pub fn open_preview(&mut self, path: &str, content: String) -> EditorSession {
        self.open_preview_in_pane(&self.focused_pane.clone(), path, content)
    }

    /// Open or replace the preview tab in `pane_id`.
    pub fn open_preview_in_pane(
        &mut self,
        pane_id: &EditorPaneId,
        path: &str,
        content: String,
    ) -> EditorSession {
        self.ensure_buffer(path, content);
        let pane_key = pane_id.0.clone();
        {
            let pane = self.panes.get_mut(&pane_key).expect("pane must exist");
            if pane.tabs.iter().any(|tab| tab.path == path) {
                pane.activate_path(path);
            } else {
                if let Some(index) = pane.tabs.iter().position(|tab| tab.preview) {
                    pane.tabs.remove(index);
                }
                pane.tabs.push(EditorPaneTab {
                    path: path.to_string(),
                    preview: true,
                    pinned: false,
                    view: EditorViewState::default(),
                });
                pane.activate_path(path);
            }
        }
        self.focused_pane = pane_id.clone();
        self.touch_recent(path);
        self.sanitize();
        self.session_by_path(path).expect("just opened")
    }

    /// Promote the active preview tab in the focused pane.
    pub fn promote_active_preview(&mut self) {
        let pane_id = self.focused_pane.clone();
        if let Some(pane) = self.panes.get_mut(pane_id.as_str()) {
            if let Some(path) = pane.active_path.clone() {
                if let Some(tab) = pane.tab_mut(&path) {
                    tab.preview = false;
                }
            }
        }
    }

    /// Close `path` in the focused pane; closes the pane when it becomes empty (if not sole pane).
    pub fn close_path(&mut self, path: &str) -> bool {
        self.close_path_in_pane(&self.focused_pane.clone(), path)
    }

    /// Close `path` in a pane.
    pub fn close_path_in_pane(&mut self, pane_id: &EditorPaneId, path: &str) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id.as_str()) else {
            return false;
        };
        if !pane.close_path(path) {
            return false;
        }
        let empty = pane.is_empty();
        if empty && self.panes.len() > 1 {
            let _ = self.close_pane(pane_id);
        } else {
            self.sanitize();
        }
        true
    }

    /// Split the focused pane; clones the active tab into the new pane (VS Code-like).
    pub fn split(&mut self, direction: SplitDirection) -> Option<EditorPaneId> {
        self.split_pane(&self.focused_pane.clone(), direction)
    }

    /// Split `pane_id`, placing a new pane beside/below it.
    pub fn split_pane(
        &mut self,
        pane_id: &EditorPaneId,
        direction: SplitDirection,
    ) -> Option<EditorPaneId> {
        if !self.panes.contains_key(pane_id.as_str()) {
            return None;
        }
        let mut new_pane = EditorPane::new();
        if let Some(active) = self.active_session_in_pane(pane_id) {
            new_pane.tabs.push(EditorPaneTab {
                path: active.path.clone(),
                preview: false,
                pinned: false,
                view: active.view.clone(),
            });
            new_pane.active_path = Some(active.path.clone());
            new_pane.touch_recent(&active.path);
        }
        let new_id = new_pane.id.clone();
        self.panes.insert(new_id.0.clone(), new_pane);
        if !self.layout.split_leaf(pane_id, direction, new_id.clone()) {
            // Fallback: wrap entire layout.
            self.layout = EditorLayoutNode::Split {
                direction,
                sizes: vec![0.5, 0.5],
                children: vec![
                    std::mem::replace(&mut self.layout, EditorLayoutNode::leaf(pane_id.clone())),
                    EditorLayoutNode::leaf(new_id.clone()),
                ],
            };
        }
        self.focused_pane = new_id.clone();
        Some(new_id)
    }

    /// Close an entire pane (no-op when it is the only pane).
    pub fn close_pane(&mut self, pane_id: &EditorPaneId) -> bool {
        if self.panes.len() <= 1 || !self.panes.contains_key(pane_id.as_str()) {
            return false;
        }
        self.panes.remove(pane_id.as_str());
        let _ = self.layout.remove_pane(pane_id);
        // If layout still references missing panes, rebuild as a flat split of remaining.
        let live = self.layout.pane_ids();
        let missing = live.iter().any(|id| !self.panes.contains_key(id.as_str()));
        if missing || live.is_empty() {
            let ids: Vec<_> = self.panes.keys().cloned().map(EditorPaneId).collect();
            self.layout = match ids.as_slice() {
                [] => unreachable!("close_pane keeps ≥1 pane"),
                [only] => EditorLayoutNode::leaf(only.clone()),
                many => EditorLayoutNode::Split {
                    direction: SplitDirection::Vertical,
                    sizes: vec![1.0 / many.len() as f32; many.len()],
                    children: many
                        .iter()
                        .map(|id| EditorLayoutNode::leaf(id.clone()))
                        .collect(),
                },
            };
        }
        if &self.focused_pane == pane_id {
            self.focused_pane = self.layout.pane_ids().into_iter().next().expect("≥1 pane");
        }
        self.sanitize();
        true
    }

    /// Move a tab from one pane to another (drag between splits).
    pub fn move_tab(
        &mut self,
        from: &EditorPaneId,
        path: &str,
        to: &EditorPaneId,
        index: Option<usize>,
    ) -> bool {
        if from == to {
            return self.activate_path_in_pane(to, path);
        }
        let Some(from_pane) = self.panes.get_mut(from.as_str()) else {
            return false;
        };
        let Some(pos) = from_pane.tabs.iter().position(|tab| tab.path == path) else {
            return false;
        };
        let tab = from_pane.tabs.remove(pos);
        if from_pane.active_path.as_deref() == Some(path) {
            from_pane.active_path = from_pane
                .tabs
                .get(pos)
                .or_else(|| pos.checked_sub(1).and_then(|i| from_pane.tabs.get(i)))
                .map(|tab| tab.path.clone());
        }
        let from_empty = from_pane.is_empty();

        let Some(to_pane) = self.panes.get_mut(to.as_str()) else {
            return false;
        };
        // If already present in destination, just activate and drop the moved duplicate view.
        if to_pane.tabs.iter().any(|existing| existing.path == path) {
            to_pane.activate_path(path);
        } else {
            let insert_at = index.unwrap_or(to_pane.tabs.len()).min(to_pane.tabs.len());
            to_pane.tabs.insert(insert_at, tab);
            to_pane.activate_path(path);
        }
        self.focused_pane = to.clone();
        self.touch_recent(path);

        if from_empty && self.panes.len() > 1 {
            let _ = self.close_pane(from);
        } else {
            self.sanitize();
        }
        true
    }

    /// Update buffer content for a path; promotes preview in all panes and marks dirty.
    pub fn set_content(&mut self, path: &str, content: String) -> bool {
        let Some(buffer) = self.buffers.get_mut(path) else {
            return false;
        };
        if buffer.content != content {
            buffer.content = content;
            buffer.dirty = true;
            for pane in self.panes.values_mut() {
                if let Some(tab) = pane.tab_mut(path) {
                    tab.preview = false;
                }
            }
        }
        true
    }

    /// Update scroll in the focused pane's tab for `path`.
    pub fn set_scroll_top(&mut self, path: &str, scroll_top: f32) -> bool {
        self.set_scroll_top_in_pane(&self.focused_pane.clone(), path, scroll_top)
    }

    /// Update scroll in a pane.
    pub fn set_scroll_top_in_pane(
        &mut self,
        pane_id: &EditorPaneId,
        path: &str,
        scroll_top: f32,
    ) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id.as_str()) else {
            return false;
        };
        let Some(tab) = pane.tab_mut(path) else {
            return false;
        };
        tab.view.scroll_top = scroll_top;
        true
    }

    /// Update cursor in the focused pane.
    pub fn set_cursor(&mut self, path: &str, line: u32, column: u32) -> bool {
        self.set_cursor_in_pane(&self.focused_pane.clone(), path, line, column)
    }

    /// Update cursor in a pane.
    pub fn set_cursor_in_pane(
        &mut self,
        pane_id: &EditorPaneId,
        path: &str,
        line: u32,
        column: u32,
    ) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id.as_str()) else {
            return false;
        };
        let Some(tab) = pane.tab_mut(path) else {
            return false;
        };
        tab.view.cursor = EditorCursor { line, column };
        true
    }

    /// Update folds in the focused pane.
    pub fn set_folded_regions(&mut self, path: &str, folded_regions: Vec<FoldedRegion>) -> bool {
        self.set_folded_regions_in_pane(&self.focused_pane.clone(), path, folded_regions)
    }

    /// Update folds in a pane.
    pub fn set_folded_regions_in_pane(
        &mut self,
        pane_id: &EditorPaneId,
        path: &str,
        folded_regions: Vec<FoldedRegion>,
    ) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id.as_str()) else {
            return false;
        };
        let Some(tab) = pane.tab_mut(path) else {
            return false;
        };
        tab.view.folded_regions = folded_regions;
        true
    }

    /// Apply full view state in the focused pane.
    pub fn set_view_state(&mut self, path: &str, view: EditorViewState) -> bool {
        self.set_view_state_in_pane(&self.focused_pane.clone(), path, view)
    }

    /// Apply full view state in a pane.
    pub fn set_view_state_in_pane(
        &mut self,
        pane_id: &EditorPaneId,
        path: &str,
        view: EditorViewState,
    ) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id.as_str()) else {
            return false;
        };
        let Some(tab) = pane.tab_mut(path) else {
            return false;
        };
        tab.view = view;
        true
    }

    /// Clear dirty after a successful save.
    pub fn mark_clean(&mut self, path: &str) -> bool {
        let Some(buffer) = self.buffers.get_mut(path) else {
            return false;
        };
        buffer.dirty = false;
        true
    }

    /// Remap an open buffer path after rename.
    pub fn remap_path(&mut self, from: &str, to: &str, name: &str) {
        if let Some(mut buffer) = self.buffers.remove(from) {
            buffer.path = to.to_string();
            buffer.name = name.to_string();
            self.buffers.insert(to.to_string(), buffer);
        }
        for pane in self.panes.values_mut() {
            for tab in &mut pane.tabs {
                if tab.path == from {
                    tab.path = to.to_string();
                }
            }
            if pane.active_path.as_deref() == Some(from) {
                pane.active_path = Some(to.to_string());
            }
            for recent in &mut pane.recently_opened {
                if recent == from {
                    *recent = to.to_string();
                }
            }
        }
        for recent in &mut self.recently_opened {
            if recent == from {
                *recent = to.to_string();
            }
        }
    }

    /// Resize a split node addressed by `node_path` from the layout root.
    pub fn resize_split(&mut self, node_path: &[usize], sizes: Vec<f32>) -> bool {
        self.layout.set_sizes_at(node_path, sizes)
    }

    /// Open files as a simple path/dirty list.
    pub fn open_files(&self) -> Vec<OpenFileState> {
        self.buffers
            .values()
            .map(|buffer| OpenFileState {
                path: buffer.path.clone(),
                dirty: buffer.dirty,
            })
            .collect()
    }

    /// Build a persistence snapshot (no buffer contents).
    pub fn snapshot(&self, settings: EditorSettings) -> EditorWorkspaceSnapshot {
        let focused_path = self.active_session().map(|session| session.path);
        let panes: Vec<PersistedEditorPane> = self
            .panes
            .values()
            .map(|pane| PersistedEditorPane {
                id: pane.id.clone(),
                tabs: pane
                    .tabs
                    .iter()
                    .map(|tab| PersistedEditorTab {
                        path: tab.path.clone(),
                        preview: tab.preview,
                        view: tab.view.clone(),
                    })
                    .collect(),
                active_path: pane.active_path.clone(),
                recently_opened: pane.recently_opened.clone(),
            })
            .collect();
        // Legacy v1 fields mirror the focused pane for older readers.
        let focused = self.panes.get(self.focused_pane.as_str());
        EditorWorkspaceSnapshot {
            version: EDITOR_WORKSPACE_SNAPSHOT_VERSION,
            tabs: focused
                .map(|pane| {
                    pane.tabs
                        .iter()
                        .map(|tab| PersistedEditorTab {
                            path: tab.path.clone(),
                            preview: tab.preview,
                            view: tab.view.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            active_path: focused_path,
            recently_opened: self.recently_opened.clone(),
            settings,
            panes,
            layout: Some(self.layout.clone()),
            focused_pane: Some(self.focused_pane.clone()),
            explorer_width: None,
            explorer_visible: None,
            bottom_panel_height: None,
            workspace_panel_width: None,
            bottom_tab: None,
            last_bottom_tab: None,
        }
    }

    /// Replace layout/panes from a snapshot (buffers must be filled by the caller).
    pub fn apply_snapshot_structure(&mut self, snapshot: &EditorWorkspaceSnapshot) {
        self.recently_opened = snapshot.recently_opened.clone();
        self.recently_opened.truncate(RECENTLY_OPENED_CAP);
        self.buffers.clear();
        self.panes.clear();

        if !snapshot.panes.is_empty() {
            for pane in &snapshot.panes {
                self.panes.insert(
                    pane.id.0.clone(),
                    EditorPane {
                        id: pane.id.clone(),
                        tabs: pane
                            .tabs
                            .iter()
                            .map(|tab| EditorPaneTab {
                                path: tab.path.clone(),
                                preview: tab.preview,
                                pinned: false,
                                view: tab.view.clone(),
                            })
                            .collect(),
                        active_path: pane.active_path.clone(),
                        recently_opened: pane.recently_opened.clone(),
                    },
                );
            }
            if let Some(layout) = snapshot.layout.clone() {
                self.layout = layout;
            } else {
                let ids: Vec<_> = snapshot.panes.iter().map(|pane| pane.id.clone()).collect();
                self.layout = match ids.as_slice() {
                    [] => EditorLayoutNode::leaf(EditorPaneId::allocate()),
                    [only] => EditorLayoutNode::leaf(only.clone()),
                    many => EditorLayoutNode::Split {
                        direction: SplitDirection::Vertical,
                        sizes: vec![1.0 / many.len() as f32; many.len()],
                        children: many
                            .iter()
                            .map(|id| EditorLayoutNode::leaf(id.clone()))
                            .collect(),
                    },
                };
            }
            self.focused_pane = snapshot
                .focused_pane
                .clone()
                .or_else(|| snapshot.panes.first().map(|pane| pane.id.clone()))
                .unwrap_or_else(EditorPaneId::allocate);
        } else {
            // v1 single-pane restore
            let mut pane = EditorPane::new();
            pane.tabs = snapshot
                .tabs
                .iter()
                .map(|tab| EditorPaneTab {
                    path: tab.path.clone(),
                    preview: tab.preview,
                    pinned: false,
                    view: tab.view.clone(),
                })
                .collect();
            pane.active_path = snapshot.active_path.clone();
            let id = pane.id.clone();
            self.panes.insert(id.0.clone(), pane);
            self.layout = EditorLayoutNode::leaf(id.clone());
            self.focused_pane = id;
        }
        self.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_creates_second_pane_with_independent_cursor() {
        let mut editors = OpenEditors::default();
        editors.open_permanent("/proj/a.rs", "fn a() {}".into());
        editors.set_cursor("/proj/a.rs", 0, 1);
        let left = editors.focused_pane.clone();
        let right = editors.split(SplitDirection::Vertical).expect("split");
        assert_ne!(left, right);
        assert_eq!(editors.panes.len(), 2);
        editors.set_cursor_in_pane(&right, "/proj/a.rs", 0, 5);
        assert_eq!(
            editors
                .active_session_in_pane(&left)
                .expect("left")
                .view
                .cursor
                .column,
            1
        );
        assert_eq!(
            editors
                .active_session_in_pane(&right)
                .expect("right")
                .view
                .cursor
                .column,
            5
        );
        // Shared buffer content
        editors.set_content("/proj/a.rs", "fn a() { /* shared */ }".into());
        assert!(editors.buffer_by_path("/proj/a.rs").unwrap().dirty);
        assert_eq!(
            editors.active_session_in_pane(&left).unwrap().content,
            "fn a() { /* shared */ }"
        );
    }

    #[test]
    fn move_tab_between_panes_and_close_empty() {
        let mut editors = OpenEditors::default();
        editors.open_permanent("/proj/a.rs", "a".into());
        editors.open_permanent("/proj/b.rs", "b".into());
        let left = editors.focused_pane.clone();
        let right = editors.split(SplitDirection::Horizontal).expect("split");
        assert!(editors.move_tab(&left, "/proj/a.rs", &right, Some(0)));
        assert!(!editors
            .sessions_in_pane(&left)
            .iter()
            .any(|session| session.path == "/proj/a.rs"));
        // Closing last tab in left closes the pane.
        let remaining = editors
            .sessions_in_pane(&left)
            .into_iter()
            .map(|session| session.path)
            .collect::<Vec<_>>();
        for path in remaining {
            editors.close_path_in_pane(&left, &path);
        }
        assert_eq!(editors.panes.len(), 1);
    }

    #[test]
    fn layout_tree_is_not_limited_to_two_panes() {
        let mut editors = OpenEditors::default();
        editors.open_permanent("/proj/a.rs", "a".into());
        editors.split(SplitDirection::Vertical).unwrap();
        editors.split(SplitDirection::Horizontal).unwrap();
        assert_eq!(editors.panes.len(), 3);
        assert_eq!(editors.layout.pane_ids().len(), 3);
    }
}
