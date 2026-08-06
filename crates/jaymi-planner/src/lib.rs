//! Planner — the orchestration kernel of Jaymi.
//!
//! Every user-facing request enters [`Planner::handle`]. Canonical stages:
//! Intent → Capability → Context Policy → Providers → Context Engine →
//! ContextBundle → Behavior (Planned) → Execution Plan → Review (if required) →
//! Action Policies → Permissions → Tool Orchestrator → Providers →
//! Execution Summary → Response.
//!
//! Meaningful tool-backed actions become an [`execution_plan::ExecutionPlan`]
//! before any tool runs. Tools never generate plans; providers never see them.
//!
//! After Context assemble, tool-backed intents dispatch through a registered
//! [`ToolRouteTable`] (Intent → tool). Session / PlanWork stay special-cased.
//! Unknown / conversational intents invoke the Reasoning Engine after
//! ContextBundle assemble — never bypassing Planner or Context.
//! Conversation runtime phase is a Planner-owned [`ConversationState`]
//! machine (Idle → Preparing Context → Reasoning / Streaming /
//! Waiting For Review / Executing → terminal).
//! See `dispatch`, `request_lifecycle`, `conversation_state`, and docs/planner.md.
//!
//! The Planner does not own long-lived Memory or Project CRUD APIs. Those
//! belong to the Memory Engine and Project Engine. Application (or tools)
//! call those engines directly for administrative operations.

#![forbid(unsafe_code)]

pub mod approval_history;
pub mod complexity;
pub mod conversation_history;
pub mod conversation_state;
pub mod conversational;
pub mod decision;
pub mod dispatch;
pub mod execution_plan;
pub mod model_selection;
pub mod paused_execution;
pub mod plan_revision;
pub mod reasoning;
pub mod request_lifecycle;
pub mod review_card;
mod routes;

pub use approval_history::{
    ApprovalDecision, ApprovalExecutionResult, ApprovalHistoryAccess, ApprovalHistoryEntry,
    ApprovalHistoryQuery, ApprovalHistoryStore, ApprovalHistoryView,
};
pub use complexity::{
    assess_conversational_complexity, assess_text, ComplexityAssessment, ConversationalComplexity,
};
pub use conversation_history::prepare_reasoning_history;
pub use conversation_state::{ConversationState, ConversationTransitionError};
pub use conversational::{
    ConversationalAssemble, ConversationalTerminal, conversation_state_for_lifecycle,
    conversational_terminal_from_event, conversational_terminal_from_response,
    lifecycle_from_reasoning_response, planner_response_from_terminal,
};
pub use model_selection::{
    prepare_reasoning_model, ModelSelection, ModelSelectionKind,
};
pub use dispatch::{
    DispatchSupport, ExecutionMeta, IntentToolHandler, PreparedToolCall, ToolRoute, ToolRouteTable,
};
pub use execution_plan::{
    EstimatedReversibility, EstimatedRisk, ExecutionPlan, ExecutionPlanId, ExecutionPlanParams,
    ExecutionStatus, ExecutionStep, ExecutionSummary, PlanLineage, PlanPermissionRequirement,
    PlanTransitionError, ReviewRequirement,
};
pub use paused_execution::{
    PauseError, PausedExecution, PausedPlanSnapshot, PausedPlanStore, DEFAULT_PAUSE_TTL,
};
pub use plan_revision::{ModificationScope, PlanHistoryEntry, PlanRevisionDraft};
pub use review_card::{EstimatedDuration, ReviewCardModel, ReviewCardState, ReviewIntent};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use decision::{DecisionEngine, Intent};
use dispatch::{missing_capability_error, missing_route_error, unknown_tool_error};
use jaymi_capabilities::{
    compose_capabilities, is_multi_capability, workspace_expansion_for, Capability,
    CapabilityComposition, CapabilityDescriptor, CapabilityDiscoveryReport, CapabilityEngineApi,
    CapabilityInspectorReport, CapabilityInventory, CapabilityPlan, DiscoveredProvider,
    DiscoveredTool, WorkspaceExpansion,
};
use jaymi_context::{AssembleHints, ContextBundle, ContextEngine, LlmContext};
use jaymi_core::{
    ActionPreview, Citation, DeletionMethod, Document, FileEntry, GitPathStatus, HealthReport,
    IntentId, JaymiError, JaymiResult, Lifecycle, LspCompletionItem, LspDiagnostic, LspHover,
    LspLocation, LspTextEdit, UserRequest,
};
use jaymi_memory_engine::{
    AssembledMemoryContext, MemoryEngineApi, PromotionAskDecision, PromotionSuggestion,
};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionCheckResult, PermissionDecision,
    PermissionEngine, PermissionRequest, PermissionScope,
};
use jaymi_policies::{ExecutionCandidate, PolicyDecision, PolicyEngine, PolicyEvaluation};
use jaymi_project_engine::{Project, ProjectContext, ProjectEngineApi, ProjectKnowledgeHit};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::{
    InternetRequirement, PrivacyMode, ToolInput, ToolOrchestrator, ToolRegistry, MANAGE_PATH_TOOL_ID,
    WRITE_FILE_TOOL_ID,
};
use reasoning::ReasoningEngine;
use jaymi_reasoning::{
    ConversationStream, ConversationStreamEvent, ReasoningRequest, StreamingLifecycle,
};
use plan_revision::apply_modification_note;

const NAME: &str = "planner";

/// Snapshot of the last conversational model resolution (Planner-owned).
#[derive(Debug, Clone, Default)]
struct LastModelSelection {
    configured: Option<jaymi_reasoning::ModelIdentifier>,
    provider: Option<jaymi_reasoning::ModelIdentifier>,
    fallback: bool,
}
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
    "context_engine",
    "capability_engine",
    "provider_registry",
    "tool_registry",
    "project_engine",
];

/// Final response produced after the request lifecycle completes.
///
/// **Canonical request context:** [`Self::context_bundle`] (via [`Self::context`]).
/// Behaviors and LLM providers must consume the bundle (or `LlmContext` derived
/// from it). Parallel `memory_context` / `project_context` / `search_context`
/// fields are removed — use accessors on the bundle instead.
#[derive(Debug, Default, Clone)]
pub struct PlannerResponse {
    /// Human-readable summary of the result.
    pub content: String,
    /// Capability selected for the request, when any.
    pub capability: Option<Capability>,
    /// Tool selected for execution, when any.
    pub tool_id: Option<String>,
    /// Provider that fulfilled the tool, when known.
    pub provider_id: Option<String>,
    /// Directory that was listed, when applicable.
    pub listed_path: Option<std::path::PathBuf>,
    /// Structured directory listing entries.
    pub entries: Vec<FileEntry>,
    /// Explainable citations for retrieved search / inventory hits.
    pub citations: Vec<Citation>,
    /// Unified document produced by the Read pipeline.
    pub document: Option<Document>,
    /// Policy evaluation for the selected tool, when evaluated.
    pub policy_evaluation: Option<PolicyEvaluation>,
    /// Permission check result for the selected tool, when evaluated.
    pub permission_result: Option<PermissionCheckResult>,
    /// True when policy or permission blocked tool execution.
    pub blocked: bool,
    /// True when an execution plan is waiting for review before tools may run.
    pub awaiting_review: bool,
    /// Project closed by a Close intent, when any (session action result — not request context).
    pub closed_project: Option<Project>,
    /// Capability composition plan (PlanWork / diagnostics; never executes tools).
    pub capability_plan: Option<CapabilityPlan>,
    /// Action execution plan for this request (Planner-owned; immutable content).
    pub execution_plan: Option<ExecutionPlan>,
    /// Summary produced after plan execution, cancellation, or review gate.
    pub execution_summary: Option<ExecutionSummary>,
    /// Workspace expansion requested by the selected capability (conversation stays).
    pub workspace: Option<WorkspaceExpansion>,
    /// Immutable Context Engine snapshot for this request.
    ///
    /// **Authoritative** request-context contract for Planner execution,
    /// Behaviors (Planned), and LLM providers (`LlmContext::from_bundle`).
    /// Always set by `Planner::handle` via `finalize`.
    pub context_bundle: Option<ContextBundle>,
    /// Project-scoped knowledge hits (files, memories, tasks, decisions, …).
    pub project_knowledge: Vec<ProjectKnowledgeHit>,
    /// Terminal session id when a terminal tool ran.
    pub terminal_session_id: Option<String>,
    /// Output produced by the latest terminal command.
    pub terminal_output: Option<String>,
    /// Full terminal scrollback for the session.
    pub terminal_scrollback: Option<String>,
    /// Terminal command history (oldest first).
    pub terminal_history: Vec<String>,
    /// Display title for the terminal session, when a terminal tool ran.
    pub terminal_title: Option<String>,
    /// Whether the terminal session is still alive after the operation.
    pub terminal_alive: Option<bool>,
    /// Current Git branch when a Git tool ran.
    pub git_branch: Option<String>,
    /// Short Git status summary.
    pub git_summary: Option<String>,
    /// Whether the probed path is inside a Git work tree.
    pub git_is_repository: Option<bool>,
    /// Unstaged modified files.
    pub git_modified: Vec<GitPathStatus>,
    /// Newly staged (added) files.
    pub git_added: Vec<GitPathStatus>,
    /// Deleted files (worktree and/or index).
    pub git_deleted: Vec<GitPathStatus>,
    /// Staged files.
    pub git_staged: Vec<GitPathStatus>,
    /// Untracked files.
    pub git_untracked: Vec<GitPathStatus>,
    /// Hover result from the language server.
    pub lsp_hover: Option<LspHover>,
    /// Completion candidates from the language server.
    pub lsp_completions: Vec<LspCompletionItem>,
    /// Diagnostics from the language server.
    pub lsp_diagnostics: Vec<LspDiagnostic>,
    /// Go-to-definition locations.
    pub lsp_definitions: Vec<LspLocation>,
    /// Find-references locations.
    pub lsp_references: Vec<LspLocation>,
    /// Rename / workspace text edits.
    pub lsp_edits: Vec<LspTextEdit>,
    /// True when this turn invoked the Reasoning Engine (conversational path).
    pub reasoning_used: bool,
    /// Reasoning provider id when reasoning ran successfully.
    pub reasoning_provider_id: Option<String>,
    /// Final streaming lifecycle for conversational turns.
    pub stream_lifecycle: Option<jaymi_reasoning::StreamingLifecycle>,
    /// Reasoning metrics (latency, tokens/sec, cancel reason, …) when reasoning ran.
    pub reasoning_metrics: Option<jaymi_reasoning::ReasoningMetrics>,
    /// Prompt diagnostics from the last conversational assemble (budget / sections).
    pub prompt_diagnostics: Option<jaymi_reasoning::PromptDiagnostics>,
    /// Registry / preferred model configured for this turn (B1.13.6).
    pub configured_model: Option<jaymi_reasoning::ModelIdentifier>,
    /// Model id attached onto `ReasoningRequest` for the provider (B1.13.6).
    pub provider_model: Option<jaymi_reasoning::ModelIdentifier>,
    /// True when the Planner fell back after a missing / unavailable model.
    pub model_fallback: bool,
    /// Conversation runtime state at response time (Planner-owned).
    pub conversation_state: ConversationState,
    /// Conversational pipeline stage timings (Developer Diagnostics only).
    pub pipeline_timing: Option<jaymi_reasoning::PipelineTiming>,
}

impl PlannerResponse {
    /// Authoritative request context for this turn (`None` only if `handle` did not run `finalize`).
    pub fn context(&self) -> Option<&ContextBundle> {
        self.context_bundle.as_ref()
    }

    /// Relevant memories from the ContextBundle (never a parallel dump).
    pub fn memory(&self) -> Option<&AssembledMemoryContext> {
        self.context_bundle.as_ref().map(ContextBundle::memory)
    }

    /// Open project workspace from the ContextBundle, when present.
    pub fn project(&self) -> Option<&ProjectContext> {
        self.context_bundle
            .as_ref()
            .and_then(ContextBundle::project)
    }

    /// Promotion suggestions from the ContextBundle (never auto-applied).
    pub fn promotion_suggestions(&self) -> &[PromotionSuggestion] {
        self.context_bundle
            .as_ref()
            .map(ContextBundle::promotion_suggestions)
            .unwrap_or(&[])
    }

    /// Promotion ask decision from the ContextBundle.
    pub fn promotion_ask(&self) -> PromotionAskDecision {
        self.context_bundle
            .as_ref()
            .map(ContextBundle::promotion_ask)
            .unwrap_or(PromotionAskDecision::Defer)
    }
}

/// Dependencies required to construct the Planner from registries.
#[derive(Clone)]
pub struct PlannerDeps {
    /// Capability Engine used for discovery, validation, and planning.
    pub capabilities: Arc<dyn CapabilityEngineApi>,
    /// Provider registry used for discovery.
    pub providers: Arc<ProviderRegistry>,
    /// Tool registry used for discovery.
    pub tools: Arc<ToolRegistry>,
    /// Orchestrator used to select and execute tools.
    pub orchestrator: ToolOrchestrator,
    /// Policy engine consulted before permissions.
    pub policies: Arc<PolicyEngine>,
    /// Permission engine consulted before tool execution.
    pub permissions: Arc<PermissionEngine>,
    /// Memory Engine — Planner never accesses memory storage directly.
    pub memory: Arc<dyn MemoryEngineApi>,
    /// Project Engine — Planner requests one assembled project context.
    pub projects: Arc<dyn ProjectEngineApi>,
    /// Context Engine — sole assembler of request context for `handle`.
    pub context: Arc<ContextEngine>,
    /// Intent → tool route table (compile-time registration; no reflection).
    pub routes: ToolRouteTable,
    /// Optional reasoning backend (`ReasoningProvider`).
    pub reasoning: Option<Arc<dyn jaymi_reasoning::ReasoningProvider>>,
    /// Optional Model Registry — populates `ReasoningRequest.model` (B1.13.6).
    pub model_registry: Option<Arc<jaymi_reasoning::ModelRegistry>>,
}

/// Planner kernel.
///
/// The Planner remains deterministic. Reasoning is delegated. Execution is
/// delegated. Every user-facing request enters [`Self::handle`].
///
/// When an [`ExecutionPlan`] requires review, the Planner **pauses** (stores
/// plan + tool input) and returns [`PlannerResponse::awaiting_review`]. The
/// conversation stays active. [`Self::resolve_review`] resumes, revises
/// (Modify → child plan), or cancels the paused plan without replanning on Approve.
pub struct Planner {
    initialized: bool,
    decision: DecisionEngine,
    reasoning: ReasoningEngine,
    capabilities: Arc<dyn CapabilityEngineApi>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    orchestrator: ToolOrchestrator,
    policies: Arc<PolicyEngine>,
    permissions: Arc<PermissionEngine>,
    memory: Arc<dyn MemoryEngineApi>,
    projects: Arc<dyn ProjectEngineApi>,
    context: Arc<ContextEngine>,
    /// Intent → tool handlers (registration-based execution routing).
    routes: ToolRouteTable,
    /// Model Registry for resolving `ReasoningRequest.model` (optional).
    model_registry: Option<Arc<jaymi_reasoning::ModelRegistry>>,
    /// Explicit preferred model override (host / UX selection).
    preferred_model: Mutex<Option<jaymi_reasoning::ModelIdentifier>>,
    /// Last model resolution for conversational diagnostics (B1.13.6).
    last_model_selection: Mutex<Option<LastModelSelection>>,
    /// How many times [`Self::handle`] has been entered (integrity tests).
    handle_count: AtomicU64,
    /// Plans waiting on conversational review (resume without replan).
    paused: Mutex<PausedPlanStore>,
    /// Lineage of proposed / revised / cancelled plans for this Planner.
    plan_history: Mutex<Vec<PlanHistoryEntry>>,
    /// Review Card decisions for transparency, reasoning, and diagnostics.
    approval_history: Mutex<ApprovalHistoryStore>,
    /// User-visible conversation runtime phase (Planner-owned transitions).
    conversation_state: Mutex<ConversationState>,
}

impl Planner {
    /// Construct a Planner that discovers capabilities through registries.
    pub fn new(deps: PlannerDeps) -> Self {
        let reasoning = match deps.reasoning {
            Some(provider) => ReasoningEngine::with_provider(provider),
            None => ReasoningEngine::new(),
        };
        Self {
            initialized: false,
            decision: DecisionEngine,
            reasoning,
            capabilities: deps.capabilities,
            providers: deps.providers,
            tools: deps.tools,
            orchestrator: deps.orchestrator,
            policies: deps.policies,
            permissions: deps.permissions,
            memory: deps.memory,
            projects: deps.projects,
            context: deps.context,
            routes: deps.routes,
            model_registry: deps.model_registry,
            preferred_model: Mutex::new(None),
            last_model_selection: Mutex::new(None),
            handle_count: AtomicU64::new(0),
            paused: Mutex::new(PausedPlanStore::default()),
            plan_history: Mutex::new(Vec::new()),
            approval_history: Mutex::new(ApprovalHistoryStore::new()),
            conversation_state: Mutex::new(ConversationState::Idle),
        }
    }

    /// Set or clear an explicit preferred reasoning model (overrides registry default).
    pub fn set_preferred_model(
        &self,
        model: Option<jaymi_reasoning::ModelIdentifier>,
    ) -> JaymiResult<()> {
        let mut guard = self
            .preferred_model
            .lock()
            .map_err(|_| JaymiError::new("preferred model lock poisoned"))?;
        *guard = model;
        Ok(())
    }

    /// Current explicit preferred model, when set.
    pub fn preferred_model(&self) -> Option<jaymi_reasoning::ModelIdentifier> {
        self.preferred_model
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Model Registry wired into this Planner, when any.
    pub fn model_registry(&self) -> Option<Arc<jaymi_reasoning::ModelRegistry>> {
        self.model_registry.clone()
    }

    /// Build a conversational [`ReasoningRequest`] with history + registry model.
    fn build_reasoning_request(
        &self,
        goal: &str,
        llm: LlmContext,
        history: Vec<jaymi_reasoning::ConversationTurn>,
    ) -> JaymiResult<ReasoningRequest> {
        let history = prepare_reasoning_history(history, goal);
        let mut request = ReasoningRequest::new(goal, llm).with_history(history);
        let configured = self.preferred_model().or_else(|| {
            self.model_registry
                .as_ref()
                .and_then(|registry| registry.default_model())
        });
        let mut provider = None;
        let mut fallback = false;
        if let Some(registry) = self.model_registry.as_ref() {
            let preferred = self.preferred_model();
            match prepare_reasoning_model(registry, preferred.as_ref()) {
                Ok(selection) => {
                    fallback = selection.used_fallback();
                    provider = Some(selection.id().clone());
                    request = request.with_model(selection.id().clone());
                    if fallback {
                        jaymi_logging::info(
                            "planner",
                            format!(
                                "model fallback → {} ({})",
                                selection.id().display(),
                                match &selection.kind {
                                    ModelSelectionKind::Fallback { reason, .. } => reason.as_str(),
                                    _ => "fallback",
                                }
                            ),
                        );
                    }
                }
                Err(err) => {
                    jaymi_logging::warn(
                        "planner",
                        format!("model registry resolution failed: {}", err.message()),
                    );
                }
            }
        }
        if let Ok(mut guard) = self.last_model_selection.lock() {
            *guard = Some(LastModelSelection {
                configured,
                provider: provider.clone(),
                fallback,
            });
        }
        Ok(request)
    }

    fn take_model_selection_fields(
        &self,
    ) -> (
        Option<jaymi_reasoning::ModelIdentifier>,
        Option<jaymi_reasoning::ModelIdentifier>,
        bool,
    ) {
        self.last_model_selection
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .map(|selection| {
                (
                    selection.configured,
                    selection.provider,
                    selection.fallback,
                )
            })
            .unwrap_or((None, None, false))
    }

    /// Current conversation runtime state (Planner-owned).
    pub fn conversation_state(&self) -> ConversationState {
        self.conversation_state
            .lock()
            .map(|guard| *guard)
            .unwrap_or(ConversationState::Idle)
    }

    /// Transition the conversation state machine. Illegal transitions are logged and ignored
    /// in production paths that must still return a response; use [`Self::try_transition_conversation`]
    /// when a hard failure is required.
    pub fn transition_conversation(&self, next: ConversationState) {
        if let Err(error) = self.try_transition_conversation(next) {
            jaymi_logging::warn("planner", error.to_string());
        }
    }

    /// Fallible conversation state transition.
    pub fn try_transition_conversation(
        &self,
        next: ConversationState,
    ) -> Result<(), ConversationTransitionError> {
        let mut guard = self
            .conversation_state
            .lock()
            .map_err(|_| ConversationTransitionError {
                from: ConversationState::Idle,
                to: next,
            })?;
        let from = *guard;
        if !ConversationState::can_transition(from, next) {
            return Err(ConversationTransitionError { from, to: next });
        }
        if from != next {
            jaymi_logging::info(
                "planner",
                format!(
                    "conversation state {} → {}",
                    from.as_str(),
                    next.as_str()
                ),
            );
            *guard = next;
        }
        Ok(())
    }

    /// Attach the current conversation state onto a response.
    fn with_conversation_state(&self, mut response: PlannerResponse) -> PlannerResponse {
        response.conversation_state = self.conversation_state();
        response
    }

    /// Finalize a response with ContextBundle + current conversation state.
    fn finalize_response(
        &self,
        response: PlannerResponse,
        bundle: ContextBundle,
    ) -> PlannerResponse {
        self.with_conversation_state(finalize(response, bundle))
    }

    /// Registered tool routes (Intent → tool) for diagnostics and tests.
    pub fn tool_routes(&self) -> &ToolRouteTable {
        &self.routes
    }

    /// Number of times [`Self::handle`] has been entered.
    pub fn handle_count(&self) -> u64 {
        self.handle_count.load(Ordering::Relaxed)
    }

    /// Snapshot of Execution Plan lineage history (oldest first).
    pub fn plan_history(&self) -> JaymiResult<Vec<PlanHistoryEntry>> {
        Ok(self
            .plan_history
            .lock()
            .map_err(|_| JaymiError::new("plan history lock poisoned"))?
            .clone())
    }

    fn record_plan_history(&self, plan: &ExecutionPlan) -> JaymiResult<()> {
        let mut history = self
            .plan_history
            .lock()
            .map_err(|_| JaymiError::new("plan history lock poisoned"))?;
        history.push(PlanHistoryEntry {
            plan_id: plan.id().clone(),
            parent_plan_id: plan.parent_plan_id().cloned(),
            revision: plan.revision(),
            status: plan.status().as_str().to_string(),
            originating_request: plan.originating_request().to_string(),
            changes: plan.revision_changes().to_vec(),
            modification_note: plan.modification_note().map(str::to_string),
        });
        Ok(())
    }

    fn update_plan_history_status(
        &self,
        plan_id: &ExecutionPlanId,
        status: ExecutionStatus,
    ) -> JaymiResult<()> {
        let mut history = self
            .plan_history
            .lock()
            .map_err(|_| JaymiError::new("plan history lock poisoned"))?;
        if let Some(entry) = history
            .iter_mut()
            .rev()
            .find(|entry| &entry.plan_id == plan_id)
        {
            entry.status = status.as_str().to_string();
        }
        Ok(())
    }

    /// Snapshot of Approval History (oldest first).
    pub fn approval_history(&self) -> JaymiResult<Vec<ApprovalHistoryEntry>> {
        Ok(self
            .approval_history
            .lock()
            .map_err(|_| JaymiError::new("approval history lock poisoned"))?
            .entries()
            .to_vec())
    }

    /// Search in-session Approval History (newest first).
    pub fn search_approval_history(
        &self,
        query: &ApprovalHistoryQuery,
    ) -> JaymiResult<Vec<ApprovalHistoryEntry>> {
        Ok(self
            .approval_history
            .lock()
            .map_err(|_| JaymiError::new("approval history lock poisoned"))?
            .search(query))
    }

    /// Search Approval History as permission-aware views.
    pub fn search_approval_history_views(
        &self,
        query: &ApprovalHistoryQuery,
        access: ApprovalHistoryAccess,
    ) -> JaymiResult<Vec<ApprovalHistoryView>> {
        Ok(self
            .approval_history
            .lock()
            .map_err(|_| JaymiError::new("approval history lock poisoned"))?
            .search_views(query, access))
    }

    fn record_approval_history(&self, entry: ApprovalHistoryEntry) -> JaymiResult<()> {
        self.approval_history
            .lock()
            .map_err(|_| JaymiError::new("approval history lock poisoned"))?
            .record(entry);
        Ok(())
    }

    /// Record a Review Card decision against the resulting Planner response.
    fn record_approval_from_resolve(
        &self,
        intent: &ReviewIntent,
        response: &PlannerResponse,
    ) -> JaymiResult<()> {
        let reviewed_plan_id = intent.plan_id().clone();
        let response_plan = response.execution_plan.as_ref();
        let (modified_plan_id, parent_plan_id, affected, goal) = match intent {
            ReviewIntent::Modify { .. } => {
                let child = response_plan;
                (
                    child.map(|plan| plan.id().clone()),
                    Some(reviewed_plan_id.clone()),
                    child
                        .map(|plan| plan.affected_resources().to_vec())
                        .unwrap_or_default(),
                    child.map(|plan| plan.originating_request().to_string()),
                )
            }
            ReviewIntent::Approve { .. } | ReviewIntent::Cancel { .. } => (
                None,
                response_plan.and_then(|plan| plan.parent_plan_id().cloned()),
                response_plan
                    .map(|plan| plan.affected_resources().to_vec())
                    .unwrap_or_default(),
                response_plan.map(|plan| plan.originating_request().to_string()),
            ),
        };

        let entry = ApprovalHistoryEntry::from_intent_and_response(
            intent,
            &reviewed_plan_id,
            modified_plan_id,
            parent_plan_id,
            affected,
            goal,
            response.execution_summary.as_ref(),
            None,
            None,
        );
        self.record_approval_history(entry)
    }

    /// Discover registered capabilities through the Capability Engine.
    pub fn discover_capabilities(&self) -> Vec<Capability> {
        self.capabilities.list()
    }

    /// Describe a capability's catalog metadata (registration optional).
    pub fn describe_capability(&self, capability: Capability) -> CapabilityDescriptor {
        self.capabilities.describe(capability)
    }

    /// Resolve a registered capability by stable id.
    pub fn resolve_capability(&self, id: &str) -> JaymiResult<Option<CapabilityDescriptor>> {
        self.capabilities.resolve(id)
    }

    /// Discover what Jaymi can currently do given live tools and providers.
    pub fn discover_capability_status(&self) -> JaymiResult<CapabilityDiscoveryReport> {
        let inventory = self.capability_inventory()?;
        self.capabilities.discover(&inventory)
    }

    /// Inspect the capability system for developers (registered, active, requirements).
    pub fn inspect_capabilities(&self) -> JaymiResult<CapabilityInspectorReport> {
        let inventory = self.capability_inventory()?;
        self.capabilities.inspect(&inventory)
    }

    /// Build a capability execution plan from declared requirements.
    ///
    /// Uses the live tool/provider inventory so availability reflects what is
    /// currently executable. Nothing is executed.
    pub fn build_capability_plan(&self, capabilities: &[Capability]) -> JaymiResult<CapabilityPlan> {
        let inventory = self.capability_inventory()?;
        self.capabilities.plan(capabilities, &inventory, None)
    }

    /// Plan work for one capability and optional goal.
    ///
    /// Resolves required tools, providers, and permissions against the live
    /// inventory. Tools are never executed by planning.
    pub fn plan_capability(
        &self,
        capability: Capability,
        goal: Option<&str>,
    ) -> JaymiResult<CapabilityPlan> {
        self.plan_capabilities(&[capability], goal)
    }

    /// Compose independent capabilities into one execution plan.
    ///
    /// Capabilities remain separate plan steps — they are never merged.
    /// Tools are never executed by planning.
    pub fn plan_capabilities(
        &self,
        capabilities: &[Capability],
        goal: Option<&str>,
    ) -> JaymiResult<CapabilityPlan> {
        let inventory = self.capability_inventory()?;
        self.capabilities.compose(capabilities, &inventory, goal)
    }

    /// Compose from a [`CapabilityComposition`] value.
    pub fn compose_capability_plan(
        &self,
        composition: &CapabilityComposition,
    ) -> JaymiResult<CapabilityPlan> {
        let inventory = self.capability_inventory()?;
        self.capabilities.compose_plan(composition, &inventory)
    }

    /// Number of registered providers visible to the Planner.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Number of registered tools visible to the Planner.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Returns true when the Planner completed initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Honest reasoning-backend status for diagnostics.
    pub fn reasoning_status(&self) -> &'static str {
        self.reasoning.status_label()
    }

    /// Whether a reasoning backend is wired and currently usable.
    pub fn reasoning_implemented(&self) -> bool {
        self.reasoning.is_implemented()
    }

    /// Access the Reasoning Engine (prompt build / provider calls).
    pub fn reasoning(&self) -> &ReasoningEngine {
        &self.reasoning
    }

    fn ensure_ready(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("planner is not initialized"))
        }
    }

    /// Snapshot live tools and providers for capability discovery and planning.
    ///
    /// Sorted by id so plans and discovery reports stay deterministic.
    fn capability_inventory(&self) -> JaymiResult<CapabilityInventory> {
        let mut tools: Vec<DiscoveredTool> = self
            .tools
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|metadata| DiscoveredTool {
                id: metadata.id,
                capabilities: metadata.capabilities,
            })
            .collect();
        tools.sort_by(|left, right| left.id.cmp(&right.id));

        let mut providers: Vec<DiscoveredProvider> = self
            .providers
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|identity| DiscoveredProvider {
                id: identity.id,
                capabilities: identity.capabilities,
            })
            .collect();
        providers.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(CapabilityInventory { tools, providers })
    }

    /// Sync Memory's session hint to the Project Engine open project.
    ///
    /// Memory does not own session open state — it only mirrors the id for
    /// context assembly. Called only from [`Self::open_project`] /
    /// [`Self::close_project`].
    fn bind_memory_project(&self, project_id: Option<&str>) -> JaymiResult<()> {
        self.memory.set_active_project(project_id)?;
        Ok(())
    }

    /// Resume the project's latest conversation when none is active.
    fn resume_project_conversation(&self, project_id: &str) -> JaymiResult<()> {
        if self.memory.active_conversation_id().is_some() {
            return Ok(());
        }
        let conversations = self.memory.list_conversations_for_project(project_id)?;
        let Some(latest) = conversations.first() else {
            return Ok(());
        };
        self.memory
            .set_active_conversation(Some(latest.id.as_str()))?;
        jaymi_logging::info(
            "planner",
            format!(
                "resumed conversation id={} for project={}",
                latest.id.as_str(),
                project_id
            ),
        );
        Ok(())
    }

    /// Sole project session open lifecycle.
    ///
    /// Continue / Open-by-id intents and Application open helpers all enter
    /// [`Self::handle`], which calls this once. Order:
    /// 1. Project Engine owns open state (`open`)
    /// 2. Memory mirrors the id for context assembly
    /// 3. Resume the project's latest conversation when none is active
    ///
    /// Do not call `projects.open` or `memory.set_active_project` for session
    /// activation anywhere else.
    fn open_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.ensure_ready()?;
        let context = self.projects.open(project_id)?;
        self.bind_memory_project(Some(project_id))?;
        self.resume_project_conversation(project_id)?;
        self.context.request_fresh_context("project_changed");
        jaymi_logging::info(
            "planner",
            format!(
                "opened project id={} name={} entries={}",
                context.project.id.as_str(),
                context.project.name,
                context.entry_count()
            ),
        );
        Ok(context)
    }

    /// Sole project session close lifecycle.
    ///
    /// Project Engine clears open state first; Memory's session hint is cleared
    /// to match. The active conversation is untouched.
    fn close_project(&self) -> JaymiResult<Option<Project>> {
        self.ensure_ready()?;
        let closed = self.projects.close()?;
        let _ = self.bind_memory_project(None);
        self.context.request_fresh_context("project_changed");
        Ok(closed)
    }

    /// Root directory of the active workspace project, when any.
    fn active_project_root(&self) -> Option<PathBuf> {
        let project_id = self.projects.active_project_id()?;
        self.projects
            .get(&project_id)
            .ok()
            .flatten()
            .and_then(|project| project.root_directory)
    }

    /// Resolve relative request paths against the active project root.
    fn resolve_workspace_path(&self, path: PathBuf) -> PathBuf {
        if path.is_absolute() {
            return path;
        }
        let Some(root) = self.active_project_root() else {
            return path;
        };
        let raw = path.to_string_lossy();
        if raw.is_empty() || raw == "." || raw == "./" {
            root
        } else {
            root.join(path)
        }
    }

    fn handle_continue_project(&self, name: &str) -> JaymiResult<PlannerResponse> {
        let Some(project) = self.projects.find_by_name(name)? else {
            return Ok(PlannerResponse {
                content: format!(
                    "No project named \"{name}\" is registered. Create the project before continuing."
                ),
                ..PlannerResponse::default()
            });
        };
        self.handle_open_project_id(project.id.as_str())
    }

    fn handle_open_project_id(&self, project_id: &str) -> JaymiResult<PlannerResponse> {
        let context = self.open_project(project_id)?;
        let content = format_project_context_summary(&context);
        // Project detail lives on ContextBundle after re-assemble in `handle`.
        let _ = context;
        Ok(PlannerResponse {
            content,
            ..PlannerResponse::default()
        })
    }

    fn handle_close_project(&self) -> JaymiResult<PlannerResponse> {
        let closed = self.close_project()?;
        let content = match &closed {
            Some(project) => format!(
                "Closed project \"{}\". The active conversation stays open.",
                project.name
            ),
            None => "No project is currently open.".to_string(),
        };
        jaymi_logging::info("planner", &content);
        Ok(PlannerResponse {
            content,
            closed_project: closed,
            ..PlannerResponse::default()
        })
    }

    /// Produce an execution plan for a goal without executing any tool.
    ///
    /// One or more independent capabilities may be composed into a single
    /// plan. Planning never requires every capability to be currently
    /// fulfillable — an incomplete plan honestly reports what is still missing.
    fn handle_plan_work(
        &self,
        capabilities: &[Capability],
        goal: &str,
    ) -> JaymiResult<PlannerResponse> {
        let ordered = compose_capabilities(capabilities)?;
        let plan = self.plan_capabilities(&ordered, Some(goal))?;
        let primary = ordered[0];
        let composition_note = if is_multi_capability(&ordered) {
            format!(
                "Composed {} independent capabilities ({}). ",
                ordered.len(),
                ordered
                    .iter()
                    .map(Capability::id)
                    .collect::<Vec<_>>()
                    .join(" → ")
            )
        } else {
            String::new()
        };
        jaymi_logging::info(
            "planner",
            format!(
                "planned work capabilities=[{}] {}",
                ordered
                    .iter()
                    .map(Capability::id)
                    .collect::<Vec<_>>()
                    .join(","),
                plan.summary()
            ),
        );
        let content = format!(
            "Execution plan (planning only — no tools were executed):\n{composition_note}{}",
            plan.render()
        );
        Ok(PlannerResponse {
            content,
            capability: Some(primary),
            capability_plan: Some(plan),
            workspace: workspace_expansion_for(
                primary,
                format!("capability {} requested workspace expansion", primary.id()),
            ),
            ..PlannerResponse::default()
        })
    }

    /// Process a user request through the canonical architectural pipeline.
    ///
    /// **Current:**
    /// User Request → Planner → Intent → Capability → Context Policy →
    /// Context Providers → Context Engine → ContextBundle →
    /// Action Policies → Permissions → Tool Orchestrator → Providers →
    /// Planner Response
    ///
    /// **Planned:** Behavior stage after ContextBundle (not implemented).
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            jaymi_logging::error("planner", "request rejected: planner is not initialized");
            return Err(JaymiError::new("planner is not initialized"));
        }

        self.handle_count.fetch_add(1, Ordering::Relaxed);

        // A new user request (not resolve_review) invalidates any paused plan —
        // editing the request must never resume a stale plan.
        self.invalidate_paused("new user request")?;
        self.transition_conversation(ConversationState::PreparingContext);

        jaymi_logging::info(
            "planner",
            format!(
                "request received content={:?} directory={:?} file={:?}",
                truncate_for_log(&request.content),
                request
                    .directory
                    .as_ref()
                    .map(|path| path.display().to_string()),
                request.file.as_ref().map(|path| path.display().to_string())
            ),
        );

        // 1. Intent Resolution
        let intent = self.decision.determine_intent(&request);

        // 2. Capability Resolution (may be empty for session intents / unknown)
        let capabilities = self.decision.required_capabilities(&intent);
        // Conversational complexity annotates AssembleHints only — never routing.
        let complexity = assess_conversational_complexity(
            &request,
            self.context.session_inputs().workspace_kind.as_deref(),
        );
        let hints = AssembleHints {
            intent: intent.id(),
            capability_ids: capabilities
                .iter()
                .map(|capability| capability.id().to_string())
                .collect(),
            complexity: Some(complexity.class_id().to_string()),
        };

        jaymi_logging::info(
            "planner",
            format!(
                "intent resolved label={} capabilities=[{}] complexity={}",
                hints.intent.as_str(),
                hints.capability_ids.join(","),
                complexity.class_id(),
            ),
        );

        // Workspace session intents mutate project state first, then assemble
        // so ContextBundle is the sole post-session request-context snapshot.
        match &intent {
            Intent::ContinueProject { name } => {
                let response = self.handle_continue_project(name)?;
                self.context.request_fresh_context("project_changed");
                let bundle = self.context.assemble_with(&request, Some(&hints))?;
                log_promotions(&bundle);
                self.transition_conversation(ConversationState::Completed);
                return Ok(self.finalize_response(response, bundle));
            }
            Intent::OpenProject { project_id } => {
                let response = self.handle_open_project_id(project_id)?;
                self.context.request_fresh_context("project_changed");
                let bundle = self.context.assemble_with(&request, Some(&hints))?;
                log_promotions(&bundle);
                self.transition_conversation(ConversationState::Completed);
                return Ok(self.finalize_response(response, bundle));
            }
            Intent::CloseProject => {
                let response = self.handle_close_project()?;
                self.context.request_fresh_context("project_changed");
                let bundle = self.context.assemble_with(&request, Some(&hints))?;
                log_promotions(&bundle);
                self.transition_conversation(ConversationState::Completed);
                return Ok(self.finalize_response(response, bundle));
            }
            _ => {}
        }

        // 3–6. Context Policy → Providers → Context Engine → ContextBundle
        let context = self.context.assemble_with(&request, Some(&hints))?;
        log_promotions(&context);

        let Some(capability) = capabilities.first().copied() else {
            // No capability ⇒ conversational / unknown. Never tool-dispatch.
            // ContextBundle is already assembled — reason through the engine.
            // History is empty on the generic handle path; Application
            // conversational UX passes prior turns via
            // handle_conversational_with_observer / start_conversation_stream.
            let mut early_pipeline = jaymi_reasoning::PipelineTiming::new();
            if let Some(inspection) = self.context.last_inspection() {
                early_pipeline.merge(conversational::pipeline_from_context_inspection(
                    &inspection,
                ));
            }
            return self.handle_conversational_request_with_observer(
                &request,
                context,
                &intent,
                Vec::new(),
                early_pipeline,
                |_| {},
            );
        };

        // Planning answers "what would this take" without needing the
        // capability to be fulfillable today.
        if let Intent::PlanWork { capabilities, goal } = &intent {
            let response = self.handle_plan_work(capabilities, goal)?;
            self.transition_conversation(ConversationState::Completed);
            return Ok(self.finalize_response(response, context));
        }

        let availability = self.capabilities.validate(capability);
        if !availability.is_executable_tier() {
            let message = format!(
                "capability {} is not executable (availability={})",
                capability.id(),
                availability.as_str()
            );
            jaymi_logging::error("planner", &message);
            self.transition_conversation(ConversationState::Failed);
            return Err(JaymiError::new(message));
        }

        // Tool-backed intents resolve through the registered route table.
        // Session / PlanWork / conversational stay special-cased above.
        let result = self.dispatch_tool_backed(intent, capability, &request.content);

        let result = match result {
            Ok(mut response) => {
                if response.capability_plan.is_none() {
                    response.capability_plan = self.plan_capability(capability, None).ok();
                }
                if response.workspace.is_none() {
                    response.workspace = workspace_expansion_for(
                        capability,
                        format!("capability {} selected for request", capability.id()),
                    );
                }
                self.transition_conversation(conversation_state_for_tool_response(&response));
                Ok(self.finalize_response(response, context.clone()))
            }
            Err(error) => {
                self.transition_conversation(ConversationState::Failed);
                Err(error)
            }
        };

        match &result {
            Ok(response) => jaymi_logging::info(
                "planner",
                format!(
                    "request completed tool={:?} provider={:?} blocked={} permission={:?}",
                    response.tool_id,
                    response.provider_id,
                    response.blocked,
                    response
                        .permission_result
                        .as_ref()
                        .map(|result| result.decision.as_str())
                ),
            ),
            Err(error) => {
                jaymi_logging::error("planner", format!("request failed: {}", error.message()))
            }
        }

        result
    }

    /// Conversational path with incremental stream observer (tokens / lifecycle).
    ///
    /// Blocking delivery mode: collects the shared [`ConversationStream`] to a
    /// terminal [`PlannerResponse`] in one call. See `conversational` module and
    /// docs/reasoning.md (dual delivery) for why this stays separate from the
    /// pumpable UI path.
    ///
    /// `history` is prior Experience turns (request-scoped). The Planner does
    /// not retain a parallel transcript — PromptBuilder formats these turns.
    pub fn handle_conversational_with_observer<F>(
        &self,
        request: UserRequest,
        history: Vec<jaymi_reasoning::ConversationTurn>,
        on_event: F,
    ) -> JaymiResult<PlannerResponse>
    where
        F: FnMut(ConversationStreamEvent),
    {
        if !self.initialized {
            return Err(JaymiError::new("planner is not initialized"));
        }
        let assembled = self.begin_conversational_assemble(&request)?;
        if conversational::is_tool_backed(&assembled.capability_ids) {
            // Not a conversational turn — fall back to full handle.
            return self.handle(request);
        }
        self.handle_conversational_request_with_observer(
            &request,
            assembled.context,
            &assembled.intent,
            history,
            assembled.pipeline,
            on_event,
        )
    }

    /// Begin a pumpable conversational stream after Context assemble.
    ///
    /// Pumpable delivery mode: callers drive [`ConversationStream::pump`] for
    /// incremental UI updates, then [`Self::complete_conversation_stream`].
    /// Shares assemble + stream start + terminal mapping with the blocking path;
    /// does not soft-fail on missing backends (host bridges to observer).
    ///
    /// `history` is prior conversation turns for PromptBuilder (not including
    /// the current `request` goal).
    pub fn start_conversation_stream(
        &self,
        request: &UserRequest,
        history: Vec<jaymi_reasoning::ConversationTurn>,
    ) -> JaymiResult<(
        ContextBundle,
        ConversationStream,
        jaymi_reasoning::PromptDiagnostics,
        jaymi_reasoning::PipelineTiming,
    )> {
        if !self.initialized {
            return Err(JaymiError::new("planner is not initialized"));
        }
        let assembled = self.begin_conversational_assemble(request)?;
        if conversational::is_tool_backed(&assembled.capability_ids) {
            return Err(JaymiError::new(
                "start_conversation_stream requires a conversational / unknown request",
            ));
        }
        let mut pipeline = assembled.pipeline;
        let context = assembled.context;
        if !self.reasoning.is_implemented() {
            return Err(JaymiError::new("no reasoning backend is available"));
        }
        let llm = LlmContext::from_bundle(&context);
        let reasoning_request =
            self.build_reasoning_request(request.content.trim(), llm, history)?;
        self.transition_conversation(ConversationState::Reasoning);
        let stream = ConversationStream::start(self.reasoning.clone(), reasoning_request)
            .map_err(|err| JaymiError::new(err.message()))?;
        // Diagnostics from the Prompt attached for delivery — not a discarded pre-build.
        let prompt_diagnostics = stream
            .prompt_diagnostics()
            .cloned()
            .ok_or_else(|| JaymiError::new("conversation stream missing prompt diagnostics"))?;
        if let Some(ms) = prompt_diagnostics.build_duration_ms {
            pipeline.set_stage("prompt_builder", ms);
        }
        Ok((context, stream, prompt_diagnostics, pipeline))
    }

    /// Finalize a pump-driven conversational stream into a [`PlannerResponse`].
    ///
    /// Uses the same terminal → response mapping as the blocking observer path.
    /// `early_pipeline` carries request/planner/context timings from stream start.
    pub fn complete_conversation_stream(
        &self,
        context: ContextBundle,
        event: ConversationStreamEvent,
        prompt_diagnostics: Option<jaymi_reasoning::PromptDiagnostics>,
        early_pipeline: Option<jaymi_reasoning::PipelineTiming>,
    ) -> JaymiResult<PlannerResponse> {
        let terminal = conversational::conversational_terminal_from_event(event)?;
        self.transition_conversation(terminal.conversation_state());
        let (configured_model, provider_model, model_fallback) =
            self.take_model_selection_fields();
        let mut response = conversational::planner_response_from_terminal(
            terminal,
            prompt_diagnostics,
            configured_model,
            provider_model,
            model_fallback,
        );
        if let Some(early) = early_pipeline {
            let mut merged = early;
            if let Some(later) = response.pipeline_timing.take() {
                merged.merge(later);
            }
            response.pipeline_timing = if merged.is_empty() {
                None
            } else {
                Some(merged)
            };
        }
        Ok(self.finalize_response(response, context))
    }

    /// Shared Intent → Capability → Complexity → AssembleHints → ContextBundle prelude.
    ///
    /// Complexity never alters Intent or Capability selection — it only annotates
    /// [`AssembleHints`] for Context relevance bias.
    fn begin_conversational_assemble(
        &self,
        request: &UserRequest,
    ) -> JaymiResult<conversational::ConversationalAssemble> {
        self.invalidate_paused("new user request")?;
        self.transition_conversation(ConversationState::PreparingContext);
        let planner_started = std::time::Instant::now();
        let intent = self.decision.determine_intent(request);
        let capabilities = self.decision.required_capabilities(&intent);
        let capability_ids: Vec<String> = capabilities
            .iter()
            .map(|capability| capability.id().to_string())
            .collect();
        let workspace_kind = self.context.session_inputs().workspace_kind;
        let complexity = assess_conversational_complexity(
            request,
            workspace_kind.as_deref(),
        );
        let hints = conversational::conversational_assemble_hints(
            &intent,
            capability_ids.clone(),
            Some(&complexity),
        );
        let planner_ms = planner_started.elapsed().as_millis() as u64;
        let context = self.context.assemble_with(request, Some(&hints))?;
        let mut pipeline = jaymi_reasoning::PipelineTiming::new();
        pipeline.set_stage("planner", planner_ms);
        if let Some(inspection) = self.context.last_inspection() {
            pipeline.merge(conversational::pipeline_from_context_inspection(&inspection));
        }
        Ok(conversational::ConversationalAssemble {
            intent,
            capability_ids,
            context,
            complexity,
            pipeline,
        })
    }

    /// Resume Reasoning after a stream retry / reconnect (Planner-owned).
    ///
    /// Legal from Streaming, Cancelled, or Failed. Experience/UI must only
    /// [`Self::conversation_state`] mirror afterward — never invent the phase.
    pub fn resume_reasoning_after_retry(&self) {
        self.transition_conversation(ConversationState::Reasoning);
    }

    /// Mirror Planner conversation state onto an active stream lifecycle event.
    pub fn mirror_stream_lifecycle(&self, lifecycle: StreamingLifecycle) {
        if let Some(state) = ConversationState::from_streaming_lifecycle(lifecycle) {
            if state.is_active() || state.is_terminal() {
                if !matches!(state, ConversationState::Idle) {
                    self.transition_conversation(state);
                }
            }
        }
    }

    fn handle_conversational_request_with_observer<F>(
        &self,
        request: &UserRequest,
        context: ContextBundle,
        intent: &Intent,
        history: Vec<jaymi_reasoning::ConversationTurn>,
        early_pipeline: jaymi_reasoning::PipelineTiming,
        mut on_event: F,
    ) -> JaymiResult<PlannerResponse>
    where
        F: FnMut(ConversationStreamEvent),
    {
        if !matches!(intent, Intent::Unknown) {
            jaymi_logging::warn(
                "planner",
                format!(
                    "empty capabilities for non-unknown intent={}; treating as conversational",
                    intent.id().as_str()
                ),
            );
        }

        jaymi_logging::info(
            "planner",
            "conversational request → ConversationStream (after ContextBundle)",
        );

        if !self.reasoning.is_implemented() {
            self.transition_conversation(ConversationState::Completed);
            return Ok(self.finalize_response(
                conversational::no_backend_soft_response(),
                context,
            ));
        }

        let llm = LlmContext::from_bundle(&context);
        let reasoning_request =
            self.build_reasoning_request(request.content.trim(), llm, history)?;
        let (configured_model, provider_model, model_fallback) =
            self.take_model_selection_fields();

        self.transition_conversation(ConversationState::Reasoning);
        let stream = match ConversationStream::start(self.reasoning.clone(), reasoning_request) {
            Ok(stream) => stream,
            Err(error) => {
                jaymi_logging::warn(
                    "planner",
                    format!("conversational stream start failed: {}", error.message()),
                );
                self.transition_conversation(ConversationState::Failed);
                return Ok(self.finalize_response(
                    conversational::stream_start_failed_response(
                        &error.message(),
                        configured_model,
                        provider_model,
                        model_fallback,
                    ),
                    context,
                ));
            }
        };
        let prompt_diagnostics = stream.prompt_diagnostics().cloned();

        match stream.run_with_observer(|event| {
            if let ConversationStreamEvent::Lifecycle(lifecycle) = &event {
                if let Some(state) = ConversationState::from_streaming_lifecycle(*lifecycle) {
                    if state.is_active() || state.is_terminal() {
                        // Skip Idle from stream; PreparingContext→Reasoning already set.
                        if !matches!(state, ConversationState::Idle) {
                            self.transition_conversation(state);
                        }
                    }
                }
            }
            on_event(event);
        }) {
            Ok(response) => {
                let terminal = conversational::conversational_terminal_from_response(response);
                let conversation_state = terminal.conversation_state();
                self.transition_conversation(conversation_state);
                jaymi_logging::info(
                    "planner",
                    format!(
                        "conversational stream lifecycle={} conversation={} provider={:?} attempts={} model={:?}",
                        terminal.lifecycle.as_str(),
                        conversation_state.as_str(),
                        terminal.provider_id,
                        terminal
                            .metrics
                            .as_ref()
                            .map(|metrics| metrics.attempts)
                            .unwrap_or(0),
                        provider_model.as_ref().map(|m| m.display())
                    ),
                );
                Ok(self.finalize_response(
                    {
                        let mut response = conversational::planner_response_from_terminal(
                            terminal,
                            prompt_diagnostics,
                            configured_model,
                            provider_model,
                            model_fallback,
                        );
                        let mut merged = early_pipeline.clone();
                        if let Some(later) = response.pipeline_timing.take() {
                            merged.merge(later);
                        }
                        response.pipeline_timing = if merged.is_empty() {
                            None
                        } else {
                            Some(merged)
                        };
                        response
                    },
                    context,
                ))
            }
            Err(error) => {
                jaymi_logging::warn(
                    "planner",
                    format!("conversational stream failed: {}", error.message()),
                );
                self.transition_conversation(ConversationState::Failed);
                Ok(self.finalize_response(
                    {
                        let mut response = conversational::stream_collect_failed_response(
                            &error.message(),
                            prompt_diagnostics,
                            configured_model,
                            provider_model,
                            model_fallback,
                        );
                        response.pipeline_timing = if early_pipeline.is_empty() {
                            None
                        } else {
                            Some(early_pipeline)
                        };
                        response
                    },
                    context,
                ))
            }
        }
    }

    /// Constrain search to the active workspace so projects stay isolated.
    fn scope_search_request(
        &self,
        mut request: jaymi_core::SearchRequest,
    ) -> jaymi_core::SearchRequest {
        if request.folder.is_none() {
            if let Some(root) = self.active_project_root() {
                request.folder = Some(root);
                request.folder_immediate = false;
            }
        }
        request
    }


    /// Execute a tool-backed intent through the registered route table.
    ///
    /// Planner still owns prepare → review → execute. Routes only build
    /// [`PreparedToolCall`] and map successful [`ToolOutput`] fields.
    fn dispatch_tool_backed(
        &self,
        intent: Intent,
        capability: Capability,
        request_text: &str,
    ) -> JaymiResult<PlannerResponse> {
        let intent_id = intent.id();
        if matches!(
            intent_id,
            IntentId::ContinueProject
                | IntentId::OpenProject
                | IntentId::CloseProject
                | IntentId::PlanWork
                | IntentId::Unknown
        ) {
            return Err(JaymiError::new(format!(
                "intent {} is not tool-routed",
                intent_id.as_str()
            )));
        }

        let handler = self
            .routes
            .get(intent_id)
            .ok_or_else(|| missing_route_error(intent_id))?;
        let route = handler.route();
        if route.capability != capability {
            return Err(JaymiError::new(format!(
                "route capability mismatch for {}: route={} decision={}",
                intent_id.as_str(),
                route.capability.id(),
                capability.id()
            )));
        }

        // Resolve the tool: prefer the registered route id; if that tool is not
        // installed, fall back to capability selection (alternate fulfillment).
        // A registered tool that does not advertise the capability is an error.
        let tool_id = match self.tools.get(route.tool_id) {
            Ok(tool) if tool.metadata().capabilities.contains(&capability) => {
                route.tool_id.to_string()
            }
            Ok(_) => return Err(missing_capability_error(route.tool_id, capability)),
            Err(_) => self.orchestrator.select(capability)?.ok_or_else(|| {
                unknown_tool_error(route.tool_id)
            })?,
        };
        // Confirm the selected tool fulfills the capability (covers fallback).
        let tool = self.tools.get(&tool_id)?;
        if !tool.metadata().capabilities.contains(&capability) {
            return Err(missing_capability_error(&tool_id, capability));
        }

        let call = handler.prepare(&intent, request_text, self)?;
        let prepared = self.prepare_execution(
            intent_id,
            &call.originating_request,
            capability,
            Some(&tool_id),
            &call.input,
            &call.resource_path,
            &call.action_label,
            call.permission_category,
            call.permission_action,
            call.expected_outputs.clone(),
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let mut plan = prepared.plan;
        let tool_id = prepared.tool_id.clone();
        let (output, execution_summary) =
            self.execute_approved_plan(&mut plan, call.input.clone())?;
        if !output.success {
            if call.soft_failure {
                return Ok(self.tool_failure_response(
                    plan,
                    capability,
                    tool_id,
                    prepared.provider_id,
                    prepared.policy_evaluation,
                    prepared.permission_result,
                    output,
                    execution_summary,
                ));
            }
            self.ensure_success(&output)?;
        }

        if let Some(reason) = call.fresh_context {
            self.context.request_fresh_context(reason);
        }

        let meta = ExecutionMeta {
            capability,
            tool_id,
            provider_id: prepared.provider_id,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            plan,
            execution_summary,
        };
        handler.respond(&call, output, meta)
    }

    /// Planner-owned deletion policy: prefer Trash; permanent only when
    /// explicitly requested or Trash is unavailable.
    fn resolve_deletion_method(
        &self,
        requested: Option<DeletionMethod>,
        request_text: &str,
    ) -> JaymiResult<DeletionMethod> {
        if matches!(requested, Some(DeletionMethod::Permanent))
            || requests_permanent_deletion(request_text)
        {
            return Ok(DeletionMethod::Permanent);
        }
        if matches!(requested, Some(DeletionMethod::Trash)) {
            if self.trash_supported_by_manage_path_tool() {
                return Ok(DeletionMethod::Trash);
            }
            return Ok(DeletionMethod::Permanent);
        }
        if self.trash_supported_by_manage_path_tool() {
            Ok(DeletionMethod::Trash)
        } else {
            Ok(DeletionMethod::Permanent)
        }
    }

    fn trash_supported_by_manage_path_tool(&self) -> bool {
        self.tools
            .get(MANAGE_PATH_TOOL_ID)
            .map(|tool| tool.supports_recoverable_delete())
            .unwrap_or(false)
    }

    /// Build an action [`ExecutionPlan`], then gate via Action Policy → Permission → Review.
    ///
    /// Tools are never executed here. Callers must run [`Self::execute_approved_plan`]
    /// only when `blocked_response` is `None` and the plan status is Approved.
    ///
    /// Decisions:
    /// - **Denied** (policy or permission) → explain why, cancel, do not execute
    /// - **RequiresApproval** (policy, permission, or ToolRisk) → Review Card,
    ///   pause until [`Self::resolve_review`]
    /// - **Allowed** → approve for execution
    ///
    /// Approval never bypasses the Planner. Tools never execute themselves.
    #[allow(clippy::too_many_arguments)]
    fn prepare_execution(
        &self,
        intent: IntentId,
        originating_request: &str,
        capability: Capability,
        preferred_tool_id: Option<&str>,
        tool_input: &ToolInput,
        path: &Path,
        action_label: &str,
        permission_category: PermissionCategory,
        permission_action: PermissionAction,
        expected_outputs: Vec<String>,
    ) -> JaymiResult<PreparedExecution> {
        let tool_id = if let Some(preferred) = preferred_tool_id {
            let tool = self
                .tools
                .get(preferred)
                .map_err(|_| unknown_tool_error(preferred))?;
            if !tool.metadata().capabilities.contains(&capability) {
                return Err(missing_capability_error(preferred, capability));
            }
            preferred.to_string()
        } else {
            self.orchestrator.select(capability)?.ok_or_else(|| {
                JaymiError::new(format!(
                    "no tool registered for capability {}",
                    capability.id()
                ))
            })?
        };

        let tool = self.tools.get(&tool_id)?;
        let metadata = tool.metadata().clone();
        let provider_id = metadata.provider.clone();
        let tool_risk = metadata
            .risk
            .effective_for(tool_input, metadata.internet);
        let resource = path.display().to_string();
        let candidate = ExecutionCandidate {
            tool_id: tool_id.clone(),
            provider_id: provider_id.clone(),
            requires_internet: matches!(metadata.internet, InternetRequirement::Required),
            local_only: matches!(metadata.privacy, PrivacyMode::LocalOnly),
            cloud_only: matches!(metadata.privacy, PrivacyMode::CloudOnly),
        };

        let policy_evaluation = self.policies.evaluate(&candidate)?;

        // Permission still runs when policy only requires approval, so the
        // Review Card and diagnostics can surface both explanations. Hard
        // policy deny skips permission (nothing to authorize).
        let permission_result = if matches!(policy_evaluation.decision, PolicyDecision::Denied) {
            None
        } else {
            let permission_request = PermissionRequest {
                category: permission_category,
                action: permission_action,
                scope: PermissionScope::Once,
                explanation: format!("{action_label} at {resource}"),
                resource: Some(resource.clone()),
            };
            jaymi_logging::info(
                "planner",
                format!(
                    "permission check category={} action={} resource={}",
                    permission_category_label(permission_category),
                    permission_action_label(permission_action),
                    path.display()
                ),
            );
            Some(self.permissions.check(&permission_request)?)
        };

        let gate = combine_gate_decision(&policy_evaluation, permission_result.as_ref(), tool_risk);
        let review_requirement = if matches!(gate, GateDecision::RequiresApproval { .. }) {
            ReviewRequirement::Required
        } else {
            ReviewRequirement::NotRequired
        };

        let mut plan = self.create_action_plan(
            intent,
            originating_request,
            capability,
            &tool_id,
            &resource,
            action_label,
            permission_category,
            permission_action,
            tool_risk,
            review_requirement,
            expected_outputs,
            tool_input.deletion_method,
            self.generate_action_preview(&tool_id, tool_input, &gate),
        );
        plan.mark_ready()
            .map_err(|error| JaymiError::new(error.to_string()))?;
        self.record_plan_history(&plan)?;

        jaymi_logging::info(
            "planner",
            format!(
                "execution plan created {} · risk={} · gate={} · review={} · tool={} provider={} · policy={}",
                plan.id(),
                tool_risk.as_str(),
                gate.as_str(),
                review_requirement.as_str(),
                candidate.tool_id,
                candidate.provider_id,
                policy_evaluation.decision.as_str()
            ),
        );

        match gate {
            GateDecision::Denied { explanation } => {
                jaymi_logging::warn(
                    "planner",
                    format!("execution denied for tool={tool_id}: {explanation}"),
                );
                let _ = plan.cancel();
                let summary = ExecutionSummary::from_plan(
                    &plan,
                    Vec::new(),
                    Vec::new(),
                    Some(explanation.clone()),
                );
                Ok(PreparedExecution {
                    plan: plan.clone(),
                    tool_id: tool_id.clone(),
                    provider_id: Some(provider_id),
                    policy_evaluation: Some(policy_evaluation.clone()),
                    permission_result: permission_result.clone(),
                    blocked_response: Some(PlannerResponse {
                        content: format!(
                            "Denied before executing '{tool_id}': {explanation}"
                        ),
                        capability: Some(capability),
                        tool_id: Some(tool_id),
                        provider_id: Some(candidate.provider_id),
                        policy_evaluation: Some(policy_evaluation),
                        permission_result,
                        blocked: true,
                        awaiting_review: false,
                        execution_plan: Some(plan),
                        execution_summary: Some(summary),
                        ..PlannerResponse::default()
                    }),
                })
            }
            GateDecision::RequiresApproval { explanation } => {
                plan.mark_awaiting_review()
                    .map_err(|error| JaymiError::new(error.to_string()))?;
                jaymi_logging::warn(
                    "planner",
                    format!(
                        "execution plan {} paused awaiting review · risk={} tool={} · {explanation}",
                        plan.id(),
                        tool_risk.as_str(),
                        tool_id
                    ),
                );
                self.pause_execution(PausedExecution {
                    plan: plan.clone(),
                    tool_input: tool_input.clone(),
                    tool_id: tool_id.clone(),
                    provider_id: Some(provider_id.clone()),
                    capability,
                    policy_evaluation: Some(policy_evaluation.clone()),
                    permission_result: permission_result.clone(),
                    paused_at: Instant::now(),
                })?;
                let summary = ExecutionSummary::from_plan(
                    &plan,
                    Vec::new(),
                    Vec::new(),
                    Some(explanation.clone()),
                );
                let review = ReviewCardModel::from_plan(&plan, Some(explanation.as_str()));
                Ok(PreparedExecution {
                    plan: plan.clone(),
                    tool_id: tool_id.clone(),
                    provider_id: Some(provider_id),
                    policy_evaluation: Some(policy_evaluation.clone()),
                    permission_result: permission_result.clone(),
                    blocked_response: Some(PlannerResponse {
                        content: review.render_text(),
                        capability: Some(capability),
                        tool_id: Some(tool_id),
                        provider_id: Some(candidate.provider_id),
                        policy_evaluation: Some(policy_evaluation),
                        permission_result,
                        blocked: true,
                        awaiting_review: true,
                        execution_plan: Some(plan),
                        execution_summary: Some(summary),
                        ..PlannerResponse::default()
                    }),
                })
            }
            GateDecision::Allowed => {
                plan.approve()
                    .map_err(|error| JaymiError::new(error.to_string()))?;
                jaymi_logging::info(
                    "planner",
                    format!(
                        "execution plan approved {} · risk={}",
                        plan.id(),
                        tool_risk.as_str()
                    ),
                );
                Ok(PreparedExecution {
                    plan,
                    tool_id,
                    provider_id: Some(provider_id),
                    policy_evaluation: Some(policy_evaluation),
                    permission_result,
                    blocked_response: None,
                })
            }
        }
    }

    /// Create a Draft action plan (content frozen at construction).
    #[allow(clippy::too_many_arguments)]
    fn create_action_plan(
        &self,
        intent: IntentId,
        originating_request: &str,
        capability: Capability,
        tool_id: &str,
        resource: &str,
        action_label: &str,
        permission_category: PermissionCategory,
        permission_action: PermissionAction,
        tool_risk: jaymi_tools::ToolRisk,
        review_requirement: ReviewRequirement,
        expected_outputs: Vec<String>,
        deletion_method: Option<DeletionMethod>,
        action_preview: Option<ActionPreview>,
    ) -> ExecutionPlan {
        ExecutionPlan::create(ExecutionPlanParams {
            originating_request: originating_request.to_string(),
            planner_intent: intent,
            capability,
            proposed_tools: vec![tool_id.to_string()],
            steps: {
                // Multiple expected_outputs entries are treated as conversational
                // plan steps (e.g. delete preview bullets). A single entry stays
                // an expected output with one action step.
                if expected_outputs.len() > 1 {
                    expected_outputs
                        .iter()
                        .enumerate()
                        .map(|(index, description)| ExecutionStep {
                            order: index + 1,
                            description: description.clone(),
                            tool_id: Some(tool_id.to_string()),
                            resource: Some(resource.to_string()),
                        })
                        .collect()
                } else {
                    vec![ExecutionStep {
                        order: 1,
                        description: action_label.to_string(),
                        tool_id: Some(tool_id.to_string()),
                        resource: Some(resource.to_string()),
                    }]
                }
            },
            estimated_risk: EstimatedRisk::from_tool_risk(tool_risk),
            affected_resources: vec![resource.to_string()],
            permissions_required: vec![PlanPermissionRequirement::from_enums(
                permission_category,
                permission_action,
            )],
            review_requirement,
            estimated_reversibility: EstimatedReversibility::from_tool_risk(tool_risk),
            expected_outputs: if expected_outputs.len() > 1 {
                vec![format!("Completed: {action_label}")]
            } else {
                expected_outputs
            },
            deletion_method,
            action_preview,
            lineage: PlanLineage::root(),
        })
    }

    /// Ask the selected tool for a read-only preview when review is required.
    fn generate_action_preview(
        &self,
        tool_id: &str,
        tool_input: &ToolInput,
        gate: &GateDecision,
    ) -> Option<ActionPreview> {
        if !matches!(gate, GateDecision::RequiresApproval { .. }) {
            return None;
        }
        match self.orchestrator.preview(tool_id, tool_input) {
            Ok(preview) => preview,
            Err(error) => Some(ActionPreview::unavailable(
                format!("Preview {tool_id}"),
                format!("Preview unavailable: {}", error.message()),
            )),
        }
    }

    /// Execute tools for an Approved plan and produce an [`ExecutionSummary`].
    fn execute_approved_plan(
        &self,
        plan: &mut ExecutionPlan,
        input: ToolInput,
    ) -> JaymiResult<(jaymi_tools::ToolOutput, ExecutionSummary)> {
        if !plan.status().may_execute() {
            return Err(JaymiError::new(format!(
                "execution plan {} is not approved (status={})",
                plan.id(),
                plan.status()
            )));
        }
        let tool_id = plan
            .primary_tool_id()
            .ok_or_else(|| JaymiError::new("execution plan has no proposed tools"))?
            .to_string();
        plan.mark_executing()
            .map_err(|error| JaymiError::new(error.to_string()))?;
        self.transition_conversation(ConversationState::Executing);
        jaymi_logging::info(
            "planner",
            format!("execution plan {} executing tool={}", plan.id(), tool_id),
        );
        let started = Instant::now();
        match self.orchestrator.execute(&tool_id, input) {
            Ok(output) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                if output.success {
                    let _ = plan.mark_completed();
                } else {
                    let _ = plan.mark_failed();
                }
                let summary =
                    ExecutionSummary::from_tool_result(plan, tool_id, &output, duration_ms);
                Ok((output, summary))
            }
            Err(error) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                let _ = plan.mark_failed();
                // Preserve structured failure context for callers that map Err
                // into a response (tests use success=false ToolOutput instead).
                let _ = duration_ms;
                Err(error)
            }
        }
    }

    /// Number of plans currently paused awaiting review.
    pub fn paused_count(&self) -> JaymiResult<usize> {
        Ok(self.paused_store()?.len())
    }

    /// True when `plan_id` has an active pause.
    pub fn is_paused(&self, plan_id: &ExecutionPlanId) -> JaymiResult<bool> {
        Ok(self.paused_store()?.contains(plan_id))
    }

    /// Read-only snapshots of every plan currently paused for review.
    pub fn paused_snapshots(&self) -> JaymiResult<Vec<PausedPlanSnapshot>> {
        Ok(self.paused_store()?.snapshots(Instant::now()))
    }

    /// Override pause TTL (tests).
    pub fn set_pause_ttl(&self, ttl: Duration) -> JaymiResult<()> {
        self.paused_store()?.set_ttl(ttl);
        Ok(())
    }

    /// Insert a pause entry (used by prepare_execution; visible for tests).
    pub fn pause_execution(&self, entry: PausedExecution) -> JaymiResult<()> {
        let mut store = self.paused_store()?;
        // One active pause: a new pause replaces any prior paused plans.
        for mut previous in store.invalidate_all() {
            let _ = previous.plan.cancel();
            jaymi_logging::info(
                "planner",
                format!(
                    "invalidated paused plan {} before new pause",
                    previous.plan.id()
                ),
            );
        }
        let id = entry.plan_id().clone();
        store.pause(entry);
        jaymi_logging::info("planner", format!("paused execution plan {id}"));
        Ok(())
    }

    /// Resolve a Review Card intent against a paused plan.
    ///
    /// - **Approve** — resume the same plan (no replan); execute tools.
    /// - **Cancel** — cancel the paused plan; nothing executes.
    /// - **Modify** — regenerate affected steps into a child plan, keep history,
    ///   and re-pause for approval. Does **not** rebuild ContextBundle unless
    ///   the modification requires it.
    ///
    /// Conversation remains active; this does not require a new user prompt for
    /// Approve / Cancel / Modify-with-note. Duplicate approval after a successful
    /// resume returns an error. Timed-out pauses are cancelled and reported.
    pub fn resolve_review(&self, intent: ReviewIntent) -> JaymiResult<PlannerResponse> {
        self.ensure_ready()?;
        let plan_id = intent.plan_id().clone();
        jaymi_logging::info(
            "planner",
            format!(
                "resolve_review intent={} plan={}",
                intent.as_str(),
                plan_id.as_str()
            ),
        );

        // Review is only valid while WaitingForReview (or after a prior terminal reset).
        match &intent {
            ReviewIntent::Approve { .. } => {
                // resume_paused → execute_approved_plan transitions to Executing.
                if matches!(
                    self.conversation_state(),
                    ConversationState::Idle | ConversationState::Completed
                ) {
                    // Resume after process restart / tests that never set WaitingForReview.
                    self.transition_conversation(ConversationState::PreparingContext);
                    self.transition_conversation(ConversationState::WaitingForReview);
                }
            }
            ReviewIntent::Cancel { .. } => {}
            ReviewIntent::Modify { .. } => {
                self.transition_conversation(ConversationState::WaitingForReview);
            }
        }

        let (response, reassemble_context) = match &intent {
            ReviewIntent::Approve { plan_id } => (self.resume_paused(plan_id.clone())?, true),
            ReviewIntent::Cancel { plan_id } => (
                self.invalidate_paused_plan(plan_id.clone(), "cancelled by user", true)?,
                true,
            ),
            ReviewIntent::Modify { plan_id, note } => {
                let note = note.clone().unwrap_or_default();
                let (response, needs_context) = self.revise_paused_plan(plan_id.clone(), note)?;
                (response, needs_context)
            }
        };

        self.record_approval_from_resolve(&intent, &response)?;
        self.transition_conversation(conversation_state_for_tool_response(&response));

        if !reassemble_context {
            // Partial / ordinary modifications skip reassemble; attach an
            // engine-minted empty bundle so ContextEngine remains the sole factory.
            return Ok(self.finalize_response(response, self.context.empty_bundle()));
        }

        // Assemble a fresh ContextBundle so resume/cancel responses stay on-contract.
        let hints = AssembleHints {
            intent: response
                .execution_plan
                .as_ref()
                .map(|plan| plan.planner_intent())
                .unwrap_or(IntentId::Unknown),
            capability_ids: response
                .capability
                .map(|capability| vec![capability.id().to_string()])
                .unwrap_or_default(),
            complexity: None,
        };
        let request = UserRequest::new("");
        let bundle = self.context.assemble_with(&request, Some(&hints))?;
        Ok(self.finalize_response(response, bundle))
    }

    /// Modify a paused plan: cancel parent, create child revision, re-pause.
    ///
    /// Returns `(response, requires_context_reassemble)`.
    fn revise_paused_plan(
        &self,
        plan_id: ExecutionPlanId,
        note: String,
    ) -> JaymiResult<(PlannerResponse, bool)> {
        let entry = {
            let mut store = self.paused_store()?;
            match store.take_for_invalidate(&plan_id) {
                Ok(entry) => entry,
                Err(PauseError::TimedOut { plan_id }) => {
                    return Ok((self.timeout_response(&plan_id)?, true));
                }
                Err(PauseError::NotFound { plan_id }) => {
                    return Err(JaymiError::new(format!(
                        "no paused plan {plan_id} to modify"
                    )));
                }
                Err(error) => return Err(JaymiError::new(error.to_string())),
            }
        };

        let mut parent = entry.plan;
        let parent_id = parent.id().clone();
        let _ = parent.cancel();
        self.update_plan_history_status(&parent_id, ExecutionStatus::Cancelled)?;

        let draft = apply_modification_note(&parent, &entry.tool_id, &entry.tool_input, &note);
        if draft.changes.is_empty() && note.trim().is_empty() {
            return Err(JaymiError::new(
                "modify requires a note describing the requested changes",
            ));
        }

        let tool = self.tools.get(&draft.tool_id)?;
        let metadata = tool.metadata().clone();
        let tool_risk = metadata
            .risk
            .effective_for(&draft.tool_input, metadata.internet);
        let review_requirement = if tool_risk.requires_review()
            || matches!(
                entry
                    .permission_result
                    .as_ref()
                    .map(|result| result.decision),
                Some(PermissionDecision::RequiresApproval)
            ) {
            ReviewRequirement::Required
        } else {
            ReviewRequirement::NotRequired
        };

        let resource = draft
            .affected_resources
            .first()
            .cloned()
            .unwrap_or_else(|| "unspecified".into());
        let permission_action = if draft.tool_id == MANAGE_PATH_TOOL_ID
            && draft.tool_input.command.as_deref() == Some("delete")
        {
            PermissionAction::Delete
        } else if draft.tool_id == WRITE_FILE_TOOL_ID
            || draft.tool_input.command.as_deref() == Some("rename")
            || draft.tool_input.command.as_deref() == Some("mkdir")
        {
            PermissionAction::Write
        } else {
            PermissionAction::Read
        };

        let mut child = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: draft.originating_request.clone(),
            planner_intent: parent.planner_intent(),
            capability: entry.capability,
            proposed_tools: vec![draft.tool_id.clone()],
            steps: draft.steps.clone(),
            estimated_risk: EstimatedRisk::from_tool_risk(tool_risk),
            affected_resources: draft.affected_resources.clone(),
            permissions_required: vec![PlanPermissionRequirement::from_enums(
                PermissionCategory::Filesystem,
                permission_action,
            )],
            review_requirement,
            estimated_reversibility: EstimatedReversibility::from_tool_risk(tool_risk),
            expected_outputs: parent.expected_outputs().to_vec(),
            deletion_method: draft.tool_input.deletion_method.or(parent.deletion_method()),
            action_preview: self
                .orchestrator
                .preview(&draft.tool_id, &draft.tool_input)
                .ok()
                .flatten()
                .or_else(|| parent.action_preview().cloned()),
            lineage: PlanLineage::revision_of(&parent, Some(note.clone()), draft.changes.clone()),
        });
        child
            .mark_ready()
            .map_err(|error| JaymiError::new(error.to_string()))?;
        self.record_plan_history(&child)?;

        // Reuse policy/permission snapshots when the tool identity is unchanged
        // and the modification is partial; otherwise re-check cheaply without
        // reassembling ContextBundle.
        let candidate = ExecutionCandidate {
            tool_id: draft.tool_id.clone(),
            provider_id: metadata.provider.clone(),
            requires_internet: matches!(metadata.internet, InternetRequirement::Required),
            local_only: matches!(metadata.privacy, PrivacyMode::LocalOnly),
            cloud_only: matches!(metadata.privacy, PrivacyMode::CloudOnly),
        };
        let policy_evaluation = if draft.tool_id == entry.tool_id && entry.policy_evaluation.is_some()
        {
            entry.policy_evaluation.clone().unwrap()
        } else {
            self.policies.evaluate(&candidate)?
        };

        let permission_result = if matches!(policy_evaluation.decision, PolicyDecision::Denied) {
            None
        } else if draft.tool_id == entry.tool_id && entry.permission_result.is_some() {
            entry.permission_result.clone()
        } else {
            let permission_request = PermissionRequest {
                category: PermissionCategory::Filesystem,
                action: permission_action,
                scope: PermissionScope::Once,
                explanation: format!("Revised plan step at {resource}"),
                resource: Some(resource.clone()),
            };
            Some(self.permissions.check(&permission_request)?)
        };

        if matches!(policy_evaluation.decision, PolicyDecision::Denied) {
            let _ = child.cancel();
            self.update_plan_history_status(child.id(), ExecutionStatus::Cancelled)?;
            let summary = ExecutionSummary::from_plan(
                &child,
                Vec::new(),
                Vec::new(),
                Some(policy_evaluation.explanation()),
            );
            return Ok((
                PlannerResponse {
                    content: format!(
                        "Revised plan {} was denied by policy: {}",
                        child.id(),
                        policy_evaluation.explanation()
                    ),
                    capability: Some(entry.capability),
                    tool_id: Some(draft.tool_id),
                    provider_id: Some(metadata.provider),
                    policy_evaluation: Some(policy_evaluation),
                    permission_result: None,
                    blocked: true,
                    execution_plan: Some(child),
                    execution_summary: Some(summary),
                    ..PlannerResponse::default()
                },
                draft.requires_context_reassemble,
            ));
        }

        if let Some(permission) = &permission_result {
            if matches!(permission.decision, PermissionDecision::Denied) {
                let _ = child.cancel();
                self.update_plan_history_status(child.id(), ExecutionStatus::Cancelled)?;
                let summary = ExecutionSummary::from_plan(
                    &child,
                    Vec::new(),
                    Vec::new(),
                    Some(permission.explanation.clone()),
                );
                return Ok((
                    PlannerResponse {
                        content: format!(
                            "Revised plan {} was denied by permission: {}",
                            child.id(),
                            permission.explanation
                        ),
                        capability: Some(entry.capability),
                        tool_id: Some(draft.tool_id),
                        provider_id: Some(metadata.provider),
                        policy_evaluation: Some(policy_evaluation),
                        permission_result: permission_result.clone(),
                        blocked: true,
                        execution_plan: Some(child),
                        execution_summary: Some(summary),
                        ..PlannerResponse::default()
                    },
                    draft.requires_context_reassemble,
                ));
            }
        }

        // Revised plans that still require review are re-paused for approval.
        child
            .mark_awaiting_review()
            .map_err(|error| JaymiError::new(error.to_string()))?;
        self.pause_execution(PausedExecution {
            plan: child.clone(),
            tool_input: draft.tool_input,
            tool_id: draft.tool_id.clone(),
            provider_id: Some(metadata.provider.clone()),
            capability: entry.capability,
            policy_evaluation: Some(policy_evaluation.clone()),
            permission_result: permission_result.clone(),
            paused_at: Instant::now(),
        })?;
        self.update_plan_history_status(child.id(), ExecutionStatus::AwaitingReview)?;

        let explanation = permission_result
            .as_ref()
            .map(|result| result.explanation.as_str());
        let review = ReviewCardModel::from_plan(&child, explanation);

        Ok((
            PlannerResponse {
                content: review.render_text(),
                capability: Some(entry.capability),
                tool_id: Some(draft.tool_id),
                provider_id: Some(metadata.provider),
                policy_evaluation: Some(policy_evaluation),
                permission_result,
                blocked: true,
                awaiting_review: true,
                execution_plan: Some(child),
                ..PlannerResponse::default()
            },
            draft.requires_context_reassemble,
        ))
    }

    fn resume_paused(&self, plan_id: ExecutionPlanId) -> JaymiResult<PlannerResponse> {
        let mut entry = {
            let mut store = self.paused_store()?;
            match store.take_for_resume(&plan_id) {
                Ok(entry) => entry,
                Err(PauseError::TimedOut { plan_id }) => {
                    return self.timeout_response(&plan_id);
                }
                Err(PauseError::NotFound { plan_id }) => {
                    return Err(JaymiError::new(format!(
                        "duplicate or unknown approval for plan {plan_id}"
                    )));
                }
                Err(error) => return Err(JaymiError::new(error.to_string())),
            }
        };

        entry
            .plan
            .approve()
            .map_err(|error| JaymiError::new(error.to_string()))?;
        jaymi_logging::info(
            "planner",
            format!(
                "resuming paused plan {} without replan tool={}",
                entry.plan.id(),
                entry.tool_id
            ),
        );

        let (output, execution_summary) =
            self.execute_approved_plan(&mut entry.plan, entry.tool_input)?;
        self.ensure_success(&output)?;

        let content = output.message.clone().unwrap_or_else(|| {
            format!(
                "Resumed and completed plan {} via {} → {}",
                entry.plan.id(),
                entry.capability.id(),
                entry.tool_id
            )
        });

        Ok(response_from_tool_output(
            content,
            entry.capability,
            entry.tool_id,
            entry.provider_id,
            entry.plan,
            execution_summary,
            entry.policy_evaluation,
            entry.permission_result,
            output,
        ))
    }

    fn invalidate_paused_plan(
        &self,
        plan_id: ExecutionPlanId,
        reason: &str,
        user_cancelled: bool,
    ) -> JaymiResult<PlannerResponse> {
        let mut entry = {
            let mut store = self.paused_store()?;
            match store.take_for_invalidate(&plan_id) {
                Ok(entry) => entry,
                Err(PauseError::TimedOut { plan_id }) => {
                    return self.timeout_response(&plan_id);
                }
                Err(PauseError::NotFound { plan_id }) => {
                    return Err(JaymiError::new(format!(
                        "no paused plan {plan_id} to invalidate ({reason})"
                    )));
                }
                Err(error) => return Err(JaymiError::new(error.to_string())),
            }
        };
        let _ = entry.plan.cancel();
        let summary = ExecutionSummary::cancelled(&entry.plan, reason);
        let content = if user_cancelled {
            format!(
                "Cancelled plan {}. No tools were executed.",
                entry.plan.id()
            )
        } else {
            format!(
                "Invalidated plan {}. Send an updated request to build a new plan. ({reason})",
                entry.plan.id()
            )
        };
        Ok(PlannerResponse {
            content,
            capability: Some(entry.capability),
            tool_id: Some(entry.tool_id),
            provider_id: entry.provider_id,
            policy_evaluation: entry.policy_evaluation,
            permission_result: entry.permission_result,
            blocked: false,
            awaiting_review: false,
            execution_plan: Some(entry.plan),
            execution_summary: Some(summary),
            ..PlannerResponse::default()
        })
    }

    fn timeout_response(&self, plan_id: &str) -> JaymiResult<PlannerResponse> {
        let plan_id = ExecutionPlanId::from_existing(plan_id);
        Ok(PlannerResponse {
            content: format!(
                "Paused plan {} timed out waiting for review. No tools were executed.",
                plan_id.as_str()
            ),
            blocked: true,
            awaiting_review: false,
            execution_summary: Some(ExecutionSummary {
                plan_id: plan_id.clone(),
                status: ExecutionStatus::Cancelled,
                goal: format!("Review timed out for plan {}", plan_id.as_str()),
                actions_performed: Vec::new(),
                resources_changed: Vec::new(),
                files_edited: Vec::new(),
                duration_ms: 0,
                warnings: Vec::new(),
                errors: vec!["review timeout".into()],
                error: Some("review timeout".into()),
                next_suggested_actions: vec![
                    "Retry the request".into(),
                    "Approve sooner when a Review Card appears".into(),
                ],
                tools_executed: Vec::new(),
                outputs: Vec::new(),
                partial: false,
                files_moved_to_trash: Vec::new(),
                files_permanently_deleted: Vec::new(),
                recovery_available: None,
                deletion_method: None,
            }),
            ..PlannerResponse::default()
        })
    }

    /// Cancel every paused plan because the user sent a new request.
    fn invalidate_paused(&self, reason: &str) -> JaymiResult<()> {
        let mut store = self.paused_store()?;
        let removed = store.invalidate_all();
        for mut entry in removed {
            let _ = entry.plan.cancel();
            jaymi_logging::info(
                "planner",
                format!(
                    "invalidated paused plan {} ({reason})",
                    entry.plan.id()
                ),
            );
        }
        Ok(())
    }

    fn paused_store(&self) -> JaymiResult<std::sync::MutexGuard<'_, PausedPlanStore>> {
        self.paused
            .lock()
            .map_err(|_| JaymiError::new(PauseError::Poisoned.to_string()))
    }

    fn ensure_success(&self, output: &jaymi_tools::ToolOutput) -> JaymiResult<()> {
        if output.success {
            Ok(())
        } else {
            Err(JaymiError::new(
                output
                    .message
                    .clone()
                    .unwrap_or_else(|| "tool execution failed".to_string()),
            ))
        }
    }

    /// Build a PlannerResponse for a soft tool failure (structured summary retained).
    #[allow(clippy::too_many_arguments)]
    fn tool_failure_response(
        &self,
        plan: ExecutionPlan,
        capability: Capability,
        tool_id: String,
        provider_id: Option<String>,
        policy_evaluation: Option<PolicyEvaluation>,
        permission_result: Option<PermissionCheckResult>,
        output: jaymi_tools::ToolOutput,
        execution_summary: ExecutionSummary,
    ) -> PlannerResponse {
        let detail = execution_summary
            .error
            .clone()
            .or(output.message)
            .unwrap_or_else(|| format!("tool '{tool_id}' failed"));
        PlannerResponse {
            content: format!("Failed while executing '{tool_id}': {detail}"),
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            policy_evaluation,
            permission_result,
            blocked: true,
            awaiting_review: false,
            execution_plan: Some(plan),
            execution_summary: Some(execution_summary),
            ..PlannerResponse::default()
        }
    }
}

struct PreparedExecution {
    plan: ExecutionPlan,
    tool_id: String,
    provider_id: Option<String>,
    policy_evaluation: Option<PolicyEvaluation>,
    permission_result: Option<PermissionCheckResult>,
    blocked_response: Option<PlannerResponse>,
}

fn response_from_tool_output(
    content: String,
    capability: Capability,
    tool_id: String,
    provider_id: Option<String>,
    plan: ExecutionPlan,
    execution_summary: ExecutionSummary,
    policy_evaluation: Option<PolicyEvaluation>,
    permission_result: Option<PermissionCheckResult>,
    output: jaymi_tools::ToolOutput,
) -> PlannerResponse {
    PlannerResponse {
        content,
        capability: Some(capability),
        tool_id: Some(tool_id),
        provider_id,
        listed_path: output.listed_path,
        entries: output.entries,
        citations: output.citations,
        document: output.document,
        policy_evaluation,
        permission_result,
        blocked: false,
        awaiting_review: false,
        execution_plan: Some(plan),
        execution_summary: Some(execution_summary),
        project_knowledge: output.project_knowledge,
        terminal_session_id: output.session_id,
        terminal_output: output.terminal_output,
        terminal_scrollback: output.terminal_scrollback,
        terminal_history: output.terminal_history,
        terminal_title: output.terminal_title,
        terminal_alive: output.terminal_alive,
        git_branch: output.git_branch,
        git_summary: output.git_summary,
        git_is_repository: output.git_is_repository,
        git_modified: output.git_modified,
        git_added: output.git_added,
        git_deleted: output.git_deleted,
        git_staged: output.git_staged,
        git_untracked: output.git_untracked,
        lsp_hover: output.lsp_hover,
        lsp_completions: output.lsp_completions,
        lsp_diagnostics: output.lsp_diagnostics,
        lsp_definitions: output.lsp_definitions,
        lsp_references: output.lsp_references,
        lsp_edits: output.lsp_edits,
        ..PlannerResponse::default()
    }
}

fn finalize(mut response: PlannerResponse, bundle: ContextBundle) -> PlannerResponse {
    // ContextBundle is the sole request-context contract. Do not mirror
    // memory / project / promotions onto parallel response fields.
    response.context_bundle = Some(bundle);
    // conversation_state is stamped by Planner::finalize_response.
    response
}

fn conversation_state_for_tool_response(response: &PlannerResponse) -> ConversationState {
    if response.awaiting_review {
        ConversationState::WaitingForReview
    } else if response.blocked {
        // Denied / cancelled without execution.
        if response
            .execution_summary
            .as_ref()
            .map(|summary| matches!(summary.status, ExecutionStatus::Cancelled))
            .unwrap_or(false)
        {
            ConversationState::Cancelled
        } else {
            ConversationState::Failed
        }
    } else if response
        .execution_summary
        .as_ref()
        .map(|summary| matches!(summary.status, ExecutionStatus::Failed))
        .unwrap_or(false)
    {
        ConversationState::Failed
    } else if response
        .execution_summary
        .as_ref()
        .map(|summary| matches!(summary.status, ExecutionStatus::Cancelled))
        .unwrap_or(false)
    {
        ConversationState::Cancelled
    } else {
        ConversationState::Completed
    }
}

fn log_promotions(bundle: &ContextBundle) {
    if !bundle.promotion_suggestions().is_empty() {
        jaymi_logging::info(
            "planner",
            format!(
                "promotion suggestions={} ask={:?}",
                bundle.promotion_suggestions().len(),
                bundle.promotion_ask()
            ),
        );
    }
}

/// Combined Policy + Permission + ToolRisk gate for one prepared execution.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GateDecision {
    Allowed,
    RequiresApproval { explanation: String },
    Denied { explanation: String },
}

impl GateDecision {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::RequiresApproval { .. } => "requires_approval",
            Self::Denied { .. } => "denied",
        }
    }
}

/// Combine Action Policy, Permission, and ToolRisk into one Planner gate.
///
/// Precedence: Denied > RequiresApproval > Allowed.
/// ToolRisk Modify / Destructive / External escalates to RequiresApproval.
fn requests_permanent_deletion(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("permanently delete")
        || lower.contains("permanent delete")
        || lower.contains("force delete")
        || lower.contains("delete permanently")
        || lower.contains("rm -rf")
        || lower.contains("without trash")
        || lower.contains("skip trash")
}

fn combine_gate_decision(
    policy: &PolicyEvaluation,
    permission: Option<&PermissionCheckResult>,
    tool_risk: jaymi_tools::ToolRisk,
) -> GateDecision {
    if matches!(policy.decision, PolicyDecision::Denied) {
        return GateDecision::Denied {
            explanation: policy.explanation(),
        };
    }

    if let Some(permission) = permission {
        if matches!(permission.decision, PermissionDecision::Denied) {
            return GateDecision::Denied {
                explanation: permission.explanation.clone(),
            };
        }
    }

    let mut reasons = Vec::new();
    if matches!(policy.decision, PolicyDecision::RequiresApproval) {
        reasons.push(policy.explanation());
    }
    if let Some(permission) = permission {
        if matches!(permission.decision, PermissionDecision::RequiresApproval) {
            reasons.push(permission.explanation.clone());
        }
    }
    if tool_risk.requires_review() {
        reasons.push(format!(
            "tool risk '{}' requires review before execution",
            tool_risk.as_str()
        ));
    }

    if reasons.is_empty() {
        GateDecision::Allowed
    } else {
        GateDecision::RequiresApproval {
            explanation: reasons.join(" · "),
        }
    }
}

fn permission_category_label(category: PermissionCategory) -> &'static str {
    match category {
        PermissionCategory::Filesystem => "filesystem",
        PermissionCategory::Internet => "internet",
        PermissionCategory::Terminal => "terminal",
        PermissionCategory::Communication => "communication",
        PermissionCategory::System => "system",
        PermissionCategory::AiProviders => "ai_providers",
    }
}

fn permission_action_label(action: PermissionAction) -> &'static str {
    match action {
        PermissionAction::Read => "read",
        PermissionAction::Write => "write",
        PermissionAction::Execute => "execute",
        PermissionAction::Delete => "delete",
        PermissionAction::Network => "network",
        PermissionAction::Import => "import",
        PermissionAction::Export => "export",
    }
}

fn format_project_context_summary(context: &ProjectContext) -> String {
    let conversation_messages: usize = context
        .conversations
        .iter()
        .map(|conversation| conversation.message_count)
        .sum();
    format!(
        "Restored project \"{}\". indexed_files={} conversations={} conversation_messages={} memories={} tasks={} decisions={} architecture={} documents={} parsed_content={} recent_work={}",
        context.project.name,
        context.indexed_files.len(),
        context.conversations.len(),
        conversation_messages,
        context.memories.entry_count(),
        context.tasks.len(),
        context.decisions.len(),
        context.architecture_documents.len(),
        context.important_documents.len(),
        context.parsed_content.len(),
        context.recent_work.len()
    )
}

fn truncate_for_log(value: &str) -> String {
    const MAX: usize = 120;
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let shortened: String = trimmed.chars().take(MAX).collect();
        format!("{shortened}…")
    }
}


impl DispatchSupport for Planner {
    fn resolve_workspace_path(&self, path: PathBuf) -> PathBuf {
        Planner::resolve_workspace_path(self, path)
    }

    fn scope_search_request(&self, request: jaymi_core::SearchRequest) -> jaymi_core::SearchRequest {
        Planner::scope_search_request(self, request)
    }

    fn resolve_deletion_method(
        &self,
        requested: Option<DeletionMethod>,
        request_text: &str,
    ) -> JaymiResult<DeletionMethod> {
        Planner::resolve_deletion_method(self, requested, request_text)
    }
}

impl Lifecycle for Planner {
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
        if !self.capabilities.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: capability engine is not ready",
            ));
        }
        if !self.providers.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: provider registry is not ready",
            ));
        }
        if !self.tools.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: tool registry is not ready",
            ));
        }
        if !self.policies.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: policy engine is not ready",
            ));
        }
        if !self.permissions.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: permission engine is not ready",
            ));
        }

        let _ = &self.decision;
        let _ = &self.reasoning;
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let registries_ready = self.capabilities.is_initialized()
            && self.providers.is_initialized()
            && self.tools.is_initialized()
            && self.policies.is_initialized()
            && self.permissions.is_initialized();
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized && registries_ready,
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::CapabilityEngine;
    use jaymi_core::{EntryType, FileType, Lifecycle};
    use jaymi_database::Database;
    use jaymi_knowledge::SqliteKnowledgeStore;
    use jaymi_memory_engine::{InMemoryMemoryStore, MemoryEngine};
    use jaymi_parsers::default_registry;
    use jaymi_permissions::PermissionDecision;
    use jaymi_project_engine::{InMemoryProjectStore, ProjectEngine};
    use jaymi_providers::{FilesystemProvider, Provider, FILESYSTEM_PROVIDER_ID};
    use jaymi_tools::{
        EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
        PrivacyMode, ReadFileTool, Reliability, ResourceCost, ResultType, SearchFilesTool, Tool,
        ToolInput, ToolMetadata, ToolOutput, ToolRisk, READ_FILE_TOOL_ID, SEARCH_FILES_TOOL_ID,
    };
    use jaymi_understanding::{ContentIntelligenceApi, SqliteContentStore, UnderstandingEngine};
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use jaymi_reasoning::{
        ReasoningCapabilities, ReasoningHealth, ReasoningModelInfo, ReasoningProvider,
        ReasoningResponse, ReasoningResult, ReasoningStream, StreamingChunk,
    };
    use jaymi_reasoning::ModelIdentifier;

    struct CountingReasoningProvider {
        id: String,
        complete_calls: AtomicU32,
        stream_calls: AtomicU32,
        last_history_len: AtomicU32,
        last_prompt_has_prior_user: std::sync::Mutex<Option<bool>>,
        last_model: std::sync::Mutex<Option<ModelIdentifier>>,
        models: Vec<&'static str>,
    }

    impl CountingReasoningProvider {
        fn new(id: &str) -> Self {
            Self {
                id: id.into(),
                complete_calls: AtomicU32::new(0),
                stream_calls: AtomicU32::new(0),
                last_history_len: AtomicU32::new(0),
                last_prompt_has_prior_user: std::sync::Mutex::new(None),
                last_model: std::sync::Mutex::new(None),
                models: vec!["mock-model"],
            }
        }

        fn with_models(mut self, models: Vec<&'static str>) -> Self {
            self.models = models;
            self
        }

        fn complete_calls(&self) -> u32 {
            self.complete_calls.load(AtomicOrdering::SeqCst)
        }

        fn stream_calls(&self) -> u32 {
            self.stream_calls.load(AtomicOrdering::SeqCst)
        }

        fn last_history_len(&self) -> u32 {
            self.last_history_len.load(AtomicOrdering::SeqCst)
        }

        fn last_model(&self) -> Option<ModelIdentifier> {
            self.last_model.lock().ok().and_then(|guard| guard.clone())
        }

        fn record_request(&self, request: &jaymi_reasoning::ReasoningRequest) {
            self.last_history_len
                .store(request.history.len() as u32, AtomicOrdering::SeqCst);
            *self.last_model.lock().expect("lock") = request.model.clone();
            let has_prior = request.prompt.as_ref().map(|prompt| {
                prompt.text.contains("user: earlier")
                    || request
                        .history
                        .iter()
                        .any(|turn| turn.content.contains("earlier"))
            });
            *self.last_prompt_has_prior_user.lock().expect("lock") = has_prior;
        }
    }

    struct ScriptedTokenStream {
        tokens: Vec<String>,
        index: usize,
        cancelled: bool,
        provider_id: String,
        model: Option<ModelIdentifier>,
    }

    impl ReasoningStream for ScriptedTokenStream {
        fn next_chunk(&mut self) -> ReasoningResult<Option<StreamingChunk>> {
            if self.cancelled {
                return Ok(Some(StreamingChunk::cancelled(self.index as u64)));
            }
            if self.index < self.tokens.len() {
                let text = self.tokens[self.index].clone();
                let chunk = StreamingChunk::token(self.index as u64, text);
                self.index += 1;
                return Ok(Some(chunk));
            }
            if self.index == self.tokens.len() {
                let mut metrics = jaymi_reasoning::ReasoningMetrics::timed(1)
                    .with_provider_id(self.provider_id.clone())
                    .with_tokens(Some(1), Some(self.tokens.len() as u64));
                if let Some(model) = self.model.clone() {
                    metrics = metrics.with_model(model);
                }
                let chunk = StreamingChunk::completed(self.index as u64, metrics);
                self.index += 1;
                return Ok(Some(chunk));
            }
            Ok(None)
        }

        fn cancel(&mut self) {
            self.cancelled = true;
        }
    }

    impl ReasoningProvider for CountingReasoningProvider {
        fn id(&self) -> &str {
            &self.id
        }

        fn display_name(&self) -> &str {
            &self.id
        }

        fn capabilities(&self) -> ReasoningCapabilities {
            ReasoningCapabilities::full()
        }

        fn health(&self) -> ReasoningHealth {
            ReasoningHealth::Ready
        }

        fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
            Ok(self
                .models
                .iter()
                .map(|name| {
                    ReasoningModelInfo::new(ModelIdentifier::new(&self.id, *name), *name)
                        .with_context_tokens(8_192)
                })
                .collect())
        }

        fn complete(
            &self,
            request: jaymi_reasoning::ReasoningRequest,
        ) -> ReasoningResult<ReasoningResponse> {
            self.complete_calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.record_request(&request);
            let mut response = ReasoningResponse::completed(format!(
                "{}:{}",
                self.id, request.goal
            ));
            if let Some(model) = request.model.clone() {
                response = response.with_model(model.clone()).with_metrics(
                    jaymi_reasoning::ReasoningMetrics::timed(1)
                        .with_provider_id(self.id.clone())
                        .with_model(model),
                );
            }
            Ok(response)
        }

        fn stream(
            &self,
            request: jaymi_reasoning::ReasoningRequest,
        ) -> ReasoningResult<Box<dyn ReasoningStream>> {
            self.stream_calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.record_request(&request);
            // Emit goal as a single token so conversational collect matches complete content.
            Ok(Box::new(ScriptedTokenStream {
                tokens: vec![format!("{}:{}", self.id, request.goal)],
                index: 0,
                cancelled: false,
                provider_id: self.id.clone(),
                model: request.model.clone(),
            }))
        }
    }

    fn test_memory_engine() -> Arc<dyn MemoryEngineApi> {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();
        Arc::new(engine)
    }

    fn test_project_engine() -> Arc<dyn ProjectEngineApi> {
        let mut engine = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
        engine.initialize().unwrap();
        Arc::new(engine)
    }

    fn planner_with_search_and_read() -> Planner {
        planner_with_tools(|tools, filesystem, content_api| {
            tools
                .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                .unwrap();
            tools
                .register_tool(Arc::new(ReadFileTool::new(content_api)))
                .unwrap();
        })
    }

    fn planner_with_tools<F>(register: F) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
    {
        planner_with_tools_and_policies(register, |policies| {
            policies.initialize().unwrap();
        })
    }

    fn planner_with_tools_and_routes<F>(register: F, routes: ToolRouteTable) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
    {
        planner_with_tools_policies_and_routes(
            register,
            |policies| {
                policies.initialize().unwrap();
            },
            routes,
        )
    }

    fn planner_with_tools_and_policies<F, P>(register: F, configure_policies: P) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
        P: FnOnce(&mut PolicyEngine),
    {
        planner_with_tools_policies_and_routes(register, configure_policies, ToolRouteTable::builtin())
    }

    fn planner_with_tools_policies_and_routes<F, P>(
        register: F,
        configure_policies: P,
        routes: ToolRouteTable,
    ) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
        P: FnOnce(&mut PolicyEngine),
    {
        planner_with_tools_policies_routes_and_reasoning(
            register,
            configure_policies,
            routes,
            None,
        )
    }

    fn planner_with_reasoning(
        reasoning: Arc<dyn jaymi_reasoning::ReasoningProvider>,
    ) -> Planner {
        planner_with_tools_policies_routes_reasoning_and_registry(
            |tools, filesystem, content_api| {
                tools
                    .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                    .unwrap();
                tools
                    .register_tool(Arc::new(ReadFileTool::new(content_api)))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
            },
            ToolRouteTable::builtin(),
            Some(reasoning),
            None,
        )
    }

    fn planner_with_reasoning_and_registry(
        reasoning: Arc<dyn jaymi_reasoning::ReasoningProvider>,
        registry: Arc<jaymi_reasoning::ModelRegistry>,
    ) -> Planner {
        planner_with_tools_policies_routes_reasoning_and_registry(
            |tools, filesystem, content_api| {
                tools
                    .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                    .unwrap();
                tools
                    .register_tool(Arc::new(ReadFileTool::new(content_api)))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
            },
            ToolRouteTable::builtin(),
            Some(reasoning),
            Some(registry),
        )
    }

    fn planner_with_tools_policies_routes_and_reasoning<F, P>(
        register: F,
        configure_policies: P,
        routes: ToolRouteTable,
        reasoning: Option<Arc<dyn jaymi_reasoning::ReasoningProvider>>,
    ) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
        P: FnOnce(&mut PolicyEngine),
    {
        planner_with_tools_policies_routes_reasoning_and_registry(
            register,
            configure_policies,
            routes,
            reasoning,
            None,
        )
    }

    fn planner_with_tools_policies_routes_reasoning_and_registry<F, P>(
        register: F,
        configure_policies: P,
        routes: ToolRouteTable,
        reasoning: Option<Arc<dyn jaymi_reasoning::ReasoningProvider>>,
        model_registry: Option<Arc<jaymi_reasoning::ModelRegistry>>,
    ) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
        P: FnOnce(&mut PolicyEngine),
    {
        let mut capabilities = CapabilityEngine::new();
        capabilities.initialize().unwrap();
        capabilities.register(Capability::Search).unwrap();
        capabilities.register(Capability::ReadDocuments).unwrap();
        capabilities.register(Capability::Code).unwrap();
        capabilities.register(Capability::FileManagement).unwrap();

        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        providers.register(&filesystem).unwrap();
        let filesystem = Arc::new(filesystem);

        let data = temp_dir().join("planner-data");
        fs::create_dir_all(&data).unwrap();
        let mut db = Database::with_data_dir(&data);
        db.initialize().unwrap();
        let db = Arc::new(db);
        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);
        let content = Arc::new(SqliteContentStore::new(Arc::clone(&db)));
        let parsers = Arc::new(default_registry().unwrap());
        let mut understanding = UnderstandingEngine::new(
            Arc::clone(&knowledge),
            content,
            Arc::clone(&filesystem),
            parsers,
        );
        understanding.initialize().unwrap();
        let understanding = Arc::new(understanding);
        let content_api = Arc::new(ContentIntelligenceApi::new(Arc::clone(&understanding)));

        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        register(
            &mut tools,
            Arc::clone(&filesystem),
            Arc::clone(&content_api),
        );
        let tools = Arc::new(tools);
        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));

        let mut policies = PolicyEngine::new();
        configure_policies(&mut policies);
        let mut permissions = PermissionEngine::new();
        permissions.initialize().unwrap();

        let memory = test_memory_engine();
        let projects = test_project_engine();
        let mut search = jaymi_search::SearchEngine::new(Arc::clone(&knowledge), None);
        search.initialize().unwrap();
        let mut context = jaymi_context::ContextEngine::new();
        context.initialize().unwrap();
        context
            .bind_sources(jaymi_context::ContextSources {
                memory: Arc::clone(&memory),
                projects: Arc::clone(&projects),
                search: Arc::new(search),
            })
            .unwrap();

        let mut planner = Planner::new(PlannerDeps {
            capabilities: Arc::new(capabilities) as Arc<dyn CapabilityEngineApi>,
            providers: Arc::new(providers),
            tools,
            orchestrator,
            policies: Arc::new(policies),
            permissions: Arc::new(permissions),
            memory,
            projects,
            context: Arc::new(context),
            routes,
            reasoning,
            model_registry,
        });
        planner.initialize().unwrap();
        planner
    }

    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jaymi-planner-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct CloudSearchTool {
        metadata: ToolMetadata,
    }

    impl CloudSearchTool {
        fn new() -> Self {
            Self {
                metadata: ToolMetadata {
                    id: "cloud_search".into(),
                    name: "Cloud Search".into(),
                    version: "0.1.0".into(),
                    description: "Requires internet".into(),
                    provider: "cloud".into(),
                    capabilities: vec![Capability::Search],
                    risk: ToolRisk::External,
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::Low,
                    memory_usage: MemoryUsage::Small,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::CloudOnly,
                    internet: InternetRequirement::Required,
                    reliability: Reliability::Experimental,
                    result_type: ResultType::SearchResults,
                },
            }
        }
    }

    impl Tool for CloudSearchTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput> {
            Ok(ToolOutput::directory_listing(Vec::new()))
        }
    }

    #[test]
    fn planner_initializes_from_registries() {
        let planner = planner_with_search_and_read();
        assert!(planner.health_check().healthy);
        assert!(planner
            .discover_capabilities()
            .contains(&Capability::Search));
        assert!(planner
            .discover_capabilities()
            .contains(&Capability::ReadDocuments));
        assert_eq!(planner.provider_count(), 1);
        assert_eq!(planner.tool_count(), 2);
    }

    #[test]
    fn list_directory_flows_through_architecture() {
        let dir = temp_dir();
        let mut file = File::create(dir.join("readme.md")).unwrap();
        write!(file, "jaymi").unwrap();
        fs::create_dir(dir.join("src")).unwrap();

        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::list_directory(&dir)).unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert_eq!(response.tool_id.as_deref(), Some("search_files"));
        assert_eq!(
            response.provider_id.as_deref(),
            Some(FILESYSTEM_PROVIDER_ID)
        );
        assert!(!response.blocked);
        assert!(response.policy_evaluation.as_ref().unwrap().allowed);
        assert_eq!(
            response.permission_result.as_ref().unwrap().decision,
            PermissionDecision::Allowed
        );
        assert_eq!(response.entries.len(), 2);
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "readme.md" && entry.entry_type == EntryType::File));
        let capability_plan = response.capability_plan.expect("capability plan");
        assert_eq!(capability_plan.capabilities(), vec![Capability::Search]);
        let action_plan = response.execution_plan.expect("action execution plan");
        assert_eq!(action_plan.status(), ExecutionStatus::Completed);
        assert_eq!(action_plan.planner_intent(), IntentId::ListDirectory);
        assert!(response.execution_summary.is_some());
    }

    #[test]
    fn read_file_returns_unified_document() {
        let dir = temp_dir();
        let path = dir.join("spec.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "# Spec\n\nDetails.").unwrap();

        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::read_file(&path)).unwrap();

        assert_eq!(response.capability, Some(Capability::ReadDocuments));
        assert_eq!(response.tool_id.as_deref(), Some("read_file"));
        assert!(!response.blocked);
        let document = response.document.expect("document");
        assert_eq!(document.file_type, FileType::Markdown);
        assert_eq!(document.title.as_deref(), Some("Spec"));
        assert_eq!(document.parser_id, "markdown");
        assert!(document.text.contains("Details."));
        assert!(response.content.contains("markdown"));
    }

    #[test]
    fn planner_does_not_call_filesystem_for_unknown_intent() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("sing a song")).unwrap();
        assert!(response.entries.is_empty());
        assert!(response.document.is_none());
        assert!(response.capability.is_none());
        assert!(!response.blocked);
        assert!(!response.content.contains("Unsupported request"));
        assert!(response.context_bundle.is_some());
    }

    #[test]
    fn conversational_request_invokes_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let response = planner
            .handle(UserRequest::new("What is a good way to learn Rust?"))
            .unwrap();
        assert!(response.reasoning_used);
        assert_eq!(response.reasoning_provider_id.as_deref(), Some("chat-mock"));
        assert!(response.content.contains("chat-mock:"));
        assert_eq!(
            response.stream_lifecycle,
            Some(jaymi_reasoning::StreamingLifecycle::Completed)
        );
        assert!(response.capability.is_none());
        assert!(response.tool_id.is_none());
        assert!(!response.awaiting_review);
        assert!(!response.blocked);
        assert_eq!(provider.stream_calls(), 1);
        assert_eq!(provider.complete_calls(), 0);
        assert!(response.context_bundle.is_some());
    }

    #[test]
    fn default_registry_model_populates_reasoning_request() {
        let provider = Arc::new(
            CountingReasoningProvider::new("chat-mock").with_models(vec!["llama", "mistral"]),
        );
        let registry = jaymi_reasoning::ModelRegistry::with_provider(
            Arc::clone(&provider) as Arc<dyn jaymi_reasoning::ReasoningProvider>,
        );
        registry.refresh().unwrap();
        registry
            .set_default(Some(ModelIdentifier::new("chat-mock", "mistral")))
            .unwrap();
        let planner =
            planner_with_reasoning_and_registry(provider.clone(), Arc::new(registry));
        let response = planner
            .handle(UserRequest::new("hello from default model"))
            .unwrap();
        assert!(response.reasoning_used);
        assert_eq!(
            provider.last_model().map(|m| m.display()),
            Some("chat-mock/mistral".into())
        );
        assert_eq!(
            response.provider_model.as_ref().map(|m| m.display()),
            Some("chat-mock/mistral".into())
        );
        assert_eq!(
            response.configured_model.as_ref().map(|m| m.display()),
            Some("chat-mock/mistral".into())
        );
        assert!(!response.model_fallback);
    }

    #[test]
    fn explicit_preferred_model_populates_reasoning_request() {
        let provider = Arc::new(
            CountingReasoningProvider::new("chat-mock").with_models(vec!["llama", "mistral"]),
        );
        let registry = jaymi_reasoning::ModelRegistry::with_provider(
            Arc::clone(&provider) as Arc<dyn jaymi_reasoning::ReasoningProvider>,
        );
        registry.refresh().unwrap();
        registry
            .set_default(Some(ModelIdentifier::new("chat-mock", "llama")))
            .unwrap();
        let planner =
            planner_with_reasoning_and_registry(provider.clone(), Arc::new(registry));
        planner
            .set_preferred_model(Some(ModelIdentifier::new("chat-mock", "mistral")))
            .unwrap();
        let response = planner
            .handle(UserRequest::new("hello from explicit model"))
            .unwrap();
        assert_eq!(
            provider.last_model().map(|m| m.display()),
            Some("chat-mock/mistral".into())
        );
        assert_eq!(
            response.configured_model.as_ref().map(|m| m.display()),
            Some("chat-mock/mistral".into())
        );
        assert!(!response.model_fallback);
    }

    #[test]
    fn unavailable_model_falls_back_to_available() {
        struct OfflineProvider {
            inner: CountingReasoningProvider,
        }
        impl ReasoningProvider for OfflineProvider {
            fn id(&self) -> &str {
                self.inner.id()
            }
            fn display_name(&self) -> &str {
                self.inner.display_name()
            }
            fn capabilities(&self) -> ReasoningCapabilities {
                self.inner.capabilities()
            }
            fn health(&self) -> ReasoningHealth {
                ReasoningHealth::Unavailable {
                    reason: "offline".into(),
                }
            }
            fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
                self.inner.list_models()
            }
            fn complete(
                &self,
                request: jaymi_reasoning::ReasoningRequest,
            ) -> ReasoningResult<ReasoningResponse> {
                self.inner.complete(request)
            }
            fn stream(
                &self,
                request: jaymi_reasoning::ReasoningRequest,
            ) -> ReasoningResult<Box<dyn ReasoningStream>> {
                self.inner.stream(request)
            }
        }
        let offline = Arc::new(OfflineProvider {
            inner: CountingReasoningProvider::new("down").with_models(vec!["dead"]),
        });
        let up = Arc::new(CountingReasoningProvider::new("up").with_models(vec!["alive"]));
        let mut registry = jaymi_reasoning::ModelRegistry::with_provider(
            Arc::clone(&offline) as Arc<dyn jaymi_reasoning::ReasoningProvider>,
        );
        registry.register_provider(Arc::clone(&up) as Arc<dyn jaymi_reasoning::ReasoningProvider>);
        registry.refresh().unwrap();
        registry
            .set_default(Some(ModelIdentifier::new("down", "dead")))
            .unwrap();
        let planner = planner_with_reasoning_and_registry(up.clone(), Arc::new(registry));
        let response = planner
            .handle(UserRequest::new("fallback please"))
            .unwrap();
        assert!(response.model_fallback);
        assert_eq!(
            response.provider_model.as_ref().map(|m| m.display()),
            Some("up/alive".into())
        );
        assert_eq!(
            up.last_model().map(|m| m.display()),
            Some("up/alive".into())
        );
    }

    #[test]
    fn missing_explicit_model_falls_back() {
        let provider = Arc::new(
            CountingReasoningProvider::new("chat-mock").with_models(vec!["llama"]),
        );
        let registry = jaymi_reasoning::ModelRegistry::with_provider(
            Arc::clone(&provider) as Arc<dyn jaymi_reasoning::ReasoningProvider>,
        );
        registry.refresh().unwrap();
        let planner =
            planner_with_reasoning_and_registry(provider.clone(), Arc::new(registry));
        planner
            .set_preferred_model(Some(ModelIdentifier::new("chat-mock", "missing")))
            .unwrap();
        let response = planner
            .handle(UserRequest::new("missing model"))
            .unwrap();
        assert!(response.model_fallback);
        assert_eq!(
            provider.last_model().map(|m| m.display()),
            Some("chat-mock/llama".into())
        );
    }

    #[test]
    fn conversational_stream_updates_incrementally() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let mut tokens = Vec::new();
        let mut lifecycles = Vec::new();
        let response = planner
            .handle_conversational_with_observer(
                UserRequest::new("Explain ownership briefly."),
                Vec::new(),
                |event| match event {
                    jaymi_reasoning::ConversationStreamEvent::Token(text) => tokens.push(text),
                    jaymi_reasoning::ConversationStreamEvent::Lifecycle(lifecycle) => {
                        lifecycles.push(lifecycle);
                    }
                    _ => {}
                },
            )
            .unwrap();
        assert!(response.reasoning_used);
        assert!(!tokens.is_empty());
        assert!(lifecycles.contains(&jaymi_reasoning::StreamingLifecycle::Thinking));
        assert!(lifecycles.contains(&jaymi_reasoning::StreamingLifecycle::Streaming));
        assert_eq!(
            response.stream_lifecycle,
            Some(jaymi_reasoning::StreamingLifecycle::Completed)
        );
        assert_eq!(response.conversation_state, ConversationState::Completed);
        assert_eq!(planner.conversation_state(), ConversationState::Completed);
    }

    #[test]
    fn conversation_state_transitions_for_streaming_completion() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider);
        assert_eq!(planner.conversation_state(), ConversationState::Idle);
        let mut seen = Vec::new();
        let response = planner
            .handle_conversational_with_observer(UserRequest::new("hello there"), Vec::new(), |event| {
                if let jaymi_reasoning::ConversationStreamEvent::Lifecycle(_) = &event {
                    seen.push(planner.conversation_state());
                }
            })
            .unwrap();
        assert!(seen.contains(&ConversationState::Reasoning) || seen.contains(&ConversationState::Streaming));
        assert_eq!(response.conversation_state, ConversationState::Completed);
        assert!(!response.awaiting_review);
    }

    #[test]
    fn single_turn_history_is_empty_on_first_message() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let _ = planner
            .handle_conversational_with_observer(
                UserRequest::new("first message"),
                Vec::new(),
                |_| {},
            )
            .unwrap();
        assert_eq!(provider.last_history_len(), 0);
    }

    #[test]
    fn multi_turn_history_reaches_reasoning_request() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let history = vec![
            jaymi_reasoning::ConversationTurn::user("earlier"),
            jaymi_reasoning::ConversationTurn::assistant("prior reply"),
        ];
        let response = planner
            .handle_conversational_with_observer(
                UserRequest::new("follow up"),
                history,
                |_| {},
            )
            .unwrap();
        assert!(response.reasoning_used);
        assert_eq!(provider.last_history_len(), 2);
        assert_eq!(
            *provider
                .last_prompt_has_prior_user
                .lock()
                .expect("lock"),
            Some(true)
        );
        let diagnostics = response.prompt_diagnostics.expect("prompt diagnostics");
        let conversation = diagnostics
            .sections
            .iter()
            .find(|section| section.id == jaymi_reasoning::PromptSectionId::Conversation)
            .expect("conversation section");
        assert!(conversation.included);
        assert!(conversation
            .note
            .as_deref()
            .is_none_or(|note| !note.contains("no conversation")));
    }

    #[test]
    fn conversation_continuity_keeps_prior_turns_across_stream_start() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let history = vec![
            jaymi_reasoning::ConversationTurn::system("Stay concise."),
            jaymi_reasoning::ConversationTurn::user("earlier"),
            jaymi_reasoning::ConversationTurn::assistant("ack"),
        ];
        let (bundle, stream, prompt_diagnostics, _pipeline) = planner
            .start_conversation_stream(&UserRequest::new("next question"), history)
            .unwrap();
        assert!(bundle.assemble_generation() >= 1);
        assert!(prompt_diagnostics
            .sections
            .iter()
            .any(|section| section.id == jaymi_reasoning::PromptSectionId::Conversation
                && section.included));
        let response = stream.collect().unwrap();
        assert!(response.content.contains("next question"));
        assert_eq!(provider.last_history_len(), 3);
    }

    /// B1.13.8 — blocking observer and pumpable complete share terminal mapping.
    #[test]
    fn pumpable_and_blocking_share_terminal_core_fields() {
        let goal = "Explain ownership briefly.";
        let blocking_provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let blocking_planner = planner_with_reasoning(blocking_provider.clone());
        let blocking = blocking_planner
            .handle_conversational_with_observer(UserRequest::new(goal), Vec::new(), |_| {})
            .unwrap();

        let pump_provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let pump_planner = planner_with_reasoning(pump_provider.clone());
        let (context, mut stream, prompt_diagnostics, early_pipeline) = pump_planner
            .start_conversation_stream(&UserRequest::new(goal), Vec::new())
            .unwrap();
        let mut terminal = None;
        for _ in 0..64 {
            match stream.pump().unwrap() {
                Some(event) if event.is_terminal() => {
                    terminal = Some(event);
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        let pumpable = pump_planner
            .complete_conversation_stream(
                context,
                terminal.expect("pumpable stream must terminate"),
                Some(prompt_diagnostics),
                Some(early_pipeline),
            )
            .unwrap();

        assert_eq!(blocking.reasoning_used, pumpable.reasoning_used);
        assert_eq!(blocking.stream_lifecycle, pumpable.stream_lifecycle);
        assert_eq!(blocking.conversation_state, pumpable.conversation_state);
        assert_eq!(
            blocking.reasoning_provider_id,
            pumpable.reasoning_provider_id
        );
        assert_eq!(
            blocking.stream_lifecycle,
            Some(jaymi_reasoning::StreamingLifecycle::Completed)
        );
        assert_eq!(blocking.conversation_state, ConversationState::Completed);
        assert!(blocking.content.contains(goal));
        assert!(pumpable.content.contains(goal));
        let blocking_diag = blocking.prompt_diagnostics.expect("blocking diagnostics");
        let pump_diag = pumpable.prompt_diagnostics.expect("pumpable diagnostics");
        assert_eq!(blocking_diag.conversation_turns, pump_diag.conversation_turns);
        assert!(!blocking_diag.sections.is_empty());
        assert_eq!(blocking_diag.sections.len(), pump_diag.sections.len());
    }

    #[test]
    fn dual_delivery_retains_prompt_diagnostics_on_both_paths() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider);
        let blocking = planner
            .handle_conversational_with_observer(
                UserRequest::new("diagnostics check"),
                Vec::new(),
                |_| {},
            )
            .unwrap();
        assert!(blocking.prompt_diagnostics.is_some());
        assert!(blocking.reasoning_metrics.is_some());

        let planner = planner_with_reasoning(Arc::new(CountingReasoningProvider::new("chat-mock")));
        let (context, stream, diagnostics, early_pipeline) = planner
            .start_conversation_stream(&UserRequest::new("diagnostics check"), Vec::new())
            .unwrap();
        let collected = stream.collect().unwrap();
        let pumpable = planner
            .complete_conversation_stream(
                context,
                jaymi_reasoning::ConversationStreamEvent::Completed(collected),
                Some(diagnostics),
                Some(early_pipeline),
            )
            .unwrap();
        assert!(pumpable.prompt_diagnostics.is_some());
        assert!(pumpable.reasoning_metrics.is_some());
        assert_eq!(
            pumpable.stream_lifecycle,
            Some(jaymi_reasoning::StreamingLifecycle::Completed)
        );
        let timing = pumpable
            .pipeline_timing
            .expect("pipeline timing retained on pumpable path");
        assert!(
            timing
                .stages
                .iter()
                .any(|stage| stage.stage == "planner"),
            "expected planner stage: {:?}",
            timing.stages
        );
        assert!(
            timing
                .stages
                .iter()
                .any(|stage| stage.stage == "context_assembly"),
            "expected context_assembly stage: {:?}",
            timing.stages
        );
        assert!(
            timing
                .stages
                .iter()
                .any(|stage| stage.stage == "prompt_builder"),
            "expected prompt_builder stage: {:?}",
            timing.stages
        );
        assert!(
            timing
                .stages
                .iter()
                .any(|stage| stage.stage == "provider_transport"),
            "expected provider_transport stage: {:?}",
            timing.stages
        );
    }

    #[test]
    fn pipeline_timing_present_on_blocking_conversational_path() {
        let planner = planner_with_reasoning(Arc::new(CountingReasoningProvider::new("chat-mock")));
        let response = planner
            .handle_conversational_with_observer(
                UserRequest::new("time the pipeline"),
                Vec::new(),
                |_| {},
            )
            .unwrap();
        let timing = response
            .pipeline_timing
            .expect("pipeline timing on blocking path");
        assert!(timing.stages.iter().any(|s| s.stage == "planner"));
        assert!(timing.stages.iter().any(|s| s.stage == "context_assembly"));
        assert!(timing
            .stages
            .iter()
            .any(|s| s.stage == "prompt_builder" || s.stage == "provider_transport"));
    }

    #[test]
    fn complexity_annotates_hints_without_changing_capabilities() {
        let planner = planner_with_reasoning(Arc::new(CountingReasoningProvider::new("chat-mock")));
        let greeting = planner
            .handle_conversational_with_observer(UserRequest::new("Hello!"), Vec::new(), |_| {})
            .unwrap();
        assert!(greeting.capability.is_none());
        assert!(greeting.reasoning_used);

        let coding = planner
            .handle_conversational_with_observer(
                UserRequest::new("How do I fix this borrow checker error?"),
                Vec::new(),
                |_| {},
            )
            .unwrap();
        // Still conversational — no tool capability invented by complexity.
        assert!(coding.capability.is_none());
        assert!(coding.reasoning_used);

        let assessment = assess_text("How do I fix this borrow checker error?", None);
        assert_eq!(assessment.class, ConversationalComplexity::CodingQuestion);
        let greeting_assessment = assess_text("Hello!", None);
        assert_eq!(greeting_assessment.class, ConversationalComplexity::Greeting);
        // DecisionEngine still returns empty capabilities for Unknown regardless of text.
        assert!(DecisionEngine
            .required_capabilities(&Intent::Unknown)
            .is_empty());
    }

    #[test]
    fn start_conversation_stream_hard_errors_without_backend() {
        let planner = planner_with_tools_policies_routes_and_reasoning(
            |tools, filesystem, content_api| {
                tools
                    .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                    .unwrap();
                tools
                    .register_tool(Arc::new(ReadFileTool::new(content_api)))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
            },
            ToolRouteTable::builtin(),
            None,
        );
        let err = match planner
            .start_conversation_stream(&UserRequest::new("hello"), Vec::new())
        {
            Ok(_) => panic!("pumpable path must hard-error without backend"),
            Err(error) => error,
        };
        assert!(err.message().contains("no reasoning backend"));

        let soft = planner
            .handle_conversational_with_observer(UserRequest::new("hello"), Vec::new(), |_| {})
            .unwrap();
        assert!(!soft.reasoning_used);
        assert_eq!(
            soft.stream_lifecycle,
            Some(jaymi_reasoning::StreamingLifecycle::Idle)
        );
        assert!(soft.content.contains("no reasoning backend"));
    }

    #[test]
    fn long_conversation_history_is_budgeted_not_dropped_silently() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let mut history = Vec::new();
        for index in 0..40 {
            history.push(jaymi_reasoning::ConversationTurn::user(format!(
                "user turn {index} with enough text to pressure the conversation budget section"
            )));
            history.push(jaymi_reasoning::ConversationTurn::assistant(format!(
                "assistant turn {index} continuing the long conversation with more content"
            )));
        }
        let response = planner
            .handle_conversational_with_observer(
                UserRequest::new("summarize our thread"),
                history,
                |_| {},
            )
            .unwrap();
        assert_eq!(provider.last_history_len(), 80);
        let diagnostics = response.prompt_diagnostics.expect("diagnostics");
        let conversation = diagnostics
            .sections
            .iter()
            .find(|section| section.id == jaymi_reasoning::PromptSectionId::Conversation)
            .expect("conversation");
        // Either included fully, truncated, or budgeted — never silently missing.
        assert!(matches!(
            conversation.disposition,
            jaymi_reasoning::PromptSectionDisposition::Included
                | jaymi_reasoning::PromptSectionDisposition::Truncated
                | jaymi_reasoning::PromptSectionDisposition::Budgeted
        ));
    }

    #[test]
    fn conversation_state_waiting_for_review_on_gated_write() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_tools_policies_routes_and_reasoning(
            |tools, filesystem, _| {
                tools
                    .register_tool(Arc::new(jaymi_tools::WriteFileTool::new(filesystem)))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
            },
            ToolRouteTable::builtin(),
            Some(provider),
        );
        let path = temp_dir().join("state.txt");
        let response = planner
            .handle(UserRequest::write_file(&path, "hello"))
            .unwrap();
        if response.awaiting_review {
            assert_eq!(
                response.conversation_state,
                ConversationState::WaitingForReview
            );
            assert_eq!(
                planner.conversation_state(),
                ConversationState::WaitingForReview
            );
            let plan_id = response.execution_plan.expect("plan").id().clone();
            let cancelled = planner
                .resolve_review(ReviewIntent::Cancel { plan_id })
                .unwrap();
            assert_eq!(cancelled.conversation_state, ConversationState::Cancelled);
            assert_eq!(planner.conversation_state(), ConversationState::Cancelled);
        }
    }

    #[test]
    fn conversation_state_tool_path_reaches_completed() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider);
        let dir = temp_dir();
        let response = planner.handle(UserRequest::list_directory(&dir)).unwrap();
        assert!(!response.reasoning_used);
        assert_eq!(response.conversation_state, ConversationState::Completed);
        assert_eq!(planner.conversation_state(), ConversationState::Completed);
    }

    #[test]
    fn conversation_state_rejects_illegal_transition() {
        let planner = planner_with_search_and_read();
        assert_eq!(planner.conversation_state(), ConversationState::Idle);
        let err = planner
            .try_transition_conversation(ConversationState::Streaming)
            .unwrap_err();
        assert_eq!(err.from, ConversationState::Idle);
        assert_eq!(err.to, ConversationState::Streaming);
        assert_eq!(planner.conversation_state(), ConversationState::Idle);
    }

    #[test]
    fn unknown_request_without_backend_is_not_unsupported() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("tell me a joke")).unwrap();
        assert!(!response.reasoning_used);
        assert!(!response.content.contains("Unsupported request"));
        assert!(response.content.contains("no reasoning backend"));
        assert!(response.context_bundle.is_some());
    }

    #[test]
    fn tool_request_does_not_invoke_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let dir = temp_dir();
        let response = planner.handle(UserRequest::list_directory(&dir)).unwrap();
        assert!(!response.reasoning_used);
        assert!(response.reasoning_provider_id.is_none());
        assert!(response.capability.is_some());
        assert!(response.tool_id.is_some());
        assert_eq!(provider.complete_calls(), 0);
        assert_eq!(provider.stream_calls(), 0);
    }

    #[test]
    fn planning_request_does_not_invoke_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(provider.clone());
        let response = planner
            .handle(UserRequest::new("Help me build an app."))
            .unwrap();
        assert!(!response.reasoning_used);
        assert!(response.capability_plan.is_some());
        assert!(response.tool_id.is_none());
        assert_eq!(provider.complete_calls(), 0);
    }

    #[test]
    fn execution_review_path_does_not_invoke_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        // Write requires approval → execution plan / review, not reasoning.
        let planner = planner_with_tools_policies_routes_and_reasoning(
            |tools, filesystem, _| {
                tools
                    .register_tool(Arc::new(jaymi_tools::WriteFileTool::new(filesystem)))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
            },
            ToolRouteTable::builtin(),
            Some(provider.clone()),
        );
        let path = temp_dir().join("exec.txt");
        let response = planner
            .handle(UserRequest::write_file(&path, "hello"))
            .unwrap();
        assert!(response.awaiting_review || response.execution_plan.is_some() || response.blocked);
        assert!(!response.reasoning_used);
        assert_eq!(provider.complete_calls(), 0);
        if response.awaiting_review {
            let plan_id = response.execution_plan.expect("plan").id().clone();
            let resumed = planner
                .resolve_review(ReviewIntent::Approve { plan_id })
                .unwrap();
            assert!(!resumed.reasoning_used);
            assert_eq!(provider.complete_calls(), 0);
        }
    }

    #[test]
    fn offline_first_requires_approval_for_cloud_only_tool() {
        let planner = planner_with_tools(|tools, _, _| {
            tools
                .register_tool(Arc::new(CloudSearchTool::new()))
                .unwrap();
        });
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(response.blocked);
        assert!(response.awaiting_review);
        assert!(response.entries.is_empty());
        assert_eq!(response.tool_id.as_deref(), Some("cloud_search"));
        assert_eq!(
            response.policy_evaluation.as_ref().unwrap().decision,
            PolicyDecision::RequiresApproval
        );
        assert!(response.permission_result.is_some());
        assert!(response.content.contains("I can do that."));
        assert!(response.content.contains("You can:"));
        assert!(response.content.contains("Offline First"));
    }

    #[test]
    fn approval_flow_resumes_write_after_review() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("approved.txt");
        let response = planner
            .handle(UserRequest::write_file(&path, "hello"))
            .expect("handle write");
        assert!(response.awaiting_review);
        assert!(!path.exists());
        assert_eq!(
            response.permission_result.as_ref().unwrap().decision,
            PermissionDecision::RequiresApproval
        );
        let plan_id = response.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .expect("approve");
        assert!(!resumed.awaiting_review);
        assert!(!resumed.blocked);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn denied_flow_explains_and_does_not_execute() {
        // Internet permission is Denied — use a cloud tool with only Offline First
        // removed and Privacy Maximum so policy also denies for a clear deny path.
        let planner = planner_with_tools_and_policies(
            |tools, _, _| {
                tools
                    .register_tool(Arc::new(CloudSearchTool::new()))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
                policies.active.clear();
                policies.active.push(jaymi_policies::Policy {
                    name: "Privacy Maximum".into(),
                    scope: jaymi_policies::PolicyScope::Global,
                    builtin: Some(jaymi_policies::BuiltinPolicy::PrivacyMaximum),
                });
            },
        );
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(response.blocked);
        assert!(!response.awaiting_review);
        assert!(response.entries.is_empty());
        assert_eq!(
            response.policy_evaluation.as_ref().unwrap().decision,
            PolicyDecision::Denied
        );
        assert!(response.permission_result.is_none());
        assert!(response.content.contains("Denied"));
        assert!(response.content.contains("Privacy Maximum"));
        assert_eq!(planner.paused_count().unwrap(), 0);
    }

    #[test]
    fn policy_override_privacy_maximum_denies_despite_offline_first_approval_path() {
        let planner = planner_with_tools_and_policies(
            |tools, _, _| {
                tools
                    .register_tool(Arc::new(CloudSearchTool::new()))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
                // Offline First is already present from initialize; add Privacy Maximum.
                policies.active.push(jaymi_policies::Policy {
                    name: "Privacy Maximum".into(),
                    scope: jaymi_policies::PolicyScope::Global,
                    builtin: Some(jaymi_policies::BuiltinPolicy::PrivacyMaximum),
                });
            },
        );
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(response.blocked);
        assert!(!response.awaiting_review);
        assert_eq!(
            response.policy_evaluation.as_ref().unwrap().decision,
            PolicyDecision::Denied
        );
        assert!(
            response
                .policy_evaluation
                .as_ref()
                .unwrap()
                .explanation()
                .contains("Privacy Maximum"),
            "policy explanation missing: {:?}",
            response.policy_evaluation
        );
        assert!(response.content.contains("Privacy Maximum"));
    }

    #[test]
    fn policy_explanation_surfaces_in_review_and_deny_responses() {
        let approval_planner = planner_with_tools(|tools, _, _| {
            tools
                .register_tool(Arc::new(CloudSearchTool::new()))
                .unwrap();
        });
        let awaiting = approval_planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(awaiting.awaiting_review);
        let summary = awaiting
            .execution_summary
            .as_ref()
            .and_then(|summary| summary.error.clone())
            .unwrap_or_default();
        assert!(
            summary.contains("Offline First") || awaiting.content.contains("Offline First"),
            "expected Offline First explanation, content={} summary={}",
            awaiting.content,
            summary
        );

        let deny_planner = planner_with_tools_and_policies(
            |tools, _, _| {
                tools
                    .register_tool(Arc::new(CloudSearchTool::new()))
                    .unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
                policies.active.clear();
                policies.active.push(jaymi_policies::Policy {
                    name: "Privacy Maximum".into(),
                    scope: jaymi_policies::PolicyScope::Global,
                    builtin: Some(jaymi_policies::BuiltinPolicy::PrivacyMaximum),
                });
            },
        );
        let denied = deny_planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(denied.content.starts_with("Denied"));
        assert!(denied
            .execution_summary
            .as_ref()
            .and_then(|summary| summary.error.as_ref())
            .is_some_and(|message| message.contains("Privacy Maximum")));
    }

    #[test]
    fn coding_goal_produces_a_plan_without_executing_tools() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Help me build an app."))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Code));
        assert!(response.tool_id.is_none());
        assert!(!response.blocked);
        assert!(response.content.contains("Execution plan"));

        let plan = response.capability_plan.expect("capability plan");
        assert_eq!(plan.goal.as_deref(), Some("Help me build an app."));
        assert_eq!(plan.steps.len(), 1);
        assert!(!plan.is_executable());
        assert!(plan
            .required_permissions()
            .iter()
            .any(|permission| permission.label() == "terminal:execute"));
    }

    #[test]
    fn composed_goal_plans_multiple_independent_capabilities() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("research then code then create"))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert!(response.tool_id.is_none());
        let plan = response.capability_plan.expect("composed plan");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(
            plan.capabilities(),
            vec![
                Capability::Search,
                Capability::Code,
                Capability::GenerateImages
            ]
        );
        assert!(response
            .content
            .contains("Composed 3 independent capabilities"));
    }

    #[test]
    fn planning_does_not_require_a_fulfillable_capability() {
        let planner = planner_with_search_and_read();
        let plan = planner
            .plan_capability(Capability::Code, Some("ship a feature"))
            .unwrap();
        assert_eq!(plan.steps[0].capability, Capability::Code);
        assert!(!plan.steps[0].tools_resolved);
        assert_eq!(
            plan.steps[0].required_tools,
            vec![
                "editor".to_string(),
                "language_server".to_string(),
                "terminal".to_string(),
                "git".to_string()
            ]
        );
    }

    fn planner_with_write() -> Planner {
        planner_with_tools(|tools, filesystem, _content| {
            tools
                .register_tool(Arc::new(jaymi_tools::WriteFileTool::new(Arc::clone(
                    &filesystem,
                ))))
                .unwrap();
        })
    }

    fn planner_with_write_and_manage() -> Planner {
        planner_with_tools(|tools, filesystem, _content| {
            tools
                .register_tool(Arc::new(jaymi_tools::WriteFileTool::new(Arc::clone(
                    &filesystem,
                ))))
                .unwrap();
            tools
                .register_tool(Arc::new(jaymi_tools::ManagePathTool::new(Arc::clone(
                    &filesystem,
                ))))
                .unwrap();
        })
    }

    fn planner_with_manage() -> (Planner, Arc<FilesystemProvider>) {
        let filesystem_slot = Arc::new(std::sync::Mutex::new(None));
        let slot = Arc::clone(&filesystem_slot);
        let planner = planner_with_tools(move |tools, filesystem, _content| {
            *slot.lock().unwrap() = Some(Arc::clone(&filesystem));
            tools
                .register_tool(Arc::new(jaymi_tools::ManagePathTool::new(Arc::clone(
                    &filesystem,
                ))))
                .unwrap();
        });
        let filesystem = filesystem_slot.lock().unwrap().clone().expect("filesystem");
        (planner, filesystem)
    }

    #[test]
    fn trash_delete() {
        let (planner, filesystem) = planner_with_manage();
        if !filesystem.supports_trash() {
            return;
        }
        let dir = temp_dir();
        let path = dir.join("trash-plan.txt");
        fs::write(&path, b"recoverable").unwrap();

        let response = planner
            .handle(UserRequest::manage_delete(&path))
            .expect("delete");
        assert!(response.awaiting_review);
        let plan = response.execution_plan.as_ref().expect("plan");
        assert_eq!(plan.deletion_method(), Some(DeletionMethod::Trash));
        let review = ReviewCardModel::from_plan(plan, None);
        assert!(review
            .approval_notice
            .contains("moves the selected files to the Trash"));
        assert!(review.plan_items.iter().any(|item| item.contains("Trash")));

        let plan_id = plan.id().clone();
        match planner.resolve_review(ReviewIntent::Approve { plan_id }) {
            Ok(resumed) => {
                assert!(!path.exists());
                let summary = resumed.execution_summary.expect("summary");
                assert_eq!(summary.deletion_method, Some(DeletionMethod::Trash));
                assert!(!summary.files_moved_to_trash.is_empty());
                assert_eq!(summary.recovery_available, Some(true));
            }
            Err(error)
                if error.message().to_ascii_lowercase().contains("trash")
                    || error.message().to_ascii_lowercase().contains("finder") => {}
            Err(error) => panic!("unexpected approve failure: {error}"),
        }
    }

    #[test]
    fn permanent_delete() {
        let (planner, _) = planner_with_manage();
        let dir = temp_dir();
        let path = dir.join("perm-plan.txt");
        fs::write(&path, b"gone").unwrap();

        let response = planner
            .handle(UserRequest::manage_delete_permanent(&path))
            .expect("permanent delete");
        assert!(response.awaiting_review);
        let plan = response.execution_plan.as_ref().expect("plan");
        assert_eq!(plan.deletion_method(), Some(DeletionMethod::Permanent));
        let review = ReviewCardModel::from_plan(plan, None);
        assert!(review
            .approval_notice
            .contains("permanently deletes these files"));

        let plan_id = plan.id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .expect("approve");
        assert!(!path.exists());
        let summary = resumed.execution_summary.expect("summary");
        assert_eq!(summary.deletion_method, Some(DeletionMethod::Permanent));
        assert!(!summary.files_permanently_deleted.is_empty());
        assert_eq!(summary.recovery_available, Some(false));
    }

    #[test]
    fn trash_unavailable() {
        let (planner, filesystem) = planner_with_manage();
        filesystem.set_trash_available(false);
        let dir = temp_dir();
        let path = dir.join("no-trash-plan.txt");
        fs::write(&path, b"x").unwrap();

        let response = planner
            .handle(UserRequest::manage_delete(&path))
            .expect("delete");
        let plan = response.execution_plan.expect("plan");
        assert_eq!(plan.deletion_method(), Some(DeletionMethod::Permanent));
        let review = ReviewCardModel::from_plan(&plan, None);
        assert!(review
            .approval_notice
            .contains("permanently deletes these files"));
    }

    #[test]
    fn execution_summary_delete() {
        let (planner, filesystem) = planner_with_manage();
        let dir = temp_dir();
        let path = dir.join("summary-delete.txt");
        fs::write(&path, b"x").unwrap();

        let method = if filesystem.supports_trash() {
            DeletionMethod::Trash
        } else {
            DeletionMethod::Permanent
        };
        let request = if method == DeletionMethod::Permanent {
            UserRequest::manage_delete_permanent(&path)
        } else {
            UserRequest::manage_delete(&path)
        };
        let response = planner.handle(request).expect("delete");
        let plan_id = response.execution_plan.expect("plan").id().clone();
        let resumed = match planner.resolve_review(ReviewIntent::Approve { plan_id }) {
            Ok(resumed) => resumed,
            Err(error)
                if error.message().to_ascii_lowercase().contains("finder")
                    || error.message().to_ascii_lowercase().contains("trash") =>
            {
                return;
            }
            Err(error) => panic!("unexpected approve failure: {error}"),
        };
        let summary = resumed.execution_summary.expect("summary");
        let rendered = summary.render_conversation();
        assert!(rendered.contains("Deletion method:"));
        assert!(
            rendered.contains("Moved to Trash:")
                || rendered.contains("Permanently deleted:")
        );
        assert!(rendered.contains("Recovery available:"));
        assert_eq!(summary.deletion_method, Some(method));
    }

    #[test]
    fn preview_generation() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("preview.txt");
        fs::write(&path, "old\n").unwrap();
        let response = planner
            .handle(UserRequest::write_file(&path, "old\nnew\n"))
            .expect("write");
        assert!(response.awaiting_review);
        let plan = response.execution_plan.expect("plan");
        let preview = plan.action_preview().expect("preview");
        assert_eq!(preview.kind, jaymi_core::PreviewKind::UnifiedDiff);
        assert!(preview.body.as_ref().is_some_and(|body| body.contains("+new")));
        let card = ReviewCardModel::from_plan(&plan, None);
        assert!(card.action_preview.is_some());
        assert!(card.render_text().contains("Preview"));
        // Preview must not have mutated the file.
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
    }

    #[test]
    fn large_preview() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("large.txt");
        let before: String = (0..80).map(|i| format!("line {i}\n")).collect();
        let after: String = (0..120).map(|i| format!("changed {i}\n")).collect();
        fs::write(&path, &before).unwrap();
        let response = planner
            .handle(UserRequest::write_file(&path, after))
            .expect("write");
        let plan = response.execution_plan.expect("plan");
        let preview = plan.action_preview().expect("preview");
        let card = ReviewCardModel::from_plan(&plan, None);
        let truncated = card.render_text_with_preview(false);
        let expanded = card.render_text_with_preview(true);
        assert!(truncated.contains("Preview"));
        assert!(
            preview.truncated
                || truncated.contains("truncated")
                || truncated.lines().count() <= expanded.lines().count()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    fn paused_write(planner: &Planner, path: &Path, contents: &str) -> ExecutionPlan {
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: format!("Write {}", path.display()),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec![jaymi_tools::WRITE_FILE_TOOL_ID.to_string()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Write file".into(),
                tool_id: Some(jaymi_tools::WRITE_FILE_TOOL_ID.to_string()),
                resource: Some(path.display().to_string()),
            }],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec![path.display().to_string()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["written file".into()],
        deletion_method: None,
        action_preview: None,
        lineage: Default::default(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        planner
            .pause_execution(PausedExecution {
                tool_id: jaymi_tools::WRITE_FILE_TOOL_ID.to_string(),
                provider_id: Some(FILESYSTEM_PROVIDER_ID.to_string()),
                capability: Capability::FileManagement,
                tool_input: ToolInput::write_file(path, contents),
                plan: plan.clone(),
                policy_evaluation: None,
                permission_result: None,
                paused_at: Instant::now(),
            })
            .unwrap();
        plan
    }

    #[test]
    fn pause() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("pause.txt");
        let plan = paused_write(&planner, &path, "paused");
        assert!(planner.is_paused(plan.id()).unwrap());
        assert_eq!(planner.paused_count().unwrap(), 1);
        assert_eq!(plan.status(), ExecutionStatus::AwaitingReview);
        assert!(!path.exists());
    }

    #[test]
    fn resume() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("resume.txt");
        let plan = paused_write(&planner, &path, "resumed-body");
        let plan_id = plan.id().clone();

        let response = planner
            .resolve_review(ReviewIntent::Approve {
                plan_id: plan_id.clone(),
            })
            .expect("resume");

        assert!(!response.awaiting_review);
        assert!(!response.blocked);
        let completed = response.execution_plan.expect("plan");
        assert_eq!(completed.id(), &plan_id);
        assert_eq!(completed.status(), ExecutionStatus::Completed);
        assert_eq!(completed.originating_request(), plan.originating_request());
        assert_eq!(planner.paused_count().unwrap(), 0);
        assert_eq!(fs::read_to_string(&path).unwrap(), "resumed-body");

        let history = planner.approval_history().unwrap();
        assert!(history.iter().any(|entry| entry.decision == ApprovalDecision::Approve));
    }

    #[test]
    fn paused_snapshots_explain_why_execution_is_paused() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("diag-pause.txt");
        let plan = paused_write(&planner, &path, "body");

        let snaps = planner.paused_snapshots().unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].plan_id, plan.id().as_str());
        assert!(snaps[0].pause_explanation.contains("PAUSED"));
        assert!(snaps[0].resume_explanation.contains("Approve resumes"));
        assert_eq!(snaps[0].tool_id, jaymi_tools::WRITE_FILE_TOOL_ID);
    }

    #[test]
    fn cancel() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("cancel.txt");
        let plan = paused_write(&planner, &path, "nope");
        let plan_id = plan.id().clone();

        let response = planner
            .resolve_review(ReviewIntent::Cancel {
                plan_id: plan_id.clone(),
            })
            .expect("cancel");

        assert!(response.content.contains("Cancelled"));
        assert_eq!(
            response.execution_plan.as_ref().unwrap().status(),
            ExecutionStatus::Cancelled
        );
        assert_eq!(planner.paused_count().unwrap(), 0);
        assert!(!path.exists());
    }

    #[test]
    fn modify() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("modify.txt");
        let next = dir.join("modify-new.txt");
        let plan = paused_write(&planner, &path, "old");
        let old_id = plan.id().clone();

        let response = planner
            .resolve_review(ReviewIntent::Modify {
                plan_id: old_id.clone(),
                note: Some(format!("use {}", next.display())),
            })
            .expect("modify");

        assert!(response.awaiting_review);
        assert!(response.content.contains("I revised the plan"));
        assert!(response.content.contains("Changes in revision"));
        assert!(response.content.contains("You can:"));
        let child = response.execution_plan.expect("child plan");
        assert_ne!(child.id(), &old_id);
        assert_eq!(child.status(), ExecutionStatus::AwaitingReview);
        assert_eq!(child.revision(), 2);
        assert_eq!(child.parent_plan_id(), Some(&old_id));
        assert!(child
            .revision_changes()
            .iter()
            .any(|change| change.contains("Retargeted") || change.contains("resource")));
        assert!(planner.is_paused(child.id()).unwrap());
        assert!(!planner.is_paused(&old_id).unwrap());
        assert!(!path.exists());

        let history = planner.plan_history().unwrap();
        assert!(history.iter().any(|entry| &entry.plan_id == child.id()));
        assert!(history
            .iter()
            .any(|entry| &entry.plan_id == child.id() && entry.parent_plan_id.as_ref() == Some(&old_id)));
    }

    #[test]
    fn partial_modification_skips_readme() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let readme = dir.join("README.md");
        let notes = dir.join("notes.txt");
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Write project files".into(),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec![jaymi_tools::WRITE_FILE_TOOL_ID.to_string()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Write files".into(),
                tool_id: Some(jaymi_tools::WRITE_FILE_TOOL_ID.to_string()),
                resource: Some(readme.display().to_string()),
            }],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec![
                readme.display().to_string(),
                notes.display().to_string(),
            ],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["written file".into()],
            deletion_method: None,
            action_preview: None,
            lineage: PlanLineage::root(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        let parent_id = plan.id().clone();
        let mut input = ToolInput::write_file(&readme, "docs");
        input.paths = vec![readme.clone(), notes.clone()];
        planner
            .pause_execution(PausedExecution {
                tool_id: jaymi_tools::WRITE_FILE_TOOL_ID.to_string(),
                provider_id: Some(FILESYSTEM_PROVIDER_ID.to_string()),
                capability: Capability::FileManagement,
                tool_input: input,
                plan: plan.clone(),
                policy_evaluation: None,
                permission_result: None,
                paused_at: Instant::now(),
            })
            .unwrap();

        let response = planner
            .resolve_review(ReviewIntent::Modify {
                plan_id: parent_id.clone(),
                note: Some("Skip README".into()),
            })
            .expect("partial modify");

        assert!(response.awaiting_review);
        let child = response.execution_plan.expect("child");
        assert_eq!(child.revision(), 2);
        assert!(child
            .revision_changes()
            .iter()
            .any(|change| change.contains("README")));
        assert!(!child
            .affected_resources()
            .iter()
            .any(|resource| resource.to_lowercase().contains("readme")));
        assert!(planner.is_paused(child.id()).unwrap());
    }

    #[test]
    fn full_modification_rename_instead_of_overwrite() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("overwrite.txt");
        let plan = paused_write(&planner, &path, "body");
        let parent_id = plan.id().clone();

        let response = planner
            .resolve_review(ReviewIntent::Modify {
                plan_id: parent_id.clone(),
                note: Some("Rename instead of overwrite".into()),
            })
            .expect("full modify");

        assert!(response.awaiting_review);
        assert_eq!(
            response.tool_id.as_deref(),
            Some(jaymi_tools::MANAGE_PATH_TOOL_ID)
        );
        let child = response.execution_plan.expect("child");
        assert_eq!(child.revision(), 2);
        assert_eq!(child.parent_plan_id(), Some(&parent_id));
        assert!(child
            .revision_changes()
            .iter()
            .any(|change| change.to_lowercase().contains("rename")));
        assert_eq!(
            child.proposed_tools(),
            &[jaymi_tools::MANAGE_PATH_TOOL_ID.to_string()]
        );
        assert!(planner.is_paused(child.id()).unwrap());
    }

    #[test]
    fn approval_after_modification() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let original = dir.join("before.txt");
        let revised = dir.join("after.txt");
        let plan = paused_write(&planner, &original, "approved-after-modify");
        let parent_id = plan.id().clone();

        let modified = planner
            .resolve_review(ReviewIntent::Modify {
                plan_id: parent_id.clone(),
                note: Some(format!("use {}", revised.display())),
            })
            .expect("modify");
        assert!(modified.awaiting_review);
        let child = modified.execution_plan.expect("child");
        let child_id = child.id().clone();
        assert_eq!(child.revision(), 2);

        let approved = planner
            .resolve_review(ReviewIntent::Approve {
                plan_id: child_id.clone(),
            })
            .expect("approve revised");

        assert!(!approved.awaiting_review);
        assert!(!approved.blocked);
        let completed = approved.execution_plan.expect("completed");
        assert_eq!(completed.id(), &child_id);
        assert_eq!(completed.status(), ExecutionStatus::Completed);
        assert_eq!(planner.paused_count().unwrap(), 0);
        assert!(!original.exists());
        assert_eq!(
            fs::read_to_string(&revised).unwrap(),
            "approved-after-modify"
        );
    }

    #[test]
    fn approval_history_records_searchable_decisions() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("history.txt");
        let plan = paused_write(&planner, &path, "body");
        let plan_id = plan.id().clone();

        planner
            .resolve_review(ReviewIntent::Approve {
                plan_id: plan_id.clone(),
            })
            .expect("approve");

        let history = planner.approval_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].decision, ApprovalDecision::Approve);
        assert_eq!(history[0].plan_id, plan_id);
        assert!(history[0].execution_result.is_some());
        assert_eq!(
            history[0].execution_result.as_ref().unwrap().status,
            "completed"
        );

        let found = planner
            .search_approval_history(&ApprovalHistoryQuery {
                decision: Some(ApprovalDecision::Approve),
                plan_id: Some(plan_id.clone()),
                text: Some("completed".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(found.len(), 1);

        let views = planner
            .search_approval_history_views(
                &ApprovalHistoryQuery {
                    plan_id: Some(plan_id),
                    ..Default::default()
                },
                ApprovalHistoryAccess::Restricted,
            )
            .unwrap();
        assert_eq!(views.len(), 1);
        assert!(views[0].redacted);
        assert!(views[0].affected_resources.is_empty());
        assert!(views[0].goal.is_none());
    }

    #[test]
    fn approval_history_records_modify_and_cancel() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("mod-hist.txt");
        let next = dir.join("mod-hist-next.txt");
        let plan = paused_write(&planner, &path, "x");
        let parent_id = plan.id().clone();

        let modified = planner
            .resolve_review(ReviewIntent::Modify {
                plan_id: parent_id.clone(),
                note: Some(format!("use {}", next.display())),
            })
            .expect("modify");
        let child_id = modified.execution_plan.unwrap().id().clone();

        planner
            .resolve_review(ReviewIntent::Cancel {
                plan_id: child_id.clone(),
            })
            .expect("cancel");

        let history = planner.approval_history().unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].decision, ApprovalDecision::Modify);
        assert_eq!(history[0].modified_plan_id.as_ref(), Some(&child_id));
        assert!(history[0]
            .reason
            .as_ref()
            .unwrap()
            .contains(next.file_name().unwrap().to_str().unwrap()));
        assert_eq!(history[1].decision, ApprovalDecision::Cancel);
        assert_eq!(history[1].plan_id, child_id);

        let by_text = planner
            .search_approval_history(&ApprovalHistoryQuery {
                text: Some("cancel".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(by_text.iter().any(|entry| entry.decision == ApprovalDecision::Cancel));
    }

    #[test]
    fn timeout() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("timeout.txt");
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Write timeout".into(),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec![jaymi_tools::WRITE_FILE_TOOL_ID.to_string()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Write file".into(),
                tool_id: Some(jaymi_tools::WRITE_FILE_TOOL_ID.to_string()),
                resource: Some(path.display().to_string()),
            }],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec![path.display().to_string()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["written file".into()],
        deletion_method: None,
        action_preview: None,
        lineage: Default::default(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        let plan_id = plan.id().clone();
        planner.set_pause_ttl(Duration::from_millis(1)).unwrap();
        planner
            .pause_execution(PausedExecution {
                tool_id: jaymi_tools::WRITE_FILE_TOOL_ID.to_string(),
                provider_id: Some(FILESYSTEM_PROVIDER_ID.to_string()),
                capability: Capability::FileManagement,
                tool_input: ToolInput::write_file(&path, "late"),
                plan,
                policy_evaluation: None,
                permission_result: None,
                paused_at: Instant::now() - Duration::from_secs(5),
            })
            .unwrap();

        let response = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .expect("timeout response");
        assert!(response.content.contains("timed out"));
        assert!(response.blocked);
        assert!(!path.exists());
        assert_eq!(planner.paused_count().unwrap(), 0);
    }

    #[test]
    fn duplicate_approval() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("dup.txt");
        let plan = paused_write(&planner, &path, "once");
        let plan_id = plan.id().clone();

        planner
            .resolve_review(ReviewIntent::Approve {
                plan_id: plan_id.clone(),
            })
            .expect("first approve");
        let err = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .expect_err("duplicate");
        assert!(
            err.message().contains("duplicate") || err.message().contains("unknown"),
            "unexpected error: {}",
            err.message()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "once");
    }

    #[test]
    fn modify_tool_risk_requires_review_before_write() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("needs-review.txt");
        let response = planner
            .handle(UserRequest::write_file(&path, "secret"))
            .expect("handle write");
        assert!(response.awaiting_review, "Modify risk must pause for review");
        assert!(response.blocked);
        assert!(!path.exists(), "must not write before approval");
        let plan = response.execution_plan.expect("plan");
        assert_eq!(plan.estimated_risk(), EstimatedRisk::Medium);
        assert_eq!(plan.review_requirement(), ReviewRequirement::Required);
        assert_eq!(planner.paused_count().unwrap(), 1);

        let plan_id = plan.id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .expect("approve");
        assert!(!resumed.awaiting_review);
        assert_eq!(fs::read_to_string(&path).unwrap(), "secret");
    }

    #[test]
    fn workspace_tool_risk_skips_review_for_read() {
        let planner = planner_with_search_and_read();
        let dir = temp_dir();
        let path = dir.join("ok.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "hello").unwrap();
        let response = planner.handle(UserRequest::read_file(&path)).unwrap();
        assert!(!response.awaiting_review);
        assert!(!response.blocked);
        assert_eq!(
            response.execution_plan.as_ref().unwrap().estimated_risk(),
            EstimatedRisk::Low
        );
    }

    #[test]
    fn successful_execution_produces_structured_summary() {
        let dir = temp_dir();
        let mut file = File::create(dir.join("a.txt")).unwrap();
        write!(file, "data").unwrap();
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::list_directory(&dir)).unwrap();
        let summary = response.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Completed);
        assert!(!summary.partial);
        assert!(!summary.goal.is_empty());
        assert!(!summary.actions_performed.is_empty());
        assert!(!summary.resources_changed.is_empty());
        assert!(summary.errors.is_empty());
        assert!(!summary.next_suggested_actions.is_empty());
        assert!(summary.render_conversation().contains("Goal:"));
        assert!(summary.tools_executed.iter().any(|id| id == "search_files"));
    }

    #[test]
    fn partial_execution_summary_marks_partial_and_warnings() {
        let planner = planner_with_tools(|tools, _, _| {
            tools
                .register_tool(Arc::new(PartialSearchTool::new()))
                .unwrap();
        });
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(!response.blocked);
        let summary = response.execution_summary.expect("summary");
        assert!(summary.partial);
        assert_eq!(summary.status, ExecutionStatus::Completed);
        assert!(summary.warnings.iter().any(|w| w.contains("truncated")));
        assert!(summary
            .next_suggested_actions
            .iter()
            .any(|a| a.contains("remainder") || a.contains("partial")));
        assert!(summary.render_conversation().contains("partial"));
    }

    #[test]
    fn failure_execution_summary_includes_errors() {
        let planner = planner_with_tools(|tools, _, _| {
            tools
                .register_tool(Arc::new(FailingSearchTool::new()))
                .unwrap();
        });
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(response.blocked);
        let summary = response.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Failed);
        assert!(!summary.errors.is_empty());
        assert!(summary.error.as_deref().is_some_and(|e| e.contains("boom")));
        assert!(summary.render_conversation().contains("Errors:"));
    }

    #[test]
    fn cancelled_execution_summary_explains_and_suggests_next() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("cancel-summary.txt");
        let response = planner
            .handle(UserRequest::write_file(&path, "nope"))
            .unwrap();
        assert!(response.awaiting_review);
        let plan_id = response.execution_plan.expect("plan").id().clone();
        let cancelled = planner
            .resolve_review(ReviewIntent::Cancel { plan_id })
            .unwrap();
        let summary = cancelled.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Cancelled);
        assert!(summary.tools_executed.is_empty());
        assert!(!summary.errors.is_empty());
        assert!(!summary.next_suggested_actions.is_empty());
        assert!(summary.render_conversation().contains("cancelled"));
        assert!(!path.exists());
    }

    struct PartialSearchTool {
        metadata: ToolMetadata,
    }

    impl PartialSearchTool {
        fn new() -> Self {
            Self {
                metadata: ToolMetadata {
                    id: "search_files".into(),
                    name: "Partial Search".into(),
                    version: "0.1.0".into(),
                    description: "Returns a partial listing".into(),
                    provider: "test".into(),
                    capabilities: vec![Capability::Search],
                    risk: ToolRisk::Safe,
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::Low,
                    memory_usage: MemoryUsage::Small,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::LocalOnly,
                    internet: InternetRequirement::Never,
                    reliability: Reliability::Experimental,
                    result_type: ResultType::SearchResults,
                },
            }
        }
    }

    impl Tool for PartialSearchTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
            let path = input
                .path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| ".".into());
            Ok(ToolOutput {
                success: true,
                message: Some("partial listing".into()),
                listed_path: input.path.clone(),
                metadata: jaymi_tools::ToolExecutionMetadata {
                    actions_performed: vec![format!("Listed part of {path}")],
                    resources_changed: vec![path],
                    warnings: vec!["listing truncated at 1 entry".into()],
                    partial: true,
                    next_suggested_actions: vec![
                        "Review what completed and retry the remainder".into(),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    struct FailingSearchTool {
        metadata: ToolMetadata,
    }

    impl FailingSearchTool {
        fn new() -> Self {
            Self {
                metadata: ToolMetadata {
                    id: "search_files".into(),
                    name: "Failing Search".into(),
                    version: "0.1.0".into(),
                    description: "Always fails".into(),
                    provider: "test".into(),
                    capabilities: vec![Capability::Search],
                    risk: ToolRisk::Safe,
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::Low,
                    memory_usage: MemoryUsage::Small,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::LocalOnly,
                    internet: InternetRequirement::Never,
                    reliability: Reliability::Experimental,
                    result_type: ResultType::SearchResults,
                },
            }
        }
    }

    impl Tool for FailingSearchTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput> {
            Ok(ToolOutput {
                success: false,
                message: Some("boom: simulated tool failure".into()),
                metadata: jaymi_tools::ToolExecutionMetadata {
                    actions_performed: vec!["Attempted directory listing".into()],
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    #[test]
    fn tool_registration() {
        let table = ToolRouteTable::builtin();
        assert_eq!(table.len(), 12, "shipping tool-backed intents must all register");
        assert!(table.contains(IntentId::ListDirectory));
        assert!(table.contains(IntentId::WriteFile));
        assert!(table.contains(IntentId::ManagePath));
        assert!(table.contains(IntentId::RunTerminal));
        assert!(table.contains(IntentId::Git));
        assert!(table.contains(IntentId::Lsp));
        assert!(!table.contains(IntentId::PlanWork));
        assert!(!table.contains(IntentId::Unknown));

        let list = table.get(IntentId::ListDirectory).expect("list route");
        assert_eq!(list.route().tool_id, SEARCH_FILES_TOOL_ID);
        assert_eq!(list.route().capability, Capability::Search);

        let mut custom = ToolRouteTable::new();
        custom.register_handler(ListDirectoryProbe);
        assert_eq!(custom.len(), 1);
        assert_eq!(
            custom.get(IntentId::ListDirectory).unwrap().route().tool_id,
            "probe_list"
        );
    }

    #[test]
    fn planner_dispatch() {
        let planner = planner_with_search_and_read();
        let dir = temp_dir();
        File::create(dir.join("a.txt")).unwrap();
        let response = planner
            .handle(UserRequest::list_directory(&dir))
            .expect("dispatch list");
        assert!(!response.blocked);
        assert_eq!(response.tool_id.as_deref(), Some(SEARCH_FILES_TOOL_ID));
        assert_eq!(response.capability, Some(Capability::Search));
        assert!(response.execution_plan.is_some());
        assert!(!response.entries.is_empty());

        let file = dir.join("note.md");
        fs::write(&file, "hello").unwrap();
        let response = planner
            .handle(UserRequest::read_file(&file))
            .expect("dispatch read");
        assert_eq!(response.tool_id.as_deref(), Some(READ_FILE_TOOL_ID));
        assert_eq!(response.capability, Some(Capability::ReadDocuments));
        assert!(response.document.is_some());
    }

    #[test]
    fn unknown_tool() {
        // Builtin ListDirectory route prefers search_files; registry is empty → error.
        let planner = planner_with_tools_and_routes(
            |_tools, _fs, _content| {},
            ToolRouteTable::builtin(),
        );
        let err = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .expect_err("missing preferred tool must fail");
        assert!(
            err.message().contains("unknown tool"),
            "expected unknown tool error, got {}",
            err.message()
        );
    }

    #[test]
    fn missing_capability() {
        // Preferred tool is registered but does not advertise Search.
        let planner = planner_with_tools(|tools, _fs, _content| {
            tools
                .register_tool(Arc::new(WrongCapabilityTool::new()))
                .unwrap();
        });
        let err = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .expect_err("capability mismatch must fail");
        assert!(
            err.message().contains("does not fulfill capability"),
            "expected missing capability error, got {}",
            err.message()
        );
    }

    /// Minimal handler used only by `tool_registration`.
    struct ListDirectoryProbe;
    impl IntentToolHandler for ListDirectoryProbe {
        fn route(&self) -> ToolRoute {
            ToolRoute {
                intent: IntentId::ListDirectory,
                capability: Capability::Search,
                tool_id: "probe_list",
            }
        }

        fn prepare(
            &self,
            _intent: &Intent,
            _request_text: &str,
            _host: &dyn DispatchSupport,
        ) -> JaymiResult<PreparedToolCall> {
            Err(JaymiError::new("probe prepare unused"))
        }

        fn respond(
            &self,
            _call: &PreparedToolCall,
            _output: ToolOutput,
            _meta: ExecutionMeta,
        ) -> JaymiResult<PlannerResponse> {
            Err(JaymiError::new("probe respond unused"))
        }
    }

    /// Tool id matches ListDirectory preferred id but ads the wrong capability.
    struct WrongCapabilityTool {
        metadata: ToolMetadata,
    }

    impl WrongCapabilityTool {
        fn new() -> Self {
            Self {
                metadata: ToolMetadata {
                    id: SEARCH_FILES_TOOL_ID.to_string(),
                    name: "Wrong Capability".into(),
                    version: "0".into(),
                    description: "ads Code instead of Search".into(),
                    provider: FILESYSTEM_PROVIDER_ID.to_string(),
                    capabilities: vec![Capability::Code],
                    risk: ToolRisk::Workspace,
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::Low,
                    memory_usage: MemoryUsage::Small,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::LocalOnly,
                    internet: InternetRequirement::Never,
                    reliability: Reliability::Experimental,
                    result_type: ResultType::StructuredData,
                },
            }
        }
    }

    impl Tool for WrongCapabilityTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput> {
            Ok(ToolOutput {
                success: true,
                ..Default::default()
            })
        }
    }
}
