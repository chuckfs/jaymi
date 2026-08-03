//! UI workspace expansion requested by capabilities.
//!
//! Conversation is permanent. Workspaces are temporary expansions that grow
//! from the right side of the conversation and never destroy it.

use crate::Capability;

/// Kind of experience workspace that may expand beside the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceKind {
    /// Default conversation-only experience (no expansion).
    Conversation,
    /// Coding / IDE workspace (explorer, editor, terminal, git, diagnostics).
    Coding,
    /// Creation / canvas workspace (images, canvas, assets).
    Creation,
    /// Research workspace (sources, documents, notes, citations, search).
    Research,
}

impl WorkspaceKind {
    /// Stable id for diagnostics and persistence.
    pub fn id(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Coding => "coding",
            Self::Creation => "creation",
            Self::Research => "research",
        }
    }

    /// Human-readable title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Conversation => "Conversation",
            Self::Coding => "Coding Workspace",
            Self::Creation => "Creation Workspace",
            Self::Research => "Research Workspace",
        }
    }

    /// True when this kind expands the UI beyond conversation-only.
    pub fn expands(self) -> bool {
        !matches!(self, Self::Conversation)
    }

    /// Parse a stable workspace id.
    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim() {
            "conversation" => Some(Self::Conversation),
            "coding" | "ide" => Some(Self::Coding),
            "creation" | "canvas" => Some(Self::Creation),
            "research" => Some(Self::Research),
            _ => None,
        }
    }
}

/// Where a workspace expands relative to the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceEdge {
    /// Expand from the right of the conversation (default).
    Right,
}

impl WorkspaceEdge {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
        }
    }
}

/// Panels that belong inside an expanded workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspacePanel {
    ProjectExplorer,
    Editor,
    Terminal,
    Git,
    Diagnostics,
    ImageGeneration,
    ImageEditing,
    Canvas,
    PromptHistory,
    Assets,
    Sources,
    Documents,
    Notes,
    Citations,
    Search,
}

impl WorkspacePanel {
    /// Stable id.
    pub fn id(self) -> &'static str {
        match self {
            Self::ProjectExplorer => "project_explorer",
            Self::Editor => "editor",
            Self::Terminal => "terminal",
            Self::Git => "git",
            Self::Diagnostics => "diagnostics",
            Self::ImageGeneration => "image_generation",
            Self::ImageEditing => "image_editing",
            Self::Canvas => "canvas",
            Self::PromptHistory => "prompt_history",
            Self::Assets => "assets",
            Self::Sources => "sources",
            Self::Documents => "documents",
            Self::Notes => "notes",
            Self::Citations => "citations",
            Self::Search => "search",
        }
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectExplorer => "Project Explorer",
            Self::Editor => "Editor",
            Self::Terminal => "Terminal",
            Self::Git => "Git",
            Self::Diagnostics => "Diagnostics",
            Self::ImageGeneration => "Image Generation",
            Self::ImageEditing => "Image Editing",
            Self::Canvas => "Canvas",
            Self::PromptHistory => "Prompt History",
            Self::Assets => "Assets",
            Self::Sources => "Sources",
            Self::Documents => "Documents",
            Self::Notes => "Notes",
            Self::Citations => "Citations",
            Self::Search => "Search",
        }
    }
}

/// Capability-driven request to expand (or keep) a workspace.
///
/// The Capability Engine never renders UI. It only describes the expansion the
/// Planner and desktop experience should honor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceExpansion {
    /// Workspace kind to show.
    pub kind: WorkspaceKind,
    /// Capability that requested the expansion.
    pub capability: Capability,
    /// Edge the workspace expands from (always right for Jaymi).
    pub expands_from: WorkspaceEdge,
    /// Panels that belong in this workspace.
    pub panels: Vec<WorkspacePanel>,
    /// Short reason for diagnostics / UI chrome.
    pub reason: String,
}

impl WorkspaceExpansion {
    /// True when the UI should expand beyond conversation-only.
    pub fn expands(&self) -> bool {
        self.kind.expands()
    }

    /// Workspace title.
    pub fn title(&self) -> &'static str {
        self.kind.title()
    }

    /// Compact summary for logs and tests.
    pub fn summary(&self) -> String {
        let panels: Vec<_> = self.panels.iter().map(|panel| panel.id()).collect();
        format!(
            "workspace={} capability={} edge={} panels=[{}]",
            self.kind.id(),
            self.capability.id(),
            self.expands_from.as_str(),
            panels.join(",")
        )
    }
}

/// Panels for a workspace kind.
pub fn workspace_panels(kind: WorkspaceKind) -> Vec<WorkspacePanel> {
    match kind {
        WorkspaceKind::Conversation => vec![],
        WorkspaceKind::Coding => vec![
            WorkspacePanel::ProjectExplorer,
            WorkspacePanel::Editor,
            WorkspacePanel::Terminal,
            WorkspacePanel::Git,
            WorkspacePanel::Diagnostics,
        ],
        WorkspaceKind::Creation => vec![
            WorkspacePanel::ImageGeneration,
            WorkspacePanel::ImageEditing,
            WorkspacePanel::Canvas,
            WorkspacePanel::PromptHistory,
            WorkspacePanel::Assets,
        ],
        WorkspaceKind::Research => vec![
            WorkspacePanel::Sources,
            WorkspacePanel::Documents,
            WorkspacePanel::Notes,
            WorkspacePanel::Citations,
            WorkspacePanel::Search,
        ],
    }
}

/// Workspace a capability requests, when any.
///
/// Returns [`None`] for conversation-only capabilities (no expansion).
pub fn capability_workspace(capability: Capability) -> Option<WorkspaceKind> {
    match capability {
        Capability::Code => Some(WorkspaceKind::Coding),
        Capability::GenerateImages => Some(WorkspaceKind::Creation),
        Capability::Search
        | Capability::ReadDocuments
        | Capability::Discover
        | Capability::BrowseTheWeb
        | Capability::Embeddings => Some(WorkspaceKind::Research),
        Capability::Chat => None,
        // Other capabilities stay conversation-first until they earn a surface.
        _ => None,
    }
}

/// Build a workspace expansion request for a capability, when applicable.
pub fn workspace_expansion_for(
    capability: Capability,
    reason: impl Into<String>,
) -> Option<WorkspaceExpansion> {
    let kind = capability_workspace(capability)?;
    Some(WorkspaceExpansion {
        kind,
        capability,
        expands_from: WorkspaceEdge::Right,
        panels: workspace_panels(kind),
        reason: reason.into(),
    })
}
