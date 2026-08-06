//! Context Engine for Jaymi.
//!
//! Assembles only the context required for the current request. The Planner
//! calls [`ContextEngine::assemble`] and does not coordinate Memory, Project,
//! Search, or session workspace state itself.
//!
//! Assemble is provider-driven: registered [`ContextProvider`]s contribute
//! sections to an immutable [`ContextBundle`]. The engine scores relevance,
//! orders by priority, and assembles within a configurable character/token
//! budget — truncating and summarizing oversized contributions while
//! preserving important metadata. Ready for future LLM context windows.
//!
//! Recently assembled bundles are cached (keyed by project, workspace,
//! conversation, active file, and request type). The cache never changes
//! correctness: entries are fingerprinted and invalidated when files,
//! project, workspace, conversation, or the search index change.
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
mod cache;
mod history;
mod inspector;
mod llm;
mod policy;
mod provider;
mod providers;
mod relevance;

pub use budget::{
    fit_contribution, measure_contribution, BudgetEstimate, BudgetUnits, ContextBudgetConfig,
    ProviderPriority, DEFAULT_CHARS_PER_TOKEN, DEFAULT_MAX_CHARACTERS, ENGINE_RESERVED_CHARACTERS,
};
pub use bundle::{
    ActiveCapabilitiesSection, ActiveProjectSection, ActiveWorkspaceSection, BudgetReport,
    BundleDiagnostic, BundlePermissionEntry, BundleSearchHit, ContextBundle, ContextBundleBuilder,
    ContextSessionInputs, ContextSource, ConversationSection, CurrentFileSection,
    CurrentSelectionSection, DiagnosticsSection, MemoryResultsSection, OpenFileEntry,
    OpenFilesSection, PermissionsSection, PlannerMetadataSection, SearchContextHint,
    SearchResultsSection, UserRequestMetadataSection,
};
pub use cache::{
    fingerprint_request, CacheIdentity, ContextBundleCache, ContextCacheEntry, ContextCacheKey,
    ContextCacheStats, DEFAULT_CACHE_CAPACITY,
};
pub use history::{
    measure_bundle_size, ContextHistory, ContextHistoryEntry, DEFAULT_HISTORY_CAPACITY,
};
pub use llm::{
    LlmActiveCapabilities, LlmActiveProject, LlmActiveWorkspace, LlmBudgetView, LlmContext,
    LlmContextSection, LlmConversation, LlmCurrentFile, LlmCurrentSelection, LlmDiagnostic,
    LlmDiagnostics, LlmMemoryItem, LlmMemoryResults, LlmOpenFileEntry, LlmOpenFiles,
    LlmPermissionEntry, LlmPermissions, LlmProjectDetailSummary, LlmPromotionSuggestion,
    LlmProviderMetadata, LlmSearchHint, LlmSearchHit, LlmSearchResults, LlmSectionContent,
    LlmSectionId, LlmUserRequest, LLM_CONTEXT_SCHEMA_VERSION,
};
pub use policy::{
    apply_contribution_constraints, default_context_policies, ContextPolicy,
    ContextPolicyCandidate, ContextPolicyDecision, ContextPolicyDecisionRecord,
    ContextPolicyEngine, ContextPolicyInputs, ContributionConstraints, JaymiDefaultContextPolicy,
    PolicyDecisionSummary, PolicyReport, Sensitivity, DEFAULT_CONTEXT_POLICY_ID,
};
pub use provider::{ContextContribution, ContextProvider, ProviderRequest};
pub use providers::{
    default_providers, ConversationProvider, DiagnosticsProvider, EditorProvider, MemoryProvider,
    PermissionProvider, ProjectProvider, ProviderDeps, SearchProvider, WorkspaceProvider,
};
pub use inspector::{
    inspect_bundle_sections, ContextInspectorReport, InspectedBundleSection, InspectedProvider,
    ProviderInspectOutcome,
};
pub use relevance::{
    IntentTag, RelevanceScore, RelevanceSignals, RequestKind, DEFAULT_RELEVANCE_THRESHOLD,
    RELEVANCE_MAX,
};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, UserRequest};
use jaymi_memory_engine::MemoryEngineApi;
use jaymi_project_engine::ProjectEngineApi;
use jaymi_search::SearchEngineApi;

const NAME: &str = "context_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
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
    /// Invalidates the ContextBundle cache when the snapshot actually changes.
    pub fn set_session_inputs(&self, inputs: ContextSessionInputs) {
        if let Ok(mut guard) = self.session.lock() {
            if *guard != inputs {
                *guard = inputs;
                drop(guard);
                self.invalidate_cache("session_inputs_changed");
            }
        }
    }

    /// Record the active UX workspace kind for the next assemble (session state).
    ///
    /// Invalidates the ContextBundle cache when the workspace kind changes.
    pub fn set_session_workspace(&self, workspace_kind: Option<String>) {
        if let Ok(mut guard) = self.session.lock() {
            if guard.workspace_kind != workspace_kind {
                guard.workspace_kind = workspace_kind;
                drop(guard);
                self.invalidate_cache("workspace_changed");
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

    /// Number of successful `assemble` calls since boot (tests / diagnostics).
    pub fn assemble_count(&self) -> u64 {
        self.assemble_count.load(Ordering::Relaxed)
    }

    /// Drop cached bundles and bump the invalidation epoch.
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
        self.invalidate_cache("context_policies_changed");
    }

    /// Register an additional Context Policy.
    pub fn register_context_policy(&self, policy: Arc<dyn ContextPolicy>) {
        if let Ok(mut guard) = self.policies.lock() {
            guard.register_policy(policy);
        }
        self.invalidate_cache("context_policy_registered");
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
    pub fn assemble(&self, request: &UserRequest) -> JaymiResult<ContextBundle> {
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
        let signals = RelevanceSignals::derive(request, &session);
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
                inspection.notes = bundle.planner_metadata().notes.clone();
                inspection.sources = bundle.sources().to_vec();
                inspection.budget = bundle.budget().cloned();
                inspection.policy = bundle.policy().cloned();
                inspection.sections =
                    inspect_bundle_sections(&bundle, budget_config.chars_per_token);
                let duration_ms = started.elapsed().as_millis() as u64;
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

        let project_open = identity
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

        // Policy → relevance → priority sort → budget → contribute.
        // Policies never gather context; they only decide participation.
        let mut ranked: Vec<(
            Arc<dyn ContextProvider>,
            RelevanceScore,
            ContextPolicyDecision,
        )> = Vec::new();
        let mut skipped_relevance = 0usize;
        let mut skipped_policy = 0usize;
        let mut inspect_providers: Vec<InspectedProvider> = Vec::new();
        let mut policy_summaries: Vec<PolicyDecisionSummary> = Vec::new();
        let mut size_before_characters = 0usize;
        let mut size_after_characters = 0usize;
        let active_policies = self.active_context_policies();

        let policy_engine = self
            .policies
            .lock()
            .map_err(|_| JaymiError::new("context policy engine lock poisoned"))?;

        for provider in &providers {
            let score = provider.relevance(&provider_request);
            let estimate = provider.estimate_size(&provider_request);
            let candidate = ContextPolicyCandidate {
                provider_id: provider.id(),
                provider_priority: provider.priority(),
                relevance: score,
                sensitivity: provider.sensitivity(),
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
                inspect_providers.push(InspectedProvider {
                    id: provider.id().to_string(),
                    priority: provider.priority().value(),
                    relevance: score.value(),
                    estimate_characters: estimate.units.characters,
                    estimate_tokens: estimate.units.estimated_tokens,
                    outcome: ProviderInspectOutcome::SkippedPolicy {
                        policy: record
                            .applied_policies
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "context_policy".into()),
                        reason: record.decision.reason.clone(),
                        sensitivity: record.sensitivity.as_str().to_string(),
                    },
                });
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
                inspect_providers.push(InspectedProvider {
                    id: provider.id().to_string(),
                    priority: record.decision.priority.value(),
                    relevance: score.value(),
                    estimate_characters: estimate.units.characters,
                    estimate_tokens: estimate.units.estimated_tokens,
                    outcome: ProviderInspectOutcome::SkippedRelevance { threshold },
                });
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

            size_after_characters =
                size_after_characters.saturating_add(estimate.units.characters);
            policy_summaries.push(summary);
            ranked.push((Arc::clone(provider), score, record.decision));
        }
        drop(policy_engine);

        ranked.sort_by(|(left, left_score, left_decision), (right, right_score, right_decision)| {
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

        for (provider, score, decision) in &ranked {
            let remaining = budget_config.remaining_characters(used_characters);
            let estimate = provider.estimate_size(&provider_request);
            let effective_can_truncate = estimate.can_truncate && decision.can_truncate;
            if remaining == 0 {
                budget_report.skipped_budget.push(provider.id().to_string());
                inspect_providers.push(InspectedProvider {
                    id: provider.id().to_string(),
                    priority: decision.priority.value(),
                    relevance: score.value(),
                    estimate_characters: estimate.units.characters,
                    estimate_tokens: estimate.units.estimated_tokens,
                    outcome: ProviderInspectOutcome::SkippedBudget {
                        remaining_characters: 0,
                        estimate_characters: estimate.units.characters,
                        reason: "budget_exhausted".into(),
                    },
                });
                continue;
            }

            if estimate.units.characters > remaining
                && !effective_can_truncate
                && !estimate.can_summarize
            {
                budget_report.skipped_budget.push(provider.id().to_string());
                inspect_providers.push(InspectedProvider {
                    id: provider.id().to_string(),
                    priority: decision.priority.value(),
                    relevance: score.value(),
                    estimate_characters: estimate.units.characters,
                    estimate_tokens: estimate.units.estimated_tokens,
                    outcome: ProviderInspectOutcome::SkippedBudget {
                        remaining_characters: remaining,
                        estimate_characters: estimate.units.characters,
                        reason: "estimate_exceeds_budget".into(),
                    },
                });
                continue;
            }

            match provider.contribute(&provider_request)? {
                Some(contribution) => {
                    let contribution =
                        apply_contribution_constraints(contribution, &decision.constraints);
                    let (fitted, outcome) = fit_contribution(
                        contribution,
                        remaining,
                        budget_config.chars_per_token,
                    );
                    if outcome.drop || fitted.is_empty() {
                        budget_report.skipped_budget.push(provider.id().to_string());
                        if let Some(summary) = &outcome.summary {
                            budget_report
                                .summaries
                                .push(format!("{}: {summary}", provider.id()));
                        }
                        inspect_providers.push(InspectedProvider {
                            id: provider.id().to_string(),
                            priority: decision.priority.value(),
                            relevance: score.value(),
                            estimate_characters: estimate.units.characters,
                            estimate_tokens: estimate.units.estimated_tokens,
                            outcome: ProviderInspectOutcome::Dropped {
                                summary: outcome.summary.clone(),
                            },
                        });
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
                    inspect_providers.push(InspectedProvider {
                        id: provider.id().to_string(),
                        priority: decision.priority.value(),
                        relevance: score.value(),
                        estimate_characters: estimate.units.characters,
                        estimate_tokens: estimate.units.estimated_tokens,
                        outcome: ProviderInspectOutcome::Contributed {
                            characters: outcome.final_units.characters,
                            estimated_tokens: outcome.final_units.estimated_tokens,
                            truncated: outcome.truncated,
                            summarized: outcome.summarized,
                            summary: outcome.summary.clone(),
                            sources: fitted.sources.clone(),
                        },
                    });
                    for source in &fitted.sources {
                        if !included.contains(source) {
                            included.push(*source);
                        }
                    }
                    builder = builder.apply_contribution(fitted);
                }
                None => {
                    declined += 1;
                    inspect_providers.push(InspectedProvider {
                        id: provider.id().to_string(),
                        priority: decision.priority.value(),
                        relevance: score.value(),
                        estimate_characters: estimate.units.characters,
                        estimate_tokens: estimate.units.estimated_tokens,
                        outcome: ProviderInspectOutcome::Declined,
                    });
                }
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
        };

        // Engine-owned stamps (not subsystem providers).
        builder = builder.user_request(UserRequestMetadataSection::from_request(request));
        if !included.contains(&ContextSource::UserRequest) {
            included.push(ContextSource::UserRequest);
        }
        let assemble_generation = self.assemble_count.fetch_add(1, Ordering::Relaxed) + 1;
        let mut notes = vec![
            format!(
                "providers contributed={contributed} declined={declined} skipped_relevance={skipped_relevance} skipped_policy={skipped_policy} skipped_budget={} truncated={} threshold={threshold} kind={} budget_chars={}/{}",
                budget_report.skipped_budget.len(),
                budget_report.truncated_providers.len(),
                signals.request_kind.as_str(),
                used_characters,
                budget_config.provider_character_budget()
            ),
            format!(
                "policy active=[{}] included=[{}] excluded=[{}] size_before={} size_after={} assembled={}",
                policy_report.active_policies.join(","),
                policy_report.included_providers().join(","),
                policy_report.excluded_providers().join(","),
                policy_report.size_before_characters,
                policy_report.size_after_characters,
                policy_report.size_assembled_characters
            ),
        ];
        notes.extend(budget_report.summaries.iter().cloned());
        builder = builder.planner_metadata(PlannerMetadataSection {
            assemble_generation,
            sources: included,
            notes,
            budget: Some(budget_report),
            policy: Some(policy_report.clone()),
        });

        let bundle = builder.build();

        let inspection = ContextInspectorReport {
            assemble_generation: bundle.assemble_generation(),
            request_preview: bundle.user_request().content_preview.clone(),
            request_kind: signals.request_kind.as_str().to_string(),
            workspace_kind: bundle.workspace_kind().map(str::to_string),
            relevance_threshold: threshold,
            providers: inspect_providers,
            sections: inspect_bundle_sections(&bundle, budget_config.chars_per_token),
            sources: bundle.sources().to_vec(),
            budget: bundle.budget().cloned(),
            notes: bundle.planner_metadata().notes.clone(),
            cache_hit: false,
            policy: Some(policy_report),
        };
        let duration_ms = started.elapsed().as_millis() as u64;
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
    fn assemble_increments_generation_and_includes_memory_source() {
        let engine = bound_engine();
        assert!(engine.providers_bound());
        assert!(engine.provider_ids().contains(&"memory"));
        assert!(engine.provider_ids().contains(&"conversation"));

        let first = engine.assemble(&UserRequest::new("hello")).unwrap();
        let second = engine.assemble(&UserRequest::new("hello again")).unwrap();
        assert_eq!(first.assemble_generation(), 1);
        assert_eq!(second.assemble_generation(), 2);
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
            permissions: PermissionsSection::default(),
            active_capabilities: ActiveCapabilitiesSection {
                capability_ids: vec!["code".into()],
            },
            search_hits: Vec::new(),
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
            vec!["code".to_string()]
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

            fn contribute(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Option<ContextContribution>> {
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

        let bundle = engine.assemble(&UserRequest::new("hello")).unwrap();
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

            fn contribute(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Option<ContextContribution>> {
                panic!("contribute must not run when relevance is below threshold");
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

            fn contribute(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Option<ContextContribution>> {
                Ok(Some(ContextContribution {
                    sources: vec![ContextSource::ActiveWorkspace],
                    active_workspace: Some(ActiveWorkspaceSection {
                        kind_id: Some("coding".into()),
                    }),
                    ..ContextContribution::default()
                }))
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

        let bundle = engine.assemble(&UserRequest::new("hello")).unwrap();
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

            fn contribute(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Option<ContextContribution>> {
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

            fn contribute(
                &self,
                _request: &ProviderRequest<'_>,
            ) -> JaymiResult<Option<ContextContribution>> {
                Ok(Some(ContextContribution {
                    sources: vec![ContextSource::ActiveWorkspace],
                    active_workspace: Some(ActiveWorkspaceSection {
                        kind_id: Some("coding".into()),
                    }),
                    ..ContextContribution::default()
                }))
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

        let bundle = engine.assemble(&UserRequest::new("hello")).unwrap();
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
        assert!(report.contributed().iter().any(|p| p.id == "memory"));
        assert!(
            report.omitted().iter().any(|p| p.id == "diagnostics"),
            "diagnostics should be relevance-omitted for plain chat"
        );
        assert!(report.budget.is_some());
        assert!(!report.sections.is_empty());
        assert!(report.render().contains("Context Inspector"));
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
        assert!(!entry.providers_used.is_empty());
        assert!(entry.providers_used.iter().any(|id| id == "memory"));
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
        assert!(json.contains("\"schema_version\":1"));
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
}
