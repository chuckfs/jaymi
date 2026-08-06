//! First-class immutable [`ContextBundle`] — the request-context snapshot.
//!
//! The bundle is assembled by the Context Engine from the rest of the system.
//! It never searches, reasons, or executes. Once [`ContextBundleBuilder::build`]
//! returns, the snapshot is immutable: fields are private and only accessors
//! are exposed. Planner execution, Behaviors, and future LLM providers take
//! this object as their standard context input.

use jaymi_memory_engine::{
    AssembledMemoryContext, PromotionAskDecision, PromotionSuggestion,
};
use jaymi_project_engine::ProjectContext;

/// Sources that contributed to an assembled context bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextSource {
    /// Currently open project (Project Engine).
    ActiveProject,
    /// Prior turns / conversation-scoped memories.
    PreviousConversation,
    /// Search Engine contribution (query pending, index summary, or attached hits).
    SearchResults,
    /// Memories selected by the Memory Engine.
    RetrievedMemories,
    /// Active UX workspace from the experience session.
    ActiveWorkspace,
    /// Promotion suggestions derived from memory.
    PromotionSuggestions,
    /// Current editor file / selection / open tabs from the session.
    EditorState,
    /// Diagnostics attached for the request.
    Diagnostics,
    /// Permission grants / decisions attached for the request.
    Permissions,
    /// Active capabilities recorded for the request.
    ActiveCapabilities,
    /// User request metadata derived from the inbound request.
    UserRequest,
}

impl ContextSource {
    /// Stable label for diagnostics and UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActiveProject => "active_project",
            Self::PreviousConversation => "previous_conversation",
            Self::SearchResults => "search_results",
            Self::RetrievedMemories => "retrieved_memories",
            Self::ActiveWorkspace => "active_workspace",
            Self::PromotionSuggestions => "promotion_suggestions",
            Self::EditorState => "editor_state",
            Self::Diagnostics => "diagnostics",
            Self::Permissions => "permissions",
            Self::ActiveCapabilities => "active_capabilities",
            Self::UserRequest => "user_request",
        }
    }
}

/// Conversation section — active conversation identity and summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationSection {
    /// Active conversation id, when any.
    pub id: Option<String>,
    /// Optional title.
    pub title: Option<String>,
    /// Lifecycle status label (`active` / `archived` / `closed`), when known.
    pub status: Option<String>,
    /// Owning project id, when any.
    pub project_id: Option<String>,
    /// Number of messages when the transcript was sampled.
    pub message_count: Option<usize>,
}

/// Active project section — identity plus optional full Project Engine snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveProjectSection {
    /// Project id, when a project is open.
    pub project_id: Option<String>,
    /// Display name.
    pub name: Option<String>,
    /// Root directory path.
    pub root_directory: Option<String>,
    /// Full Project Engine context when assembled.
    pub detail: Option<ProjectContext>,
}

/// Active UX workspace section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveWorkspaceSection {
    /// Workspace kind id (`coding`, `research`, `creative`, …).
    pub kind_id: Option<String>,
}

/// Current file in the active editor, when any.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentFileSection {
    /// Absolute path.
    pub path: Option<String>,
    /// True when the buffer has unsaved edits.
    pub dirty: bool,
    /// Language id when known (`rust`, `typescript`, …).
    pub language: Option<String>,
}

/// Current text selection in the active editor, when any.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentSelectionSection {
    /// Path the selection belongs to.
    pub path: Option<String>,
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based start column.
    pub start_column: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end column.
    pub end_column: u32,
    /// Selected text when captured.
    pub text: Option<String>,
}

/// One open editor tab / buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileEntry {
    /// Absolute path.
    pub path: String,
    /// True when the buffer has unsaved edits.
    pub dirty: bool,
    /// True when this is the focused / active tab.
    pub active: bool,
}

/// Open files section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenFilesSection {
    /// Open buffers in display order.
    pub files: Vec<OpenFileEntry>,
}

/// Lightweight search coordination included when a structured search request
/// is present. Full retrieval still happens through tools — this does not
/// execute search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchContextHint {
    /// True when the user request carries a structured search query.
    pub structured_query_pending: bool,
    /// Free-text query preview, when any.
    pub query_preview: Option<String>,
    /// Active project search index document count, when a project is open.
    pub project_indexed_documents: Option<u64>,
}

/// One search hit attached to the bundle (never produced by searching here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleSearchHit {
    /// Stable item identity.
    pub item_id: String,
    /// Display title.
    pub title: String,
    /// Absolute path when applicable.
    pub path: Option<String>,
    /// Relevance score when known.
    pub score: Option<u32>,
    /// Match reason label when known.
    pub match_reason: Option<String>,
    /// Optional preview / snippet.
    pub preview: Option<String>,
    /// Zero-based start line when known.
    pub line: Option<u32>,
    /// Zero-based start column when known.
    pub column: Option<u32>,
}

/// Search results section — coordination hint plus any pre-attached hits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResultsSection {
    /// Lightweight coordination hint (does not replace tool search).
    pub hint: Option<SearchContextHint>,
    /// Hits supplied by the rest of the system (Search Engine / prior tool).
    pub hits: Vec<BundleSearchHit>,
}

/// Memory results section — relevant memories and promotion suggestions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryResultsSection {
    /// Relevant memories for the request (never a full dump).
    pub memory: AssembledMemoryContext,
    /// Promotion suggestions (never auto-applied).
    pub promotion_suggestions: Vec<PromotionSuggestion>,
    /// Whether the Planner should ask the user about promotions.
    pub promotion_ask: PromotionAskDecision,
}

impl Default for MemoryResultsSection {
    fn default() -> Self {
        Self {
            memory: AssembledMemoryContext {
                memories: Vec::new(),
                project_id: None,
                conversation_id: None,
                candidate_count: 0,
                truncated: false,
            },
            promotion_suggestions: Vec::new(),
            promotion_ask: PromotionAskDecision::Defer,
        }
    }
}

/// One diagnostic attached for the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleDiagnostic {
    /// Absolute path when known.
    pub path: Option<String>,
    /// Severity label (`error` / `warning` / `info` / `hint`).
    pub severity: String,
    /// Diagnostic message.
    pub message: String,
    /// Zero-based start line when known.
    pub line: Option<u32>,
    /// Zero-based start column when known.
    pub column: Option<u32>,
    /// Source / code when known (`rustc`, `eslint`, …).
    pub source: Option<String>,
}

/// Diagnostics section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosticsSection {
    /// Attached diagnostics for this request.
    pub diagnostics: Vec<BundleDiagnostic>,
}

/// One permission grant / decision snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePermissionEntry {
    /// Permission category label.
    pub category: String,
    /// Permission action label.
    pub action: String,
    /// Decision label (`allowed` / `denied` / `requires_approval`).
    pub decision: String,
    /// Optional resource path or identifier.
    pub resource: Option<String>,
    /// Human-readable explanation.
    pub explanation: Option<String>,
}

/// Permissions section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionsSection {
    /// Known grants / decisions attached for this request.
    pub entries: Vec<BundlePermissionEntry>,
}

/// Report of how budgeting shaped the assembled bundle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BudgetReport {
    /// Configured character limit (including reserved).
    pub max_characters: usize,
    /// Optional token limit.
    pub max_tokens: Option<usize>,
    /// Characters consumed by accepted provider contributions.
    pub used_characters: usize,
    /// Estimated tokens for `used_characters`.
    pub estimated_tokens: usize,
    /// Providers whose contributions were truncated / summarized.
    pub truncated_providers: Vec<String>,
    /// Providers skipped because nothing would fit.
    pub skipped_budget: Vec<String>,
    /// Human-readable summaries produced while fitting.
    pub summaries: Vec<String>,
}

/// Planner-facing metadata about how this bundle was assembled.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerMetadataSection {
    /// Monotonic assemble generation for diagnostics / tests.
    pub assemble_generation: u64,
    /// Sources included in this bundle.
    pub sources: Vec<ContextSource>,
    /// Optional free-form notes for diagnostics.
    pub notes: Vec<String>,
    /// Budget accounting for this assemble (LLM-ready).
    pub budget: Option<BudgetReport>,
    /// Context Policy explainability report.
    pub policy: Option<crate::PolicyReport>,
}

/// Active capabilities recorded for this request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveCapabilitiesSection {
    /// Stable capability ids (`code`, `search`, …).
    pub capability_ids: Vec<String>,
}

/// Metadata derived from the inbound [`jaymi_core::UserRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserRequestMetadataSection {
    /// Truncated content preview for logs / providers.
    pub content_preview: String,
    /// True when a structured directory list was requested.
    pub has_directory: bool,
    /// True when a structured file read was requested.
    pub has_file: bool,
    /// True when a structured write was requested.
    pub has_write_file: bool,
    /// True when a structured search was requested.
    pub has_search: bool,
    /// True when project-knowledge search was requested.
    pub has_project_knowledge: bool,
    /// True when a terminal operation was requested.
    pub has_terminal: bool,
    /// True when a Git operation was requested.
    pub has_git: bool,
    /// True when an LSP operation was requested.
    pub has_lsp: bool,
    /// True when discovery / index was requested.
    pub has_discover_or_index: bool,
    /// True when open/close project was requested.
    pub has_project_session: bool,
}

impl UserRequestMetadataSection {
    /// Derive metadata from a user request without interpreting intent.
    pub fn from_request(request: &jaymi_core::UserRequest) -> Self {
        const PREVIEW_CHARS: usize = 120;
        let content = request.content.trim();
        let content_preview = if content.chars().count() > PREVIEW_CHARS {
            let truncated: String = content.chars().take(PREVIEW_CHARS).collect();
            format!("{truncated}…")
        } else {
            content.to_string()
        };
        Self {
            content_preview,
            has_directory: request.directory.is_some() || request.project_tree.is_some(),
            has_file: request.file.is_some(),
            has_write_file: request.write_file.is_some() || request.manage_path.is_some(),
            has_search: request.search.is_some(),
            has_project_knowledge: request.project_knowledge.is_some(),
            has_terminal: request.terminal.is_some(),
            has_git: request.git.is_some(),
            has_lsp: request.lsp.is_some(),
            has_discover_or_index: request.discover
                || request.discovery_kind.is_some()
                || request.index_root.is_some(),
            has_project_session: request.open_project_id.is_some() || request.close_project,
        }
    }
}

/// Complete context assembled for a single request.
///
/// Immutable once constructed via [`ContextBundleBuilder::build`]. This is the
/// standard object passed into Planner execution, Behaviors, and future LLM
/// providers. It is a pure snapshot — it does not search or reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    conversation: ConversationSection,
    active_project: ActiveProjectSection,
    active_workspace: ActiveWorkspaceSection,
    current_file: CurrentFileSection,
    current_selection: CurrentSelectionSection,
    open_files: OpenFilesSection,
    search_results: SearchResultsSection,
    memory_results: MemoryResultsSection,
    diagnostics: DiagnosticsSection,
    permissions: PermissionsSection,
    planner_metadata: PlannerMetadataSection,
    active_capabilities: ActiveCapabilitiesSection,
    user_request: UserRequestMetadataSection,
}

impl Default for ContextBundle {
    fn default() -> Self {
        ContextBundleBuilder::new().build()
    }
}

impl ContextBundle {
    /// Start building an immutable context bundle.
    pub fn builder() -> ContextBundleBuilder {
        ContextBundleBuilder::new()
    }

    /// Conversation section.
    pub fn conversation(&self) -> &ConversationSection {
        &self.conversation
    }

    /// Active project section.
    pub fn active_project(&self) -> &ActiveProjectSection {
        &self.active_project
    }

    /// Active workspace section.
    pub fn active_workspace(&self) -> &ActiveWorkspaceSection {
        &self.active_workspace
    }

    /// Current file section.
    pub fn current_file(&self) -> &CurrentFileSection {
        &self.current_file
    }

    /// Current selection section.
    pub fn current_selection(&self) -> &CurrentSelectionSection {
        &self.current_selection
    }

    /// Open files section.
    pub fn open_files(&self) -> &OpenFilesSection {
        &self.open_files
    }

    /// Search results section.
    pub fn search_results(&self) -> &SearchResultsSection {
        &self.search_results
    }

    /// Memory results section.
    pub fn memory_results(&self) -> &MemoryResultsSection {
        &self.memory_results
    }

    /// Diagnostics section.
    pub fn diagnostics(&self) -> &DiagnosticsSection {
        &self.diagnostics
    }

    /// Permissions section.
    pub fn permissions(&self) -> &PermissionsSection {
        &self.permissions
    }

    /// Planner metadata section.
    pub fn planner_metadata(&self) -> &PlannerMetadataSection {
        &self.planner_metadata
    }

    /// Active capabilities section.
    pub fn active_capabilities(&self) -> &ActiveCapabilitiesSection {
        &self.active_capabilities
    }

    /// User request metadata section.
    pub fn user_request(&self) -> &UserRequestMetadataSection {
        &self.user_request
    }

    // --- Compatibility accessors used by Planner / existing tests ---

    /// Sources included in this bundle (Planner metadata).
    pub fn sources(&self) -> &[ContextSource] {
        &self.planner_metadata.sources
    }

    /// Relevant memories for the request.
    pub fn memory(&self) -> &AssembledMemoryContext {
        &self.memory_results.memory
    }

    /// Promotion suggestions.
    pub fn promotion_suggestions(&self) -> &[PromotionSuggestion] {
        &self.memory_results.promotion_suggestions
    }

    /// Promotion ask decision.
    pub fn promotion_ask(&self) -> PromotionAskDecision {
        self.memory_results.promotion_ask
    }

    /// Open project workspace context, when a project is active.
    pub fn project(&self) -> Option<&ProjectContext> {
        self.active_project.detail.as_ref()
    }

    /// Active UX workspace kind id, when set.
    pub fn workspace_kind(&self) -> Option<&str> {
        self.active_workspace.kind_id.as_deref()
    }

    /// Search coordination hint, when any.
    pub fn search(&self) -> Option<&SearchContextHint> {
        self.search_results.hint.as_ref()
    }

    /// Monotonic assemble generation for diagnostics / tests.
    pub fn assemble_generation(&self) -> u64 {
        self.planner_metadata.assemble_generation
    }

    /// Budget report for this assemble, when recorded.
    pub fn budget(&self) -> Option<&BudgetReport> {
        self.planner_metadata.budget.as_ref()
    }

    /// Context Policy report for this assemble, when recorded.
    pub fn policy(&self) -> Option<&crate::PolicyReport> {
        self.planner_metadata.policy.as_ref()
    }

    /// Convert this bundle into the LLM-facing structured representation.
    ///
    /// Pure data transform — no model calls, no prompt construction.
    pub fn to_llm_context(&self) -> crate::LlmContext {
        crate::LlmContext::from_bundle(self)
    }

    /// Restamp a cached bundle for a new assemble entry (generation + request metadata).
    ///
    /// Preserves provider sections; updates planner generation / notes and
    /// user-request metadata so cache hits stay correct for this request.
    pub fn restamp_cache_hit(
        mut self,
        assemble_generation: u64,
        request: &jaymi_core::UserRequest,
        note: impl Into<String>,
    ) -> Self {
        self.planner_metadata.assemble_generation = assemble_generation;
        self.planner_metadata.notes.push(note.into());
        self.user_request = UserRequestMetadataSection::from_request(request);
        self
    }
}

/// Builder for an immutable [`ContextBundle`].
///
/// Collects sections, then [`Self::build`] consumes the builder and freezes
/// the snapshot. The bundle performs no searching or reasoning.
#[derive(Debug, Clone, Default)]
pub struct ContextBundleBuilder {
    conversation: ConversationSection,
    active_project: ActiveProjectSection,
    active_workspace: ActiveWorkspaceSection,
    current_file: CurrentFileSection,
    current_selection: CurrentSelectionSection,
    open_files: OpenFilesSection,
    search_results: SearchResultsSection,
    memory_results: MemoryResultsSection,
    diagnostics: DiagnosticsSection,
    permissions: PermissionsSection,
    planner_metadata: PlannerMetadataSection,
    active_capabilities: ActiveCapabilitiesSection,
    user_request: UserRequestMetadataSection,
}

impl ContextBundleBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the conversation section.
    pub fn conversation(mut self, section: ConversationSection) -> Self {
        self.conversation = section;
        self
    }

    /// Set the active project section.
    pub fn active_project(mut self, section: ActiveProjectSection) -> Self {
        self.active_project = section;
        self
    }

    /// Set the active workspace section.
    pub fn active_workspace(mut self, section: ActiveWorkspaceSection) -> Self {
        self.active_workspace = section;
        self
    }

    /// Set the current file section.
    pub fn current_file(mut self, section: CurrentFileSection) -> Self {
        self.current_file = section;
        self
    }

    /// Set the current selection section.
    pub fn current_selection(mut self, section: CurrentSelectionSection) -> Self {
        self.current_selection = section;
        self
    }

    /// Set the open files section.
    pub fn open_files(mut self, section: OpenFilesSection) -> Self {
        self.open_files = section;
        self
    }

    /// Set the search results section.
    pub fn search_results(mut self, section: SearchResultsSection) -> Self {
        self.search_results = section;
        self
    }

    /// Set the memory results section.
    pub fn memory_results(mut self, section: MemoryResultsSection) -> Self {
        self.memory_results = section;
        self
    }

    /// Set the diagnostics section.
    pub fn diagnostics(mut self, section: DiagnosticsSection) -> Self {
        self.diagnostics = section;
        self
    }

    /// Set the permissions section.
    pub fn permissions(mut self, section: PermissionsSection) -> Self {
        self.permissions = section;
        self
    }

    /// Set the planner metadata section.
    pub fn planner_metadata(mut self, section: PlannerMetadataSection) -> Self {
        self.planner_metadata = section;
        self
    }

    /// Set the active capabilities section.
    pub fn active_capabilities(mut self, section: ActiveCapabilitiesSection) -> Self {
        self.active_capabilities = section;
        self
    }

    /// Set the user request metadata section.
    pub fn user_request(mut self, section: UserRequestMetadataSection) -> Self {
        self.user_request = section;
        self
    }

    /// Merge a provider contribution into this builder.
    ///
    /// Only `Some` section fields replace existing values. Source tags are
    /// ignored here — the Context Engine folds them into planner metadata.
    pub fn apply_contribution(mut self, contribution: crate::ContextContribution) -> Self {
        if let Some(section) = contribution.conversation {
            self.conversation = section;
        }
        if let Some(section) = contribution.active_project {
            self.active_project = section;
        }
        if let Some(section) = contribution.active_workspace {
            self.active_workspace = section;
        }
        if let Some(section) = contribution.current_file {
            self.current_file = section;
        }
        if let Some(section) = contribution.current_selection {
            self.current_selection = section;
        }
        if let Some(section) = contribution.open_files {
            self.open_files = section;
        }
        if let Some(section) = contribution.search_results {
            self.search_results = section;
        }
        if let Some(section) = contribution.memory_results {
            self.memory_results = section;
        }
        if let Some(section) = contribution.diagnostics {
            self.diagnostics = section;
        }
        if let Some(section) = contribution.permissions {
            self.permissions = section;
        }
        if let Some(section) = contribution.active_capabilities {
            self.active_capabilities = section;
        }
        let _ = contribution.sources;
        self
    }

    /// Freeze the collected sections into an immutable [`ContextBundle`].
    pub fn build(self) -> ContextBundle {
        ContextBundle {
            conversation: self.conversation,
            active_project: self.active_project,
            active_workspace: self.active_workspace,
            current_file: self.current_file,
            current_selection: self.current_selection,
            open_files: self.open_files,
            search_results: self.search_results,
            memory_results: self.memory_results,
            diagnostics: self.diagnostics,
            permissions: self.permissions,
            planner_metadata: self.planner_metadata,
            active_capabilities: self.active_capabilities,
            user_request: self.user_request,
        }
    }
}

/// Session inputs the host may push before assemble (editor / UX state).
///
/// These values are copied into the bundle; the Context Engine does not
/// discover them by searching the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextSessionInputs {
    /// Active UX workspace kind id.
    pub workspace_kind: Option<String>,
    /// Current editor file.
    pub current_file: CurrentFileSection,
    /// Current editor selection.
    pub current_selection: CurrentSelectionSection,
    /// Open editor tabs.
    pub open_files: OpenFilesSection,
    /// Diagnostics attached for the next request.
    pub diagnostics: DiagnosticsSection,
    /// Permission grants attached for the next request.
    pub permissions: PermissionsSection,
    /// Active capability ids for the next request.
    pub active_capabilities: ActiveCapabilitiesSection,
    /// Pre-attached search hits (not queried here).
    pub search_hits: Vec<BundleSearchHit>,
}
