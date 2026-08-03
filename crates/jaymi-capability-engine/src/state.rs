//! Temporary runtime state owned by an expanded capability workspace.
//!
//! Capability state is ephemeral. It disappears when the workspace closes
//! unless the caller explicitly promotes an entry elsewhere (conversation,
//! project memory, etc.).

use crate::{Capability, WorkspaceKind};

/// One open file in a coding workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileState {
    /// Filesystem path.
    pub path: String,
    /// True when the buffer has unsaved edits.
    pub dirty: bool,
}

/// One terminal session in a coding workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSessionState {
    /// Stable session id for this workspace lifetime.
    pub id: String,
    /// Working directory, when known.
    pub cwd: Option<String>,
    /// Last command preview (not a full scrollback buffer).
    pub last_command: Option<String>,
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
}

/// Temporary state for the Coding workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodingState {
    /// Open editor files.
    pub open_files: Vec<OpenFileState>,
    /// Active terminal sessions.
    pub terminal_sessions: Vec<TerminalSessionState>,
    /// Current diagnostics.
    pub diagnostics: Vec<DiagnosticState>,
}

impl CodingState {
    /// Number of tracked entries across open files, terminals, and diagnostics.
    pub fn entry_count(&self) -> usize {
        self.open_files.len() + self.terminal_sessions.len() + self.diagnostics.len()
    }
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
                        .open_files
                        .iter()
                        .find(|file| file.path == entry_id)
                        .map(|file| format!("Open file: {}", file.path))
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
