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
//! [`ToolRouteTable`] (Intent → tool). Session / PlanWork / Unknown stay
//! special-cased. That is intentional — not an Application→Engine bypass.
//! See `dispatch`, `request_lifecycle`, and docs/planner.md.
//!
//! The Planner does not own long-lived Memory or Project CRUD APIs. Those
//! belong to the Memory Engine and Project Engine. Application (or tools)
//! call those engines directly for administrative operations.

#![forbid(unsafe_code)]

pub mod approval_history;
pub mod decision;
pub mod dispatch;
pub mod execution_plan;
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
use jaymi_context::{AssembleHints, ContextBundle, ContextEngine};
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
use plan_revision::apply_modification_note;

const NAME: &str = "planner";
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
    /// How many times [`Self::handle`] has been entered (integrity tests).
    handle_count: AtomicU64,
    /// Plans waiting on conversational review (resume without replan).
    paused: Mutex<PausedPlanStore>,
    /// Lineage of proposed / revised / cancelled plans for this Planner.
    plan_history: Mutex<Vec<PlanHistoryEntry>>,
    /// Review Card decisions for transparency, reasoning, and diagnostics.
    approval_history: Mutex<ApprovalHistoryStore>,
}

impl Planner {
    /// Construct a Planner that discovers capabilities through registries.
    pub fn new(deps: PlannerDeps) -> Self {
        Self {
            initialized: false,
            decision: DecisionEngine,
            reasoning: ReasoningEngine,
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
            handle_count: AtomicU64::new(0),
            paused: Mutex::new(PausedPlanStore::default()),
            plan_history: Mutex::new(Vec::new()),
            approval_history: Mutex::new(ApprovalHistoryStore::new()),
        }
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

    /// Whether a reasoning backend is wired.
    pub fn reasoning_implemented(&self) -> bool {
        self.reasoning.is_implemented()
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
        self.context.invalidate_cache("project_changed");
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
        self.context.invalidate_cache("project_changed");
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
        let hints = AssembleHints {
            intent: intent.id(),
            capability_ids: capabilities
                .iter()
                .map(|capability| capability.id().to_string())
                .collect(),
        };

        jaymi_logging::info(
            "planner",
            format!(
                "intent resolved label={} capabilities=[{}]",
                hints.intent.as_str(),
                hints.capability_ids.join(",")
            ),
        );

        // Workspace session intents mutate project state first, then assemble
        // so ContextBundle is the sole post-session request-context snapshot.
        match &intent {
            Intent::ContinueProject { name } => {
                let response = self.handle_continue_project(name)?;
                self.context.invalidate_cache("project_changed");
                let bundle = self.context.assemble_with(&request, Some(&hints))?;
                log_promotions(&bundle);
                return Ok(finalize(response, bundle));
            }
            Intent::OpenProject { project_id } => {
                let response = self.handle_open_project_id(project_id)?;
                self.context.invalidate_cache("project_changed");
                let bundle = self.context.assemble_with(&request, Some(&hints))?;
                log_promotions(&bundle);
                return Ok(finalize(response, bundle));
            }
            Intent::CloseProject => {
                let response = self.handle_close_project()?;
                self.context.invalidate_cache("project_changed");
                let bundle = self.context.assemble_with(&request, Some(&hints))?;
                log_promotions(&bundle);
                return Ok(finalize(response, bundle));
            }
            _ => {}
        }

        // 3–6. Context Policy → Providers → Context Engine → ContextBundle
        let context = self.context.assemble_with(&request, Some(&hints))?;
        log_promotions(&context);

        let Some(capability) = capabilities.first().copied() else {
            jaymi_logging::warn(
                "planner",
                "unsupported request; no capability mapped for intent",
            );
            return Ok(finalize(
                PlannerResponse {
                    content: "Unsupported request. Try: list <directory>, read <file>, search <query>, index <path>, or ask what files exist".to_string(),
                    ..PlannerResponse::default()
                },
                context,
            ));
        };

        // Planning answers "what would this take" without needing the
        // capability to be fulfillable today.
        if let Intent::PlanWork { capabilities, goal } = &intent {
            let response = self.handle_plan_work(capabilities, goal)?;
            return Ok(finalize(response, context));
        }

        let availability = self.capabilities.validate(capability);
        if !availability.is_executable_tier() {
            let message = format!(
                "capability {} is not executable (availability={})",
                capability.id(),
                availability.as_str()
            );
            jaymi_logging::error("planner", &message);
            return Err(JaymiError::new(message));
        }

        // Reasoning Engine is intentionally unused for these deterministic paths.
        let _ = &self.reasoning;

        // Tool-backed intents resolve through the registered route table.
        // Session / PlanWork / Unknown stay special-cased above.
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
                Ok(finalize(response, context.clone()))
            }
            Err(error) => Err(error),
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

        if let Some(reason) = call.invalidate_cache {
            self.context.invalidate_cache(reason);
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

        if !reassemble_context {
            // Partial / ordinary modifications reuse the existing session context.
            return Ok(finalize(response, ContextBundle::default()));
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
        };
        let request = UserRequest::new("");
        let bundle = self.context.assemble_with(&request, Some(&hints))?;
        Ok(finalize(response, bundle))
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
    response
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
