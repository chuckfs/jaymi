//! Context Provider contract — subsystems propose [`ContextCandidate`]s.
//!
//! The Context Engine orchestrates [`ContextProvider`]s without depending on
//! their internal implementation. Providers expose deterministic
//! [`Self::relevance`] and [`Self::estimate_size`], plus a
//! [`Self::priority`]. The engine skips low-relevance providers, allocates
//! budget to higher-priority providers first, and fits oversized
//! contributions after materializing selected candidates.
//!
//! Sprint **B2.13.1:** every provider exposes candidates through
//! [`Self::propose_candidates`]. [`Self::contribute`] is a convenience that
//! materializes those candidates — not a parallel assemble path.

use jaymi_core::JaymiResult;
use jaymi_core::UserRequest;

use crate::bundle::{
    ActiveCapabilitiesSection, ActiveProjectSection, ActiveWorkspaceSection, ContextSessionInputs,
    ContextSource, ConversationSection, CurrentFileSection, CurrentSelectionSection,
    DiagnosticsSection, FileSummariesSection, GitStatusSection, MemoryResultsSection,
    OpenFilesSection, PermissionsSection, SearchResultsSection, WorkspaceInventorySection,
};
use crate::budget::{BudgetEstimate, ProviderPriority};
use crate::candidate::{materialize_candidates, ContextCandidate};
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
///
/// Produced by the Context Engine via [`materialize_candidates`] after Policy
/// selection — providers propose candidates; they do not merge into a bundle.
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
    /// Editor intelligence section derived from [`crate::EditorSnapshot`].
    pub editor_intelligence: Option<crate::EditorIntelligenceSection>,
    /// Project intelligence section derived from [`crate::ProjectSnapshot`].
    pub project_intelligence: Option<crate::ProjectIntelligenceSection>,
    /// Runtime intelligence section derived from [`crate::RuntimeSnapshot`].
    pub runtime_intelligence: Option<crate::RuntimeIntelligenceSection>,
    /// Workspace activity memory section derived from [`crate::WorkspaceMemorySnapshot`].
    pub workspace_memory: Option<crate::WorkspaceMemorySection>,
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
            && self.editor_intelligence.is_none()
            && self.project_intelligence.is_none()
            && self.runtime_intelligence.is_none()
            && self.workspace_memory.is_none()
    }
}

/// Subsystem that may propose data for a [`crate::ContextBundle`].
///
/// Implementations own their dependencies. The Context Engine calls
/// [`Self::relevance`], [`Self::priority`], [`Self::estimate_size`], then
/// [`Self::propose_candidates`] (Sprint B2.7 / B2.13.1). Context Policy scores
/// candidates; the engine materializes selected ones. The engine never inspects
/// provider internals.
///
/// **Providers must not assemble [`crate::ContextBundle`]s.** They only propose
/// candidates. They do not apply Context Policy or allocate budget.
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
    /// Used for budgeting before propose. Prefer over-estimates for bulky
    /// payloads so the engine can reserve room for higher-priority providers.
    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate;

    /// Propose [`ContextCandidate`] nodes for Context Policy selection.
    ///
    /// Required path for production providers (Sprint B2.13.1). Never builds a
    /// [`crate::ContextBundle`]. Return an empty vec when there is nothing to add.
    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>>;

    /// Materialize proposed candidates into a section contribution.
    ///
    /// Default implementation folds [`Self::propose_candidates`] through
    /// [`materialize_candidates`]. Prefer overriding [`Self::propose_candidates`]
    /// only — do not use this as a parallel assemble path.
    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        let candidates = self.propose_candidates(request)?;
        if candidates.is_empty() {
            Ok(None)
        } else {
            Ok(Some(materialize_candidates(&candidates)))
        }
    }
}
