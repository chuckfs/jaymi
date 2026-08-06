//! Context Provider contract — subsystems contribute to a [`ContextBundle`].
//!
//! The Context Engine orchestrates [`ContextProvider`]s without depending on
//! their internal implementation. Providers expose deterministic
//! [`Self::relevance`] and [`Self::estimate_size`], plus a
//! [`Self::priority`]. The engine skips low-relevance providers, allocates
//! budget to higher-priority providers first, and fits oversized
//! contributions. Providers may also return [`None`] from [`Self::contribute`]
//! when they have nothing to add.

use jaymi_core::JaymiResult;
use jaymi_core::UserRequest;

use crate::bundle::{
    ActiveCapabilitiesSection, ActiveProjectSection, ActiveWorkspaceSection, ContextSessionInputs,
    ContextSource, ConversationSection, CurrentFileSection, CurrentSelectionSection,
    DiagnosticsSection, FileSummariesSection, GitStatusSection, MemoryResultsSection,
    OpenFilesSection, PermissionsSection, SearchResultsSection, WorkspaceInventorySection,
};
use crate::budget::{BudgetEstimate, ProviderPriority};
use crate::relevance::{RelevanceScore, RelevanceSignals};

/// Read-only inputs available to every provider during assemble.
#[derive(Debug, Clone, Copy)]
pub struct ProviderRequest<'a> {
    /// Inbound user request.
    pub request: &'a UserRequest,
    /// Host session snapshot (workspace / editor / diagnostics / …).
    pub session: &'a ContextSessionInputs,
    /// Deterministic relevance cues (intent / capability / workspace / kind).
    pub relevance: &'a RelevanceSignals,
}

/// Partial context contribution from one provider.
///
/// Only populated fields are merged into the bundle builder. Empty / default
/// sections should be omitted (`None`) so other providers are not overwritten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextContribution {
    /// Sources this provider claims for the request.
    pub sources: Vec<ContextSource>,
    /// Conversation section, when contributing.
    pub conversation: Option<ConversationSection>,
    /// Active project section, when contributing.
    pub active_project: Option<ActiveProjectSection>,
    /// Active workspace section, when contributing.
    pub active_workspace: Option<ActiveWorkspaceSection>,
    /// Current file section, when contributing.
    pub current_file: Option<CurrentFileSection>,
    /// Current selection section, when contributing.
    pub current_selection: Option<CurrentSelectionSection>,
    /// Open files section, when contributing.
    pub open_files: Option<OpenFilesSection>,
    /// Search results section, when contributing.
    pub search_results: Option<SearchResultsSection>,
    /// Memory results section, when contributing.
    pub memory_results: Option<MemoryResultsSection>,
    /// Diagnostics section, when contributing.
    pub diagnostics: Option<DiagnosticsSection>,
    /// Git status section, when contributing.
    pub git_status: Option<GitStatusSection>,
    /// Workspace inventory section, when contributing.
    pub workspace_inventory: Option<WorkspaceInventorySection>,
    /// File summaries section, when contributing.
    pub file_summaries: Option<FileSummariesSection>,
    /// Permissions section, when contributing.
    pub permissions: Option<PermissionsSection>,
    /// Active capabilities section, when contributing.
    pub active_capabilities: Option<ActiveCapabilitiesSection>,
}

impl ContextContribution {
    /// Create an empty contribution (sources still may be filled).
    pub fn new() -> Self {
        Self::default()
    }

    /// True when this contribution carries no section data and no sources.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
            && self.conversation.is_none()
            && self.active_project.is_none()
            && self.active_workspace.is_none()
            && self.current_file.is_none()
            && self.current_selection.is_none()
            && self.open_files.is_none()
            && self.search_results.is_none()
            && self.memory_results.is_none()
            && self.diagnostics.is_none()
            && self.git_status.is_none()
            && self.workspace_inventory.is_none()
            && self.file_summaries.is_none()
            && self.permissions.is_none()
            && self.active_capabilities.is_none()
    }
}

/// Subsystem that may contribute data to a [`crate::ContextBundle`].
///
/// Implementations own their dependencies. The Context Engine calls
/// [`Self::relevance`], [`Self::priority`], [`Self::estimate_size`], then
/// [`Self::contribute`] — it never inspects provider internals. Context
/// Policies decide participation without mutating providers.
pub trait ContextProvider: Send + Sync {
    /// Stable provider identity for diagnostics and logs.
    fn id(&self) -> &'static str;

    /// Budget priority — higher values receive character/token budget first.
    fn priority(&self) -> ProviderPriority;

    /// How sensitive this provider's contributions are.
    ///
    /// Used by Context Policies; providers must not change this dynamically
    /// based on gathered content beyond their declared category.
    fn sensitivity(&self) -> crate::Sensitivity {
        crate::Sensitivity::for_provider(self.id())
    }

    /// Deterministic relevance of this provider for the current request (0..=100).
    ///
    /// Considers user intent tags, active capabilities, workspace, and request
    /// kind. Must not use AI / model scoring.
    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore;

    /// Estimate the size of a contribution without performing heavy work when possible.
    ///
    /// Used for budgeting before [`Self::contribute`]. Prefer over-estimates for
    /// bulky payloads so the engine can reserve room for higher-priority providers.
    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate;

    /// Optionally contribute context for this request.
    ///
    /// Only called when Context Policies allow participation, relevance meets
    /// the engine threshold (unless bypassed), and budget remains. Return
    /// `Ok(None)` to decline (nothing to add).
    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>>;
}
