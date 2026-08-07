//! Context Engine for Jaymi.
//!
//! Assembles only the context required for the current request. The Planner
//! resolves Intent and Capability first, then calls
//! [`ContextEngine::assemble_with`] with [`AssembleHints`]. The engine does
//! not coordinate Memory, Project, Search, or session workspace state itself.
//!
//! **Sole factory:** [`ContextEngine`] is the only production creator of
//! [`ContextBundle`] (`assemble_with` / `assemble` / `empty_bundle` /
//! `reuse_bundle`). The Planner requests bundles; it never constructs them.
//!
//! Assemble is provider-driven: registered [`ContextProvider`]s propose
//! [`ContextCandidate`] nodes (Sprint B2.7). Context Policy scores relevance,
//! recency, importance, privacy, and budget; only selected candidates are
//! materialized into an immutable [`ContextBundle`]. Providers never assemble
//! bundles. Ready for future LLM context windows.
//!
//! Recently assembled bundles are cached (keyed by project, workspace,
//! conversation revision, session fingerprint, active file, and request type).
//! The cache never changes correctness: entries are fingerprinted and
//! invalidated when files, project, workspace, conversation, diagnostics, or
//! the search index change. Planner asks for a fresh assemble via
//! [`ContextEngine::request_fresh_context`] and never touches cache keys.
//!
//! Context History retains recent bundles (timestamp, request, providers,
//! size, duration) for debugging and future reasoning transparency.
//!
//! The LLM-facing Context API converts a [`ContextBundle`] into a structured
//! [`LlmContext`] for future model consumers — no model calls, no prompts.
//!
//! Context Policies filter and prioritize providers before assembly so bundles
//! stay relevant, minimal, privacy-aware, deterministic, and explainable.
//!
//! The bundle is the standard object passed into Planner execution, Behaviors,
//! and future LLM providers. It never searches or reasons.

#![forbid(unsafe_code)]

mod budget;
mod bundle;
mod candidate;
mod cache;
mod history;
mod inspector;
mod llm;
mod policy;
mod provider;
mod providers;
mod relevance;
mod workspace_snapshot;
mod editor_snapshot;
mod project_snapshot;
mod git_snapshot;
mod runtime_snapshot;
mod workspace_memory;

pub use candidate::{
    candidates_from_contribution, materialize_candidates, score_candidate, score_recency,
    select_candidates_for_budget, CandidateDecisionSummary, CandidateEdge, CandidateGraph,
    CandidateItemDecision, CandidatePayload, CandidateScores, CandidateSelection,
    CandidateSelectionReport, ContextCandidate, ContextCandidateId, ContextCandidateKind,
};
pub use budget::{
    fit_contribution, measure_contribution, BudgetEstimate, BudgetUnits, ContextBudgetConfig,
    ProviderPriority, DEFAULT_CHARS_PER_TOKEN, DEFAULT_MAX_CHARACTERS, ENGINE_RESERVED_CHARACTERS,
};
pub use bundle::{
    ActiveCapabilitiesSection, ActiveProjectSection, ActiveWorkspaceSection, BudgetReport,
    BundleDiagnostic, BundlePermissionEntry, BundleSearchHit, ContextBundle, ContextBundleBuilder,
    ContextSessionInputs, ContextSource, ConversationSection, CurrentFileSection,
    CurrentSelectionSection, DiagnosticsSection, FileSummariesSection, FileSummaryEntry,
    GitStatusSection, MemoryResultsSection, OpenFileEntry, OpenFilesSection, PermissionsSection,
    PlannerMetadataSection, SearchContextHint, SearchResultsSection, UserRequestMetadataSection,
    WorkspaceInventorySection,
};
pub use cache::{
    fingerprint_request, fingerprint_session, CacheIdentity, ContextBundleCache, ContextCacheEntry,
    ContextCacheKey, ContextCacheStats, DEFAULT_CACHE_CAPACITY,
};
pub use history::{ContextHistory, ContextHistoryEntry, DEFAULT_HISTORY_CAPACITY};
pub use llm::{
    LlmActiveCapabilities, LlmActiveProject, LlmActiveWorkspace, LlmBudgetView, LlmContext,
    LlmContextSection, LlmConversation, LlmCurrentFile, LlmCurrentSelection, LlmDiagnostic,
    LlmDiagnostics, LlmEditorCodeLens, LlmEditorHover, LlmEditorIntelligence, LlmEditorReference,
    LlmEditorSymbol, LlmEnvironmentalResolution, LlmFileSummaries, LlmFileSummaryEntry,
    LlmGitCommit, LlmGitStatus, LlmMemoryItem, LlmMemoryResults, LlmOpenFileEntry, LlmOpenFiles,
    LlmPermissionEntry, LlmPermissions, LlmProjectDetailSummary, LlmProjectIntelligence,
    LlmPromotionSuggestion, LlmProviderMetadata, LlmRuntimeIntelligence, LlmSearchHint,
    LlmSearchHit, LlmSearchResults, LlmSectionContent, LlmSectionId, LlmUserRequest,
    LlmWorkspaceInventory, LlmWorkspaceMemory, LLM_CONTEXT_SCHEMA_VERSION,
};
pub use policy::{
    apply_contribution_constraints, apply_policy_to_contribution, assess_context_selection,
    default_context_policies, ContextPolicy, ContextPolicyCandidate, ContextPolicyDecision,
    ContextPolicyDecisionRecord, ContextPolicyEngine, ContextPolicyInputs,
    ContextSelectionAssessment, ContextSelectionProfile, ContributionConstraints,
    JaymiDefaultContextPolicy, PolicyDecisionSummary, PolicyReport, Sensitivity,
    DEFAULT_CONTEXT_POLICY_ID,
};
pub use provider::{ContextContribution, ContextProvider, ProviderRequest};
pub use providers::{
    default_providers, ConversationProvider, DiagnosticsProvider, EditorProvider,
    FileSummariesProvider, GitStatusProvider, MemoryProvider, PermissionProvider, ProjectProvider,
    ProviderDeps, RuntimeProvider, SearchProvider, WorkspaceInventoryProvider, WorkspaceMemoryProvider,
    WorkspaceProvider,
};
pub use inspector::{
    inspect_bundle_sections, measure_bundle_size, ContextInspectorReport, InspectedBundleSection,
    InspectedProvider, ProviderInspectOutcome,
};
pub use relevance::{
    complexity_provider_tier, AssembleHints, ComplexityProviderTier, EnvironmentalHints, IntentTag,
    RelevanceScore,
    RelevanceSignals, RequestKind, DEFAULT_RELEVANCE_THRESHOLD, RELEVANCE_MAX,
};
pub use workspace_snapshot::{
    observe_toolchain, open_files_from_entries, ActiveProjectRef, BuildSystemKind, CursorPosition,
    PackageManagerKind, ToolchainObservation, WorkspaceSnapshot, WorkspaceSnapshotObservation,
};
pub use editor_snapshot::{
    EditorCodeLens, EditorHover, EditorIntelligenceSection, EditorRange, EditorReference,
    EditorSemanticToken, EditorSnapshot, EditorSnapshotObservation, EditorSymbol,
};
pub use project_snapshot::{
    observe_project_intelligence, CargoProjectMeta, DependencyGraphSummary, NpmProjectMeta,
    ProjectIntelligenceSection, ProjectMetadata, ProjectSnapshot, ProjectSnapshotHostFacts,
    ProjectSnapshotObservation, RepositoryMetadata, WorkspaceLayoutSummary,
};
pub use git_snapshot::{
    GitPathEntry, GitSnapshot, GitSnapshotObservation,
};
pub use runtime_snapshot::{
    observe_runtime_intelligence, RuntimeCommandKind, RuntimeCommandOutcome,
    RuntimeIntelligenceSection, RuntimeProcessRef, RuntimeSnapshot, RuntimeSnapshotHostFacts,
    RuntimeSnapshotObservation, RuntimeTerminalSessionFact, TerminalOutputSummary,
};
pub use workspace_memory::{
    observe_workspace_memory, WorkspaceMemoryCommand, WorkspaceMemoryHostFacts,
    WorkspaceMemoryPath, WorkspaceMemorySection, WorkspaceMemorySnapshot,
    WORKSPACE_MEMORY_SECTION_COMMAND_CAP, WORKSPACE_MEMORY_SECTION_PATH_CAP,
};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, UserRequest};
use jaymi_memory_engine::MemoryEngineApi;
use jaymi_project_engine::ProjectEngineApi;
use jaymi_search::SearchEngineApi;

const NAME: &str = "context_engine";
/// Lifecycle peers required before `initialize`.
///
/// Project Engine and Search Engine are **not** listed here: Application boots
/// Context after Memory, then binds Project + Search later via
/// [`ContextEngine::bind_sources`]. Health reports whether sources are bound.
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "memory_engine",
];

/// Runtime sources used to install the default provider set.
///
/// Prefer [`ContextEngine::bind_providers`] when supplying a custom set.
#[derive(Clone)]
pub struct ContextSources {
    /// Memory Engine for conversation / memory providers.
    pub memory: Arc<dyn MemoryEngineApi>,
    /// Project Engine for project / search coordination providers.
    pub projects: Arc<dyn ProjectEngineApi>,
    /// Search Engine — wired into [`SearchProvider`] (never executes search tools).
    pub search: Arc<dyn SearchEngineApi>,
}

/// Context Engine — orchestrates [`ContextProvider`]s into a [`ContextBundle`].
pub struct ContextEngine {
    initialized: bool,
    providers: Mutex<Vec<Arc<dyn ContextProvider>>>,
    session: Mutex<ContextSessionInputs>,
    relevance_threshold: AtomicU64,
    budget: Mutex<ContextBudgetConfig>,
    last_inspection: Mutex<Option<ContextInspectorReport>>,
    assemble_count: AtomicU64,
    cache: Mutex<ContextBundleCache>,
    identity: Mutex<Option<CacheIdentity>>,
    history: Mutex<ContextHistory>,
    policies: Mutex<ContextPolicyEngine>,
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextEngine {
    /// Create an uninitialized context engine (providers bound later).
    pub fn new() -> Self {
        Self {
            initialized: false,
            providers: Mutex::new(Vec::new()),
            session: Mutex::new(ContextSessionInputs::default()),
            relevance_threshold: AtomicU64::new(u64::from(DEFAULT_RELEVANCE_THRESHOLD)),
            budget: Mutex::new(ContextBudgetConfig::default()),
            last_inspection: Mutex::new(None),
            assemble_count: AtomicU64::new(0),
            cache: Mutex::new(ContextBundleCache::with_capacity(DEFAULT_CACHE_CAPACITY)),
            identity: Mutex::new(None),
            history: Mutex::new(ContextHistory::with_capacity(DEFAULT_HISTORY_CAPACITY)),
            policies: Mutex::new(ContextPolicyEngine::with_defaults()),
        }
    }

    /// Install the default provider set from Memory / Project / Search backends.
    pub fn bind_sources(&self, sources: ContextSources) -> JaymiResult<()> {
        if let Ok(mut guard) = self.identity.lock() {
            *guard = Some(CacheIdentity {
                memory: Arc::clone(&sources.memory),
                projects: Arc::clone(&sources.projects),
            });
        }
        self.bind_providers(default_providers(ProviderDeps {
            memory: sources.memory,
            projects: sources.projects,
            search: sources.search,
        }))
    }

    /// Replace the registered provider list (engine does not inspect internals).
    pub fn bind_providers(&self, providers: Vec<Arc<dyn ContextProvider>>) -> JaymiResult<()> {
        if !self.initialized {
            return Err(JaymiError::new(
                "context engine must be initialized before binding providers",
            ));
        }
        let ids: Vec<&str> = providers.iter().map(|provider| provider.id()).collect();
        *self
            .providers
            .lock()
            .map_err(|_| JaymiError::new("context providers lock poisoned"))? = providers;
        jaymi_logging::info(
            "context",
            format!("context providers bound count={} ids={:?}", ids.len(), ids),
        );
        Ok(())
    }

    /// Append a provider without replacing the existing set.
    pub fn register_provider(&self, provider: Arc<dyn ContextProvider>) -> JaymiResult<()> {
        if !self.initialized {
            return Err(JaymiError::new(
                "context engine must be initialized before registering providers",
            ));
        }
        let id = provider.id();
        self.providers
            .lock()
            .map_err(|_| JaymiError::new("context providers lock poisoned"))?
            .push(provider);
        jaymi_logging::info("context", format!("context provider registered id={id}"));
        Ok(())
    }

    /// True when at least one provider is registered.
    pub fn sources_bound(&self) -> bool {
        self.providers_bound()
    }

    /// True when at least one provider is registered.
    pub fn providers_bound(&self) -> bool {
        self.providers
            .lock()
            .map(|guard| !guard.is_empty())
            .unwrap_or(false)
    }

    /// Registered provider ids (diagnostics / tests).
    pub fn provider_ids(&self) -> Vec<&'static str> {
        self.providers
            .lock()
            .map(|guard| guard.iter().map(|provider| provider.id()).collect())
            .unwrap_or_default()
    }

    /// Replace the full session input snapshot used by the next assemble.
    ///
    /// Requests a fresh assemble when the snapshot actually changes. Reason is
    /// specialized when only one dimension moved (workspace / diagnostics / …).
    pub fn set_session_inputs(&self, inputs: ContextSessionInputs) {
        if let Ok(mut guard) = self.session.lock() {
            if *guard == inputs {
                return;
            }
            let reason = session_change_reason(&guard, &inputs);
            *guard = inputs;
            drop(guard);
            self.request_fresh_context(reason);
        }
    }

    /// Record the active UX workspace kind for the next assemble (session state).
    ///
    /// Requests a fresh assemble when the workspace kind changes.
    pub fn set_session_workspace(&self, workspace_kind: Option<String>) {
        if let Ok(mut guard) = self.session.lock() {
            if guard.workspace_kind != workspace_kind {
                guard.workspace_kind = workspace_kind;
                drop(guard);
                self.request_fresh_context("workspace_changed");
            }
        }
    }

    /// Active UX workspace kind id, when set.
    pub fn session_workspace(&self) -> Option<String> {
        self.session
            .lock()
            .ok()
            .and_then(|guard| guard.workspace_kind.clone())
    }

    /// Copy of the current session inputs.
    pub fn session_inputs(&self) -> ContextSessionInputs {
        self.session
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Latest Coding [`WorkspaceSnapshot`] from session inputs, when captured.
    ///
    /// Observational only — never assembles a ContextBundle.
    pub fn workspace_snapshot(&self) -> Option<crate::WorkspaceSnapshot> {
        self.session_inputs().workspace_snapshot
    }

    /// Latest [`EditorSnapshot`] from session inputs, when captured (Sprint B2.3).
    ///
    /// Read-only observation — never assembles a ContextBundle and never calls LSP.
    pub fn editor_snapshot(&self) -> Option<crate::EditorSnapshot> {
        self.session_inputs().editor_snapshot
    }

    /// Latest [`ProjectSnapshot`] from session inputs, when captured (Sprint B2.4).
    ///
    /// Read-only observation — never assembles a ContextBundle and never scans FS.
    pub fn project_snapshot(&self) -> Option<crate::ProjectSnapshot> {
        self.session_inputs().project_snapshot
    }

    /// Latest [`GitSnapshot`] from session inputs, when captured (Sprint B2.5).
    ///
    /// Read-only observation — never assembles a ContextBundle and never runs git.
    pub fn git_snapshot(&self) -> Option<crate::GitSnapshot> {
        self.session_inputs().git_snapshot
    }

    /// Latest [`RuntimeSnapshot`] from session inputs, when captured (Sprint B2.6).
    ///
    /// Read-only observation — never assembles a ContextBundle and never re-runs
    /// cargo / tests.
    pub fn runtime_snapshot(&self) -> Option<crate::RuntimeSnapshot> {
        self.session_inputs().runtime_snapshot
    }

    /// Latest [`WorkspaceMemorySnapshot`] from session inputs, when captured (Sprint B2.9).
    ///
    /// Read-only observation — never writes Conversation Memory and never
    /// assembles a ContextBundle.
    pub fn workspace_memory_snapshot(&self) -> Option<crate::WorkspaceMemorySnapshot> {
        self.session_inputs().workspace_memory_snapshot
    }

    /// Number of successful `assemble` calls since boot (tests / diagnostics).
    pub fn assemble_count(&self) -> u64 {
        self.assemble_count.load(Ordering::Relaxed)
    }

    /// Drop cached bundles and bump the invalidation epoch.
    ///
    /// Prefer [`Self::request_fresh_context`] from Planner / Application call sites —
    /// that name is the opaque “need a fresh assemble” seam. This method is the
    /// cache implementation detail.
    pub fn invalidate_cache(&self, reason: impl Into<String>) {
        let reason = reason.into();
        if let Ok(mut guard) = self.cache.lock() {
            guard.invalidate(reason.clone());
        }
        jaymi_logging::info(
            "context",
            format!("context bundle cache invalidated reason={reason}"),
        );
    }

    /// Ask the Context Engine for a fresh assemble on the next request.
    ///
    /// Planner-facing seam: callers state *why* context must be fresh
    /// (`conversation_changed`, `workspace_changed`, `project_changed`,
    /// `diagnostics_changed`, `files_changed`, …) without knowing cache keys,
    /// epochs, or LRU. Implementation clears the ContextBundle cache.
    pub fn request_fresh_context(&self, reason: impl Into<String>) {
        self.invalidate_cache(reason);
    }

    /// Snapshot of ContextBundle cache statistics.
    pub fn cache_stats(&self) -> ContextCacheStats {
        self.cache
            .lock()
            .map(|guard| guard.stats())
            .unwrap_or_default()
    }

    /// Active Context Policy ids.
    pub fn active_context_policies(&self) -> Vec<String> {
        self.policies
            .lock()
            .map(|guard| {
                guard
                    .active_policy_ids()
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Replace the Context Policy set (tests / future enterprise hooks).
    pub fn set_context_policies(&self, policies: Vec<Arc<dyn ContextPolicy>>) {
        if let Ok(mut guard) = self.policies.lock() {
            guard.set_policies(policies);
        }
        self.request_fresh_context("context_policies_changed");
    }

    /// Register an additional Context Policy.
    pub fn register_context_policy(&self, policy: Arc<dyn ContextPolicy>) {
        if let Ok(mut guard) = self.policies.lock() {
            guard.register_policy(policy);
        }
        self.request_fresh_context("context_policy_registered");
    }

    /// Recent Context History entries (newest first) for inspection / transparency.
    pub fn history(&self) -> Vec<ContextHistoryEntry> {
        self.history
            .lock()
            .map(|guard| guard.entries())
            .unwrap_or_default()
    }

    /// Number of retained Context History entries.
    pub fn history_len(&self) -> usize {
        self.history
            .lock()
            .map(|guard| guard.len())
            .unwrap_or(0)
    }

    /// Most recent Context History entry, when any.
    pub fn history_latest(&self) -> Option<ContextHistoryEntry> {
        self.history
            .lock()
            .ok()
            .and_then(|guard| guard.latest().cloned())
    }

    /// Clear retained Context History (diagnostics / tests).
    pub fn clear_history(&self) {
        if let Ok(mut guard) = self.history.lock() {
            guard.clear();
        }
    }

    fn record_history(
        &self,
        bundle: &ContextBundle,
        inspection: &ContextInspectorReport,
        duration_ms: u64,
        chars_per_token: usize,
    ) {
        let entry = ContextHistoryEntry::from_assemble(
            bundle.clone(),
            inspection,
            duration_ms,
            chars_per_token,
        );
        if let Ok(mut guard) = self.history.lock() {
            guard.push(entry);
        }
    }

    /// Minimum [`RelevanceScore`] required for a provider to contribute.
    pub fn relevance_threshold(&self) -> u8 {
        self.relevance_threshold.load(Ordering::Relaxed).min(100) as u8
    }

    /// Override the relevance threshold (clamped to 0..=100).
    pub fn set_relevance_threshold(&self, threshold: u8) {
        self.relevance_threshold
            .store(u64::from(threshold.min(100)), Ordering::Relaxed);
    }

    /// Current context budget configuration.
    pub fn budget_config(&self) -> ContextBudgetConfig {
        self.budget
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Replace the context budget configuration.
    pub fn set_budget_config(&self, config: ContextBudgetConfig) {
        if let Ok(mut guard) = self.budget.lock() {
            *guard = config;
        }
    }

    /// Convenience: set the maximum character budget (keeps other budget fields).
    pub fn set_max_characters(&self, max_characters: usize) {
        if let Ok(mut guard) = self.budget.lock() {
            guard.max_characters = max_characters.max(ENGINE_RESERVED_CHARACTERS);
        }
    }

    /// Latest Context Inspector report (diagnostics only; never affects execution).
    pub fn last_inspection(&self) -> Option<ContextInspectorReport> {
        self.last_inspection
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Alias for diagnostics dashboards.
    pub fn inspect_last(&self) -> Option<ContextInspectorReport> {
        self.last_inspection()
    }

    /// Convert a [`ContextBundle`] into the LLM-facing structured representation.
    ///
    /// Pure transform for future language-model consumers. Does **not** call
    /// models and does **not** build prompts. LLMs should consume this API
    /// instead of querying individual subsystems.
    pub fn to_llm_context(&self, bundle: &ContextBundle) -> LlmContext {
        LlmContext::from_bundle(bundle)
    }

    /// Deterministic JSON serialization of a bundle's LLM-facing view.
    pub fn serialize_llm_context(&self, bundle: &ContextBundle) -> JaymiResult<String> {
        self.to_llm_context(bundle).to_json()
    }

    /// Build only the context required for the current request.
    ///
    /// Orchestrates registered [`ContextProvider`]s. Each provider may decline.
    /// The engine merges contributions and stamps request / planner metadata —
    /// it does not call Memory / Project / Search APIs directly.
    ///
    /// Recently assembled bundles may be returned from cache when the key
    /// matches; hits still bump `assemble_count` and restamp generation /
    /// request metadata so Planner integrity is unchanged.
    ///
    /// Assemble context for a request (test / admin helper).
    ///
    /// **Request path:** always use [`Self::assemble_with`] via the Planner so
    /// Intent and Capability selection stay Planner-owned. This hint-less entry
    /// uses only [`jaymi_core::IntentId::from_structured_request`].
    pub fn assemble(&self, request: &UserRequest) -> JaymiResult<ContextBundle> {
        self.assemble_with(request, None)
    }

    /// Mint an empty [`ContextBundle`] without running providers.
    ///
    /// **Sole-factory contract:** production code outside this crate must obtain
    /// bundles only through [`Self::assemble_with`], [`Self::assemble`],
    /// [`Self::empty_bundle`], or [`Self::reuse_bundle`]. The Planner may request
    /// an empty bundle when review flows intentionally skip reassemble — it must
    /// never construct [`ContextBundle`] itself.
    ///
    /// Behavior matches a freshly built empty builder snapshot (same as the
    /// historical `ContextBundle::default()` placeholder).
    pub fn empty_bundle(&self) -> ContextBundle {
        let _ = self; // instance method so callers go through the engine seam
        ContextBundleBuilder::new().build()
    }

    /// Return a previously assembled bundle unchanged (ownership-preserving clone).
    ///
    /// Use when the Planner must attach an existing snapshot without reassemble.
    /// Does not invent sections — the bundle was already minted by this engine.
    pub fn reuse_bundle(&self, bundle: &ContextBundle) -> ContextBundle {
        let _ = self;
        bundle.clone()
    }

    /// Assemble context after Planner Intent / Capability resolution.
    ///
    /// `hints` carry Planner-selected [`jaymi_core::IntentId`] and capability ids.
    /// The engine only assembles — it does not determine Intent, select Capabilities,
    /// execute tools, or invent session state.
    pub fn assemble_with(
        &self,
        request: &UserRequest,
        hints: Option<&AssembleHints>,
    ) -> JaymiResult<ContextBundle> {
        if !self.initialized {
            return Err(JaymiError::new("context engine is not initialized"));
        }
        let providers = self
            .providers
            .lock()
            .map_err(|_| JaymiError::new("context providers lock poisoned"))?
            .clone();
        if providers.is_empty() {
            return Err(JaymiError::new("context engine providers are not bound"));
        }

        let started = Instant::now();
        let session = self.session_inputs();
        let signals = RelevanceSignals::derive_with(request, &session, hints);
        let threshold = self.relevance_threshold();
        let budget_config = self.budget_config();
        let identity = self
            .identity
            .lock()
            .ok()
            .and_then(|guard| guard.clone());

        let policy_fingerprint = self
            .policies
            .lock()
            .map(|guard| guard.fingerprint())
            .unwrap_or(0);
        let hints_fingerprint = hints.map(AssembleHints::fingerprint).unwrap_or(0);
        let cache_key = {
            let epoch = self
                .cache
                .lock()
                .map(|guard| guard.epoch())
                .unwrap_or(0);
            ContextCacheKey::build(
                epoch,
                identity.as_ref(),
                &session,
                request,
                &signals,
                threshold,
                &budget_config,
                policy_fingerprint,
                hints_fingerprint,
            )
        };

        if let Ok(mut cache) = self.cache.lock() {
            if let Some(entry) = cache.get(&cache_key) {
                let assemble_generation =
                    self.assemble_count.fetch_add(1, Ordering::Relaxed) + 1;
                let bundle = entry.bundle.restamp_cache_hit(
                    assemble_generation,
                    request,
                    format!(
                        "cache_hit key_type={} epoch={}",
                        cache_key.request_type, cache_key.epoch
                    ),
                );
                let mut inspection = entry.inspection;
                inspection.assemble_generation = assemble_generation;
                inspection.cache_hit = true;
                inspection.request_preview = bundle.user_request().content_preview.clone();
                let duration_ms = started.elapsed().as_millis() as u64;
                inspection.finalize(duration_ms, &bundle, budget_config.chars_per_token);
                self.record_history(
                    &bundle,
                    &inspection,
                    duration_ms,
                    budget_config.chars_per_token,
                );
                if let Ok(mut guard) = self.last_inspection.lock() {
                    *guard = Some(inspection);
                }
                jaymi_logging::info(
                    "context",
                    format!(
                        "context bundle cache hit generation={} kind={} project={:?} conversation={:?} duration_ms={}",
                        assemble_generation,
                        cache_key.request_type,
                        cache_key.project_id,
                        cache_key.conversation_id,
                        duration_ms
                    ),
                );
                return Ok(bundle);
            }
        }

        let provider_request = ProviderRequest {
            request,
            session: &session,
            relevance: &signals,
        };

        // Prefer host-supplied session flag; fall back to live Project Engine identity
        // so Continue/Open within the same handle (session prepared before open) still works.
        let project_open = session.project_open
            || identity
                .as_ref()
                .and_then(|id| id.projects.open_project_id())
                .is_some();
        let policy_inputs = ContextPolicyInputs {
            request,
            session: &session,
            signals: &signals,
            project_open,
            max_sensitivity: Sensitivity::Private,
        };
        let selection_assessment = assess_context_selection(request, &signals);

        // Policy → relevance → priority sort → propose candidates → candidate policy
        // (relevance/recency/importance/privacy) → budget select → materialize.
        // Policies never gather context; providers never assemble bundles.
        let mut ranked: Vec<(
            Arc<dyn ContextProvider>,
            RelevanceScore,
            ContextPolicyDecision,
            usize,
        )> = Vec::new();
        let mut skipped_relevance = 0usize;
        let mut skipped_policy = 0usize;
        let mut skipped_approval = 0usize;
        let mut inspect_providers: Vec<InspectedProvider> = Vec::new();
        let mut policy_summaries: Vec<PolicyDecisionSummary> = Vec::new();
        let mut size_before_characters = 0usize;
        let mut size_after_characters = 0usize;
        let active_policies = self.active_context_policies();

        let policy_engine = self
            .policies
            .lock()
            .map_err(|_| JaymiError::new("context policy engine lock poisoned"))?;

        for (evaluation_order, provider) in providers.iter().enumerate() {
            if matches!(
                signals.complexity_tier_for(provider.id()),
                Some(ComplexityProviderTier::Excluded)
            ) {
                let estimate = provider.estimate_size(&provider_request);
                let sensitivity = provider.sensitivity();
                inspect_providers.push(InspectedProvider::new(
                    evaluation_order,
                    None,
                    provider.id(),
                    provider.priority().value(),
                    0,
                    sensitivity.as_str(),
                    false,
                    "n/a",
                    false,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedComplexity {
                        complexity: signals
                            .complexity_id()
                            .unwrap_or("unknown")
                            .to_string(),
                    },
                ));
                jaymi_logging::info(
                    "context",
                    format!(
                        "provider complexity-excluded id={} complexity={:?}",
                        provider.id(),
                        signals.complexity_id()
                    ),
                );
                continue;
            }

            let score = signals.score_for_provider(
                provider.id(),
                provider.relevance(&provider_request),
            );
            let estimate = provider.estimate_size(&provider_request);
            let sensitivity = provider.sensitivity();
            let candidate = ContextPolicyCandidate {
                provider_id: provider.id(),
                provider_priority: provider.priority(),
                relevance: score,
                sensitivity,
                estimate,
                inputs: &policy_inputs,
            };
            let record = policy_engine.evaluate_candidate(&candidate);
            let mut summary = PolicyDecisionSummary::from_record(&record);
            size_before_characters =
                size_before_characters.saturating_add(estimate.units.characters);

            if !record.decision.participate {
                skipped_policy += 1;
                policy_summaries.push(summary);
                inspect_providers.push(InspectedProvider::new(
                    evaluation_order,
                    None,
                    provider.id(),
                    provider.priority().value(),
                    score.value(),
                    sensitivity.as_str(),
                    record.decision.requires_user_approval,
                    "n/a",
                    record.decision.can_truncate,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedPolicy {
                        policy: record
                            .applied_policies
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "context_policy".into()),
                        reason: record.decision.reason.clone(),
                        sensitivity: record.sensitivity.as_str().to_string(),
                    },
                ));
                jaymi_logging::info(
                    "context",
                    format!(
                        "provider policy-excluded id={} reason={}",
                        provider.id(),
                        record.decision.reason
                    ),
                );
                continue;
            }

            if !record.decision.bypass_relevance && !score.meets(threshold) {
                skipped_relevance += 1;
                summary.included = false;
                summary.reason = format!(
                    "{} (then relevance {} < threshold {})",
                    summary.reason,
                    score.value(),
                    threshold
                );
                policy_summaries.push(summary);
                inspect_providers.push(InspectedProvider::new(
                    evaluation_order,
                    None,
                    provider.id(),
                    record.decision.priority.value(),
                    score.value(),
                    sensitivity.as_str(),
                    record.decision.requires_user_approval,
                    if record.decision.requires_user_approval {
                        "pending"
                    } else {
                        "not_required"
                    },
                    record.decision.can_truncate,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedRelevance { threshold },
                ));
                jaymi_logging::info(
                    "context",
                    format!(
                        "provider skipped id={} relevance={} threshold={} kind={} workspace={:?}",
                        provider.id(),
                        score,
                        threshold,
                        signals.request_kind.as_str(),
                        signals.workspace_kind
                    ),
                );
                continue;
            }

            if record.decision.requires_user_approval
                && !session
                    .approved_context_providers
                    .iter()
                    .any(|id| id == provider.id())
            {
                skipped_approval += 1;
                summary.included = false;
                summary.approval_status = "pending".into();
                summary.reason = format!(
                    "{} (awaiting user approval for provider '{}')",
                    summary.reason,
                    provider.id()
                );
                policy_summaries.push(summary);
                inspect_providers.push(InspectedProvider::new(
                    evaluation_order,
                    None,
                    provider.id(),
                    record.decision.priority.value(),
                    score.value(),
                    sensitivity.as_str(),
                    true,
                    "pending",
                    record.decision.can_truncate,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedApproval {
                        reason: record.decision.reason.clone(),
                        sensitivity: record.sensitivity.as_str().to_string(),
                    },
                ));
                jaymi_logging::info(
                    "context",
                    format!(
                        "provider approval-required id={} reason={}",
                        provider.id(),
                        record.decision.reason
                    ),
                );
                continue;
            }

            if record.decision.requires_user_approval {
                summary.approval_status = "approved".into();
            }

            size_after_characters =
                size_after_characters.saturating_add(estimate.units.characters);
            policy_summaries.push(summary);
            ranked.push((
                Arc::clone(provider),
                score,
                record.decision,
                evaluation_order,
            ));
        }
        drop(policy_engine);

        ranked.sort_by(|(left, left_score, left_decision, _), (right, right_score, right_decision, _)| {
            right_decision
                .priority
                .cmp(&left_decision.priority)
                .then_with(|| right_score.cmp(left_score))
                .then_with(|| left.id().cmp(right.id()))
        });

        let mut builder = ContextBundle::builder();
        let mut included: Vec<ContextSource> = Vec::new();
        let mut contributed = 0usize;
        let mut declined = 0usize;
        let mut used_characters = 0usize;
        let mut budget_report = BudgetReport {
            max_characters: budget_config.max_characters,
            max_tokens: budget_config.max_tokens,
            used_characters: 0,
            estimated_tokens: 0,
            truncated_providers: Vec::new(),
            skipped_budget: Vec::new(),
            summaries: Vec::new(),
        };
        let mut candidate_selection = CandidateSelectionReport::default();
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        for (allocation_order, (provider, score, decision, evaluation_order)) in
            ranked.iter().enumerate()
        {
            let remaining = budget_config.remaining_characters(used_characters);
            let estimate = provider.estimate_size(&provider_request);
            let sensitivity = provider.sensitivity().as_str();
            let approval_status = if decision.requires_user_approval {
                "approved"
            } else {
                "not_required"
            };
            let provider_allows_fit = estimate.can_truncate || estimate.can_summarize;
            if remaining == 0 {
                budget_report.skipped_budget.push(provider.id().to_string());
                if let Some(summary) = policy_summaries
                    .iter_mut()
                    .find(|item| item.provider_id == provider.id())
                {
                    summary.truncation_reason = Some("budget_exhausted".into());
                }
                inspect_providers.push(InspectedProvider::new(
                    *evaluation_order,
                    Some(allocation_order),
                    provider.id(),
                    decision.priority.value(),
                    score.value(),
                    sensitivity,
                    decision.requires_user_approval,
                    approval_status,
                    decision.can_truncate,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedBudget {
                        remaining_characters: 0,
                        estimate_characters: estimate.units.characters,
                        reason: "budget_exhausted".into(),
                    },
                ));
                continue;
            }

            if estimate.units.characters > remaining && !decision.can_truncate {
                budget_report.skipped_budget.push(provider.id().to_string());
                if let Some(summary) = policy_summaries
                    .iter_mut()
                    .find(|item| item.provider_id == provider.id())
                {
                    summary.included = false;
                    summary.truncation_reason = Some("policy_forbids_truncation".into());
                    summary.reason = format!(
                        "{} (omitted: policy forbids truncation; estimate={} remaining={})",
                        summary.reason, estimate.units.characters, remaining
                    );
                }
                inspect_providers.push(InspectedProvider::new(
                    *evaluation_order,
                    Some(allocation_order),
                    provider.id(),
                    decision.priority.value(),
                    score.value(),
                    sensitivity,
                    decision.requires_user_approval,
                    approval_status,
                    decision.can_truncate,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedBudget {
                        remaining_characters: remaining,
                        estimate_characters: estimate.units.characters,
                        reason: "policy_forbids_truncation".into(),
                    },
                ));
                continue;
            }

            if estimate.units.characters > remaining && !provider_allows_fit {
                budget_report.skipped_budget.push(provider.id().to_string());
                if let Some(summary) = policy_summaries
                    .iter_mut()
                    .find(|item| item.provider_id == provider.id())
                {
                    summary.truncation_reason = Some("estimate_exceeds_budget".into());
                }
                inspect_providers.push(InspectedProvider::new(
                    *evaluation_order,
                    Some(allocation_order),
                    provider.id(),
                    decision.priority.value(),
                    score.value(),
                    sensitivity,
                    decision.requires_user_approval,
                    approval_status,
                    decision.can_truncate,
                    estimate.units.characters,
                    estimate.units.estimated_tokens,
                    ProviderInspectOutcome::SkippedBudget {
                        remaining_characters: remaining,
                        estimate_characters: estimate.units.characters,
                        reason: "estimate_exceeds_budget".into(),
                    },
                ));
                continue;
            }

            match {
                let propose_started = Instant::now();
                let result = provider.propose_candidates(&provider_request);
                let contribute_ms = propose_started.elapsed().as_millis() as u64;
                (result, contribute_ms)
            } {
                (Ok(candidates), contribute_ms) if !candidates.is_empty() => {
                    // Sprint B2.7: Context Policy evaluates each candidate;
                    // only selected nodes materialize into bundle sections.
                    let mut scored = Vec::new();
                    {
                        let policy_engine = self
                            .policies
                            .lock()
                            .map_err(|_| JaymiError::new("context policy lock poisoned"))?;
                        for candidate in candidates {
                            let item = policy_engine.evaluate_candidate_item(
                                &candidate,
                                score.value(),
                                now_unix,
                                &policy_inputs,
                            );
                            candidate_selection.proposed =
                                candidate_selection.proposed.saturating_add(1);
                            if item.select {
                                scored.push((candidate, item.scores, item.reason));
                            } else {
                                candidate_selection.rejected_policy =
                                    candidate_selection.rejected_policy.saturating_add(1);
                                if candidate_selection.decisions.len() < 128 {
                                    candidate_selection.decisions.push(
                                        CandidateDecisionSummary {
                                            candidate_id: candidate.id.0.clone(),
                                            provider_id: candidate.provider_id.to_string(),
                                            kind: candidate.kind.as_str().to_string(),
                                            selected: false,
                                            reason: item.reason,
                                            relevance: item.scores.relevance,
                                            recency: item.scores.recency,
                                            importance: item.scores.importance,
                                            estimated_chars: candidate.estimated_chars(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                    let selection = select_candidates_for_budget(&scored, remaining);
                    candidate_selection.rejected_budget = candidate_selection
                        .rejected_budget
                        .saturating_add(selection.report.rejected_budget);
                    candidate_selection.selected = candidate_selection
                        .selected
                        .saturating_add(selection.report.selected);
                    for row in selection.report.decisions {
                        if candidate_selection.decisions.len() < 128 {
                            candidate_selection.decisions.push(row);
                        }
                    }
                    if selection.selected.is_empty() {
                        declined += 1;
                        inspect_providers.push(
                            InspectedProvider::new(
                                *evaluation_order,
                                Some(allocation_order),
                                provider.id(),
                                decision.priority.value(),
                                score.value(),
                                sensitivity,
                                decision.requires_user_approval,
                                approval_status,
                                decision.can_truncate,
                                estimate.units.characters,
                                estimate.units.estimated_tokens,
                                ProviderInspectOutcome::Declined,
                            )
                            .with_duration_ms(contribute_ms),
                        );
                        continue;
                    }
                    let contribution = materialize_candidates(&selection.selected);
                    let (contribution, enforced) =
                        apply_policy_to_contribution(contribution, decision);
                    if let Some(summary) = policy_summaries
                        .iter_mut()
                        .find(|item| item.provider_id == provider.id())
                    {
                        summary.constraints = enforced;
                    }

                    let measured =
                        measure_contribution(&contribution, budget_config.chars_per_token);
                    let (fitted, outcome) = if measured.characters > remaining {
                        if !decision.can_truncate {
                            budget_report.skipped_budget.push(provider.id().to_string());
                            if let Some(summary) = policy_summaries
                                .iter_mut()
                                .find(|item| item.provider_id == provider.id())
                            {
                                summary.included = false;
                                summary.truncation_reason =
                                    Some("policy_forbids_truncation".into());
                                summary.reason = format!(
                                    "{} (omitted: policy forbids truncation after constraints; size={} remaining={})",
                                    summary.reason, measured.characters, remaining
                                );
                            }
                            inspect_providers.push(
                                InspectedProvider::new(
                                    *evaluation_order,
                                    Some(allocation_order),
                                    provider.id(),
                                    decision.priority.value(),
                                    score.value(),
                                    sensitivity,
                                    decision.requires_user_approval,
                                    approval_status,
                                    decision.can_truncate,
                                    estimate.units.characters,
                                    estimate.units.estimated_tokens,
                                    ProviderInspectOutcome::SkippedBudget {
                                        remaining_characters: remaining,
                                        estimate_characters: measured.characters,
                                        reason: "policy_forbids_truncation".into(),
                                    },
                                )
                                .with_duration_ms(contribute_ms),
                            );
                            continue;
                        }
                        fit_contribution(
                            contribution,
                            remaining,
                            budget_config.chars_per_token,
                        )
                    } else {
                        (
                            contribution,
                            crate::budget::FitOutcome {
                                truncated: false,
                                summarized: false,
                                summary: None,
                                final_units: measured,
                                drop: false,
                            },
                        )
                    };
                    if outcome.drop || fitted.is_empty() {
                        budget_report.skipped_budget.push(provider.id().to_string());
                        if let Some(summary) = &outcome.summary {
                            budget_report
                                .summaries
                                .push(format!("{}: {summary}", provider.id()));
                        }
                        inspect_providers.push(
                            InspectedProvider::new(
                                *evaluation_order,
                                Some(allocation_order),
                                provider.id(),
                                decision.priority.value(),
                                score.value(),
                                sensitivity,
                                decision.requires_user_approval,
                                approval_status,
                                decision.can_truncate,
                                estimate.units.characters,
                                estimate.units.estimated_tokens,
                                ProviderInspectOutcome::Dropped {
                                    summary: outcome.summary.clone(),
                                },
                            )
                            .with_duration_ms(contribute_ms),
                        );
                        continue;
                    }

                    if outcome.truncated || outcome.summarized {
                        budget_report
                            .truncated_providers
                            .push(provider.id().to_string());
                        if let Some(summary) = policy_summaries
                            .iter_mut()
                            .find(|item| item.provider_id == provider.id())
                        {
                            summary.truncated = true;
                            summary.truncation_reason = Some(if outcome.summarized {
                                "budget_summarize".into()
                            } else {
                                "budget_fit".into()
                            });
                        }
                    }
                    if let Some(summary) = &outcome.summary {
                        budget_report
                            .summaries
                            .push(format!("{}: {summary}", provider.id()));
                    }

                    used_characters =
                        used_characters.saturating_add(outcome.final_units.characters);
                    contributed += 1;
                    inspect_providers.push(
                        InspectedProvider::new(
                            *evaluation_order,
                            Some(allocation_order),
                            provider.id(),
                            decision.priority.value(),
                            score.value(),
                            sensitivity,
                            decision.requires_user_approval,
                            approval_status,
                            decision.can_truncate,
                            estimate.units.characters,
                            estimate.units.estimated_tokens,
                            ProviderInspectOutcome::Contributed {
                                characters: outcome.final_units.characters,
                                estimated_tokens: outcome.final_units.estimated_tokens,
                                truncated: outcome.truncated,
                                summarized: outcome.summarized,
                                summary: outcome.summary.clone(),
                                sources: fitted.sources.clone(),
                            },
                        )
                        .with_duration_ms(contribute_ms),
                    );
                    for source in &fitted.sources {
                        if !included.contains(source) {
                            included.push(*source);
                        }
                    }
                    builder = builder.apply_contribution(fitted);
                }
                (Ok(_), contribute_ms) => {
                    declined += 1;
                    inspect_providers.push(
                        InspectedProvider::new(
                            *evaluation_order,
                            Some(allocation_order),
                            provider.id(),
                            decision.priority.value(),
                            score.value(),
                            sensitivity,
                            decision.requires_user_approval,
                            approval_status,
                            decision.can_truncate,
                            estimate.units.characters,
                            estimate.units.estimated_tokens,
                            ProviderInspectOutcome::Declined,
                        )
                        .with_duration_ms(contribute_ms),
                    );
                }
                (Err(err), _) => return Err(err),
            }
        }

        budget_report.used_characters = used_characters;
        budget_report.estimated_tokens =
            budget_config.tokens_for_chars(used_characters);

        let policy_report = PolicyReport {
            active_policies,
            decisions: policy_summaries,
            size_before_characters,
            size_after_characters,
            size_assembled_characters: used_characters,
            candidate_selection,
            selection_profile: Some(selection_assessment.profile.as_str().to_string()),
            selection_rules: selection_assessment
                .matched_rules
                .iter()
                .map(|rule| (*rule).to_string())
                .collect(),
        };

        // Engine-owned stamps (not subsystem providers).
        builder = builder.user_request(UserRequestMetadataSection::from_request(request));
        if !included.contains(&ContextSource::UserRequest) {
            included.push(ContextSource::UserRequest);
        }
        let assemble_generation = self.assemble_count.fetch_add(1, Ordering::Relaxed) + 1;
        let mut notes = vec![
            format!(
                "providers contributed={contributed} declined={declined} skipped_relevance={skipped_relevance} skipped_policy={skipped_policy} skipped_approval={skipped_approval} skipped_budget={} truncated={} threshold={threshold} kind={} budget_chars={}/{}",
                budget_report.skipped_budget.len(),
                budget_report.truncated_providers.len(),
                signals.request_kind.as_str(),
                used_characters,
                budget_config.provider_character_budget()
            ),
            format!(
                "policy active=[{}] included=[{}] excluded=[{}] pending_approval=[{}] size_before={} size_after={} assembled={}",
                policy_report.active_policies.join(","),
                policy_report.included_providers().join(","),
                policy_report.excluded_providers().join(","),
                policy_report.pending_approval_providers().join(","),
                policy_report.size_before_characters,
                policy_report.size_after_characters,
                policy_report.size_assembled_characters
            ),
        ];
        if let Some(intent) = signals.planner_intent.as_deref() {
            notes.push(format!(
                "pipeline intent={intent} intent_id={} capabilities=[{}]",
                signals.intent.as_str(),
                signals.active_capabilities.join(",")
            ));
        }
        let environmental = hints.and_then(|h| h.environmental.clone());
        if let Some(env) = environmental.as_ref() {
            if env.needed {
                notes.push(format!(
                    "environmental_resolution ambiguous={} primary={:?} rules=[{}] bindings=[{}]",
                    env.ambiguous,
                    env.primary_path,
                    env.rules.join(","),
                    env.bindings.join("; ")
                ));
            }
        }
        if let Some(understanding) = hints.and_then(|h| h.understanding.as_ref()) {
            notes.push(format!("coding_understanding={understanding}"));
        }
        if let Some(review) = hints.and_then(|h| h.review.as_ref()) {
            notes.push(format!("coding_review={review}"));
        }
        if let Some(coding_plan) = hints.and_then(|h| h.coding_plan.as_ref()) {
            notes.push(format!("coding_plan={coding_plan}"));
        }
        notes.extend(budget_report.summaries.iter().cloned());
        builder = builder.planner_metadata(PlannerMetadataSection {
            assemble_generation,
            sources: included,
            notes,
            budget: Some(budget_report),
            policy: Some(policy_report.clone()),
            environmental,
        });

        let bundle = builder.build();

        let mut inspection = ContextInspectorReport {
            assemble_generation: bundle.assemble_generation(),
            request_preview: bundle.user_request().content_preview.clone(),
            request_kind: signals.request_kind.as_str().to_string(),
            workspace_kind: bundle.workspace_kind().map(str::to_string),
            relevance_threshold: threshold,
            providers: inspect_providers,
            contributor_order: Vec::new(),
            sections: Vec::new(),
            sources: Vec::new(),
            budget: None,
            notes: Vec::new(),
            cache_hit: false,
            duration_ms: 0,
            bundle_size_characters: 0,
            bundle_size_estimated_tokens: 0,
            policy: Some(policy_report),
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        inspection.finalize(duration_ms, &bundle, budget_config.chars_per_token);
        self.record_history(
            &bundle,
            &inspection,
            duration_ms,
            budget_config.chars_per_token,
        );
        if let Ok(mut guard) = self.last_inspection.lock() {
            *guard = Some(inspection.clone());
        }
        if let Ok(mut cache) = self.cache.lock() {
            // Re-read epoch in case of concurrent invalidate during assemble.
            let mut key = cache_key;
            key.epoch = cache.epoch();
            cache.insert(
                key,
                ContextCacheEntry {
                    bundle: bundle.clone(),
                    inspection,
                },
            );
        }

        jaymi_logging::info(
            "context",
            format!(
                "assembled context memories={} project={} workspace={:?} search={} generation={} contributed={} declined={} duration_ms={}",
                bundle.memory().len(),
                bundle.active_project().name.as_deref().unwrap_or("-"),
                bundle.workspace_kind(),
                bundle.search().is_some() || !bundle.search_results().hits.is_empty(),
                assemble_generation,
                contributed,
                declined,
                duration_ms
            ),
        );

        Ok(bundle)
    }
}

impl jaymi_core::Lifecycle for ContextEngine {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let bound = self.providers_bound();
        let healthy = self.initialized && bound;
        HealthReport::new(
            NAME,
            self.initialized,
            healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            (
                "status".to_string(),
                if healthy {
                    "operational".to_string()
                } else if self.initialized {
                    "awaiting_providers".to_string()
                } else {
                    "not_initialized".to_string()
                },
            ),
            ("providers_bound".to_string(), bound.to_string()),
            (
                "provider_count".to_string(),
                self.provider_ids().len().to_string(),
            ),
            (
                "assemble_count".to_string(),
                self.assemble_count().to_string(),
            ),
            (
                "cache_hits".to_string(),
                self.cache_stats().hits.to_string(),
            ),
            (
                "cache_misses".to_string(),
                self.cache_stats().misses.to_string(),
            ),
            (
                "cache_epoch".to_string(),
                self.cache_stats().epoch.to_string(),
            ),
            (
                "history_len".to_string(),
                self.history_len().to_string(),
            ),
            (
                "bound_sources".to_string(),
                if bound {
                    "memory+project+search (via bind_sources after lifecycle init)".to_string()
                } else {
                    "none — call bind_sources after Project/Search boot".to_string()
                },
            ),
            (
                "note".to_string(),
                "Planner request context is assembled via ContextProviders into an immutable ContextBundle"
                    .to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        if let Ok(mut guard) = self.providers.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.session.lock() {
            *guard = ContextSessionInputs::default();
        }
        if let Ok(mut guard) = self.budget.lock() {
            *guard = ContextBudgetConfig::default();
        }
        if let Ok(mut guard) = self.last_inspection.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.cache.lock() {
            guard.invalidate("shutdown");
        }
        if let Ok(mut guard) = self.identity.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.history.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.policies.lock() {
            *guard = ContextPolicyEngine::with_defaults();
        }
        Ok(())
    }
}

/// Classify why session inputs changed (for fresh-context reason labels).
fn session_change_reason(
    previous: &ContextSessionInputs,
    next: &ContextSessionInputs,
) -> &'static str {
    if previous.workspace_kind != next.workspace_kind {
        return "workspace_changed";
    }
    if previous.project_open != next.project_open
        || previous.project_indexed_documents != next.project_indexed_documents
    {
        return "project_changed";
    }
    if previous.diagnostics != next.diagnostics {
        return "diagnostics_changed";
    }
    if previous.git_status != next.git_status {
        return "git_status_changed";
    }
    if previous.workspace_inventory != next.workspace_inventory {
        return "workspace_inventory_changed";
    }
    if previous.file_summaries != next.file_summaries {
        return "file_summaries_changed";
    }
    if previous.current_file != next.current_file
        || previous.current_selection != next.current_selection
        || previous.open_files != next.open_files
    {
        return "editor_changed";
    }
    if previous.permissions != next.permissions {
        return "permissions_changed";
    }
    if previous.search_hits != next.search_hits {
        return "search_hits_changed";
    }
    if previous.workspace_snapshot != next.workspace_snapshot {
        return "workspace_snapshot_changed";
    }
    if previous.editor_snapshot != next.editor_snapshot {
        return "editor_snapshot_changed";
    }
    if previous.project_snapshot != next.project_snapshot {
        return "project_snapshot_changed";
    }
    if previous.git_snapshot != next.git_snapshot {
        return "git_snapshot_changed";
    }
    if previous.runtime_snapshot != next.runtime_snapshot {
        return "runtime_snapshot_changed";
    }
    if previous.workspace_memory_snapshot != next.workspace_memory_snapshot {
        return "workspace_memory_snapshot_changed";
    }
    "session_inputs_changed"
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::Lifecycle;
    use jaymi_database::Database;
    use jaymi_knowledge::SqliteKnowledgeStore;
    use jaymi_memory_engine::{InMemoryMemoryStore, MemoryEngine};
    use jaymi_project_engine::{InMemoryProjectStore, ProjectEngine};
    use jaymi_search::SearchEngine;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jaymi-context-unit-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bound_engine() -> ContextEngine {
        let mut memory = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        memory.initialize().unwrap();
        let memory = Arc::new(memory) as Arc<dyn MemoryEngineApi>;

        let mut projects = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
        projects.initialize().unwrap();
        let projects = Arc::new(projects) as Arc<dyn ProjectEngineApi>;

        let data = temp_dir();
        let mut db = Database::with_data_dir(&data);
        db.initialize().unwrap();
        let db = Arc::new(db);
        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);
        let mut search = SearchEngine::new(Arc::clone(&knowledge), None);
        search.initialize().unwrap();
        let search = Arc::new(search) as Arc<dyn SearchEngineApi>;

        let mut engine = ContextEngine::new();
        engine.initialize().unwrap();
        engine
            .bind_sources(ContextSources {
                memory,
                projects,
                search,
            })
            .unwrap();
        engine
    }

    #[test]
    fn lifecycle_reports_memory_dependency() {
        let engine = ContextEngine::new();
        let health = engine.health_check();
        assert!(health.dependencies.contains(&"memory_engine".to_string()));
        assert!(!health.healthy);
    }

    #[test]
    fn empty_bundle_and_reuse_are_engine_minted() {
        let engine = ContextEngine::new();
        let empty = engine.empty_bundle();
        assert_eq!(empty.assemble_generation(), 0);
        assert!(empty.sources().is_empty());
        assert!(empty.user_request().content_preview.is_empty());
        let reused = engine.reuse_bundle(&empty);
        assert_eq!(reused, empty);
        // Same snapshot shape as the historical Default placeholder.
        assert_eq!(empty, ContextBundle::default());
    }

    #[test]
    fn assemble_increments_generation_and_includes_user_request() {
        let engine = bound_engine();
        assert!(engine.providers_bound());
        assert!(engine.provider_ids().contains(&"memory"));
        assert!(engine.provider_ids().contains(&"conversation"));

        let first = engine.assemble(&UserRequest::new("hello")).unwrap();
        let second = engine.assemble(&UserRequest::new("hello again")).unwrap();
        assert_eq!(first.assemble_generation(), 1);
        assert_eq!(second.assemble_generation(), 2);
        // MemoryProvider contributes an engine snapshot when it participates
        // (bodies may be empty; promotions still surface).
        assert!(first.sources().contains(&ContextSource::RetrievedMemories));
        assert!(first.sources().contains(&ContextSource::UserRequest));
        assert_eq!(engine.assemble_count(), 2);
        assert!(engine.health_check().healthy);
    }

    #[test]
    fn context_bundle_construction() {
        let bundle = ContextBundle::builder()
            .conversation(ConversationSection {
                id: Some("conv-1".into()),
                title: Some("Planning".into()),
                status: Some("active".into()),
                project_id: Some("proj-1".into()),
                message_count: Some(4),
            })
            .active_project(ActiveProjectSection {
                project_id: Some("proj-1".into()),
                name: Some("Jaymi".into()),
                root_directory: Some("/tmp/jaymi".into()),
                detail: None,
            })
            .active_workspace(ActiveWorkspaceSection {
                kind_id: Some("coding".into()),
            })
            .current_file(CurrentFileSection {
                path: Some("/tmp/jaymi/src/lib.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            })
            .current_selection(CurrentSelectionSection {
                path: Some("/tmp/jaymi/src/lib.rs".into()),
                start_line: 10,
                start_column: 0,
                end_line: 12,
                end_column: 4,
                text: Some("fn main".into()),
            })
            .open_files(OpenFilesSection {
                files: vec![
                    OpenFileEntry {
                        path: "/tmp/jaymi/src/lib.rs".into(),
                        dirty: true,
                        active: true,
                    },
                    OpenFileEntry {
                        path: "/tmp/jaymi/Cargo.toml".into(),
                        dirty: false,
                        active: false,
                    },
                ],
            })
            .search_results(SearchResultsSection {
                hint: Some(SearchContextHint {
                    structured_query_pending: true,
                    query_preview: Some("ContextBundle".into()),
                    project_indexed_documents: Some(42),
                }),
                hits: vec![BundleSearchHit {
                    item_id: "hit-1".into(),
                    title: "bundle.rs".into(),
                    path: Some("/tmp/jaymi/crates/jaymi-context/src/bundle.rs".into()),
                    score: Some(90),
                    match_reason: Some("filename_contains".into()),
                    preview: None,
                    line: Some(1),
                    column: Some(0),
                }],
            })
            .memory_results(MemoryResultsSection::default())
            .diagnostics(DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    path: Some("/tmp/jaymi/src/lib.rs".into()),
                    severity: "warning".into(),
                    message: "unused import".into(),
                    line: Some(3),
                    column: Some(0),
                    source: Some("rustc".into()),
                }],
            })
            .permissions(PermissionsSection {
                entries: vec![BundlePermissionEntry {
                    category: "filesystem".into(),
                    action: "read".into(),
                    decision: "allowed".into(),
                    resource: Some("/tmp/jaymi".into()),
                    explanation: Some("project root".into()),
                }],
            })
            .planner_metadata(PlannerMetadataSection {
                assemble_generation: 7,
                sources: vec![
                    ContextSource::ActiveProject,
                    ContextSource::RetrievedMemories,
                    ContextSource::UserRequest,
                ],
                notes: vec!["unit-test".into()],
                budget: None,
                policy: None,
                environmental: None,
            })
            .active_capabilities(ActiveCapabilitiesSection {
                capability_ids: vec!["code".into(), "search".into()],
            })
            .user_request(UserRequestMetadataSection {
                content_preview: "open lib.rs".into(),
                has_file: true,
                ..UserRequestMetadataSection::default()
            })
            .build();

        assert_eq!(bundle.conversation().id.as_deref(), Some("conv-1"));
        assert_eq!(bundle.active_project().name.as_deref(), Some("Jaymi"));
        assert_eq!(bundle.workspace_kind(), Some("coding"));
        assert_eq!(
            bundle.current_file().path.as_deref(),
            Some("/tmp/jaymi/src/lib.rs")
        );
        assert_eq!(bundle.current_selection().text.as_deref(), Some("fn main"));
        assert_eq!(bundle.open_files().files.len(), 2);
        assert_eq!(bundle.search_results().hits.len(), 1);
        assert!(bundle.memory().is_empty());
        assert_eq!(bundle.diagnostics().diagnostics.len(), 1);
        assert_eq!(bundle.permissions().entries.len(), 1);
        assert_eq!(bundle.assemble_generation(), 7);
        assert_eq!(bundle.active_capabilities().capability_ids.len(), 2);
        assert!(bundle.user_request().has_file);
        assert!(bundle.sources().contains(&ContextSource::ActiveProject));
    }

    #[test]
    fn context_bundle_immutability() {
        let bundle = ContextBundle::builder()
            .conversation(ConversationSection {
                id: Some("conv-immutable".into()),
                title: Some("Frozen".into()),
                ..ConversationSection::default()
            })
            .active_workspace(ActiveWorkspaceSection {
                kind_id: Some("coding".into()),
            })
            .planner_metadata(PlannerMetadataSection {
                assemble_generation: 1,
                sources: vec![ContextSource::ActiveWorkspace, ContextSource::UserRequest],
                notes: Vec::new(),
                budget: None,
                policy: None,
                environmental: None,
            })
            .build();

        let first = bundle.conversation().clone();
        let second = bundle.conversation().clone();
        assert_eq!(first, second);
        assert_eq!(bundle.workspace_kind(), Some("coding"));
        assert_eq!(bundle.assemble_generation(), 1);

        let cloned = bundle.clone();
        assert_eq!(bundle, cloned);
        assert_eq!(
            cloned.conversation().id.as_deref(),
            Some("conv-immutable")
        );

        let rebuilt = ContextBundle::builder()
            .conversation(ConversationSection {
                id: Some("conv-other".into()),
                ..ConversationSection::default()
            })
            .build();
        assert_ne!(bundle.conversation().id, rebuilt.conversation().id);
        assert_eq!(
            bundle.conversation().id.as_deref(),
            Some("conv-immutable")
        );
    }

    #[test]
    fn assemble_copies_session_editor_and_request_metadata() {
        let engine = bound_engine();
        engine.set_session_inputs(ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: CurrentFileSection {
                path: Some("/proj/main.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            current_selection: CurrentSelectionSection {
                path: Some("/proj/main.rs".into()),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 8,
                text: Some("fn hello".into()),
            },
            open_files: OpenFilesSection {
                files: vec![OpenFileEntry {
                    path: "/proj/main.rs".into(),
                    dirty: false,
                    active: true,
                }],
            },
            diagnostics: DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    path: Some("/proj/main.rs".into()),
                    severity: "error".into(),
                    message: "boom".into(),
                    line: Some(1),
                    column: Some(0),
                    source: None,
                }],
            },
            git_status: GitStatusSection::default(),
            workspace_inventory: WorkspaceInventorySection::default(),
            file_summaries: FileSummariesSection::default(),
            permissions: PermissionsSection::default(),
            active_capabilities: ActiveCapabilitiesSection::default(),
            search_hits: Vec::new(),
            approved_context_providers: vec!["editor".into()],
            project_open: true,
            project_indexed_documents: None,
        
            workspace_snapshot: None,
            editor_snapshot: None,
            project_snapshot: None,
            git_snapshot: None,
            runtime_snapshot: None,
            workspace_memory_snapshot: None,
});

        let bundle = engine
            .assemble(&UserRequest::read_file("/proj/main.rs"))
            .unwrap();

        assert_eq!(bundle.workspace_kind(), Some("coding"));
        assert_eq!(bundle.current_file().path.as_deref(), Some("/proj/main.rs"));
        assert_eq!(bundle.current_selection().text.as_deref(), Some("fn hello"));
        assert!(
            bundle.open_files().files.is_empty(),
            "context policy excludes open editors by default"
        );
        assert_eq!(bundle.diagnostics().diagnostics.len(), 1);
        assert_eq!(
            bundle.active_capabilities().capability_ids,
            vec!["read_documents".to_string()],
            "hint-less assemble uses Intent defaults, not session catalog"
        );
        assert!(bundle.user_request().has_file);
        assert!(bundle.sources().contains(&ContextSource::EditorState));
        assert!(bundle.sources().contains(&ContextSource::Diagnostics));
        assert!(bundle.sources().contains(&ContextSource::ActiveCapabilities));
        assert!(bundle.sources().contains(&ContextSource::UserRequest));
        let policy = bundle.policy().expect("policy report");
        assert!(policy
            .decisions
            .iter()
            .any(|d| d.provider_id == "editor" && d.included && d.constraints.contains(&"exclude_open_files".to_string())));
    }

    #[test]
    fn providers_may_decline_when_irrelevant() {
        let calls = Arc::new(AtomicUsize::new(0));
        let declines = Arc::new(AtomicUsize::new(0));

        struct CountingProvider {
            id: &'static str,
            contribute_flag: bool,
            calls: Arc<AtomicUsize>,
            declines: Arc<AtomicUsize>,
        }

        impl ContextProvider for CountingProvider {
            fn id(&self) -> &'static str {
                self.id
            }

            fn priority(&self) -> ProviderPriority {
                ProviderPriority::CRITICAL
            }

            fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
                RelevanceScore::HIGH
            }

            fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
                BudgetEstimate::metadata(BudgetUnits::from_characters(64, 4))
            }

            fn propose_candidates(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Vec<ContextCandidate>> {
                let contribution = (|| -> JaymiResult<Option<ContextContribution>> {
                self.calls.fetch_add(1, AtomicOrdering::Relaxed);
                if self.contribute_flag {
                    Ok(Some(ContextContribution {
                        sources: vec![ContextSource::ActiveWorkspace],
                        active_workspace: Some(ActiveWorkspaceSection {
                            kind_id: Some("coding".into()),
                        }),
                        ..ContextContribution::default()
                    }))
                } else {
                    self.declines.fetch_add(1, AtomicOrdering::Relaxed);
                    Ok(None)
                }
            
                })()?;
                Ok(match contribution {
                    Some(contribution) => candidates_from_contribution(
                        self.id(),
                        contribution,
                        self.sensitivity(),
                        self.priority(),
                        self.relevance(_request).value(),
                    ),
                    None => Vec::new(),
                })
            }
        }

        let mut engine = ContextEngine::new();
        engine.initialize().unwrap();
        engine
            .bind_providers(vec![
                Arc::new(CountingProvider {
                    id: "yes",
                    contribute_flag: true,
                    calls: Arc::clone(&calls),
                    declines: Arc::clone(&declines),
                }),
                Arc::new(CountingProvider {
                    id: "no",
                    contribute_flag: false,
                    calls: Arc::clone(&calls),
                    declines: Arc::clone(&declines),
                }),
            ])
            .unwrap();

        // Non-greeting text: Greeting's strict allowlist would omit unknown providers.
        let bundle = engine
            .assemble(&UserRequest::new("what can you tell me"))
            .unwrap();
        assert_eq!(calls.load(AtomicOrdering::Relaxed), 2);
        assert_eq!(declines.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(bundle.workspace_kind(), Some("coding"));
        assert!(bundle.sources().contains(&ContextSource::ActiveWorkspace));
        assert!(bundle.sources().contains(&ContextSource::UserRequest));
        assert!(bundle
            .planner_metadata()
            .notes
            .iter()
            .any(|note| note.contains("declined=1")));
    }

    #[test]
    fn session_providers_decline_without_session_data() {
        let engine = bound_engine();
        let bundle = engine.assemble(&UserRequest::new("plain chat")).unwrap();
        assert!(!bundle.sources().contains(&ContextSource::ActiveWorkspace));
        assert!(!bundle.sources().contains(&ContextSource::EditorState));
        assert!(!bundle.sources().contains(&ContextSource::Diagnostics));
        assert!(!bundle.sources().contains(&ContextSource::Permissions));
        assert!(bundle.sources().contains(&ContextSource::RetrievedMemories));
    }

    #[test]
    fn engine_skips_providers_below_relevance_threshold() {
        struct LowRelevanceProvider;

        impl ContextProvider for LowRelevanceProvider {
            fn id(&self) -> &'static str {
                "low"
            }

            fn priority(&self) -> ProviderPriority {
                ProviderPriority::DIAGNOSTICS
            }

            fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
                RelevanceScore::new(10)
            }

            fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
                BudgetEstimate::metadata(BudgetUnits::from_characters(32, 4))
            }

            fn propose_candidates(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Vec<ContextCandidate>> {
                let contribution = (|| -> JaymiResult<Option<ContextContribution>> {
                panic!("contribute must not run when relevance is below threshold");
            
                })()?;
                Ok(match contribution {
                    Some(contribution) => candidates_from_contribution(
                        self.id(),
                        contribution,
                        self.sensitivity(),
                        self.priority(),
                        self.relevance(_request).value(),
                    ),
                    None => Vec::new(),
                })
            }
        }

        struct HighRelevanceProvider;

        impl ContextProvider for HighRelevanceProvider {
            fn id(&self) -> &'static str {
                "high"
            }

            fn priority(&self) -> ProviderPriority {
                ProviderPriority::CRITICAL
            }

            fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
                RelevanceScore::HIGH
            }

            fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
                BudgetEstimate::metadata(BudgetUnits::from_characters(64, 4))
            }

            fn propose_candidates(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Vec<ContextCandidate>> {
                let contribution = (|| -> JaymiResult<Option<ContextContribution>> {
                Ok(Some(ContextContribution {
                    sources: vec![ContextSource::ActiveWorkspace],
                    active_workspace: Some(ActiveWorkspaceSection {
                        kind_id: Some("coding".into()),
                    }),
                    ..ContextContribution::default()
                }))
            
                })()?;
                Ok(match contribution {
                    Some(contribution) => candidates_from_contribution(
                        self.id(),
                        contribution,
                        self.sensitivity(),
                        self.priority(),
                        self.relevance(_request).value(),
                    ),
                    None => Vec::new(),
                })
            }
        }

        let mut engine = ContextEngine::new();
        engine.initialize().unwrap();
        engine.set_relevance_threshold(40);
        engine
            .bind_providers(vec![
                Arc::new(LowRelevanceProvider),
                Arc::new(HighRelevanceProvider),
            ])
            .unwrap();

        // Non-greeting text so custom providers are not selection-omitted.
        let bundle = engine
            .assemble(&UserRequest::new("explain this briefly"))
            .unwrap();
        assert_eq!(bundle.workspace_kind(), Some("coding"));
        assert!(bundle
            .planner_metadata()
            .notes
            .iter()
            .any(|note| note.contains("skipped_relevance=1")));
    }

    #[test]
    fn chat_request_skips_diagnostics_without_coding_cues() {
        let engine = bound_engine();
        let bundle = engine.assemble(&UserRequest::new("hello there")).unwrap();
        assert!(
            !bundle.sources().contains(&ContextSource::Diagnostics),
            "diagnostics should be relevance-skipped for plain chat"
        );
        assert!(bundle.sources().contains(&ContextSource::RetrievedMemories));
        assert!(bundle
            .planner_metadata()
            .notes
            .iter()
            .any(|note| note.contains("skipped_relevance=")));
    }

    #[test]
    fn search_request_keeps_search_provider_relevant() {
        use jaymi_core::SearchRequest;
        let engine = bound_engine();
        engine.set_session_inputs(ContextSessionInputs {
            search_hits: vec![BundleSearchHit {
                item_id: "1".into(),
                title: "hit".into(),
                path: None,
                score: Some(1),
                match_reason: None,
                preview: None,
                line: None,
                column: None,
            }],
            ..ContextSessionInputs::default()
        });
        let bundle = engine
            .assemble(&UserRequest::search(SearchRequest::free_text("fungi")))
            .unwrap();
        assert!(bundle.sources().contains(&ContextSource::SearchResults));
        assert_eq!(bundle.search_results().hits.len(), 1);
    }

    #[test]
    fn higher_priority_providers_receive_budget_first() {
        struct BloatedLowPriority;

        impl ContextProvider for BloatedLowPriority {
            fn id(&self) -> &'static str {
                "bloated"
            }

            fn priority(&self) -> ProviderPriority {
                ProviderPriority::DIAGNOSTICS
            }

            fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
                RelevanceScore::HIGH
            }

            fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
                BudgetEstimate::flexible(BudgetUnits::from_characters(5_000, 4))
            }

            fn propose_candidates(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Vec<ContextCandidate>> {
                let contribution = (|| -> JaymiResult<Option<ContextContribution>> {
                Ok(Some(ContextContribution {
                    sources: vec![ContextSource::SearchResults],
                    search_results: Some(SearchResultsSection {
                        hint: None,
                        hits: (0..40)
                            .map(|i| BundleSearchHit {
                                item_id: format!("b-{i}"),
                                title: format!("bloated-{i}"),
                                path: None,
                                score: Some(1),
                                match_reason: None,
                                preview: Some("x".repeat(200)),
                                line: None,
                                column: None,
                            })
                            .collect(),
                    }),
                    ..ContextContribution::default()
                }))
            
                })()?;
                Ok(match contribution {
                    Some(contribution) => candidates_from_contribution(
                        self.id(),
                        contribution,
                        self.sensitivity(),
                        self.priority(),
                        self.relevance(_request).value(),
                    ),
                    None => Vec::new(),
                })
            }
        }

        struct TinyHighPriority;

        impl ContextProvider for TinyHighPriority {
            fn id(&self) -> &'static str {
                "tiny"
            }

            fn priority(&self) -> ProviderPriority {
                ProviderPriority::CRITICAL
            }

            fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
                RelevanceScore::HIGH
            }

            fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
                BudgetEstimate::metadata(BudgetUnits::from_characters(32, 4))
            }

            fn propose_candidates(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Vec<ContextCandidate>> {
                let contribution = (|| -> JaymiResult<Option<ContextContribution>> {
                Ok(Some(ContextContribution {
                    sources: vec![ContextSource::ActiveWorkspace],
                    active_workspace: Some(ActiveWorkspaceSection {
                        kind_id: Some("coding".into()),
                    }),
                    ..ContextContribution::default()
                }))
            
                })()?;
                Ok(match contribution {
                    Some(contribution) => candidates_from_contribution(
                        self.id(),
                        contribution,
                        self.sensitivity(),
                        self.priority(),
                        self.relevance(_request).value(),
                    ),
                    None => Vec::new(),
                })
            }
        }

        let mut engine = ContextEngine::new();
        engine.initialize().unwrap();
        engine.set_budget_config(ContextBudgetConfig {
            max_characters: 800,
            max_tokens: None,
            chars_per_token: 4,
            reserved_characters: 100,
        });
        // Register low priority first to prove ordering is by priority, not registration.
        engine
            .bind_providers(vec![
                Arc::new(BloatedLowPriority),
                Arc::new(TinyHighPriority),
            ])
            .unwrap();

        // Non-greeting text so custom providers participate under GeneralChat.
        let bundle = engine
            .assemble(&UserRequest::new("budget ordering check"))
            .unwrap();
        assert_eq!(bundle.workspace_kind(), Some("coding"));
        let report = bundle.budget().expect("budget report");
        assert!(report.used_characters <= 700);
        assert!(
            report.truncated_providers.iter().any(|id| id == "bloated")
                || report.skipped_budget.iter().any(|id| id == "bloated")
                || bundle.search_results().hits.len() < 40,
            "bloated provider should be fitted or reduced under budget"
        );
    }

    #[test]
    fn budget_report_records_usage() {
        let engine = bound_engine();
        engine.set_max_characters(4_000);
        let bundle = engine.assemble(&UserRequest::new("hello")).unwrap();
        let report = bundle.budget().expect("budget report");
        assert_eq!(report.max_characters, 4_000);
        assert!(report.used_characters <= 4_000);
        assert!(report.estimated_tokens > 0 || report.used_characters == 0);
    }

    #[test]
    fn context_inspector_records_latest_assemble_without_affecting_bundle() {
        let engine = bound_engine();
        assert!(engine.last_inspection().is_none());

        let bundle = engine.assemble(&UserRequest::new("hello inspector")).unwrap();
        let report = engine.inspect_last().expect("inspection recorded");

        assert_eq!(report.assemble_generation, bundle.assemble_generation());
        assert!(report.request_preview.contains("hello inspector"));
        assert!(!report.providers.is_empty());
        assert!(
            report.contributed().iter().any(|p| p.id == "conversation")
                || report.contributed().iter().any(|p| p.id == "memory")
                || report.contributed().iter().any(|p| p.id == "permission")
                || report.contributed().iter().any(|p| p.id == "workspace"),
            "at least one core provider should contribute (greeting → memory)"
        );
        assert!(
            report.omitted().iter().any(|p| p.id == "diagnostics"),
            "diagnostics should be relevance-omitted for plain chat"
        );
        assert!(report.budget.is_some());
        assert!(!report.sections.is_empty());
        assert_eq!(report.cache_status(), "miss");
        assert!(report.bundle_size_characters > 0);
        assert!(!report.contributor_order.is_empty() || report.contributed().is_empty());
        assert!(
            report
                .providers
                .windows(2)
                .all(|pair| pair[0].evaluation_order <= pair[1].evaluation_order),
            "providers must be sorted by evaluation order"
        );
        assert!(report.providers.iter().all(|provider| !provider.sensitivity.is_empty()));
        assert!(report.providers.iter().all(|provider| {
            matches!(
                provider.approval_status.as_str(),
                "not_required" | "approved" | "pending" | "n/a"
            )
        }));
        let rendered = report.render();
        assert!(rendered.contains("Context Inspector"));
        assert!(rendered.contains("duration_ms="));
        assert!(rendered.contains("final_bundle="));
        assert!(rendered.contains("provider_order (contributors):"));
        assert!(rendered.contains("cache=miss"));
        // Bundle identity unchanged by inspector recording.
        assert_eq!(bundle.sources(), report.sources.as_slice());
    }

    #[test]
    fn context_bundle_cache_hits_same_key() {
        let engine = bound_engine();
        let request = UserRequest::new("cache me");
        let first = engine.assemble(&request).unwrap();
        let second = engine.assemble(&request).unwrap();
        let stats = engine.cache_stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(engine.assemble_count(), 2);
        assert_eq!(second.assemble_generation(), first.assemble_generation() + 1);
        assert!(second
            .planner_metadata()
            .notes
            .iter()
            .any(|note| note.contains("cache_hit")));
        let inspection = engine.inspect_last().expect("inspection");
        assert!(inspection.cache_hit);
        assert_eq!(inspection.cache_status(), "hit");
        assert!(inspection.render().contains("cache=hit"));
        assert!(inspection.bundle_size_characters > 0);
    }

    #[test]
    fn context_bundle_cache_misses_after_invalidate() {
        let engine = bound_engine();
        let request = UserRequest::new("same content");
        let _ = engine.assemble(&request).unwrap();
        engine.invalidate_cache("files_changed");
        let _ = engine.assemble(&request).unwrap();
        let stats = engine.cache_stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);
        assert_eq!(stats.invalidations, 1);
        assert_eq!(stats.epoch, 1);
        assert!(!engine.inspect_last().unwrap().cache_hit);
    }

    #[test]
    fn context_bundle_cache_misses_on_request_fingerprint_change() {
        let engine = bound_engine();
        let _ = engine.assemble(&UserRequest::new("alpha")).unwrap();
        let _ = engine.assemble(&UserRequest::new("beta")).unwrap();
        let stats = engine.cache_stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);
    }

    #[test]
    fn unchanged_workspace_does_not_invalidate_cache() {
        let engine = bound_engine();
        engine.set_session_workspace(Some("coding".into()));
        let epoch_after_set = engine.cache_stats().epoch;
        let _ = engine.assemble(&UserRequest::new("hello")).unwrap();
        engine.set_session_workspace(Some("coding".into()));
        assert_eq!(engine.cache_stats().epoch, epoch_after_set);
        let _ = engine.assemble(&UserRequest::new("hello")).unwrap();
        assert_eq!(engine.cache_stats().hits, 1);
    }

    #[test]
    fn context_history_records_timestamp_request_providers_size_duration() {
        let engine = bound_engine();
        assert!(engine.history().is_empty());

        let bundle = engine
            .assemble(&UserRequest::new("history please"))
            .unwrap();
        let entries = engine.history();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.assemble_generation, bundle.assemble_generation());
        assert!(entry.timestamp_unix_ms > 0);
        assert!(entry.request.contains("history please"));
        // Bare assemble may have zero provider contributions when Memory/Conversation
        // decline and no session workspace is set — UserRequest is engine-stamped.
        assert!(entry.bundle_size_characters > 0);
        // Duration is wall-clock; allow zero on very fast machines.
        assert_eq!(entry.bundle.assemble_generation(), bundle.assemble_generation());
        assert!(!entry.cache_hit);

        let _ = engine.assemble(&UserRequest::new("history please")).unwrap();
        let entries = engine.history();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].cache_hit);
        assert!(!entries[1].cache_hit);
        assert!(entries[0].summary().contains("cache_hit=true"));
    }

    #[test]
    fn llm_context_api_is_stable_and_deterministic() {
        let engine = bound_engine();
        engine.set_session_workspace(Some("coding".into()));
        let bundle = engine
            .assemble(&UserRequest::new("llm facing context"))
            .unwrap();

        let llm = engine.to_llm_context(&bundle);
        assert_eq!(llm.schema_version, LLM_CONTEXT_SCHEMA_VERSION);
        assert_eq!(llm.assemble_generation, bundle.assemble_generation());
        assert_eq!(
            llm.sections
                .iter()
                .map(|section| section.id)
                .collect::<Vec<_>>(),
            LlmSectionId::ORDER.to_vec()
        );
        assert!(llm
            .sections
            .iter()
            .any(|section| section.id == LlmSectionId::UserRequest && section.present));
        assert!(!llm.providers.sources.is_empty());

        let json = engine.serialize_llm_context(&bundle).unwrap();
        assert_eq!(json, bundle.to_llm_context().to_json().unwrap());
        assert!(json.contains(&format!("\"schema_version\":{LLM_CONTEXT_SCHEMA_VERSION}")));
        assert!(json.contains("llm facing context"));
    }

    #[test]
    fn context_policies_exclude_search_without_retrieval() {
        let engine = bound_engine();
        let bundle = engine.assemble(&UserRequest::new("hello there")).unwrap();
        let policy = bundle.policy().expect("policy report");
        assert!(
            policy
                .decisions
                .iter()
                .any(|d| d.provider_id == "search" && !d.included),
            "search should be policy-excluded for plain chat"
        );
        assert!(
            policy
                .decisions
                .iter()
                .any(|d| d.provider_id == "conversation" && d.included),
            "conversation always included"
        );
        assert!(
            policy
                .decisions
                .iter()
                .any(|d| d.provider_id == "permission" && d.included),
            "permission always included"
        );
        assert!(policy.size_before_characters >= policy.size_after_characters);
        let inspection = engine.inspect_last().unwrap();
        assert!(inspection.policy.is_some());
        assert!(inspection.render().contains("Context Policy"));
        assert!(inspection
            .providers
            .iter()
            .any(|p| matches!(p.outcome, ProviderInspectOutcome::SkippedPolicy { .. })));
    }

    #[test]
    fn context_policies_are_deterministic() {
        let engine = bound_engine();
        let request = UserRequest::new("hello policies");
        let first = engine.assemble(&request).unwrap();
        engine.invalidate_cache("test");
        let second = engine.assemble(&request).unwrap();
        assert_eq!(
            first.policy().unwrap().decisions,
            second.policy().unwrap().decisions
        );
    }

    #[test]
    fn sensitivity_filtering_blocks_oversensitive_providers() {
        let engine = bound_engine();
        // Replace with a policy that lowers max by denying Sensitive/Private via custom policy.
        struct BlockPrivate;
        impl ContextPolicy for BlockPrivate {
            fn id(&self) -> &'static str {
                "block_private"
            }
            fn evaluate(
                &self,
                candidate: &ContextPolicyCandidate<'_>,
            ) -> ContextPolicyDecision {
                if candidate.sensitivity >= Sensitivity::Private {
                    ContextPolicyDecision::deny("private blocked for test")
                } else {
                    ContextPolicyDecision::allow(
                        "ok",
                        candidate.provider_priority,
                    )
                }
            }
        }
        engine.set_context_policies(vec![Arc::new(BlockPrivate)]);
        let bundle = engine.assemble(&UserRequest::new("hello")).unwrap();
        let policy = bundle.policy().unwrap();
        assert!(policy
            .decisions
            .iter()
            .any(|d| d.provider_id == "memory" && !d.included));
        assert!(policy
            .decisions
            .iter()
            .any(|d| d.provider_id == "conversation" && !d.included));
    }

    #[test]
    fn selection_text_is_omitted_until_editor_approved() {
        let engine = bound_engine();
        engine.set_session_inputs(ContextSessionInputs {
            current_file: CurrentFileSection {
                path: Some("/proj/main.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            current_selection: CurrentSelectionSection {
                path: Some("/proj/main.rs".into()),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 8,
                text: Some("fn hello".into()),
            },
            ..ContextSessionInputs::default()
        });
        let bundle = engine
            .assemble(&UserRequest::read_file("/proj/main.rs"))
            .unwrap();
        assert!(
            bundle.current_selection().text.is_none(),
            "selection text must wait for approval"
        );
        let policy = bundle.policy().unwrap();
        let editor = policy
            .decisions
            .iter()
            .find(|d| d.provider_id == "editor")
            .expect("editor decision");
        assert!(!editor.included);
        assert_eq!(editor.approval_status, "pending");
        let inspection = engine.inspect_last().unwrap();
        assert!(inspection.providers.iter().any(|p| {
            p.id == "editor" && matches!(p.outcome, ProviderInspectOutcome::SkippedApproval { .. })
        }));
    }

    #[test]
    fn policy_forbids_truncation_skips_oversized_provider() {
        struct FixedBlob;
        impl ContextProvider for FixedBlob {
            fn id(&self) -> &'static str {
                "fixed_blob"
            }
            fn priority(&self) -> ProviderPriority {
                ProviderPriority::new(1)
            }
            fn relevance(&self, _: &ProviderRequest<'_>) -> RelevanceScore {
                RelevanceScore::HIGH
            }
            fn estimate_size(&self, _: &ProviderRequest<'_>) -> BudgetEstimate {
                BudgetEstimate {
                    units: BudgetUnits {
                        characters: 50_000,
                        estimated_tokens: 12_500,
                    },
                    can_truncate: true,
                    can_summarize: true,
                }
            }
            fn propose_candidates(
                &self,
                request: &ProviderRequest<'_>,
            ) -> JaymiResult<Vec<ContextCandidate>> {
                let contribution = (|| -> JaymiResult<Option<ContextContribution>> {
                Ok(Some(ContextContribution {
                    sources: vec![ContextSource::UserRequest],
                    diagnostics: Some(DiagnosticsSection {
                        diagnostics: vec![BundleDiagnostic {
                            path: None,
                            severity: "info".into(),
                            message: "x".repeat(50_000),
                            line: None,
                            column: None,
                            source: None,
                        }],
                    }),
                    ..ContextContribution::default()
                }))
            
                })()?;
                Ok(match contribution {
                    Some(contribution) => candidates_from_contribution(
                        self.id(),
                        contribution,
                        self.sensitivity(),
                        self.priority(),
                        self.relevance(request).value(),
                    ),
                    None => Vec::new(),
                })
            }
        }

        struct ForbidTruncate;
        impl ContextPolicy for ForbidTruncate {
            fn id(&self) -> &'static str {
                "forbid_truncate"
            }
            fn evaluate(
                &self,
                candidate: &ContextPolicyCandidate<'_>,
            ) -> ContextPolicyDecision {
                if candidate.provider_id == "fixed_blob" {
                    let mut decision =
                        ContextPolicyDecision::allow("allow but no truncate", ProviderPriority::new(1));
                    decision.can_truncate = false;
                    decision.bypass_relevance = true;
                    decision
                } else {
                    ContextPolicyDecision::allow("ok", candidate.provider_priority)
                }
            }
        }

        let engine = bound_engine();
        engine.set_budget_config(ContextBudgetConfig {
            max_characters: 2_000,
            max_tokens: Some(500),
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
            reserved_characters: ENGINE_RESERVED_CHARACTERS,
        });
        let mut providers = engine
            .providers
            .lock()
            .expect("providers")
            .clone();
        providers.push(Arc::new(FixedBlob));
        engine.bind_providers(providers).unwrap();
        engine.set_context_policies(vec![Arc::new(ForbidTruncate)]);

        let bundle = engine.assemble(&UserRequest::new("hello truncate policy")).unwrap();
        let policy = bundle.policy().unwrap();
        let blob = policy
            .decisions
            .iter()
            .find(|d| d.provider_id == "fixed_blob")
            .expect("blob decision");
        assert!(
            blob.truncation_reason
                .as_deref()
                == Some("policy_forbids_truncation")
                || !blob.included,
            "expected policy_forbids_truncation, got {:?}",
            blob
        );
        let inspection = engine.inspect_last().unwrap();
        assert!(inspection.providers.iter().any(|p| {
            p.id == "fixed_blob"
                && matches!(
                    &p.outcome,
                    ProviderInspectOutcome::SkippedBudget {
                        reason,
                        ..
                    } if reason == "policy_forbids_truncation"
                )
        }));
    }

    #[test]
    fn permission_summary_constraint_is_enforced_in_bundle() {
        let engine = bound_engine();
        engine.set_session_inputs(ContextSessionInputs {
            permissions: PermissionsSection {
                entries: vec![BundlePermissionEntry {
                    category: "fs".into(),
                    action: "read".into(),
                    decision: "allowed".into(),
                    resource: Some("/tmp/secret".into()),
                    explanation: Some("because".into()),
                }],
            },
            ..ContextSessionInputs::default()
        });
        let bundle = engine.assemble(&UserRequest::new("hello")).unwrap();
        let entry = &bundle.permissions().entries[0];
        assert!(entry.resource.is_none());
        assert!(entry.explanation.is_none());
        let policy = bundle.policy().unwrap();
        assert!(policy.decisions.iter().any(|d| {
            d.provider_id == "permission"
                && d.included
                && d.constraints.iter().any(|c| c == "permission_summary_only")
        }));
    }

    #[test]
    fn production_providers_propose_typed_candidates_not_legacy() {
        // Sprint B2.13.1 — every default provider exposes propose_candidates with
        // typed payloads (no LegacyContribution fallback on the production path).
        use crate::candidate::ContextCandidateKind;
        use crate::BundleDiagnostic;
        use crate::OpenFileEntry;

        let engine = bound_engine();
        engine.set_session_inputs(ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: CurrentFileSection {
                path: Some("src/main.rs".into()),
                language: Some("rust".into()),
                ..CurrentFileSection::default()
            },
            open_files: OpenFilesSection {
                files: vec![OpenFileEntry {
                    path: "src/lib.rs".into(),
                    dirty: false,
                    active: false,
                }],
            },
            diagnostics: DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    severity: "error".into(),
                    message: "boom".into(),
                    path: Some("src/main.rs".into()),
                    line: Some(1),
                    column: None,
                    source: None,
                }],
            },
            file_summaries: FileSummariesSection {
                entries: vec![crate::FileSummaryEntry {
                    path: "src/main.rs".into(),
                    summary: "fn main".into(),
                    language: Some("rust".into()),
                    line_count: Some(10),
                }],
            },
            git_status: GitStatusSection {
                is_repository: true,
                summary: "main · clean".into(),
                ..GitStatusSection::default()
            },
            ..ContextSessionInputs::default()
        });

        let request = UserRequest::new("why won't this compile?");
        let bundle = engine.assemble(&request).unwrap();
        let policy = bundle.policy().expect("policy report");
        assert!(
            policy.candidate_selection.proposed > 0,
            "expected candidate proposals"
        );
        assert!(
            policy
                .candidate_selection
                .decisions
                .iter()
                .all(|d| d.kind != ContextCandidateKind::LegacyContribution.as_str()),
            "production path must not emit LegacyContribution: {:?}",
            policy.candidate_selection.decisions
        );
        // Editor / diagnostics fine-grained kinds should appear when selected.
        let kinds: Vec<_> = policy
            .candidate_selection
            .decisions
            .iter()
            .map(|d| d.kind.as_str())
            .collect();
        assert!(
            kinds.iter().any(|k| *k == "diagnostic"
                || *k == "current_file"
                || *k == "open_file"
                || *k == "file_summary"
                || *k == "conversation"),
            "expected typed candidate kinds, got {kinds:?}"
        );
    }
}
