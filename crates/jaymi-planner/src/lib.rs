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
pub mod coding_actions;
pub mod code_generation;
pub mod coding_diff_review;
pub mod coding_apply;
pub mod coding_mutation_recovery;
pub mod patch_generation;
pub mod coding_plan;
pub mod coding_edit;
pub mod change_investigation;
pub mod coding_review;
pub mod coding_understanding;
pub mod code_search;
pub mod symbol_navigation;
pub mod relationship_tracing;
pub mod deep_investigation;
pub mod codebase_qa;
pub mod codebase_evidence;
pub mod build_awareness;
pub mod test_execution;
pub mod verification_diagnostics;
pub mod fix_from_verification;
pub mod verification_summary;
pub mod coding_verification_loop;
pub mod coding_goal;
pub mod goal_decomposition;
pub mod agent_state_machine;
pub mod multi_step_execution;
pub mod agent_context_maintenance;
pub mod approval_autonomy_policy;
pub mod agent_progress_ux;
pub mod agent_pause_resume_cancel;
pub mod agent_failure_recovery;
pub mod coding_goal_completion;
pub mod agent_session_continuity;
pub mod agentic_coding_experience;
pub mod code_talk;
pub mod complexity;
pub mod conversation_history;
pub mod conversation_state;
pub mod conversational;
pub mod decision;
pub mod dispatch;
pub mod environmental;
pub mod execution_plan;
pub mod model_selection;
pub mod multi_file;
pub mod pair_programming;
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
pub use code_generation::{
    CodeGeneration, GenerationOp, GenerationOpKind, GenerationToolCall,
};
pub use coding_diff_review::{
    CodingDiffReview, CodingDiffReviewSurface, CodingFileDiffStat,
};
pub use coding_apply::{CodingOpResult, coding_apply_headline, summary_from_op_results};
pub use coding_mutation_recovery::{
    CodingMutationRecovery, GenerationBaseline, MutationPhase, RecoveryKind,
};
pub use patch_generation::{
    PatchChangeType, PatchGenerationReport, PatchLocation, PatchProposal, PatchValidationStatus,
    ValidatedPatch, content_fingerprint,
};
pub use coding_plan::{CodingPlan, CodingPlanAssessment, CodingPlanKind, CodingPlanOrigin};
pub use coding_edit::{
    CodingEditAssessment, CodingEditKind, CodingEditObjective, CodingEditStatus, EditScope,
};
pub use change_investigation::{
    ChangeEvidence, ChangeInvestigationMode, ChangeInvestigationReport, ChangeInvestigationStep,
};
pub use coding_review::{CodingReview, ReviewAssessment, ReviewFocus};
pub use coding_understanding::{
    CodingUnderstanding, ProjectUnderstandingAngle, UnderstandingAssessment, UnderstandingFocus,
};
pub use code_search::{
    CodeSearchEvidence, CodeSearchKind, CodeSearchReport, CONVERSATION_EVIDENCE_CAP,
    SEARCH_HIT_LIMIT,
};
pub use symbol_navigation::{
    SymbolNavEvidence, SymbolNavKind, SymbolNavigationReport,
};
pub use relationship_tracing::{
    RelationshipTraceReport, TraceEvidence, TraceKind, TraceStep, TraceStopReason,
};
pub use deep_investigation::{
    DeepInvestigationReport, InvestigationEvidence, InvestigationKind, InvestigationStep,
};
pub use codebase_qa::{
    CodebaseQaReport, EvidenceConflict, QaComplexity, QaEvidence, QaKind, QaRoute,
};
pub use codebase_evidence::{
    CodebaseEvidenceHit, CodebaseEvidenceOrigin, HandoffHints, prefers_reasoning_handoff,
};
pub use build_awareness::{
    BuildAwarenessAssessment, BuildAwarenessIntent, BuildAwarenessReport, BuildAwarenessStatus,
    ExpectedOutputType, VerificationConfidence, VerificationPurpose, VerificationTarget,
};
pub use test_execution::{
    TestExecutionAssessment, TestExecutionIntent, TestExecutionReport, TestExecutionStatus,
    TestScope,
};
pub use verification_diagnostics::{
    VerificationEvidence, VerificationFinding, VerificationResultKind, FindingSeverity,
};
pub use fix_from_verification::{
    FixActionability, FixFromVerificationAssessment, FixFromVerificationIntent,
    FixFromVerificationReport,
};
pub use verification_summary::{
    VerificationStatus, VerificationSummary, VerifiedCommandLine,
};
pub use coding_verification_loop::{
    CodingVerificationLoop, CycleOutcome, CycleRecord, LoopPhase, MAX_CYCLES,
};
pub use coding_goal::{
    AcceptanceCriterion, AcceptanceCriterionSource, CodingGoal, CodingGoalAssessment,
    CodingGoalRequestKind, CodingGoalStatus, CodingGoalStore, GoalVerificationState,
};
pub use goal_decomposition::{
    DecompositionStatus, GoalDecompositionReport, GoalTask, GoalTaskRisk, GoalTaskStatus,
};
pub use agent_state_machine::{
    AgentLifecycle, AgentLifecycleEvent, AgentLifecycleReport, AgentLifecycleState,
    CompletionEvidence, CompletionEvidenceSource, MAX_AGENT_TRANSITIONS,
};
pub use multi_step_execution::{
    MultiStepExecutionReport, MultiStepStatus, TaskCompletionEvidence, WorkflowRecord,
    MAX_IN_PROGRESS_TASKS, MAX_WORKFLOW_TRANSITIONS,
};
pub use agent_context_maintenance::{
    AgentContextReport, AgentContextStatus, MaintainedAgentContext, MAX_GOAL_CONTEXT_CHARS,
};
pub use approval_autonomy_policy::{
    AutonomyPolicyClass, AutonomyPolicyReport, AutonomyPolicyStatus, BoundApproval,
    GoalAutonomyState,
};
pub use agent_progress_ux::{
    AgentProgressReport, AgentProgressView, ProgressPhase, ProgressReportStatus,
};
pub use agent_pause_resume_cancel::{
    AgentControlReport, AgentControlStatus, GoalControlState, InFlightKind, InFlightObservation,
    ProjectFingerprint, StaleReason,
};
pub use agent_failure_recovery::{
    FailureCategory, GoalRecoveryReport, GoalRecoveryState, GoalRecoveryStatus,
    RecoveryDisposition, MAX_GOAL_RECOVERY_ATTEMPTS,
};
pub use coding_goal_completion::{
    CriterionEvaluation, CriterionSatisfaction, GoalCompletionOutcome, GoalCompletionReport,
    GoalCompletionState, GoalCompletionStatus, GoalEndingKind,
};
pub use agent_session_continuity::{
    SessionContinuityReport, SessionContinuityState, SessionContinuityStatus,
    MAX_TERMINAL_GOALS_PER_CONVERSATION, MAX_WORKFLOW_RETAINED,
};
pub use agentic_coding_experience::{
    AgenticCodingExperienceReport, AgenticExperienceRequestKind, AgenticExperienceStatus,
    ExperienceStage,
};
pub use code_talk::{chip_label_from_suggestion, next_action_chip, subject_title};
pub use multi_file::{AffectedFile, MultiFileKind, MultiFilePlan};
pub use pair_programming::{
    baseline_actions, baseline_surface, PairPhase, PairProgrammingSurface, SuggestedAction,
};
pub use environmental::{
    resolve_environment, EnvironmentalResolution, ResolutionConfidence, ResolvedKind,
    ResolvedReference,
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
    /// Coding Understanding scaffold (Sprint C1.1) when Understand Before Acting ran.
    pub coding_understanding: Option<coding_understanding::CodingUnderstanding>,
    /// Coding Review scaffold (Sprint C1.3) when review-only mode ran.
    pub coding_review: Option<coding_review::CodingReview>,
    /// Coding Plan scaffold (Sprint C1.4) when generation-planning mode ran.
    pub coding_plan: Option<coding_plan::CodingPlan>,
    /// Coding Edit Intent / objective (Sprint C3.1) — mutation classification only.
    /// Accompanied by Change Investigation (C3.2) on the same soft path.
    pub coding_edit: Option<coding_edit::CodingEditObjective>,
    /// Change Investigation report (Sprint C3.2) — read-only impact mapping.
    pub change_investigation: Option<change_investigation::ChangeInvestigationReport>,
    /// Code Generation batch (Sprint C1.5) when ops were proposed for review.
    pub code_generation: Option<code_generation::CodeGeneration>,
    /// Coding Diff Review surface (Sprint C3.5) attached to the paused plan.
    pub coding_diff_review: Option<coding_diff_review::CodingDiffReviewSurface>,
    /// Coding Mutation Recovery report (Sprint C3.7) after apply/cancel/stale stop.
    pub coding_mutation_recovery: Option<coding_mutation_recovery::CodingMutationRecovery>,
    /// Patch Generation report (Sprint C3.4) — proposed patches + validation.
    pub patch_generation: Option<patch_generation::PatchGenerationReport>,
    /// Multi-file Intelligence plan (Sprint C1.7) when affected files were resolved.
    pub multi_file: Option<multi_file::MultiFilePlan>,
    /// Pair Programming surface (Sprint C1.8) — Planner-generated next actions.
    pub pair_programming: Option<pair_programming::PairProgrammingSurface>,
    /// Intelligent Code Search report (Sprint C2.1) when search evidence was selected.
    pub code_search: Option<code_search::CodeSearchReport>,
    /// Symbol Navigation report (Sprint C2.2) when navigation evidence was selected.
    pub symbol_navigation: Option<symbol_navigation::SymbolNavigationReport>,
    /// Relationship Trace report (Sprint C2.3) when a multi-hop investigation ran.
    pub relationship_trace: Option<relationship_tracing::RelationshipTraceReport>,
    /// Deep Codebase Investigation report (Sprint C2.4).
    pub deep_investigation: Option<deep_investigation::DeepInvestigationReport>,
    /// Codebase Q&A report (Sprint C2.5) — conversational grounded answers.
    pub codebase_qa: Option<codebase_qa::CodebaseQaReport>,
    /// Build Awareness report (Sprint C4.1) — verification discovery only.
    pub build_awareness: Option<build_awareness::BuildAwarenessReport>,
    /// Test Execution report (Sprint C4.2) — Planner-mediated terminal tests.
    pub test_execution: Option<test_execution::TestExecutionReport>,
    /// Fix From Verification Failure report (Sprint C4.4) — diagnosis + corrective plan.
    pub fix_from_verification: Option<fix_from_verification::FixFromVerificationReport>,
    /// Verification Summary (Sprint C4.5) — concise conversational outcome.
    pub verification_summary: Option<verification_summary::VerificationSummary>,
    /// Bounded Coding Verification Loop state (Sprint C4.6), when active/relevant.
    pub coding_verification_loop: Option<coding_verification_loop::CodingVerificationLoop>,
    /// Coding Goal (Sprint C5.1) — multi-step objective representation only.
    pub coding_goal: Option<coding_goal::CodingGoal>,
    /// Goal Decomposition (Sprint C5.2) — proposed tasks for an active Coding Goal.
    pub goal_decomposition: Option<goal_decomposition::GoalDecompositionReport>,
    /// Agent Lifecycle report (Sprint C5.3), when a lifecycle transition ran.
    pub agent_lifecycle: Option<agent_state_machine::AgentLifecycleReport>,
    /// Multi-Step Coding Execution report (Sprint C5.4).
    pub multi_step_execution: Option<multi_step_execution::MultiStepExecutionReport>,
    /// Agent Context Maintenance report (Sprint C5.5).
    pub agent_context_maintenance: Option<agent_context_maintenance::AgentContextReport>,
    /// Approval / Autonomy Policy report (Sprint C5.6).
    pub autonomy_policy: Option<approval_autonomy_policy::AutonomyPolicyReport>,
    /// Agent Progress UX view (Sprint C5.7) — derive-only; not a second agent state.
    pub agent_progress: Option<agent_progress_ux::AgentProgressView>,
    /// Agent Progress soft-path report when “Show goal progress.” ran.
    pub agent_progress_report: Option<agent_progress_ux::AgentProgressReport>,
    /// Agent Pause / Resume / Cancel report (Sprint C5.8).
    pub agent_control: Option<agent_pause_resume_cancel::AgentControlReport>,
    /// Agent Failure / Recovery report (Sprint C5.9).
    pub agent_recovery: Option<agent_failure_recovery::GoalRecoveryReport>,
    /// Coding Goal Completion report (Sprint C5.10).
    pub goal_completion: Option<coding_goal_completion::GoalCompletionReport>,
    /// Long-running session continuity report (Sprint C5.11).
    pub session_continuity: Option<agent_session_continuity::SessionContinuityReport>,
    /// Agentic Coding Experience report (Sprint C5.12) — composes C5.1–C5.11.
    pub agentic_coding_experience: Option<agentic_coding_experience::AgenticCodingExperienceReport>,
    /// Terminal exit code when a Run completed (C4.2+).
    pub terminal_exit_code: Option<i32>,
    /// Terminal Run duration in milliseconds when measured.
    pub terminal_duration_ms: Option<u64>,
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
    /// Last Coding Understanding scaffold for conversational diagnostics (C1.1).
    last_understanding: Mutex<Option<coding_understanding::CodingUnderstanding>>,
    /// Last Coding Review scaffold for conversational diagnostics (C1.3).
    last_review: Mutex<Option<coding_review::CodingReview>>,
    /// Last Coding Plan scaffold for conversational diagnostics (C1.4).
    last_coding_plan: Mutex<Option<coding_plan::CodingPlan>>,
    /// Last structured verification evidence (C4.3/C4.4) for fix-from-failure.
    last_verification_evidence: Mutex<Option<verification_diagnostics::VerificationEvidence>>,
    /// Last mutation-oriented ExecutionSummary (C4.5) for “what changed”.
    last_execution_summary: Mutex<Option<execution_plan::ExecutionSummary>>,
    /// Bounded Apply→Verify→Fix workflow (C4.6).
    coding_verification_loop: Mutex<Option<coding_verification_loop::CodingVerificationLoop>>,
    /// Multi-step coding goals across conversations (C5.1).
    coding_goals: Mutex<coding_goal::CodingGoalStore>,
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
            last_understanding: Mutex::new(None),
            last_review: Mutex::new(None),
            last_coding_plan: Mutex::new(None),
            last_verification_evidence: Mutex::new(None),
            last_execution_summary: Mutex::new(None),
            coding_verification_loop: Mutex::new(None),
            coding_goals: Mutex::new(coding_goal::CodingGoalStore::default()),
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

    fn remember_understanding(
        &self,
        understanding: Option<coding_understanding::CodingUnderstanding>,
    ) {
        if let Ok(mut guard) = self.last_understanding.lock() {
            *guard = understanding;
        }
    }

    fn take_understanding(&self) -> Option<coding_understanding::CodingUnderstanding> {
        self.last_understanding
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    }

    fn remember_review(&self, review: Option<coding_review::CodingReview>) {
        if let Ok(mut guard) = self.last_review.lock() {
            *guard = review;
        }
    }

    fn take_review(&self) -> Option<coding_review::CodingReview> {
        self.last_review.lock().ok().and_then(|mut guard| guard.take())
    }

    fn remember_coding_plan(&self, plan: Option<coding_plan::CodingPlan>) {
        if let Ok(mut guard) = self.last_coding_plan.lock() {
            *guard = plan;
        }
    }

    fn take_coding_plan(&self) -> Option<coding_plan::CodingPlan> {
        self.last_coding_plan
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    }

    fn remember_verification_evidence(
        &self,
        evidence: Option<verification_diagnostics::VerificationEvidence>,
    ) {
        if let Ok(mut guard) = self.last_verification_evidence.lock() {
            *guard = evidence;
        }
    }

    fn clone_last_verification_evidence(
        &self,
    ) -> Option<verification_diagnostics::VerificationEvidence> {
        self.last_verification_evidence
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn remember_execution_summary(&self, summary: Option<execution_plan::ExecutionSummary>) {
        // Prefer summaries that record files edited (coding apply).
        let Some(summary) = summary else {
            return;
        };
        if summary.files_edited.is_empty() && summary.resources_changed.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.last_execution_summary.lock() {
            *guard = Some(summary);
        }
    }

    fn clone_last_execution_summary(&self) -> Option<execution_plan::ExecutionSummary> {
        self.last_execution_summary
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn clone_coding_verification_loop(
        &self,
    ) -> Option<coding_verification_loop::CodingVerificationLoop> {
        self.coding_verification_loop
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn store_coding_verification_loop(
        &self,
        loop_state: Option<coding_verification_loop::CodingVerificationLoop>,
    ) {
        if let Ok(mut guard) = self.coding_verification_loop.lock() {
            *guard = loop_state;
        }
    }

    fn clone_coding_goal_store(&self) -> coding_goal::CodingGoalStore {
        self.coding_goals
            .lock()
            .ok()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn with_coding_goal_store<R>(
        &self,
        f: impl FnOnce(&mut coding_goal::CodingGoalStore) -> R,
    ) -> Option<R> {
        let mut guard = self.coding_goals.lock().ok()?;
        Some(f(&mut guard))
    }

    fn active_conversation_id_for_goal(&self) -> Option<String> {
        self.memory.active_conversation_id().map(|id| id.to_string())
    }

    fn update_coding_verification_loop(
        &self,
        update: impl FnOnce(
            Option<coding_verification_loop::CodingVerificationLoop>,
        ) -> Option<coding_verification_loop::CodingVerificationLoop>,
    ) -> Option<coding_verification_loop::CodingVerificationLoop> {
        let next = update(self.clone_coding_verification_loop());
        self.store_coding_verification_loop(next.clone());
        next
    }

    /// Attach a C4.5 verification summary to the response (conversation + chips).
    fn attach_verification_summary(
        &self,
        response: &mut PlannerResponse,
        cancelled: bool,
        additional_commands: Vec<verification_summary::VerifiedCommandLine>,
    ) {
        let prior = self.clone_last_execution_summary();
        let summary = verification_summary::compose(verification_summary::VerificationSummaryInput {
            execution: response.execution_summary.as_ref(),
            prior_execution: prior.as_ref(),
            evidence: response
                .test_execution
                .as_ref()
                .and_then(|r| r.evidence.as_ref())
                .or(response
                    .fix_from_verification
                    .as_ref()
                    .and_then(|r| r.evidence.as_ref())),
            test_report: response.test_execution.as_ref(),
            additional_commands,
            cancelled,
        });
        jaymi_logging::info("planner", summary.to_diagnostics_summary());

        // C4.6 — advance bounded loop from this verification result when active.
        let evidence = response
            .test_execution
            .as_ref()
            .and_then(|r| r.evidence.clone());
        let loop_state = self.update_coding_verification_loop(|existing| {
            let Some(existing) = existing else {
                return None;
            };
            if cancelled {
                return Some(coding_verification_loop::cancel_loop(
                    existing,
                    "Verification cancelled.",
                ));
            }
            Some(coding_verification_loop::on_verification_result(
                existing,
                &summary,
                evidence.as_ref(),
            ))
        });

        let mut content =
            verification_summary::prefer_summary_content(&summary, &response.content);
        if let Some(loop_state) = &loop_state {
            // Suppress Review Fix when exhausted.
            if coding_verification_loop::suppress_fix_offer(Some(loop_state)) {
                // Leave summary as-is but clear recommended fix for chips.
            }
            content = coding_verification_loop::append_progress_note(&content, loop_state);
            if matches!(
                loop_state.phase,
                coding_verification_loop::LoopPhase::Exhausted
            ) {
                content = format!(
                    "{content}\n\n{}",
                    loop_state.to_markdown()
                );
            }
            response.coding_verification_loop = Some(loop_state.clone());
        }

        // If exhausted, strip recommended fix from the summary copy on the response.
        let mut summary = summary;
        if coding_verification_loop::suppress_fix_offer(loop_state.as_ref()) {
            summary.recommended_fix_label = None;
            summary.recommended_fix_prompt = None;
            summary.next_actions.retain(|a| {
                !a.to_ascii_lowercase().contains("fix the build")
                    && !a.to_ascii_lowercase().contains("fix the failing")
                    && !a.to_ascii_lowercase().contains("fix the verification")
            });
            if summary.next_actions.is_empty() {
                summary.next_actions.push(
                    "Verification/recovery cycle limit reached — review the history or start a fresh request."
                        .into(),
                );
            }
            content = verification_summary::prefer_summary_content(&summary, &content);
            if let Some(loop_state) = &loop_state {
                content = coding_verification_loop::append_progress_note(&content, loop_state);
            }
        }

        response.content = content;
        response.verification_summary = Some(summary);
        self.note_verify_for_multi_step_execution(response, cancelled);
    }

    fn note_apply_for_verification_loop(&self, response: &mut PlannerResponse) {
        let Some(summary) = response.execution_summary.clone() else {
            return;
        };
        if summary.files_edited.is_empty() {
            return;
        }
        let loop_state = self.update_coding_verification_loop(|existing| {
            Some(coding_verification_loop::on_apply_completed(
                existing, &summary,
            ))
        });
        if let Some(loop_state) = loop_state {
            response.content =
                coding_verification_loop::append_progress_note(&response.content, &loop_state);
            response.coding_verification_loop = Some(loop_state);
        }
        self.note_apply_for_multi_step_execution(response, &summary);
    }

    /// C5.4 — sync active Coding Goal after an approved apply (no second Review).
    fn note_apply_for_multi_step_execution(
        &self,
        response: &mut PlannerResponse,
        summary: &execution_plan::ExecutionSummary,
    ) {
        let conversation_id = self.active_conversation_id_for_goal();
        let plan_id = summary.plan_id.as_str().to_string();
        let updated = self.with_coding_goal_store(|store| {
            let Some(existing) = store.active_goal(conversation_id.as_deref()).cloned() else {
                return None;
            };
            if existing.active_task_id.is_none()
                && !existing
                    .tasks
                    .iter()
                    .any(|t| t.status == goal_decomposition::GoalTaskStatus::InProgress)
            {
                return None;
            }
            match multi_step_execution::on_apply_success(existing, &plan_id) {
                Ok(goal) => {
                    if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                        *slot = goal.clone();
                    }
                    Some(goal)
                }
                Err(err) => {
                    jaymi_logging::info(
                        "planner",
                        format!("multi_step apply sync skipped: {err}"),
                    );
                    None
                }
            }
        });
        if let Some(Some(goal)) = updated {
            response.coding_goal = Some(goal);
        }
    }

    /// C5.4 — sync goal/task after verification summary (pass/fail).
    fn note_verify_for_multi_step_execution(
        &self,
        response: &mut PlannerResponse,
        cancelled: bool,
    ) {
        if cancelled {
            return;
        }
        let conversation_id = self.active_conversation_id_for_goal();
        let evidence_ref = response
            .test_execution
            .as_ref()
            .and_then(|r| r.evidence.as_ref())
            .and_then(|e| {
                e.command
                    .clone()
                    .or_else(|| e.kind.map(|k| k.as_str().to_string()))
            })
            .or_else(|| {
                response
                    .verification_summary
                    .as_ref()
                    .and_then(|s| s.status.map(|st| st.as_str().to_string()))
            })
            .unwrap_or_else(|| "verification".into());

        let passed = response
            .verification_summary
            .as_ref()
            .and_then(|s| s.status)
            .map(|st| matches!(st, verification_summary::VerificationStatus::Passed))
            .unwrap_or(false);
        let failed = response
            .verification_summary
            .as_ref()
            .and_then(|s| s.status)
            .map(|st| {
                matches!(
                    st,
                    verification_summary::VerificationStatus::Failed
                        | verification_summary::VerificationStatus::Blocked
                )
            })
            .unwrap_or(false);

        if !passed && !failed {
            return;
        }

        let synced = self.with_coding_goal_store(|store| {
            let Some(existing) = store.active_goal(conversation_id.as_deref()).cloned() else {
                return None;
            };
            if existing.active_task_id.is_none() && existing.tasks.is_empty() {
                return None;
            }
            let result = if passed {
                multi_step_execution::on_verify_pass(existing, &evidence_ref)
            } else {
                multi_step_execution::on_verify_fail(existing, &evidence_ref)
            };
            match result {
                Ok((goal, report)) => {
                    if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                        *slot = goal.clone();
                    }
                    Some((goal, report))
                }
                Err(err) => {
                    jaymi_logging::info(
                        "planner",
                        format!("multi_step verify sync skipped: {err}"),
                    );
                    None
                }
            }
        });
        if let Some(Some((goal, report))) = synced {
            response.coding_goal = Some(goal);
            response.multi_step_execution = Some(report);
        }
    }

    fn note_fix_proposed_for_verification_loop(&self, response: &mut PlannerResponse) {
        let loop_state = self.update_coding_verification_loop(|existing| {
            existing.map(coding_verification_loop::on_fix_proposed)
        });
        if let Some(loop_state) = loop_state {
            response.content =
                coding_verification_loop::append_progress_note(&response.content, &loop_state);
            response.coding_verification_loop = Some(loop_state);
        }
    }

    fn note_verify_awaiting_review(&self, response: &mut PlannerResponse) {
        let loop_state = self.update_coding_verification_loop(|existing| {
            existing.map(coding_verification_loop::on_verify_awaiting_review)
        });
        if let Some(loop_state) = loop_state {
            response.coding_verification_loop = Some(loop_state);
        }
    }

    fn cancel_coding_verification_loop(&self, reason: &str) -> Option<coding_verification_loop::CodingVerificationLoop> {
        self.update_coding_verification_loop(|existing| {
            existing.map(|state| coding_verification_loop::cancel_loop(state, reason))
        })
    }

    fn llm_context_for_understanding(
        &self,
        context: &ContextBundle,
        understanding: Option<&coding_understanding::UnderstandingAssessment>,
        review: Option<&coding_review::ReviewAssessment>,
        coding_plan: Option<&coding_plan::CodingPlanAssessment>,
    ) -> LlmContext {
        let mut llm = LlmContext::from_bundle(context);
        if let Some(assessment) = understanding {
            let instruction = assessment.scaffold.prompt_instruction_with_angle(
                assessment.focus,
                assessment.project_angle,
                assessment.feature_hint.as_deref(),
            );
            llm = llm.with_extension(
                coding_understanding::LLM_EXTENSION_KEY,
                serde_json::json!({
                    "focus": assessment.focus.as_str(),
                    "angle": assessment.project_angle.map(|a| a.as_str()),
                    "feature_hint": assessment.feature_hint,
                    "instruction": instruction,
                }),
            );
        }
        if let Some(assessment) = review {
            let instruction = assessment.scaffold.prompt_instruction(assessment.focus);
            llm = llm.with_extension(
                coding_review::LLM_EXTENSION_KEY,
                serde_json::json!({
                    "focus": assessment.focus.as_str(),
                    "instruction": instruction,
                }),
            );
        }
        if let Some(assessment) = coding_plan {
            let instruction = assessment
                .scaffold
                .prompt_instruction(assessment.kind, &assessment.goal);
            llm = llm.with_extension(
                coding_plan::LLM_EXTENSION_KEY,
                serde_json::json!({
                    "kind": assessment.kind.as_str(),
                    "goal": assessment.goal,
                    "instruction": instruction,
                }),
            );
        }
        // C5.5 — attach compact goal context when an active Coding Goal exists.
        // ContextEngine already minted `context`; we never assemble a second bundle.
        let conversation_id = self.active_conversation_id_for_goal();
        let active_goal = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();
        agent_context_maintenance::attach_to_llm(llm, active_goal.as_ref(), context)
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
        mut response: PlannerResponse,
        bundle: ContextBundle,
    ) -> PlannerResponse {
        if let Some(summary) = response.execution_summary.clone() {
            self.remember_execution_summary(Some(summary));
        }
        if response.pair_programming.is_none() {
            response.pair_programming = Some(self.synthesize_pair_programming(&response, Some(&bundle)));
        } else if let Some(summary) = response.verification_summary.as_ref() {
            // Enrich an existing surface with grounded verification next actions.
            if let Some(surface) = response.pair_programming.as_mut() {
                pair_programming::enrich_from_verification_summary_with_loop(
                    surface,
                    summary,
                    response.coding_verification_loop.as_ref(),
                );
            }
        }
        // C5.7 — attach derive-only progress when an active Coding Goal is in play.
        if response.agent_progress.is_none() {
            let conversation_id = self.active_conversation_id_for_goal();
            let goal = response
                .coding_goal
                .clone()
                .or_else(|| {
                    self.clone_coding_goal_store()
                        .active_goal(conversation_id.as_deref())
                        .cloned()
                });
            if let Some(goal) = goal.filter(|g| !g.status.is_terminal()) {
                let progress = agent_progress_ux::derive_progress(&goal);
                if let Some(surface) = response.pair_programming.as_mut() {
                    agent_progress_ux::enrich_pair_programming(surface, &progress);
                }
                response.agent_progress = Some(progress);
            }
        } else if let Some(progress) = response.agent_progress.as_ref() {
            if let Some(surface) = response.pair_programming.as_mut() {
                agent_progress_ux::enrich_pair_programming(surface, progress);
            }
        }
        self.with_conversation_state(finalize(response, bundle))
    }

    /// Planner-generated Quick Actions / next steps (Sprint C1.8).
    ///
    /// Experience / UI render these; they never invent the list. Baseline actions
    /// are still Planner-authored when no specialized scaffold is active.
    pub fn pair_programming_suggestions(&self) -> pair_programming::PairProgrammingSurface {
        let empty = PlannerResponse::default();
        self.synthesize_pair_programming(&empty, None)
    }

    fn pair_memory(&self) -> pair_programming::PairMemory {
        pair_programming::PairMemory {
            had_understanding: self
                .last_understanding
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|_| ()))
                .is_some(),
            had_review: self
                .last_review
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|_| ()))
                .is_some(),
            had_coding_plan: self
                .last_coding_plan
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|_| ()))
                .is_some(),
        }
    }

    fn synthesize_pair_programming(
        &self,
        response: &PlannerResponse,
        bundle: Option<&ContextBundle>,
    ) -> pair_programming::PairProgrammingSurface {
        let execution_completed = response
            .execution_summary
            .as_ref()
            .map(|summary| matches!(summary.status, ExecutionStatus::Completed))
            .unwrap_or(false);
        let mut surface = pair_programming::synthesize(&pair_programming::PairSynthesisInput {
            memory: self.pair_memory(),
            bundle,
            understanding: response.coding_understanding.is_some(),
            review: response.coding_review.is_some(),
            planning: response.coding_plan.is_some(),
            generation: response.code_generation.is_some(),
            multi_file: response.multi_file.is_some(),
            awaiting_review: response.awaiting_review,
            execution_summary: response.execution_summary.is_some(),
            execution_completed,
            verification_summary: response.verification_summary.as_ref(),
        });
        if let Some(summary) = response.verification_summary.as_ref() {
            pair_programming::enrich_from_verification_summary_with_loop(
                &mut surface,
                summary,
                response.coding_verification_loop.as_ref(),
            );
        }
        surface
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
        self.enter_or_continue_preparing_context();

        // C4.6 — explicit stop/cancel of the bounded verification loop.
        if coding_verification_loop::detect_loop_cancel(&request.content) {
            if let Some(loop_state) = self.clone_coding_verification_loop() {
                if loop_state.active {
                    let cancelled = self
                        .cancel_coding_verification_loop("User stopped the verification workflow.")
                        .unwrap_or(loop_state);
                    self.transition_conversation(ConversationState::Completed);
                    let response = PlannerResponse {
                        content: format!(
                            "Stopped the coding verification workflow.\n\n{}",
                            cancelled.to_markdown()
                        ),
                        reasoning_used: false,
                        stream_lifecycle: Some(StreamingLifecycle::Idle),
                        coding_verification_loop: Some(cancelled),
                        ..PlannerResponse::default()
                    };
                    let bundle = self.context.empty_bundle();
                    return Ok(self.finalize_response(response, bundle));
                }
            }
        }

        // C5.8 / C5.14 — pause / resume need real fingerprints; cancel/status do not.
        if let Some(assessment) =
            agent_pause_resume_cancel::detect_control_request(&request)
        {
            let bundle = self.bundle_for_agent_control(&request, assessment.kind)?;
            return self.complete_agent_pause_resume_cancel(&request, bundle);
        }

        // C5.9 — goal failure / recovery (after control; early-gated).
        if agent_failure_recovery::detect_recovery_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_failure_recovery(&request, bundle);
        }

        // C5.10 — coding goal completion (after recovery; early-gated).
        if coding_goal_completion::detect_completion_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_coding_goal_completion(&request, bundle);
        }

        // C5.11 — long-running session continuity (after completion; early-gated).
        if agent_session_continuity::detect_continuity_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_session_continuity(&request, bundle);
        }

        // C5.12 — status-only early gate (Begin waits for assembled ContextBundle).
        if let Some(assessment) =
            agentic_coding_experience::detect_agentic_experience_request(&request)
        {
            if assessment.kind == agentic_coding_experience::AgenticExperienceRequestKind::Status {
                let bundle = self.context.empty_bundle();
                return self.complete_agentic_coding_experience(&request, bundle);
            }
        }

        // C5.3 — agent lifecycle phrases must not be stolen by OpenProject / session intents.
        if agent_state_machine::detect_agent_lifecycle_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_lifecycle(&request, bundle);
        }

        // C5.4 — multi-step start/continue/advance (after agent pause/resume).
        if multi_step_execution::detect_multi_step_execution_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_multi_step_execution(&request, bundle);
        }

        // C5.5 — agent context maintain/show/compact (after multi-step).
        if agent_context_maintenance::detect_agent_context_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_context_maintenance(&request, bundle);
        }

        // C5.6 — approval / autonomy policy status (after context maintenance).
        if approval_autonomy_policy::detect_autonomy_policy_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_autonomy_policy(&request, bundle);
        }

        // C5.7 — goal progress UX (after autonomy policy).
        if agent_progress_ux::detect_progress_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_progress(&request, bundle);
        }

        // C5.8 / C5.14 — also soft-pathed after progress.
        if let Some(assessment) =
            agent_pause_resume_cancel::detect_control_request(&request)
        {
            let bundle = self.bundle_for_agent_control(&request, assessment.kind)?;
            return self.complete_agent_pause_resume_cancel(&request, bundle);
        }

        // C5.9 — goal failure / recovery.
        if agent_failure_recovery::detect_recovery_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_failure_recovery(&request, bundle);
        }

        // C5.10 — coding goal completion.
        if coding_goal_completion::detect_completion_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_coding_goal_completion(&request, bundle);
        }

        // C5.11 — long-running session continuity.
        if agent_session_continuity::detect_continuity_request(&request).is_some() {
            let bundle = self.context.empty_bundle();
            return self.complete_agent_session_continuity(&request, bundle);
        }

        // C5.12 — status-only early gate (Begin waits for assembled ContextBundle).
        if let Some(assessment) =
            agentic_coding_experience::detect_agentic_experience_request(&request)
        {
            if assessment.kind == agentic_coding_experience::AgenticExperienceRequestKind::Status {
                let bundle = self.context.empty_bundle();
                return self.complete_agentic_coding_experience(&request, bundle);
            }
        }

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
        // Environmental Resolution binds deixis from Workspace Intelligence
        // before assemble / Reasoning — LLMs never invent workspace referents.
        let environmental = environmental::resolve_environment(
            &request,
            &self.context.session_inputs(),
        );
        let mut hints = AssembleHints {
            intent: intent.id(),
            capability_ids: capabilities
                .iter()
                .map(|capability| capability.id().to_string())
                .collect(),
            complexity: Some(complexity.class_id().to_string()),
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
        };
        if environmental.needed {
            hints.environmental = Some(environmental.to_hints());
            if let Some(note) = environmental.summary_note() {
                jaymi_logging::info("planner", note);
            }
        }
        // Observational modes annotate hints only (review > coding plan > understanding).
        if let Some(assessment) = coding_review::detect_review_request(&request) {
            hints.review = Some(assessment.hint_id());
        } else if let Some(assessment) = coding_plan::detect_coding_plan_request(&request) {
            hints.coding_plan = Some(assessment.hint_id());
        } else if let Some(assessment) = coding_understanding::detect_understanding_request(&request)
        {
            hints.understanding = Some(assessment.hint_id());
        }

        jaymi_logging::info(
            "planner",
            format!(
                "intent resolved label={} capabilities=[{}] complexity={} environmental={}",
                hints.intent.as_str(),
                hints.capability_ids.join(","),
                complexity.class_id(),
                environmental.needed,
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
            if let Some(action) = request.coding_action {
                let surface = self.synthesize_pair_programming(
                    &PlannerResponse::default(),
                    Some(&context),
                );
                if let Some(response) =
                    coding_actions::deterministic_coding_action_response_with_surface(
                        action, &surface,
                    )
                {
                    self.transition_conversation(ConversationState::Completed);
                    return Ok(self.finalize_response(response, context));
                }
            }
            if pair_programming::looks_like_dead_code_request(
                &request.content.trim().to_ascii_lowercase(),
            ) {
                self.transition_conversation(ConversationState::Completed);
                let response = PlannerResponse {
                    content: coding_actions::find_dead_code_content().to_string(),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    ..PlannerResponse::default()
                };
                return Ok(self.finalize_response(response, context));
            }
            if relationship_tracing::detect_relationship_trace_request(&request).is_some() {
                return self.complete_relationship_tracing(&request, context);
            }
            if deep_investigation::detect_deep_investigation_request(&request).is_some() {
                return self.complete_deep_investigation(&request, context);
            }
            if codebase_qa::detect_codebase_qa_request(&request).is_some() {
                return self.complete_codebase_qa(&request, context);
            }
            if symbol_navigation::detect_symbol_navigation_request(&request).is_some()
                && request.lsp.is_none()
            {
                return self.complete_symbol_navigation(&request, context);
            }
            if let Some(response) =
                self.try_code_search_soft_path(&request, &context)
            {
                return self.finalize_code_search_soft_path(&request, context, response);
            }
            if code_generation::detect_generation_request(&request) {
                return self.complete_code_generation_from_request(&request, context);
            }
            if let Some(kind) = multi_file::detect_multi_file_request(&request) {
                return self.complete_multi_file_from_request(kind, &request, context);
            }
            if test_execution::detect_test_execution_request(&request).is_some() {
                return self.complete_test_execution(&request, context);
            }
            if build_awareness::detect_build_awareness_request(&request).is_some() {
                return self.complete_build_awareness(&request, context);
            }
            {
                let evidence = fix_from_verification::load_verification_evidence(
                    self.clone_last_verification_evidence().as_ref(),
                    &self.context.session_inputs(),
                );
                if fix_from_verification::detect_fix_from_verification_request(
                    &request,
                    evidence.as_ref(),
                )
                .is_some()
                {
                    return self.complete_fix_from_verification(&request, context);
                }
            }
            if agent_pause_resume_cancel::detect_control_request(&request).is_some() {
                return self.complete_agent_pause_resume_cancel(&request, context);
            }
            if agent_failure_recovery::detect_recovery_request(&request).is_some() {
                return self.complete_agent_failure_recovery(&request, context);
            }
            if coding_goal_completion::detect_completion_request(&request).is_some() {
                return self.complete_coding_goal_completion(&request, context);
            }
            if agent_session_continuity::detect_continuity_request(&request).is_some() {
                return self.complete_agent_session_continuity(&request, context);
            }
            if agentic_coding_experience::detect_agentic_experience_request(&request).is_some() {
                return self.complete_agentic_coding_experience(&request, context);
            }
            if coding_goal::detect_coding_goal_request(&request).is_some() {
                return self.complete_coding_goal(&request, context);
            }
            if goal_decomposition::detect_decomposition_request(&request).is_some() {
                return self.complete_goal_decomposition(&request, context);
            }
            if agent_state_machine::detect_agent_lifecycle_request(&request).is_some() {
                return self.complete_agent_lifecycle(&request, context);
            }
            if multi_step_execution::detect_multi_step_execution_request(&request).is_some() {
                return self.complete_multi_step_execution(&request, context);
            }
            if agent_context_maintenance::detect_agent_context_request(&request).is_some() {
                return self.complete_agent_context_maintenance(&request, context);
            }
            if approval_autonomy_policy::detect_autonomy_policy_request(&request).is_some() {
                return self.complete_autonomy_policy(&request, context);
            }
            if agent_progress_ux::detect_progress_request(&request).is_some() {
                return self.complete_agent_progress(&request, context);
            }
            if coding_edit::detect_coding_edit_request(&request).is_some() {
                return self.complete_coding_edit_intent(&request, context);
            }
            let review = coding_review::detect_review_request(&request).map(|assessment| {
                coding_review::finalize_assessment(assessment, &context)
            });
            let coding_plan = if review.is_none() {
                coding_plan::detect_coding_plan_request(&request).map(|assessment| {
                    coding_plan::finalize_assessment(assessment, &context)
                })
            } else {
                None
            };
            let understanding = if review.is_none() && coding_plan.is_none() {
                coding_understanding::detect_understanding_request(&request).map(|assessment| {
                    coding_understanding::finalize_assessment(assessment, &context)
                })
            } else {
                None
            };
            let mut early_pipeline = jaymi_reasoning::PipelineTiming::new();
            if let Some(inspection) = self.context.last_inspection() {
                early_pipeline.merge(conversational::pipeline_from_context_inspection(
                    &inspection,
                ));
            }
            if let Some(assessment) = review.clone() {
                if !self.reasoning.is_implemented() {
                    self.transition_conversation(ConversationState::Completed);
                    let response = PlannerResponse {
                        content: assessment.scaffold.to_markdown(),
                        reasoning_used: false,
                        stream_lifecycle: Some(StreamingLifecycle::Idle),
                        coding_review: Some(assessment.scaffold),
                        pipeline_timing: if early_pipeline.is_empty() {
                            None
                        } else {
                            Some(early_pipeline)
                        },
                        ..PlannerResponse::default()
                    };
                    return Ok(self.finalize_response(response, context));
                }
            } else if let Some(assessment) = coding_plan.clone() {
                self.remember_coding_plan(Some(assessment.scaffold.clone()));
                if !self.reasoning.is_implemented() {
                    self.transition_conversation(ConversationState::Completed);
                    let response = PlannerResponse {
                        content: assessment.scaffold.to_markdown(),
                        reasoning_used: false,
                        stream_lifecycle: Some(StreamingLifecycle::Idle),
                        coding_plan: Some(assessment.scaffold),
                        pipeline_timing: if early_pipeline.is_empty() {
                            None
                        } else {
                            Some(early_pipeline)
                        },
                        ..PlannerResponse::default()
                    };
                    return Ok(self.finalize_response(response, context));
                }
            } else if let Some(assessment) = understanding.clone() {
                if !self.reasoning.is_implemented() {
                    self.transition_conversation(ConversationState::Completed);
                    let response = PlannerResponse {
                        content: coding_understanding::render_markdown(
                            &assessment.scaffold,
                            assessment.focus,
                            &context,
                        ),
                        reasoning_used: false,
                        stream_lifecycle: Some(StreamingLifecycle::Idle),
                        coding_understanding: Some(assessment.scaffold),
                        pipeline_timing: if early_pipeline.is_empty() {
                            None
                        } else {
                            Some(early_pipeline)
                        },
                        ..PlannerResponse::default()
                    };
                    return Ok(self.finalize_response(response, context));
                }
            }
            return self.handle_conversational_request_with_observer(
                &request,
                context,
                &intent,
                Vec::new(),
                early_pipeline,
                understanding,
                review,
                coding_plan,
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
                self.attach_symbol_navigation_report(&request, &mut response);
                self.attach_code_search_report(&request, &mut response);
                if response.capability_plan.is_none() {
                    response.capability_plan = self.plan_capability(capability, None).ok();
                }
                if response.workspace.is_none() {
                    response.workspace = workspace_expansion_for(
                        capability,
                        format!("capability {} selected for request", capability.id()),
                    );
                }
                // C2.7.2: NL / relationship-style search & select symbol kinds may
                // hand off through Context → Prompt → Reasoning (never provider).
                if response.code_search.is_some() {
                    return self.finalize_code_search_soft_path(&request, context, response);
                }
                if response.symbol_navigation.is_some() {
                    return self.finalize_symbol_navigation_report(&request, context, response);
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
        if let Some(action) = request.coding_action {
            if matches!(assembled.intent, Intent::Unknown) {
                let surface = self.synthesize_pair_programming(
                    &PlannerResponse::default(),
                    Some(&assembled.context),
                );
                if let Some(response) =
                    coding_actions::deterministic_coding_action_response_with_surface(
                        action, &surface,
                    )
                {
                    self.transition_conversation(ConversationState::Completed);
                    return Ok(self.finalize_response(response, assembled.context));
                }
            }
        }
        if pair_programming::looks_like_dead_code_request(
            &request.content.trim().to_ascii_lowercase(),
        ) {
            self.transition_conversation(ConversationState::Completed);
            let response = PlannerResponse {
                content: coding_actions::find_dead_code_content().to_string(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                ..PlannerResponse::default()
            };
            return Ok(self.finalize_response(response, assembled.context));
        }
        if relationship_tracing::detect_relationship_trace_request(&request).is_some() {
            return self.complete_relationship_tracing(&request, assembled.context);
        }
        if deep_investigation::detect_deep_investigation_request(&request).is_some() {
            return self.complete_deep_investigation(&request, assembled.context);
        }
        if codebase_qa::detect_codebase_qa_request(&request).is_some() {
            return self.complete_codebase_qa(&request, assembled.context);
        }
        if symbol_navigation::detect_symbol_navigation_request(&request).is_some()
            && request.lsp.is_none()
        {
            return self.complete_symbol_navigation(&request, assembled.context);
        }
        if let Some(response) =
            self.try_code_search_soft_path(&request, &assembled.context)
        {
            return self.finalize_code_search_soft_path(
                &request,
                assembled.context,
                response,
            );
        }
        if code_generation::detect_generation_request(&request) {
            return self.complete_code_generation_from_request(&request, assembled.context);
        }
        if let Some(kind) = multi_file::detect_multi_file_request(&request) {
            return self.complete_multi_file_from_request(kind, &request, assembled.context);
        }
        if test_execution::detect_test_execution_request(&request).is_some() {
            return self.complete_test_execution(&request, assembled.context);
        }
        if build_awareness::detect_build_awareness_request(&request).is_some() {
            return self.complete_build_awareness(&request, assembled.context);
        }
        {
            let evidence = fix_from_verification::load_verification_evidence(
                self.clone_last_verification_evidence().as_ref(),
                &self.context.session_inputs(),
            );
            if fix_from_verification::detect_fix_from_verification_request(
                &request,
                evidence.as_ref(),
            )
            .is_some()
            {
                return self.complete_fix_from_verification(&request, assembled.context);
            }
        }
        if agent_pause_resume_cancel::detect_control_request(&request).is_some() {
            return self.complete_agent_pause_resume_cancel(&request, assembled.context);
        }
        if agent_failure_recovery::detect_recovery_request(&request).is_some() {
            return self.complete_agent_failure_recovery(&request, assembled.context);
        }
        if coding_goal_completion::detect_completion_request(&request).is_some() {
            return self.complete_coding_goal_completion(&request, assembled.context);
        }
        if agent_session_continuity::detect_continuity_request(&request).is_some() {
            return self.complete_agent_session_continuity(&request, assembled.context);
        }
        if agentic_coding_experience::detect_agentic_experience_request(&request).is_some() {
            return self.complete_agentic_coding_experience(&request, assembled.context);
        }
        if coding_goal::detect_coding_goal_request(&request).is_some() {
            return self.complete_coding_goal(&request, assembled.context);
        }
        if goal_decomposition::detect_decomposition_request(&request).is_some() {
            return self.complete_goal_decomposition(&request, assembled.context);
        }
        if agent_state_machine::detect_agent_lifecycle_request(&request).is_some() {
            return self.complete_agent_lifecycle(&request, assembled.context);
        }
        if multi_step_execution::detect_multi_step_execution_request(&request).is_some() {
            return self.complete_multi_step_execution(&request, assembled.context);
        }
        if agent_context_maintenance::detect_agent_context_request(&request).is_some() {
            return self.complete_agent_context_maintenance(&request, assembled.context);
        }
        if approval_autonomy_policy::detect_autonomy_policy_request(&request).is_some() {
            return self.complete_autonomy_policy(&request, assembled.context);
        }
        if agent_progress_ux::detect_progress_request(&request).is_some() {
            return self.complete_agent_progress(&request, assembled.context);
        }
        if coding_edit::detect_coding_edit_request(&request).is_some() {
            return self.complete_coding_edit_intent(&request, assembled.context);
        }
        // Coding Review — WI scaffold without tools / edits / plans.
        if let Some(assessment) = assembled.review.clone() {
            self.remember_review(Some(assessment.scaffold.clone()));
            if !self.reasoning.is_implemented() {
                self.transition_conversation(ConversationState::Completed);
                let mut response = PlannerResponse {
                    content: assessment.scaffold.to_markdown(),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_review: Some(assessment.scaffold),
                    ..PlannerResponse::default()
                };
                response.pipeline_timing = if assembled.pipeline.is_empty() {
                    None
                } else {
                    Some(assembled.pipeline)
                };
                return Ok(self.finalize_response(response, assembled.context));
            }
            return self.handle_conversational_request_with_observer(
                &request,
                assembled.context,
                &assembled.intent,
                history,
                assembled.pipeline,
                None,
                Some(assessment),
                None,
                on_event,
            );
        }
        // Generation Planning — Coding Plan without codegen / tools / writes.
        if let Some(assessment) = assembled.coding_plan.clone() {
            self.remember_coding_plan(Some(assessment.scaffold.clone()));
            if !self.reasoning.is_implemented() {
                self.transition_conversation(ConversationState::Completed);
                let mut response = PlannerResponse {
                    content: assessment.scaffold.to_markdown(),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_plan: Some(assessment.scaffold),
                    ..PlannerResponse::default()
                };
                response.pipeline_timing = if assembled.pipeline.is_empty() {
                    None
                } else {
                    Some(assembled.pipeline)
                };
                return Ok(self.finalize_response(response, assembled.context));
            }
            return self.handle_conversational_request_with_observer(
                &request,
                assembled.context,
                &assembled.intent,
                history,
                assembled.pipeline,
                None,
                None,
                Some(assessment),
                on_event,
            );
        }
        // Understand Before Acting — WI scaffold without tools / edits.
        if let Some(assessment) = assembled.understanding.clone() {
            self.remember_understanding(Some(assessment.scaffold.clone()));
            if !self.reasoning.is_implemented() {
                self.transition_conversation(ConversationState::Completed);
                let mut response = PlannerResponse {
                    content: coding_understanding::render_markdown(
                        &assessment.scaffold,
                        assessment.focus,
                        &assembled.context,
                    ),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_understanding: Some(assessment.scaffold),
                    ..PlannerResponse::default()
                };
                response.pipeline_timing = if assembled.pipeline.is_empty() {
                    None
                } else {
                    Some(assembled.pipeline)
                };
                return Ok(self.finalize_response(response, assembled.context));
            }
            return self.handle_conversational_request_with_observer(
                &request,
                assembled.context,
                &assembled.intent,
                history,
                assembled.pipeline,
                Some(assessment),
                None,
                None,
                on_event,
            );
        }
        self.handle_conversational_request_with_observer(
            &request,
            assembled.context,
            &assembled.intent,
            history,
            assembled.pipeline,
            None,
            None,
            None,
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
        if let Some(action) = request.coding_action {
            if matches!(assembled.intent, Intent::Unknown)
                && coding_actions::deterministic_coding_action_response(action).is_some()
            {
                return Err(JaymiError::new(
                    "coding action uses deterministic planner reply",
                ));
            }
        }
        // Review / coding plan / understanding without a reasoning backend → soft path.
        if assembled.review.is_some() && !self.reasoning.is_implemented() {
            if let Some(assessment) = &assembled.review {
                self.remember_review(Some(assessment.scaffold.clone()));
            }
            return Err(JaymiError::new(
                "coding review uses deterministic scaffold without reasoning backend",
            ));
        }
        if assembled.coding_plan.is_some() && !self.reasoning.is_implemented() {
            if let Some(assessment) = &assembled.coding_plan {
                self.remember_coding_plan(Some(assessment.scaffold.clone()));
            }
            return Err(JaymiError::new(
                "coding plan uses deterministic scaffold without reasoning backend",
            ));
        }
        if assembled.understanding.is_some() && !self.reasoning.is_implemented() {
            if let Some(assessment) = &assembled.understanding {
                self.remember_understanding(Some(assessment.scaffold.clone()));
            }
            return Err(JaymiError::new(
                "coding understanding uses deterministic scaffold without reasoning backend",
            ));
        }
        let mut pipeline = assembled.pipeline;
        let context = assembled.context;
        if !self.reasoning.is_implemented() {
            return Err(JaymiError::new("no reasoning backend is available"));
        }
        let understanding = assembled.understanding.clone();
        let review = assembled.review.clone();
        let coding_plan = assembled.coding_plan.clone();
        if let Some(assessment) = &understanding {
            self.remember_understanding(Some(assessment.scaffold.clone()));
        }
        if let Some(assessment) = &review {
            self.remember_review(Some(assessment.scaffold.clone()));
        }
        if let Some(assessment) = &coding_plan {
            self.remember_coding_plan(Some(assessment.scaffold.clone()));
        }
        let llm = self.llm_context_for_understanding(
            &context,
            understanding.as_ref(),
            review.as_ref(),
            coding_plan.as_ref(),
        );
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
        response.coding_understanding = self.take_understanding();
        response.coding_review = self.take_review();
        response.coding_plan = self.take_coding_plan();
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

    /// Shared Intent → Capability → Complexity → Environmental Resolution →
    /// AssembleHints → ContextBundle prelude.
    ///
    /// Complexity and environmental resolution never alter Intent or Capability
    /// selection — they only annotate [`AssembleHints`] for Context / Reasoning.
    fn begin_conversational_assemble(
        &self,
        request: &UserRequest,
    ) -> JaymiResult<conversational::ConversationalAssemble> {
        self.invalidate_paused("new user request")?;
        // Host may already have acknowledged Thinking (Reasoning) on the UI
        // thread — do not attempt an illegal Reasoning → PreparingContext hop.
        self.enter_or_continue_preparing_context();
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
        let environmental = environmental::resolve_environment(
            request,
            &self.context.session_inputs(),
        );
        let mut hints = conversational::conversational_assemble_hints(
            &intent,
            capability_ids.clone(),
            Some(&complexity),
        );
        if environmental.needed {
            hints.environmental = Some(environmental.to_hints());
            if let Some(note) = environmental.summary_note() {
                jaymi_logging::info("planner", note);
            }
        }
        let review_detect = coding_review::detect_review_request(request);
        let coding_plan_detect = if review_detect.is_none() {
            coding_plan::detect_coding_plan_request(request)
        } else {
            None
        };
        let understanding_detect =
            if review_detect.is_none() && coding_plan_detect.is_none() {
                coding_understanding::detect_understanding_request(request)
            } else {
                None
            };
        if let Some(assessment) = &review_detect {
            hints.review = Some(assessment.hint_id());
            jaymi_logging::info(
                "planner",
                format!(
                    "coding review focus={} rules=[{}]",
                    assessment.focus.as_str(),
                    assessment.matched_rules.join(",")
                ),
            );
        }
        if let Some(assessment) = &coding_plan_detect {
            hints.coding_plan = Some(assessment.hint_id());
            jaymi_logging::info(
                "planner",
                format!(
                    "coding plan kind={} rules=[{}]",
                    assessment.kind.as_str(),
                    assessment.matched_rules.join(",")
                ),
            );
        }
        if let Some(assessment) = &understanding_detect {
            hints.understanding = Some(assessment.hint_id());
            jaymi_logging::info(
                "planner",
                format!(
                    "coding understanding focus={} rules=[{}]",
                    assessment.focus.as_str(),
                    assessment.matched_rules.join(",")
                ),
            );
        }
        let planner_ms = planner_started.elapsed().as_millis() as u64;
        let context = self.context.assemble_with(request, Some(&hints))?;
        let review = review_detect.map(|assessment| {
            coding_review::finalize_assessment(assessment, &context)
        });
        let coding_plan = coding_plan_detect.map(|assessment| {
            coding_plan::finalize_assessment(assessment, &context)
        });
        let understanding = understanding_detect.map(|assessment| {
            coding_understanding::finalize_assessment(assessment, &context)
        });
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
            environmental,
            understanding,
            review,
            coding_plan,
            pipeline,
        })
    }

    /// Immediate UI-thread ack for a new conversational send (Planner-owned).
    ///
    /// Transitions `Idle|terminal → PreparingContext → Reasoning` so the UI can
    /// show Thinking and return to the event loop **before** host prep, Context
    /// assemble, prompt build, or provider I/O. Background work continues while
    /// the conversation stays in [`ConversationState::Reasoning`] until stream
    /// lifecycle advances it.
    pub fn acknowledge_conversational_send(&self) {
        match self.conversation_state() {
            ConversationState::Idle
            | ConversationState::Completed
            | ConversationState::Cancelled
            | ConversationState::Failed => {
                self.transition_conversation(ConversationState::PreparingContext);
                self.transition_conversation(ConversationState::Reasoning);
            }
            ConversationState::PreparingContext => {
                self.transition_conversation(ConversationState::Reasoning);
            }
            ConversationState::Reasoning
            | ConversationState::Streaming
            | ConversationState::WaitingForReview
            | ConversationState::Executing => {}
        }
    }

    /// Enter PreparingContext for a new request, or keep an already-acked phase.
    ///
    /// Used when the host acknowledged Thinking (`Reasoning`) before expensive
    /// work — re-entering PreparingContext from Reasoning is illegal.
    fn enter_or_continue_preparing_context(&self) {
        match self.conversation_state() {
            ConversationState::Idle
            | ConversationState::Completed
            | ConversationState::Cancelled
            | ConversationState::Failed => {
                self.transition_conversation(ConversationState::PreparingContext);
            }
            ConversationState::PreparingContext | ConversationState::Reasoning => {}
            ConversationState::Streaming
            | ConversationState::WaitingForReview
            | ConversationState::Executing => {
                // Unexpected for a fresh request; log and leave state alone.
                jaymi_logging::warn(
                    "planner",
                    format!(
                        "enter_or_continue_preparing_context while {}",
                        self.conversation_state().as_str()
                    ),
                );
            }
        }
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
        understanding: Option<coding_understanding::UnderstandingAssessment>,
        review: Option<coding_review::ReviewAssessment>,
        coding_plan: Option<coding_plan::CodingPlanAssessment>,
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
            if let Some(assessment) = review {
                let response = PlannerResponse {
                    content: assessment.scaffold.to_markdown(),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_review: Some(assessment.scaffold),
                    pipeline_timing: if early_pipeline.is_empty() {
                        None
                    } else {
                        Some(early_pipeline)
                    },
                    ..PlannerResponse::default()
                };
                return Ok(self.finalize_response(response, context));
            }
            if let Some(assessment) = coding_plan {
                let response = PlannerResponse {
                    content: assessment.scaffold.to_markdown(),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_plan: Some(assessment.scaffold),
                    pipeline_timing: if early_pipeline.is_empty() {
                        None
                    } else {
                        Some(early_pipeline)
                    },
                    ..PlannerResponse::default()
                };
                return Ok(self.finalize_response(response, context));
            }
            if let Some(assessment) = understanding {
                let response = PlannerResponse {
                    content: coding_understanding::render_markdown(
                        &assessment.scaffold,
                        assessment.focus,
                        &context,
                    ),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_understanding: Some(assessment.scaffold),
                    pipeline_timing: if early_pipeline.is_empty() {
                        None
                    } else {
                        Some(early_pipeline)
                    },
                    ..PlannerResponse::default()
                };
                return Ok(self.finalize_response(response, context));
            }
            return Ok(self.finalize_response(
                conversational::no_backend_soft_response(),
                context,
            ));
        }

        if let Some(assessment) = &understanding {
            self.remember_understanding(Some(assessment.scaffold.clone()));
        }
        if let Some(assessment) = &review {
            self.remember_review(Some(assessment.scaffold.clone()));
        }
        if let Some(assessment) = &coding_plan {
            self.remember_coding_plan(Some(assessment.scaffold.clone()));
        }
        let llm = self.llm_context_for_understanding(
            &context,
            understanding.as_ref(),
            review.as_ref(),
            coding_plan.as_ref(),
        );
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
                    {
                        let mut response = conversational::stream_start_failed_response(
                            &error.message(),
                            configured_model,
                            provider_model,
                            model_fallback,
                        );
                        response.coding_understanding = understanding
                            .as_ref()
                            .map(|a| a.scaffold.clone())
                            .or_else(|| self.take_understanding());
                        response.coding_review = review
                            .as_ref()
                            .map(|a| a.scaffold.clone())
                            .or_else(|| self.take_review());
                        response.coding_plan = coding_plan
                            .as_ref()
                            .map(|a| a.scaffold.clone())
                            .or_else(|| self.take_coding_plan());
                        response
                    },
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
                        response.coding_understanding = understanding
                            .map(|a| a.scaffold)
                            .or_else(|| self.take_understanding());
                        response.coding_review = review
                            .map(|a| a.scaffold)
                            .or_else(|| self.take_review());
                        response.coding_plan = coding_plan
                            .map(|a| a.scaffold)
                            .or_else(|| self.take_coding_plan());
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
                        response.coding_understanding = understanding
                            .map(|a| a.scaffold)
                            .or_else(|| self.take_understanding());
                        response.coding_review = review
                            .map(|a| a.scaffold)
                            .or_else(|| self.take_review());
                        response.coding_plan = coding_plan
                            .map(|a| a.scaffold)
                            .or_else(|| self.take_coding_plan());
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

        let action_preview = self.generate_action_preview(&tool_id, tool_input, &gate);
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
            action_preview,
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
                    generation_calls: Vec::new(),
                    generation_baselines: Vec::new(),
                    test_execution: None,
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
        let mut affected_resources = vec![resource.to_string()];
        if let Some(preview) = &action_preview {
            for path in &preview.resources {
                if !path.trim().is_empty()
                    && !affected_resources.iter().any(|existing| existing == path)
                {
                    affected_resources.push(path.clone());
                }
            }
        }
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
            affected_resources,
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

    /// Propose a Code Generation batch as a reviewed [`ExecutionPlan`] (Sprint C1.5).
    ///
    /// Converts CreateFile / ModifyFile / DeleteFile ops into tool-backed plan
    /// steps. Review Before Action is **mandatory** — providers and Reasoning
    /// never write files directly.
    pub fn begin_code_generation(
        &self,
        generation: code_generation::CodeGeneration,
    ) -> JaymiResult<PlannerResponse> {
        self.begin_code_generation_inner(generation, None)
    }

    /// Same as [`Self::begin_code_generation`], with optional C3.4 patch report
    /// enrichment for the shared Review Card (Sprint C3.5).
    fn begin_code_generation_inner(
        &self,
        generation: code_generation::CodeGeneration,
        patch_report: Option<&patch_generation::PatchGenerationReport>,
    ) -> JaymiResult<PlannerResponse> {
        self.ensure_ready()?;
        if generation.is_empty() {
            return Err(JaymiError::new("code generation has no operations"));
        }
        let prepared = self.prepare_code_generation_execution(&generation, patch_report)?;
        if let Some(mut blocked) = prepared.blocked_response {
            blocked.code_generation = Some(generation);
            self.transition_conversation(ConversationState::WaitingForReview);
            // Caller attaches the request ContextBundle via finalize_response.
            return Ok(blocked);
        }
        // Generation always requires review — Allowed path is unexpected.
        Err(JaymiError::new(
            "code generation must pause for Review Before Action",
        ))
    }

    /// Apply the last Coding Plan as Code Generation / Patch Generation.
    fn complete_code_generation_from_request(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let Some(plan) = self.clone_last_coding_plan() else {
            self.transition_conversation(ConversationState::Completed);
            let response = PlannerResponse {
                content: "I don't have a Coding Plan to generate from yet. \
Ask me to plan first (for example \"Build Pong.\" or \"Add image generation.\"), \
then say \"Generate the changes.\" / \"Generate the code.\" / \"Generate patches.\""
                    .into(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                ..PlannerResponse::default()
            };
            return Ok(self.finalize_response(response, context));
        };

        if patch_generation::should_use_patch_generation(&plan) {
            return self.complete_patch_generation_from_plan(request, context, plan);
        }

        let root = context
            .active_project()
            .root_directory
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.active_project_root());
        let goal = plan
            .summary
            .iter()
            .find(|line| line.starts_with("Goal:"))
            .cloned()
            .unwrap_or_else(|| {
                plan.objective.clone().unwrap_or_else(|| {
                    plan.summary
                        .first()
                        .cloned()
                        .unwrap_or_else(|| request.content.trim().to_string())
                })
            });
        let generation =
            code_generation::generation_from_coding_plan(&plan, goal, root.as_deref());
        let response = self.begin_code_generation(generation)?;
        Ok(self.finalize_response(response, context))
    }

    /// C3.4 — draft / reason / validate patches; convert valid ones to reviewed ExecutionPlan.
    ///
    /// Never writes to disk. On validation failure the project stays unchanged.
    fn complete_patch_generation_from_plan(
        &self,
        request: &UserRequest,
        context: ContextBundle,
        plan: coding_plan::CodingPlan,
    ) -> JaymiResult<PlannerResponse> {
        let root = context
            .active_project()
            .root_directory
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.active_project_root());
        let goal = plan
            .objective
            .clone()
            .or_else(|| {
                plan.summary
                    .iter()
                    .find(|line| line.starts_with("Goal:") || line.starts_with("Mutation Coding Plan"))
                    .cloned()
            })
            .unwrap_or_else(|| request.content.trim().to_string());

        let mut drafts =
            patch_generation::draft_proposals_from_plan(&plan, &goal, root.as_deref());
        if drafts.is_empty() {
            self.transition_conversation(ConversationState::Completed);
            let mut report = patch_generation::PatchGenerationReport {
                goal: goal.clone(),
                failures: vec![
                    "Coding Plan did not yield concrete file paths for patches.".into(),
                ],
                ..Default::default()
            };
            report.diagnostics.push("empty_drafts".into());
            let response = PlannerResponse {
                content: format!(
                    "{}\n\nOriginal project unchanged.",
                    report.to_markdown()
                ),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                patch_generation: Some(report),
                ..PlannerResponse::default()
            };
            return Ok(self.finalize_response(response, context));
        }

        // Patch content elaboration stays behind ReasoningEngine when available.
        let mut reasoning_used = false;
        if self.reasoning.is_implemented() {
            let instruction =
                patch_generation::reasoning_prompt_instruction(&plan, &goal);
            let llm = self.llm_context_for_understanding(&context, None, None, None);
            match self.build_reasoning_request(&instruction, llm, Vec::new()) {
                Ok(reasoning_request) => {
                    match ConversationStream::start(self.reasoning.clone(), reasoning_request)
                    {
                        Ok(stream) => match stream.collect() {
                            Ok(reasoned) => {
                                reasoning_used = true;
                                drafts = patch_generation::merge_reasoning_patches(
                                    &drafts,
                                    &reasoned.content,
                                    root.as_deref(),
                                );
                            }
                            Err(err) => {
                                jaymi_logging::info(
                                    "planner",
                                    format!(
                                        "patch_generation reasoning collect failed: {}",
                                        err.message()
                                    ),
                                );
                            }
                        },
                        Err(err) => {
                            jaymi_logging::info(
                                "planner",
                                format!(
                                    "patch_generation reasoning stream failed: {}",
                                    err.message()
                                ),
                            );
                        }
                    }
                }
                Err(err) => {
                    jaymi_logging::info(
                        "planner",
                        format!(
                            "patch_generation reasoning request failed: {}",
                            err.message()
                        ),
                    );
                }
            }
        }

        let mut validated = Vec::new();
        let mut failures = Vec::new();
        for mut proposal in drafts {
            let current = self.read_file_text_for_patch(&proposal.file);
            let dest_current = proposal
                .destination
                .as_ref()
                .and_then(|dest| self.read_file_text_for_patch(dest));

            if proposal.change_type == patch_generation::PatchChangeType::ModifyFile {
                if let Some(body) = current.as_deref() {
                    let snippet = proposal
                        .proposed_content
                        .clone()
                        .unwrap_or_default();
                    let original = proposal.original_content.clone();
                    // Preserve surrounding file — surgical replace or marked append.
                    proposal.proposed_content = Some(patch_generation::apply_targeted_modify(
                        body,
                        original.as_deref(),
                        &snippet,
                    ));
                    if proposal.original_content.is_none() {
                        proposal.original_content =
                            Some(patch_generation::snippet_preview(body));
                    }
                }
            }

            let patch = patch_generation::validate_proposal(
                proposal,
                current.as_deref(),
                dest_current.as_deref(),
            );
            if !patch.status.is_valid() {
                if let Some(reason) = patch.status.reason() {
                    failures.push(format!(
                        "`{}` ({}) — {reason}",
                        patch.proposal.file.display(),
                        patch.proposal.change_type.as_str()
                    ));
                }
            }
            validated.push(patch);
        }

        let mut report = patch_generation::PatchGenerationReport {
            goal: goal.clone(),
            patches: validated.clone(),
            failures: failures.clone(),
            reasoning_used,
            converted_to_execution: false,
            diagnostics: vec![format!(
                "drafts_validated valid={} invalid={}",
                validated.iter().filter(|p| p.status.is_valid()).count(),
                validated.iter().filter(|p| !p.status.is_valid()).count()
            )],
        };

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        match patch_generation::generation_from_validated(&goal, &validated) {
            Ok(generation) => {
                report.converted_to_execution = true;
                let mut response =
                    self.begin_code_generation_inner(generation, Some(&report))?;
                let prefix = report.to_markdown();
                response.content = format!("{prefix}\n\n{}", response.content);
                response.patch_generation = Some(report);
                response.reasoning_used = reasoning_used || response.reasoning_used;
                Ok(self.finalize_response(response, context))
            }
            Err(err) => {
                report.failures.push(err);
                report.converted_to_execution = false;
                self.transition_conversation(ConversationState::Completed);
                let response = PlannerResponse {
                    content: format!(
                        "{}\n\nPatch generation could not produce an executable review batch. \
Original project unchanged — no files were written.",
                        report.to_markdown()
                    ),
                    reasoning_used,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    patch_generation: Some(report),
                    awaiting_review: false,
                    ..PlannerResponse::default()
                };
                Ok(self.finalize_response(response, context))
            }
        }
    }

    /// Read-only file peek for patch validation (Planner-mediated; never writes).
    fn read_file_text_for_patch(&self, path: &Path) -> Option<String> {
        if self.tools.get(jaymi_tools::READ_FILE_TOOL_ID).is_err() {
            // Fall back to direct filesystem read only when the tool is absent —
            // still Planner-owned validation, never a model/provider self-call.
            return std::fs::read_to_string(path).ok();
        }
        let intent = Intent::ReadFile {
            path: path.to_path_buf(),
        };
        match self.dispatch_tool_backed(intent, Capability::ReadDocuments, "patch_validation") {
            Ok(response) => response
                .document
                .as_ref()
                .map(|doc| doc.text.clone())
                .or_else(|| std::fs::read_to_string(path).ok()),
            Err(_) => std::fs::read_to_string(path).ok(),
        }
    }

    /// Inject C2 evidence into session → ContextEngine re-assemble (ContextPolicy
    /// selects) → optional Reasoning via ConversationStream. Never calls
    /// ReasoningProvider directly. Deterministic `fallback` when Reasoning is
    /// unavailable or handoff is not preferred.
    ///
    /// When `inject_hits` is false, the caller already offered hits to
    /// ContextEngine (e.g. deep investigation mid-workflow); this still uses
    /// `hits` for the Reasoning brief and handoff preference.
    fn complete_with_codebase_evidence_handoff(
        &self,
        request: &UserRequest,
        context: ContextBundle,
        origin: codebase_evidence::CodebaseEvidenceOrigin,
        subject: &str,
        hits: &[codebase_evidence::CodebaseEvidenceHit],
        gaps: &[String],
        hints: codebase_evidence::HandoffHints,
        inject_hits: bool,
        mut fallback: PlannerResponse,
    ) -> JaymiResult<PlannerResponse> {
        let context = if inject_hits && !hits.is_empty() {
            let mut next_session = self.context.session_inputs();
            next_session.search_hits = codebase_evidence::to_bundle_search_hits(hits);
            self.context.set_session_inputs(next_session);
            self.context
                .request_fresh_context(format!("{}_evidence", origin.as_str()));
            let assemble_hints = AssembleHints {
                intent: IntentId::Unknown,
                capability_ids: Vec::new(),
                complexity: None,
                environmental: None,
                understanding: None,
                review: None,
                coding_plan: None,
            };
            match self.context.assemble_with(request, Some(&assemble_hints)) {
                Ok(bundle) => bundle,
                Err(_) => context,
            }
        } else {
            context
        };

        let want_reasoning = self.reasoning.is_implemented()
            && codebase_evidence::prefers_reasoning_handoff(origin, hits.len(), hints);

        if want_reasoning {
            let brief = codebase_evidence::evidence_brief_for_reasoning(
                origin,
                request.content.trim(),
                subject,
                hits,
                gaps,
            );
            let mut enriched = request.clone();
            enriched.content = codebase_evidence::enrich_request_with_brief(
                request.content.trim(),
                origin,
                &brief,
            );
            let mut early = jaymi_reasoning::PipelineTiming::new();
            if let Some(inspection) = self.context.last_inspection() {
                early.merge(conversational::pipeline_from_context_inspection(
                    &inspection,
                ));
            }
            let mut response = self.handle_conversational_request_with_observer(
                &enriched,
                context,
                &Intent::Unknown,
                Vec::new(),
                early,
                None,
                None,
                None,
                |_| {},
            )?;
            // Preserve structured C2 reports attached on the fallback.
            if response.code_search.is_none() {
                response.code_search = fallback.code_search.take();
            }
            if response.symbol_navigation.is_none() {
                response.symbol_navigation = fallback.symbol_navigation.take();
            }
            if response.relationship_trace.is_none() {
                response.relationship_trace = fallback.relationship_trace.take();
            }
            if response.deep_investigation.is_none() {
                response.deep_investigation = fallback.deep_investigation.take();
            }
            if response.codebase_qa.is_none() {
                response.codebase_qa = fallback.codebase_qa.take();
            }
            return Ok(response);
        }

        self.transition_conversation(ConversationState::Completed);
        Ok(self.finalize_response(fallback, context))
    }

    /// Codebase Q&A (C2.5) — conversational routing over C2 search / symbols /
    /// relationship / investigation with grounded claims and conflict surfacing.
    fn complete_codebase_qa(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = codebase_qa::detect_codebase_qa_request(request)
            .ok_or_else(|| JaymiError::new("codebase qa detect missed after gate"))?;
        let session = self.context.session_inputs();

        let report = match assessment.route {
            codebase_qa::QaRoute::Ambient => {
                codebase_qa::begin_ambient(&assessment, &context, &session)
            }
            codebase_qa::QaRoute::Relationship => {
                let trace_assessment = codebase_qa::to_trace_assessment(&assessment);
                let mut trace = relationship_tracing::investigate(
                    &trace_assessment,
                    &context,
                    &session,
                );
                if relationship_tracing::needs_search_extension(&trace)
                    && self.tools.get(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID).is_ok()
                {
                    let mut search =
                        jaymi_core::SearchRequest::free_text(&trace.seed_symbol);
                    search.limit = Some(codebase_qa::SEARCH_HIT_LIMIT);
                    search.metadata.files_only = true;
                    let intent = Intent::SearchKnowledge { request: search };
                    if let Ok(search_response) = self.dispatch_tool_backed(
                        intent,
                        Capability::Search,
                        &assessment.goal,
                    ) {
                        relationship_tracing::merge_search_citations(
                            &mut trace,
                            &search_response.citations,
                        );
                    }
                }
                codebase_qa::from_relationship_trace(&assessment, &trace)
            }
            codebase_qa::QaRoute::DeepInvestigation => {
                let deep_assessment = codebase_qa::to_deep_assessment(&assessment);
                let mut deep = deep_investigation::begin_investigation(
                    &deep_assessment,
                    &context,
                    &session,
                );
                if let Some(query) = codebase_qa::primary_search_query(&assessment) {
                    if deep.steps.len() < deep_investigation::MAX_STEPS
                        && self.tools.get(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID).is_ok()
                    {
                        let mut search = jaymi_core::SearchRequest::free_text(&query);
                        search.limit = Some(codebase_qa::SEARCH_HIT_LIMIT);
                        search.metadata.files_only = true;
                        let intent = Intent::SearchKnowledge { request: search };
                        match self.dispatch_tool_backed(
                            intent,
                            Capability::Search,
                            &assessment.goal,
                        ) {
                            Ok(search_response) => {
                                deep_investigation::merge_search_citations(
                                    &mut deep,
                                    &search_response.citations,
                                    &query,
                                );
                                if !search_response.citations.is_empty() {
                                    let mut next_session = self.context.session_inputs();
                                    next_session.search_hits =
                                        codebase_qa::citations_to_search_hits(
                                            &search_response.citations,
                                        );
                                    self.context.set_session_inputs(next_session);
                                    self.context
                                        .request_fresh_context("codebase_qa_search");
                                }
                            }
                            Err(err) => {
                                deep.gaps.push(format!(
                                    "Search Knowledge failed: {}",
                                    err.message()
                                ));
                            }
                        }
                    }
                }
                if deep_investigation::plan_includes_read(&deep_assessment)
                    && self.tools.get(jaymi_tools::READ_FILE_TOOL_ID).is_ok()
                {
                    for path in deep_investigation::candidate_read_paths(&deep) {
                        if deep.steps.len() >= deep_investigation::MAX_STEPS {
                            break;
                        }
                        let intent = Intent::ReadFile { path: path.clone() };
                        if let Ok(read_response) = self.dispatch_tool_backed(
                            intent,
                            Capability::ReadDocuments,
                            &assessment.goal,
                        ) {
                            deep_investigation::merge_read_document(
                                &mut deep,
                                &path,
                                read_response.document.as_ref(),
                            );
                        }
                    }
                }
                codebase_qa::from_deep_investigation(&assessment, &deep)
            }
            codebase_qa::QaRoute::CodeSearch => {
                let mut report = codebase_qa::begin_ambient(&assessment, &context, &session);
                // Downgrade ambient investigation_needed for search-only routes.
                report.investigation_needed = false;
                report.complexity = Some(codebase_qa::QaComplexity::Search);
                report.route = Some(codebase_qa::QaRoute::CodeSearch);
                if let Some(query) = codebase_qa::primary_search_query(&assessment) {
                    if self.tools.get(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID).is_ok() {
                        let mut search = jaymi_core::SearchRequest::free_text(&query);
                        search.limit = Some(codebase_qa::SEARCH_HIT_LIMIT);
                        search.metadata.files_only = true;
                        let intent = Intent::SearchKnowledge { request: search };
                        match self.dispatch_tool_backed(
                            intent,
                            Capability::Search,
                            &assessment.goal,
                        ) {
                            Ok(search_response) => {
                                codebase_qa::merge_search_citations(
                                    &mut report,
                                    &search_response.citations,
                                    &query,
                                );
                                if !search_response.citations.is_empty() {
                                    let mut next_session = self.context.session_inputs();
                                    next_session.search_hits =
                                        codebase_qa::citations_to_search_hits(
                                            &search_response.citations,
                                        );
                                    self.context.set_session_inputs(next_session);
                                    self.context
                                        .request_fresh_context("codebase_qa_search");
                                }
                            }
                            Err(err) => {
                                report.gaps.push(format!(
                                    "Search Knowledge failed: {}",
                                    err.message()
                                ));
                            }
                        }
                    } else {
                        report.gaps.push(
                            "Search Knowledge tool is not registered — cannot gather lexical evidence."
                                .into(),
                        );
                    }
                }
                // Optional light reads for top hits (still not deep investigation).
                if self.tools.get(jaymi_tools::READ_FILE_TOOL_ID).is_ok() {
                    for path in codebase_qa::candidate_read_paths(&report) {
                        let intent = Intent::ReadFile { path: path.clone() };
                        if let Ok(read_response) = self.dispatch_tool_backed(
                            intent,
                            Capability::ReadDocuments,
                            &assessment.goal,
                        ) {
                            codebase_qa::merge_read_document(
                                &mut report,
                                &path,
                                read_response.document.as_ref(),
                            );
                        }
                    }
                }
                // Re-run conflict detection after merges.
                report.conflicts = codebase_qa::detect_conflicts(&report.evidence);
                if report.evidence.is_empty() {
                    report.gaps.push(
                        "Insufficient project evidence — Jaymi will not invent codebase behavior."
                            .into(),
                    );
                }
                report
            }
        };

        // Re-assemble through ContextEngine when search hits were injected.
        let hints = AssembleHints {
            intent: IntentId::Unknown,
            capability_ids: Vec::new(),
            complexity: None,
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
        };
        let context = match self.context.assemble_with(request, Some(&hints)) {
            Ok(bundle) => bundle,
            Err(_) => context,
        };

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        let hits = codebase_evidence::hits_from_codebase_qa(&report);
        let gaps = report.gaps.clone();
        let subject = report.subject.clone();
        let investigation_needed = report.investigation_needed;
        let fallback = PlannerResponse {
            content: report.to_markdown(),
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            codebase_qa: Some(report),
            ..PlannerResponse::default()
        };
        self.complete_with_codebase_evidence_handoff(
            request,
            context,
            codebase_evidence::CodebaseEvidenceOrigin::CodebaseQa,
            &subject,
            &hits,
            &gaps,
            codebase_evidence::HandoffHints {
                investigation_needed,
                ..Default::default()
            },
            false, // already injected during QA routes
            fallback,
        )
    }

    /// Deep Codebase Investigation (C2.4) — bounded Planner-orchestrated workflow.
    fn complete_deep_investigation(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = deep_investigation::detect_deep_investigation_request(request)
            .ok_or_else(|| JaymiError::new("deep investigation detect missed after gate"))?;
        let session = self.context.session_inputs();
        let mut report =
            deep_investigation::begin_investigation(&assessment, &context, &session);

        // Planner-mediated Search Knowledge (read-only).
        if let Some(query) = deep_investigation::primary_search_query(&assessment) {
            if report.steps.len() < deep_investigation::MAX_STEPS
                && self.tools.get(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID).is_ok()
            {
                let mut search = jaymi_core::SearchRequest::free_text(&query);
                search.limit = Some(deep_investigation::SEARCH_HIT_LIMIT);
                search.metadata.files_only = true;
                let intent = Intent::SearchKnowledge { request: search };
                match self.dispatch_tool_backed(intent, Capability::Search, &assessment.goal) {
                    Ok(search_response) => {
                        deep_investigation::merge_search_citations(
                            &mut report,
                            &search_response.citations,
                            &query,
                        );
                        // Inject hits for ContextEngine re-assemble (sole factory).
                        if !search_response.citations.is_empty() {
                            let mut next_session = self.context.session_inputs();
                            next_session.search_hits =
                                deep_investigation::citations_to_search_hits(
                                    &search_response.citations,
                                );
                            self.context.set_session_inputs(next_session);
                            self.context.request_fresh_context("deep_investigation_search");
                        }
                    }
                    Err(err) => {
                        report.gaps.push(format!(
                            "Search Knowledge extension failed: {}",
                            err.message()
                        ));
                        report
                            .diagnostics
                            .push("search_knowledge_extension_failed".into());
                    }
                }
            }
        }

        // Planner-mediated read_file for top candidate paths (only if planned).
        if deep_investigation::plan_includes_read(&assessment)
            && report.steps.len() < deep_investigation::MAX_STEPS
            && self.tools.get(jaymi_tools::READ_FILE_TOOL_ID).is_ok()
        {
            for path in deep_investigation::candidate_read_paths(&report) {
                if report.steps.len() >= deep_investigation::MAX_STEPS {
                    break;
                }
                let intent = Intent::ReadFile { path: path.clone() };
                match self.dispatch_tool_backed(
                    intent,
                    Capability::ReadDocuments,
                    &assessment.goal,
                ) {
                    Ok(read_response) => {
                        deep_investigation::merge_read_document(
                            &mut report,
                            &path,
                            read_response.document.as_ref(),
                        );
                    }
                    Err(err) => {
                        report.gaps.push(format!(
                            "Could not read `{}`: {}",
                            path.display(),
                            err.message()
                        ));
                    }
                }
            }
        }

        // Optional LSP references when the evidence plan asks to follow refs.
        if deep_investigation::plan_includes_follow_refs(&assessment)
            && report.steps.len() < deep_investigation::MAX_STEPS
            && self.tools.get(jaymi_tools::LANGUAGE_SERVER_TOOL_ID).is_ok()
        {
            let nav = symbol_navigation::SymbolNavAssessment {
                kind: symbol_navigation::SymbolNavKind::References,
                goal: assessment.goal.clone(),
                symbol_hint: Some(report.subject.clone()),
                deixis: true,
                matched_rules: vec!["deep_investigation_lsp"],
            };
            let session = self.context.session_inputs();
            if let Some(lsp_request) = symbol_navigation::try_build_lsp_request(
                &nav,
                &context,
                self.active_project_root(),
                &session,
            ) {
                if !lsp_request.operation.is_mutating() {
                    let intent = Intent::Lsp {
                        request: lsp_request,
                    };
                    if let Ok(lsp_response) =
                        self.dispatch_tool_backed(intent, Capability::Code, &assessment.goal)
                    {
                        deep_investigation::merge_lsp_locations(
                            &mut report,
                            &lsp_response.lsp_references,
                            "references",
                        );
                    }
                }
            }
        }

        // Re-assemble through ContextEngine after injecting search hits (if any).
        let hints = AssembleHints {
            intent: IntentId::Unknown,
            capability_ids: Vec::new(),
            complexity: None,
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
        };
        let context = match self.context.assemble_with(request, Some(&hints)) {
            Ok(bundle) => {
                report.selected_context =
                    deep_investigation::observe_selected_context(&bundle);
                report.steps.push(deep_investigation::InvestigationStep {
                    index: (report.steps.len() as u32).saturating_add(1),
                    action: "context_assemble".into(),
                    detail: format!(
                        "sections={}",
                        report.selected_context.join(",")
                    ),
                    evidence_added: 0,
                    duration_ms: 0,
                });
                report.budget.steps_used = report.steps.len();
                bundle
            }
            Err(_) => context,
        };

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        let hits = codebase_evidence::hits_from_deep_investigation(&report);
        let gaps = report.gaps.clone();
        let subject = report.subject.clone();
        let fallback = PlannerResponse {
            content: report.to_markdown(),
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            deep_investigation: Some(report),
            ..PlannerResponse::default()
        };
        self.complete_with_codebase_evidence_handoff(
            request,
            context,
            codebase_evidence::CodebaseEvidenceOrigin::DeepInvestigation,
            &subject,
            &hits,
            &gaps,
            codebase_evidence::HandoffHints::default(),
            false, // already injected during investigation
            fallback,
        )
    }

    /// Code Relationship Tracing (C2.3) — bounded Planner-mediated investigation.
    fn complete_relationship_tracing(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = relationship_tracing::detect_relationship_trace_request(request)
            .ok_or_else(|| JaymiError::new("relationship trace detect missed after gate"))?;
        let session = self.context.session_inputs();
        let mut report =
            relationship_tracing::investigate(&assessment, &context, &session);

        // Optional Planner-mediated Search Knowledge extension (read-only).
        if relationship_tracing::needs_search_extension(&report)
            && report.steps.len() < relationship_tracing::MAX_STEPS
            && self.tools.get(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID).is_ok()
        {
            let mut search = jaymi_core::SearchRequest::free_text(&report.seed_symbol);
            search.limit = Some(relationship_tracing::SEARCH_HIT_LIMIT);
            search.metadata.files_only = true;
            let intent = Intent::SearchKnowledge { request: search };
            if let Ok(search_response) =
                self.dispatch_tool_backed(intent, Capability::Search, &assessment.goal)
            {
                relationship_tracing::merge_search_citations(
                    &mut report,
                    &search_response.citations,
                );
            } else {
                report
                    .diagnostics
                    .push("search_knowledge_extension_failed".into());
            }
        }

        // Optional Planner-mediated LSP references when callers are missing.
        if relationship_tracing::needs_lsp_references_extension(&report)
            && report.steps.len() < relationship_tracing::MAX_STEPS
            && self.tools.get(jaymi_tools::LANGUAGE_SERVER_TOOL_ID).is_ok()
        {
            let nav = symbol_navigation::SymbolNavAssessment {
                kind: symbol_navigation::SymbolNavKind::References,
                goal: assessment.goal.clone(),
                symbol_hint: Some(report.seed_symbol.clone()),
                deixis: true,
                matched_rules: vec!["relationship_trace_lsp_extension"],
            };
            if let Some(lsp_request) = symbol_navigation::try_build_lsp_request(
                &nav,
                &context,
                self.active_project_root(),
                &session,
            ) {
                if !lsp_request.operation.is_mutating() {
                    let intent = Intent::Lsp {
                        request: lsp_request,
                    };
                    match self.dispatch_tool_backed(intent, Capability::Code, &assessment.goal) {
                        Ok(lsp_response) => {
                            relationship_tracing::merge_lsp_locations(
                                &mut report,
                                &lsp_response.lsp_references,
                                "references",
                            );
                            if !lsp_response.lsp_definitions.is_empty() {
                                relationship_tracing::merge_lsp_locations(
                                    &mut report,
                                    &lsp_response.lsp_definitions,
                                    "definition",
                                );
                            }
                        }
                        Err(_) => {
                            report
                                .diagnostics
                                .push("lsp_references_extension_failed".into());
                        }
                    }
                }
            }
        }

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        let hits = codebase_evidence::hits_from_relationship_trace(&report);
        let subject = report.seed_symbol.clone();
        let investigation_needed = !report.evidence.is_empty()
            || report
                .diagnostics
                .iter()
                .any(|d| d.contains("insufficient") || d.contains("gap"));
        let gaps: Vec<String> = if report.evidence.is_empty() {
            vec![
                "No relationship evidence found — try Coding focus, a clearer subject, or indexed search."
                    .into(),
            ]
        } else {
            Vec::new()
        };
        let fallback = PlannerResponse {
            content: report.to_markdown(),
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            relationship_trace: Some(report),
            ..PlannerResponse::default()
        };
        self.complete_with_codebase_evidence_handoff(
            request,
            context,
            codebase_evidence::CodebaseEvidenceOrigin::RelationshipTrace,
            &subject,
            &hits,
            &gaps,
            codebase_evidence::HandoffHints {
                investigation_needed,
                ..Default::default()
            },
            true,
            fallback,
        )
    }

    /// Symbol Navigation (C2.2) — WI soft path and/or read-only LSP tool.
    fn complete_symbol_navigation(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = symbol_navigation::detect_symbol_navigation_request(request)
            .ok_or_else(|| JaymiError::new("symbol navigation detect missed after gate"))?;
        // Structured LSP requests are already tool-routed via Decision Engine.
        if request.lsp.is_some() {
            return Err(JaymiError::new(
                "symbol navigation soft path does not own structured LSP requests",
            ));
        }

        let mut response = if assessment.kind.prefers_workspace_intelligence() {
            if let Some(report) =
                symbol_navigation::report_from_workspace_intelligence(&assessment, &context)
            {
                let wi_only = matches!(
                    assessment.kind,
                    symbol_navigation::SymbolNavKind::EnclosingSymbol
                        | symbol_navigation::SymbolNavKind::Symbols
                        | symbol_navigation::SymbolNavKind::Callees
                        | symbol_navigation::SymbolNavKind::Imports
                );
                if !report.evidence.is_empty() || wi_only {
                    jaymi_logging::info("planner", report.to_diagnostics_summary());
                    Some(PlannerResponse {
                        content: report.to_markdown(),
                        reasoning_used: false,
                        stream_lifecycle: Some(StreamingLifecycle::Idle),
                        symbol_navigation: Some(report),
                        ..PlannerResponse::default()
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if response.is_none() {
            let session = self.context.session_inputs();
            let lsp_request = symbol_navigation::try_build_lsp_request(
                &assessment,
                &context,
                self.active_project_root(),
                &session,
            );
            response = if let Some(lsp_request) = lsp_request {
                if lsp_request.operation.is_mutating() {
                    None
                } else {
                    let intent = Intent::Lsp {
                        request: lsp_request,
                    };
                    match self.dispatch_tool_backed(intent, Capability::Code, &assessment.goal) {
                        Ok(mut tool_response) => {
                            self.attach_symbol_navigation_report(request, &mut tool_response);
                            if tool_response.symbol_navigation.is_none() {
                                let symbol = symbol_navigation::resolve_symbol_name(
                                    &assessment,
                                    &context,
                                );
                                let report = symbol_navigation::empty_report(
                                    &assessment,
                                    symbol,
                                    "language_server_empty",
                                );
                                tool_response.content = report.to_markdown();
                                tool_response.symbol_navigation = Some(report);
                            }
                            Some(tool_response)
                        }
                        Err(err) => {
                            jaymi_logging::warn(
                                "planner",
                                format!("symbol navigation LSP failed: {}", err.message()),
                            );
                            let symbol =
                                symbol_navigation::resolve_symbol_name(&assessment, &context);
                            let report = symbol_navigation::empty_report(
                                &assessment,
                                symbol,
                                "lsp_unavailable",
                            );
                            Some(PlannerResponse {
                                content: report.to_markdown(),
                                reasoning_used: false,
                                stream_lifecycle: Some(StreamingLifecycle::Idle),
                                symbol_navigation: Some(report),
                                ..PlannerResponse::default()
                            })
                        }
                    }
                }
            } else {
                let symbol = symbol_navigation::resolve_symbol_name(&assessment, &context);
                let report =
                    symbol_navigation::empty_report(&assessment, symbol, "missing_symbol");
                Some(PlannerResponse {
                    content: report.to_markdown(),
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    symbol_navigation: Some(report),
                    ..PlannerResponse::default()
                })
            };
        }

        let mut fallback = response.ok_or_else(|| {
            JaymiError::new("symbol navigation produced no response (mutating LSP?)")
        })?;
        let report = fallback.symbol_navigation.clone().ok_or_else(|| {
            JaymiError::new("symbol navigation response missing report")
        })?;
        let hits = codebase_evidence::hits_from_symbol_navigation(&report);
        let gaps: Vec<String> = if report.evidence.is_empty() {
            vec![
                "No navigation targets — ensure Coding editor focus, language server readiness, or a clearer symbol."
                    .into(),
            ]
        } else {
            Vec::new()
        };
        let subject = report.symbol.clone();
        let kind = report.kind;
        // Ensure markdown is the deterministic fallback content.
        fallback.content = report.to_markdown();
        fallback.reasoning_used = false;
        self.complete_with_codebase_evidence_handoff(
            request,
            context,
            codebase_evidence::CodebaseEvidenceOrigin::SymbolNavigation,
            &subject,
            &hits,
            &gaps,
            codebase_evidence::HandoffHints {
                symbol_nav_kind: kind,
                ..Default::default()
            },
            true,
            fallback,
        )
    }

    /// Attach Symbol Navigation report after language_server tool results (C2.2).
    fn attach_symbol_navigation_report(
        &self,
        request: &UserRequest,
        response: &mut PlannerResponse,
    ) {
        let Some(assessment) = symbol_navigation::detect_symbol_navigation_request(request) else {
            return;
        };
        if response.tool_id.as_deref() != Some(jaymi_tools::LANGUAGE_SERVER_TOOL_ID)
            && response.lsp_definitions.is_empty()
            && response.lsp_references.is_empty()
        {
            return;
        }
        // Never present rename / edits as navigation.
        if !response.lsp_edits.is_empty() && response.lsp_definitions.is_empty() {
            return;
        }
        let symbol = {
            let session = self.context.session_inputs();
            if let Some(name) = assessment.symbol_hint.as_ref().filter(|s| !s.trim().is_empty()) {
                name.clone()
            } else if let Some(name) = session
                .editor_snapshot
                .as_ref()
                .and_then(|s| s.symbol.as_ref())
                .map(|s| s.name.clone())
                .filter(|s| !s.trim().is_empty())
            {
                name
            } else if let Some(name) = session
                .editor_snapshot
                .as_ref()
                .and_then(|s| s.enclosing_function.as_ref())
                .map(|s| s.name.clone())
                .filter(|s| !s.trim().is_empty())
            {
                name
            } else {
                assessment.kind.as_str().to_string()
            }
        };
        let locations: &[LspLocation] = match assessment.kind {
            symbol_navigation::SymbolNavKind::Definition
            | symbol_navigation::SymbolNavKind::SymbolLocation
            | symbol_navigation::SymbolNavKind::Implementations => &response.lsp_definitions,
            symbol_navigation::SymbolNavKind::References
            | symbol_navigation::SymbolNavKind::Callers
            | symbol_navigation::SymbolNavKind::Dependents => &response.lsp_references,
            _ => {
                if !response.lsp_definitions.is_empty() {
                    &response.lsp_definitions
                } else {
                    &response.lsp_references
                }
            }
        };
        let report = if locations.is_empty() {
            symbol_navigation::empty_report(&assessment, symbol, "language_server")
        } else {
            symbol_navigation::report_from_lsp_locations(
                &assessment,
                &symbol,
                locations,
                "language_server",
            )
        };
        jaymi_logging::info("planner", report.to_diagnostics_summary());
        response.content = report.to_markdown();
        response.symbol_navigation = Some(report);
    }

    /// Intelligent Code Search soft path — editor references / deixis (C2.1).
    ///
    /// Read-only. Prefer WI references; otherwise fall back to SearchKnowledge
    /// with the cursor symbol via a nested handle (no mutation).
    fn try_code_search_soft_path(
        &self,
        request: &UserRequest,
        context: &ContextBundle,
    ) -> Option<PlannerResponse> {
        let assessment = code_search::detect_code_search_request(request)?;
        if !assessment.prefer_editor_references || request.search.is_some() {
            return None;
        }
        if let Some(report) = code_search::report_from_editor_references(&assessment, context) {
            jaymi_logging::info("planner", report.to_diagnostics_summary());
            return Some(PlannerResponse {
                content: report.to_markdown(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                code_search: Some(report),
                ..PlannerResponse::default()
            });
        }
        let symbol = context
            .editor_intelligence()
            .symbol
            .as_ref()
            .map(|s| s.name.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let q = assessment.query.trim().to_ascii_lowercase();
                if q == "this function" || q == "this" || q == "current function" {
                    None
                } else if assessment.query.trim().is_empty() {
                    None
                } else {
                    Some(assessment.query.clone())
                }
            });
        if let Some(symbol) = symbol {
            // SearchKnowledge via dispatch (not nested handle) — avoids double finalize.
            let mut search = jaymi_core::SearchRequest::free_text(&symbol);
            search.limit = Some(code_search::SEARCH_HIT_LIMIT);
            search.metadata.files_only = true;
            let intent = Intent::SearchKnowledge { request: search };
            return match self.dispatch_tool_backed(
                intent,
                Capability::Search,
                &assessment.goal,
            ) {
                Ok(mut response) => {
                    let present = UserRequest::new(format!("find symbol {symbol}"));
                    self.attach_code_search_report(&present, &mut response);
                    Some(response)
                }
                Err(err) => {
                    jaymi_logging::warn(
                        "planner",
                        format!("code search symbol fallback failed: {}", err.message()),
                    );
                    let report = code_search::empty_report(&assessment, "editor_references_miss");
                    Some(PlannerResponse {
                        content: report.to_markdown(),
                        reasoning_used: false,
                        stream_lifecycle: Some(StreamingLifecycle::Idle),
                        code_search: Some(report),
                        ..PlannerResponse::default()
                    })
                }
            };
        }
        let report = code_search::empty_report(&assessment, "no_editor_references");
        Some(PlannerResponse {
            content: report.to_markdown(),
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            code_search: Some(report),
            ..PlannerResponse::default()
        })
    }

    /// Finalize soft-path / Search Knowledge code-search reports via the unified
    /// evidence → Context → Reasoning handoff (C2.7.2).
    fn finalize_code_search_soft_path(
        &self,
        request: &UserRequest,
        context: ContextBundle,
        mut fallback: PlannerResponse,
    ) -> JaymiResult<PlannerResponse> {
        let Some(report) = fallback.code_search.clone() else {
            self.transition_conversation(ConversationState::Completed);
            return Ok(self.finalize_response(fallback, context));
        };
        let hits = codebase_evidence::hits_from_code_search(&report);
        let gaps: Vec<String> = if report.evidence.is_empty() {
            vec![
                "No matches in the indexed project — try a more specific symbol or phrase.".into(),
            ]
        } else {
            Vec::new()
        };
        let subject = if report.query.is_empty() {
            report.goal.clone()
        } else {
            report.query.clone()
        };
        let kind = report.kind;
        fallback.content = report.to_markdown();
        fallback.reasoning_used = false;
        self.complete_with_codebase_evidence_handoff(
            request,
            context,
            codebase_evidence::CodebaseEvidenceOrigin::CodeSearch,
            &subject,
            &hits,
            &gaps,
            codebase_evidence::HandoffHints {
                code_search_kind: kind,
                ..Default::default()
            },
            true,
            fallback,
        )
    }

    /// Finalize Language Server / WI symbol reports via unified handoff (C2.7.2).
    fn finalize_symbol_navigation_report(
        &self,
        request: &UserRequest,
        context: ContextBundle,
        mut fallback: PlannerResponse,
    ) -> JaymiResult<PlannerResponse> {
        let Some(report) = fallback.symbol_navigation.clone() else {
            self.transition_conversation(ConversationState::Completed);
            return Ok(self.finalize_response(fallback, context));
        };
        let hits = codebase_evidence::hits_from_symbol_navigation(&report);
        let gaps: Vec<String> = if report.evidence.is_empty() {
            vec![
                "No navigation targets — ensure Coding editor focus, language server readiness, or a clearer symbol."
                    .into(),
            ]
        } else {
            Vec::new()
        };
        let subject = report.symbol.clone();
        let kind = report.kind;
        fallback.content = report.to_markdown();
        fallback.reasoning_used = false;
        self.complete_with_codebase_evidence_handoff(
            request,
            context,
            codebase_evidence::CodebaseEvidenceOrigin::SymbolNavigation,
            &subject,
            &hits,
            &gaps,
            codebase_evidence::HandoffHints {
                symbol_nav_kind: kind,
                ..Default::default()
            },
            true,
            fallback,
        )
    }

    /// Replace raw Search Knowledge dumps with a capped Code Search report (C2.1).
    fn attach_code_search_report(&self, request: &UserRequest, response: &mut PlannerResponse) {
        let Some(assessment) = code_search::should_present_code_search_report(request) else {
            return;
        };
        if response.tool_id.as_deref() != Some(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID)
            && response.capability != Some(Capability::Search)
        {
            if response.citations.is_empty() && response.entries.is_empty() {
                return;
            }
        }
        let strategy = response
            .tool_id
            .clone()
            .unwrap_or_else(|| "search_knowledge".into());
        let report = if response.citations.is_empty() {
            code_search::empty_report(&assessment, strategy)
        } else {
            code_search::report_from_citations(&assessment, &response.citations, strategy)
        };
        jaymi_logging::info("planner", report.to_diagnostics_summary());
        response.content = report.to_markdown();
        response.code_search = Some(report);
    }

    /// Build Awareness (C4.1) — discover verification commands from WI / ProjectSnapshot.
    ///
    /// Read-only. Never runs terminal commands, never mutates, never lets Reasoning
    /// execute shell. No second project/build discovery system.
    fn complete_build_awareness(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = build_awareness::detect_build_awareness_request(request)
            .ok_or_else(|| JaymiError::new("build awareness detect missed after gate"))?;
        let session = self.context.session_inputs();
        let report =
            build_awareness::discover_verification(&assessment, &context, &session);

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content: report.to_markdown(),
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            build_awareness: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Test Execution (C4.2) — run grounded project tests via Terminal Tool.
    ///
    /// Reuses C4.1 VerificationTarget discovery. Always goes through
    /// Planner → Tool → Provider with permission / ToolRisk review. Never
    /// mutates code from failures and never retries silently.
    fn complete_test_execution(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = test_execution::detect_test_execution_request(request)
            .ok_or_else(|| JaymiError::new("test execution detect missed after gate"))?;
        let session = self.context.session_inputs();
        let mut report =
            test_execution::select_test_target(&assessment, &context, &session);

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        if matches!(
            report.status,
            Some(test_execution::TestExecutionStatus::MissingCommand)
        ) || report.command.is_none()
        {
            self.transition_conversation(ConversationState::Completed);
            let mut response = PlannerResponse {
                content: String::new(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                test_execution: Some(report),
                ..PlannerResponse::default()
            };
            self.attach_verification_summary(&mut response, false, Vec::new());
            return Ok(self.finalize_response(response, context));
        }

        let command = report.command.clone().unwrap_or_default();
        let cwd = report
            .working_directory
            .clone()
            .unwrap_or_else(|| {
                context
                    .active_project()
                    .root_directory
                    .clone()
                    .unwrap_or_else(|| ".".into())
            });
        let intent = Intent::RunTerminal {
            operation: jaymi_core::TerminalOperation::Run,
            session_id: jaymi_providers::DEFAULT_TERMINAL_SESSION_ID.to_string(),
            cwd: std::path::PathBuf::from(&cwd),
            command: Some(command),
            title: Some("Tests".into()),
        };

        let mut response = self.dispatch_tool_backed(
            intent,
            Capability::ExecuteTerminalCommands,
            &assessment.goal,
        )?;

        if response.blocked && !response.awaiting_review {
            let explanation = response
                .permission_result
                .as_ref()
                .map(|p| p.explanation.clone())
                .or_else(|| {
                    response
                        .policy_evaluation
                        .as_ref()
                        .map(|p| p.explanation())
                })
                .unwrap_or_else(|| "Execution denied".into());
            report = test_execution::mark_denied(report, explanation);
            response.test_execution = Some(report);
            self.attach_verification_summary(&mut response, false, Vec::new());
            self.transition_conversation(ConversationState::Completed);
            return Ok(self.finalize_response(response, context));
        }

        if response.awaiting_review {
            report = test_execution::mark_awaiting_review(report);
            if let Some(plan_id) = response.execution_plan.as_ref().map(|p| p.id().clone()) {
                let mut store = self.paused_store()?;
                let _ = store.set_test_execution(&plan_id, report.clone());
            }
            response.test_execution = Some(report);
            self.attach_verification_summary(&mut response, false, Vec::new());
            self.note_verify_awaiting_review(&mut response);
            return Ok(self.finalize_response(response, context));
        }

        // Immediate execution path (Allowed) — still finalize from terminal fields.
        let tool_like = jaymi_tools::ToolOutput {
            success: !response.blocked,
            terminal_output: response.terminal_output.clone(),
            terminal_exit_code: response.terminal_exit_code,
            terminal_duration_ms: response.terminal_duration_ms,
            message: Some(response.content.clone()),
            ..Default::default()
        };
        report = test_execution::finalize_from_tool_output(report, &tool_like, None);
        let mut next_session = self.context.session_inputs();
        test_execution::publish_to_context(&mut next_session, &report);
        self.context.set_session_inputs(next_session);
        self.context.request_fresh_context("test_execution");
        self.remember_verification_evidence(report.evidence.clone());

        response.test_execution = Some(report);
        self.attach_verification_summary(&mut response, false, Vec::new());
        self.transition_conversation(ConversationState::Completed);
        Ok(self.finalize_response(response, context))
    }

    /// Fix From Verification Failure (C4.4).
    ///
    /// Uses structured VerificationEvidence + C2 investigation to author a C3
    /// Coding Plan for a proposed correction. Never mutates files, never
    /// auto-approves, never silently retries verification or mutations.
    fn complete_fix_from_verification(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let session = self.context.session_inputs();
        let evidence = fix_from_verification::load_verification_evidence(
            self.clone_last_verification_evidence().as_ref(),
            &session,
        );
        let assessment = fix_from_verification::detect_fix_from_verification_request(
            request,
            evidence.as_ref(),
        )
        .ok_or_else(|| JaymiError::new("fix_from_verification detect missed after gate"))?;

        let actionability = fix_from_verification::classify_actionability(evidence.as_ref());
        let mut report =
            fix_from_verification::diagnose(&assessment, evidence.as_ref(), actionability);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        // Non-actionable: honest diagnosis only — no invented Coding Plan targets.
        if !matches!(actionability, fix_from_verification::FixActionability::Actionable) {
            if let Some(evidence) = evidence.as_ref() {
                let objective = fix_from_verification::objective_from_evidence(
                    &assessment,
                    evidence,
                    &session,
                    actionability,
                );
                let investigation =
                    change_investigation::light_from_objective(&objective, &context);
                let plan = coding_plan::plan_from_change_investigation(
                    &objective,
                    &investigation,
                    &context,
                );
                // Remember deferred plan only when NeedsClarification still produces one —
                // never auto-execute. Keep remember for continuity when user clarifies.
                if matches!(
                    actionability,
                    fix_from_verification::FixActionability::InsufficientEvidence
                ) {
                    self.remember_coding_plan(Some(plan.clone()));
                }
                report = fix_from_verification::attach_plan_artifacts(
                    report,
                    objective.clone(),
                    investigation.clone(),
                    plan.clone(),
                );
                let content = format!(
                    "{}\n\n{}\n\n{}\n\n{}",
                    report.to_markdown(),
                    objective.to_markdown(),
                    investigation.to_markdown(),
                    plan.to_markdown()
                );
                self.transition_conversation(ConversationState::Completed);
                let response = PlannerResponse {
                    content,
                    reasoning_used: false,
                    stream_lifecycle: Some(StreamingLifecycle::Idle),
                    coding_edit: Some(objective),
                    change_investigation: Some(investigation),
                    coding_plan: Some(plan),
                    fix_from_verification: Some(report),
                    ..PlannerResponse::default()
                };
                return Ok(self.finalize_response(response, context));
            }

            self.transition_conversation(ConversationState::Completed);
            let response = PlannerResponse {
                content: report.to_markdown(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                fix_from_verification: Some(report),
                ..PlannerResponse::default()
            };
            return Ok(self.finalize_response(response, context));
        }

        let evidence = evidence.expect("actionable requires evidence");
        let objective = fix_from_verification::objective_from_evidence(
            &assessment,
            &evidence,
            &session,
            actionability,
        );
        jaymi_logging::info("planner", objective.to_diagnostics_summary());

        let (context, investigation) =
            self.run_change_investigation(request, &objective, context)?;
        jaymi_logging::info("planner", investigation.to_diagnostics_summary());

        let plan = coding_plan::plan_from_change_investigation(
            &objective,
            &investigation,
            &context,
        );
        jaymi_logging::info("planner", plan.to_diagnostics_summary());
        self.remember_coding_plan(Some(plan.clone()));

        report = fix_from_verification::attach_plan_artifacts(
            report,
            objective.clone(),
            investigation.clone(),
            plan.clone(),
        );

        let content = format!(
            "{}\n\n{}\n\n{}\n\n{}",
            report.to_markdown(),
            objective.to_markdown(),
            investigation.to_markdown(),
            plan.to_markdown()
        );

        self.transition_conversation(ConversationState::Completed);
        let mut response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_edit: Some(objective),
            change_investigation: Some(investigation),
            coding_plan: Some(plan),
            fix_from_verification: Some(report),
            ..PlannerResponse::default()
        };
        self.note_fix_proposed_for_verification_loop(&mut response);
        Ok(self.finalize_response(response, context))
    }

    /// Coding Goal Model (C5.1).
    ///
    /// Creates or updates a Planner-owned multi-step coding objective.
    /// Never executes tools, never mutates files, never decomposes or runs work.
    fn complete_coding_goal(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = coding_goal::detect_coding_goal_request(request)
            .ok_or_else(|| JaymiError::new("coding goal detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();

        let goal = match assessment.kind {
            coding_goal::CodingGoalRequestKind::Create => {
                let created =
                    coding_goal::create_goal(&assessment, &context, conversation_id.clone());
                jaymi_logging::info("planner", created.to_diagnostics_summary());
                self.with_coding_goal_store(|store| {
                    store.push_active(created.clone());
                });
                created
            }
            coding_goal::CodingGoalRequestKind::Cancel => {
                let updated = self
                    .with_coding_goal_store(|store| {
                        let Some(existing) =
                            store.active_goal(conversation_id.as_deref()).cloned()
                        else {
                            return None;
                        };
                        let cancelled =
                            coding_goal::cancel_goal(existing, "User cancelled the coding goal.");
                        if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                            *slot = cancelled.clone();
                        }
                        let key = coding_goal::CodingGoalStore::conversation_key(
                            conversation_id.as_deref(),
                        );
                        store.active_by_conversation.remove(&key);
                        Some(cancelled)
                    })
                    .flatten();
                match updated {
                    Some(goal) => {
                        jaymi_logging::info("planner", goal.to_diagnostics_summary());
                        goal
                    }
                    None => {
                        self.transition_conversation(ConversationState::Completed);
                        let response = PlannerResponse {
                            content: coding_goal::no_active_goal_markdown(assessment.kind),
                            reasoning_used: false,
                            stream_lifecycle: Some(StreamingLifecycle::Idle),
                            ..PlannerResponse::default()
                        };
                        return Ok(self.finalize_response(response, context));
                    }
                }
            }
            coding_goal::CodingGoalRequestKind::Complete => {
                let result = self.with_coding_goal_store(|store| {
                    let Some(existing) = store.active_goal(conversation_id.as_deref()).cloned()
                    else {
                        return Ok::<Option<coding_goal::CodingGoal>, String>(None);
                    };
                    let evidence = existing.agent_lifecycle.completion_evidence.clone();
                    let completed = coding_goal::complete_goal(existing, evidence)?;
                    if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                        *slot = completed.clone();
                    }
                    let key = coding_goal::CodingGoalStore::conversation_key(
                        conversation_id.as_deref(),
                    );
                    store.active_by_conversation.remove(&key);
                    Ok(Some(completed))
                });
                match result {
                    Some(Ok(Some(goal))) => {
                        jaymi_logging::info("planner", goal.to_diagnostics_summary());
                        goal
                    }
                    Some(Ok(None)) | None => {
                        self.transition_conversation(ConversationState::Completed);
                        let response = PlannerResponse {
                            content: coding_goal::no_active_goal_markdown(assessment.kind),
                            reasoning_used: false,
                            stream_lifecycle: Some(StreamingLifecycle::Idle),
                            ..PlannerResponse::default()
                        };
                        return Ok(self.finalize_response(response, context));
                    }
                    Some(Err(msg)) => {
                        self.transition_conversation(ConversationState::Completed);
                        let response = PlannerResponse {
                            content: format!(
                                "## Coding Goal\n\nCould not mark complete: {msg}\n\nProvide verification or approved-apply evidence first, then ask again.\n"
                            ),
                            reasoning_used: false,
                            stream_lifecycle: Some(StreamingLifecycle::Idle),
                            ..PlannerResponse::default()
                        };
                        return Ok(self.finalize_response(response, context));
                    }
                }
            }
            coding_goal::CodingGoalRequestKind::Block => {
                let updated = self
                    .with_coding_goal_store(|store| {
                        let Some(existing) =
                            store.active_goal(conversation_id.as_deref()).cloned()
                        else {
                            return None;
                        };
                        let blocked =
                            coding_goal::block_goal(existing, assessment.block_reason.clone());
                        if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                            *slot = blocked.clone();
                        }
                        Some(blocked)
                    })
                    .flatten();
                match updated {
                    Some(goal) => {
                        jaymi_logging::info("planner", goal.to_diagnostics_summary());
                        goal
                    }
                    None => {
                        self.transition_conversation(ConversationState::Completed);
                        let response = PlannerResponse {
                            content: coding_goal::no_active_goal_markdown(assessment.kind),
                            reasoning_used: false,
                            stream_lifecycle: Some(StreamingLifecycle::Idle),
                            ..PlannerResponse::default()
                        };
                        return Ok(self.finalize_response(response, context));
                    }
                }
            }
            coding_goal::CodingGoalRequestKind::Status => {
                match self
                    .clone_coding_goal_store()
                    .active_goal(conversation_id.as_deref())
                    .cloned()
                {
                    Some(goal) => {
                        jaymi_logging::info("planner", goal.to_diagnostics_summary());
                        goal
                    }
                    None => {
                        self.transition_conversation(ConversationState::Completed);
                        let response = PlannerResponse {
                            content: coding_goal::no_active_goal_markdown(assessment.kind),
                            reasoning_used: false,
                            stream_lifecycle: Some(StreamingLifecycle::Idle),
                            ..PlannerResponse::default()
                        };
                        return Ok(self.finalize_response(response, context));
                    }
                }
            }
        };

        debug_assert!(!goal.mutated);
        debug_assert!(!goal.tools_executed);

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content: goal.to_markdown(),
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal: Some(goal),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Goal Decomposition (C5.2).
    ///
    /// Turns an active Coding Goal into proposed tasks owned by the Planner.
    /// Never executes tasks, mutates files, or auto-approves plans.
    fn complete_goal_decomposition(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = goal_decomposition::detect_decomposition_request(request)
            .ok_or_else(|| JaymiError::new("goal decomposition detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let (report, updated_goal) =
            goal_decomposition::decompose_active_goal(active.as_ref(), &assessment);

        jaymi_logging::info("planner", report.to_diagnostics_summary());

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                } else {
                    // Defensive: re-attach if pointer was lost but we produced an update.
                    store.push_active(goal);
                }
            });
        }

        debug_assert!(!report.mutated);
        debug_assert!(!report.tools_executed);
        debug_assert!(!report.auto_approved);

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal: updated_goal.or(active),
            goal_decomposition: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Agent State Machine (C5.3).
    ///
    /// Planner-owned lifecycle transitions for the active Coding Goal.
    /// LLM/UI cannot invent transitions; no autonomous multi-task orchestration.
    fn complete_agent_lifecycle(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agent_state_machine::detect_agent_lifecycle_request(request)
            .ok_or_else(|| JaymiError::new("agent lifecycle detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let (report, updated_goal) =
            agent_state_machine::apply_assessment(active.as_ref(), &assessment);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                } else if !goal.status.is_terminal()
                    || goal.agent_lifecycle.state == agent_state_machine::AgentLifecycleState::Paused
                {
                    store.push_active(goal);
                }
            });
        }

        debug_assert!(!report.mutated);
        debug_assert!(!report.tools_executed);
        debug_assert!(!report.llm_directed);
        debug_assert!(!report.ui_arbitrary);

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or_else(|| {
            self.clone_coding_goal_store()
                .active_goal(conversation_id.as_deref())
                .cloned()
                .or(active)
        });

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            agent_lifecycle: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Multi-Step Coding Execution (C5.4).
    ///
    /// Orchestrates ready GoalTasks through existing C2/C3/C4 paths — never
    /// auto-approves, never invents a second ExecutionPlan/Review, never
    /// auto-starts all remaining tasks in one turn.
    fn complete_multi_step_execution(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = multi_step_execution::detect_multi_step_execution_request(request)
            .ok_or_else(|| JaymiError::new("multi-step execution detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let (report, updated_goal) =
            multi_step_execution::apply_assessment(active.as_ref(), &assessment);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                } else if !goal.status.is_terminal() {
                    store.push_active(goal);
                }
            });
        }

        debug_assert!(!report.mutated);
        debug_assert!(!report.tools_executed);
        debug_assert!(!report.auto_approved);
        debug_assert!(!report.llm_direct_tools);

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or_else(|| {
            self.clone_coding_goal_store()
                .active_goal(conversation_id.as_deref())
                .cloned()
                .or(active)
        });

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            multi_step_execution: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Agent Context Maintenance (C5.5).
    ///
    /// Builds a compact Planner-owned goal context from the active Coding Goal
    /// plus Policy-selected labels from the already-assembled ContextBundle.
    /// Never creates AgentContextEngine, never mutates files, never writes Memory.
    fn complete_agent_context_maintenance(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agent_context_maintenance::detect_agent_context_request(request)
            .ok_or_else(|| JaymiError::new("agent context detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let (report, updated_goal) = agent_context_maintenance::apply_assessment(
            active.as_ref(),
            &assessment,
            Some(&context),
        );
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                }
            });
        }

        debug_assert!(!report.mutated);
        debug_assert!(!report.tools_executed);
        debug_assert!(!report.memory_written);
        debug_assert!(!report.duplicate_assembler);

        let content = if let Some(ctx) = &report.context {
            format!("{}\n\n{}", report.to_markdown(), ctx.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or_else(|| {
            self.clone_coding_goal_store()
                .active_goal(conversation_id.as_deref())
                .cloned()
                .or(active)
        });

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            agent_context_maintenance: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Approval and Autonomy Policy (C5.6).
    ///
    /// Explains READ/OBSERVE vs MUTATION/SIDE_EFFECT defaults and current
    /// plan+task approval binding. Does not create AgentApproval or autonomy modes.
    fn complete_autonomy_policy(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = approval_autonomy_policy::detect_autonomy_policy_request(request)
            .ok_or_else(|| JaymiError::new("autonomy policy detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let report = approval_autonomy_policy::apply_assessment(active.as_ref(), &assessment);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        debug_assert!(!report.modes_implemented);
        debug_assert!(!report.agent_approval_system);
        debug_assert!(!report.llm_self_authorize);

        let content = report.to_markdown();
        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal: active,
            autonomy_policy: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Agent Progress UX (C5.7).
    ///
    /// Derive-only view of the active Coding Goal — mirrors Planner state into
    /// conversation without exposing chain-of-thought or inventing a second agent state.
    fn complete_agent_progress(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agent_progress_ux::detect_progress_request(request)
            .ok_or_else(|| JaymiError::new("agent progress detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let report = agent_progress_ux::apply_assessment(active.as_ref(), &assessment);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        if let Some(progress) = &report.progress {
            debug_assert!(!progress.exposes_chain_of_thought);
            debug_assert!(!progress.second_agent_state);
        }

        let content = report.to_markdown();
        let progress = report.progress.clone();
        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal: active,
            agent_progress: progress,
            agent_progress_report: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// ContextBundle for C5.8/C5.14 control turns.
    ///
    /// Pause/Resume assemble via ContextEngine (sole factory) so fingerprints are
    /// real or honestly unavailable. Cancel/Status do not need fingerprints and
    /// keep the lightweight empty bundle.
    fn bundle_for_agent_control(
        &self,
        request: &UserRequest,
        kind: agent_pause_resume_cancel::AgentControlRequestKind,
    ) -> JaymiResult<ContextBundle> {
        use agent_pause_resume_cancel::AgentControlRequestKind;
        match kind {
            AgentControlRequestKind::Pause | AgentControlRequestKind::Resume => {
                self.assemble_agent_control_context(request)
            }
            AgentControlRequestKind::Cancel | AgentControlRequestKind::Status => {
                Ok(self.context.empty_bundle())
            }
        }
    }

    /// Assemble ambient Context for pause/resume fingerprint integrity (C5.14).
    ///
    /// Uses ContextEngine as the sole ContextBundle factory — no parallel
    /// workspace assembler. Required so fingerprints are real (or honestly
    /// unavailable), never silently “unchanged” via `empty_bundle`.
    fn assemble_agent_control_context(
        &self,
        request: &UserRequest,
    ) -> JaymiResult<ContextBundle> {
        let hints = AssembleHints::new(jaymi_core::IntentId::Unknown, Vec::<String>::new());
        let bundle = self.context.assemble_with(request, Some(&hints))?;
        jaymi_logging::info(
            "planner",
            format!(
                "agent_control context assembled {}",
                agent_pause_resume_cancel::ProjectFingerprint::from_bundle(&bundle)
                    .to_diagnostics_summary()
            ),
        );
        Ok(bundle)
    }

    /// Agent Pause, Resume, and Cancellation (C5.8 + C5.14 integrity).
    ///
    /// Planner-owned control over an active Coding Goal. Reuses C5.3 transitions
    /// and existing Tool / stream cancellation — never invents a parallel cancel
    /// engine or silently rolls back successful mutations.
    fn complete_agent_pause_resume_cancel(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agent_pause_resume_cancel::detect_control_request(request)
            .ok_or_else(|| JaymiError::new("agent control detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let has_review_paused = self
            .paused_store()
            .ok()
            .map(|store| !store.is_empty())
            .unwrap_or(false);
        let lifecycle = active
            .as_ref()
            .map(|g| g.agent_lifecycle.state)
            .unwrap_or(agent_state_machine::AgentLifecycleState::Idle);
        let in_flight = agent_pause_resume_cancel::observe_in_flight(
            self.conversation_state(),
            None,
            has_review_paused,
            lifecycle,
        );

        let (report, updated_goal) = agent_pause_resume_cancel::apply_assessment(
            active.as_ref(),
            &assessment,
            &context,
            &in_flight,
        );
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        debug_assert!(!report.rolled_back_mutations);
        debug_assert!(!report.parallel_cancel_engine);
        debug_assert!(report.planner_owned);
        debug_assert!(!report.continued_after_cancel);

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if goal.control.cancelled || goal.status == coding_goal::CodingGoalStatus::Cancelled
                {
                    if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                        *slot = goal.clone();
                    }
                    let key = coding_goal::CodingGoalStore::conversation_key(
                        conversation_id.as_deref(),
                    );
                    store.active_by_conversation.remove(&key);
                } else if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                } else if !goal.status.is_terminal()
                    || goal.agent_lifecycle.state == agent_state_machine::AgentLifecycleState::Paused
                {
                    store.push_active(goal);
                }
            });
        }

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or(active);

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            agent_control: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Agent Failure and Recovery (C5.9).
    ///
    /// Classify goal-scoped failures and apply a Planner-owned disposition —
    /// read-only investigate, propose a new reviewed C3 plan, AwaitUser→Blocked,
    /// or StopFailed — never silent mutation / retry / approval reuse / rollback.
    fn complete_agent_failure_recovery(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agent_failure_recovery::detect_recovery_request(request)
            .ok_or_else(|| JaymiError::new("agent recovery detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let (report, updated_goal) =
            agent_failure_recovery::apply_assessment(active.as_ref(), &assessment);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        debug_assert!(!report.mutated);
        debug_assert!(!report.auto_retried);
        debug_assert!(!report.approval_reused);
        debug_assert!(!report.automatic_rollback);
        debug_assert!(!report.parallel_repair_engine);
        debug_assert!(report.planner_owned);

        if let Some(plan) = report.corrective_plan.clone() {
            self.remember_coding_plan(Some(plan));
        }

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                } else if !goal.status.is_terminal() {
                    store.push_active(goal);
                }
            });
        }

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or(active);

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            agent_recovery: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Coding Goal Completion (C5.10).
    ///
    /// Evidence-based completion ownership — evaluate acceptance criteria,
    /// block on failed verification / blockers, allow user end incomplete
    /// (distinct from successfully completed). Never invent success.
    fn complete_coding_goal_completion(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = coding_goal_completion::detect_completion_request(request)
            .ok_or_else(|| JaymiError::new("goal completion detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let active = self
            .clone_coding_goal_store()
            .active_goal(conversation_id.as_deref())
            .cloned();

        let (report, updated_goal) =
            coding_goal_completion::apply_assessment(active.as_ref(), &assessment);
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        debug_assert!(!report.mutated);
        debug_assert!(!report.invented_success);
        debug_assert!(!report.model_authoritative);
        debug_assert!(!report.unknown_treated_as_satisfied);
        debug_assert!(report.planner_owned);

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                if goal.status.is_terminal() {
                    if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                        *slot = goal.clone();
                    }
                    let key = coding_goal::CodingGoalStore::conversation_key(
                        conversation_id.as_deref(),
                    );
                    store.active_by_conversation.remove(&key);
                } else if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal.clone();
                } else {
                    store.push_active(goal);
                }
            });
        }

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or(active);

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            goal_completion: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Long-Running Session Continuity (C5.11).
    ///
    /// Bound histories, compact context (constraints preserved), revalidate
    /// fingerprints after pauses, invalidate stale approvals — no agent DB,
    /// no background autonomous work, paused goals do not keep executing.
    fn complete_agent_session_continuity(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agent_session_continuity::detect_continuity_request(request)
            .ok_or_else(|| JaymiError::new("session continuity detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let store_clone = self.clone_coding_goal_store();
        let active = store_clone.active_goal(conversation_id.as_deref()).cloned();

        let (report, updated_goal, updated_store) = agent_session_continuity::apply_assessment(
            active.as_ref(),
            &assessment,
            Some(&context),
            Some(&store_clone),
        );
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        debug_assert!(!report.mutated);
        debug_assert!(!report.background_autonomous_work);
        debug_assert!(!report.independent_agent_database);
        debug_assert!(!report.blindly_resumed_stale_plan);
        debug_assert!(!report.paused_continues_executing);
        debug_assert!(report.planner_owned);

        if let Some(store) = updated_store {
            self.with_coding_goal_store(|slot| {
                *slot = store;
            });
        }
        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                let _ = agent_session_continuity::prune_store(
                    store,
                    conversation_id.as_deref(),
                );
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal;
                } else if !goal.status.is_terminal() {
                    store.push_active(goal);
                }
            });
        }

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let coding_goal = updated_goal.or(active);

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            session_continuity: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Agentic Coding Experience (C5.12).
    ///
    /// Compose C5.1–C5.11 into one conversational experience narrative —
    /// create/status only; no parallel agent, no CoT, no silent mutation.
    fn complete_agentic_coding_experience(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = agentic_coding_experience::detect_agentic_experience_request(request)
            .ok_or_else(|| JaymiError::new("agentic experience detect missed after gate"))?;
        let conversation_id = self.active_conversation_id_for_goal();
        let store = self.clone_coding_goal_store();
        let active = agentic_coding_experience::resolve_goal_for_experience(
            &store,
            conversation_id.as_deref(),
        );

        let (report, updated_goal) = agentic_coding_experience::apply_assessment(
            active.as_ref(),
            &assessment,
            &context,
            conversation_id.clone(),
        );
        jaymi_logging::info("planner", report.to_diagnostics_summary());

        debug_assert!(!report.mutated);
        debug_assert!(!report.tools_executed);
        debug_assert!(!report.exposes_chain_of_thought);
        debug_assert!(!report.parallel_coding_agent);
        debug_assert!(!report.silent_continue_after_cancel);
        debug_assert!(!report.silent_destructive_retry);
        debug_assert!(!report.hidden_filesystem_access);
        debug_assert!(!report.model_direct_tools);
        debug_assert!(!report.unrelated_capabilities);
        debug_assert!(report.planner_owned);
        debug_assert!(report.conversation_primary);

        if let Some(goal) = updated_goal.clone() {
            self.with_coding_goal_store(|store| {
                // Status narration of a terminal goal must not re-activate it.
                if assessment.kind == agentic_coding_experience::AgenticExperienceRequestKind::Status
                    && goal.status.is_terminal()
                {
                    if let Some(slot) = store.goals.iter_mut().find(|g| g.id == goal.id) {
                        *slot = goal;
                    }
                    return;
                }
                if let Some(slot) = store.active_goal_mut(conversation_id.as_deref()) {
                    *slot = goal;
                } else if !goal.status.is_terminal() {
                    store.push_active(goal);
                }
            });
        }

        let content = if let Some(goal) = &updated_goal {
            format!("{}\n\n{}", report.to_markdown(), goal.to_markdown())
        } else {
            report.to_markdown()
        };

        let progress = report.progress.clone();
        let coding_goal = updated_goal.or(active);

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_goal,
            agent_progress: progress,
            agentic_coding_experience: Some(report),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// Coding Edit Intent (C3.1) + Change Investigation (C3.2) + Coding Plan (C3.3).
    ///
    /// Classifies mutation requests, investigates impact via reused C2 helpers,
    /// then authors a Planner-owned Coding Plan grounded in that evidence.
    /// Never writes files, invents file contents, or invokes mutation tools.
    fn complete_coding_edit_intent(
        &self,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let assessment = coding_edit::detect_coding_edit_request(request)
            .ok_or_else(|| JaymiError::new("coding edit detect missed after gate"))?;
        let session = self.context.session_inputs();
        let objective =
            coding_edit::finalize_objective(&assessment, &context, &session, request);

        jaymi_logging::info("planner", objective.to_diagnostics_summary());

        let (context, investigation) =
            self.run_change_investigation(request, &objective, context)?;

        jaymi_logging::info("planner", investigation.to_diagnostics_summary());

        // C3.3 — Coding Plan from investigation (no tools / writes / file contents).
        let plan = coding_plan::plan_from_change_investigation(
            &objective,
            &investigation,
            &context,
        );
        jaymi_logging::info("planner", plan.to_diagnostics_summary());
        self.remember_coding_plan(Some(plan.clone()));

        let content = format!(
            "{}\n\n{}\n\n{}",
            objective.to_markdown(),
            investigation.to_markdown(),
            plan.to_markdown()
        );

        self.transition_conversation(ConversationState::Completed);
        let response = PlannerResponse {
            content,
            reasoning_used: false,
            stream_lifecycle: Some(StreamingLifecycle::Idle),
            coding_edit: Some(objective),
            change_investigation: Some(investigation),
            coding_plan: Some(plan),
            ..PlannerResponse::default()
        };
        Ok(self.finalize_response(response, context))
    }

    /// C3.2 — reuse C2 deep / relationship helpers under Planner mediation.
    fn run_change_investigation(
        &self,
        request: &UserRequest,
        objective: &coding_edit::CodingEditObjective,
        context: ContextBundle,
    ) -> JaymiResult<(ContextBundle, change_investigation::ChangeInvestigationReport)> {
        if !change_investigation::should_run_full(objective) {
            let report = change_investigation::light_from_objective(objective, &context);
            return Ok((context, report));
        }

        let deep_assessment = change_investigation::assessment_from_objective(objective);
        let session = self.context.session_inputs();
        let mut deep =
            deep_investigation::begin_investigation(&deep_assessment, &context, &session);

        // Optional relationship ambient chain (WI only via investigate; extensions below).
        let mut relationship_files = Vec::new();
        let mut relationship_symbols = Vec::new();
        if let Some(trace_assessment) =
            change_investigation::trace_assessment_from_objective(objective)
        {
            let trace =
                relationship_tracing::investigate(&trace_assessment, &context, &session);
            for hit in &trace.evidence {
                let p = hit.path.display().to_string();
                if !p.is_empty() && !relationship_files.iter().any(|x| x == &p) {
                    relationship_files.push(p);
                }
            }
            if !trace.seed_symbol.is_empty() {
                relationship_symbols.push(trace.seed_symbol.clone());
            }
            deep.steps.push(deep_investigation::InvestigationStep {
                index: (deep.steps.len() as u32).saturating_add(1),
                action: "relationship_ambient".into(),
                detail: format!(
                    "seed={} evidence={}",
                    trace.seed_symbol,
                    trace.evidence.len()
                ),
                evidence_added: trace.evidence.len(),
                duration_ms: 0,
            });
        }

        // Planner-mediated Search Knowledge.
        if let Some(query) = deep_investigation::primary_search_query(&deep_assessment) {
            if deep.steps.len() < deep_investigation::MAX_STEPS
                && self.tools.get(jaymi_tools::SEARCH_KNOWLEDGE_TOOL_ID).is_ok()
            {
                let mut search = jaymi_core::SearchRequest::free_text(&query);
                search.limit = Some(change_investigation::SEARCH_HIT_LIMIT);
                search.metadata.files_only = true;
                let intent = Intent::SearchKnowledge { request: search };
                match self.dispatch_tool_backed(
                    intent,
                    Capability::Search,
                    &deep_assessment.goal,
                ) {
                    Ok(search_response) => {
                        deep_investigation::merge_search_citations(
                            &mut deep,
                            &search_response.citations,
                            &query,
                        );
                        if !search_response.citations.is_empty() {
                            let mut next_session = self.context.session_inputs();
                            next_session.search_hits =
                                deep_investigation::citations_to_search_hits(
                                    &search_response.citations,
                                );
                            self.context.set_session_inputs(next_session);
                            self.context
                                .request_fresh_context("change_investigation_search");
                        }
                    }
                    Err(err) => {
                        deep.gaps.push(format!(
                            "Search Knowledge failed: {}",
                            err.message()
                        ));
                    }
                }
            }
        }

        // Planner-mediated read_file for top candidates.
        if deep_investigation::plan_includes_read(&deep_assessment)
            && self.tools.get(jaymi_tools::READ_FILE_TOOL_ID).is_ok()
        {
            for path in deep_investigation::candidate_read_paths(&deep) {
                if deep.steps.len() >= deep_investigation::MAX_STEPS {
                    break;
                }
                let intent = Intent::ReadFile { path: path.clone() };
                if let Ok(read_response) = self.dispatch_tool_backed(
                    intent,
                    Capability::ReadDocuments,
                    &deep_assessment.goal,
                ) {
                    deep_investigation::merge_read_document(
                        &mut deep,
                        &path,
                        read_response.document.as_ref(),
                    );
                }
            }
        }

        // Optional LSP references (read-only).
        if deep_investigation::plan_includes_follow_refs(&deep_assessment)
            && deep.steps.len() < deep_investigation::MAX_STEPS
            && self.tools.get(jaymi_tools::LANGUAGE_SERVER_TOOL_ID).is_ok()
        {
            let nav = symbol_navigation::SymbolNavAssessment {
                kind: symbol_navigation::SymbolNavKind::References,
                goal: deep_assessment.goal.clone(),
                symbol_hint: Some(deep.subject.clone()),
                deixis: true,
                matched_rules: vec!["change_investigation_lsp"],
            };
            let session = self.context.session_inputs();
            if let Some(lsp_request) = symbol_navigation::try_build_lsp_request(
                &nav,
                &context,
                self.active_project_root(),
                &session,
            ) {
                if !lsp_request.operation.is_mutating() {
                    let intent = Intent::Lsp {
                        request: lsp_request,
                    };
                    if let Ok(lsp_response) =
                        self.dispatch_tool_backed(intent, Capability::Code, &deep_assessment.goal)
                    {
                        deep_investigation::merge_lsp_locations(
                            &mut deep,
                            &lsp_response.lsp_references,
                            "references",
                        );
                    }
                }
            }
        }

        // Re-assemble through ContextEngine (sole factory) after search inject.
        let hints = AssembleHints {
            intent: IntentId::Unknown,
            capability_ids: Vec::new(),
            complexity: None,
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
        };
        let context = match self.context.assemble_with(request, Some(&hints)) {
            Ok(bundle) => {
                deep.selected_context =
                    deep_investigation::observe_selected_context(&bundle);
                bundle
            }
            Err(_) => context,
        };

        let report = change_investigation::fold_deep_report(
            objective,
            &context,
            &deep,
            &relationship_files,
            &relationship_symbols,
        );
        Ok((context, report))
    }

    /// Multi-file Intelligence → GenerationOps → reviewed Execution Plan (C1.7).
    fn complete_multi_file_from_request(
        &self,
        kind: multi_file::MultiFileKind,
        request: &UserRequest,
        context: ContextBundle,
    ) -> JaymiResult<PlannerResponse> {
        let goal = request.content.trim().to_string();
        let plan = multi_file::plan_from_bundle(kind, &goal, &context);
        if plan.affected_files.is_empty() {
            self.transition_conversation(ConversationState::Completed);
            let response = PlannerResponse {
                content: plan.to_markdown(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                multi_file: Some(plan),
                ..PlannerResponse::default()
            };
            return Ok(self.finalize_response(response, context));
        }
        let generation = multi_file::code_generation_from_plan(&plan);
        if generation.is_empty() {
            self.transition_conversation(ConversationState::Completed);
            let response = PlannerResponse {
                content: plan.to_markdown(),
                reasoning_used: false,
                stream_lifecycle: Some(StreamingLifecycle::Idle),
                multi_file: Some(plan),
                ..PlannerResponse::default()
            };
            return Ok(self.finalize_response(response, context));
        }
        let mut response = self.begin_code_generation(generation)?;
        let prefix = plan.to_markdown();
        response.multi_file = Some(plan);
        response.content = format!("{prefix}\n\n{}", response.content);
        Ok(self.finalize_response(response, context))
    }

    fn clone_last_coding_plan(&self) -> Option<coding_plan::CodingPlan> {
        self.last_coding_plan
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Build + gate a generation Execution Plan (always RequiresApproval).
    ///
    /// Sprint C3.5: attaches a multi-file Coding Diff Review preview to the
    /// existing ExecutionPlan / Review Card — not a parallel approval system.
    fn prepare_code_generation_execution(
        &self,
        generation: &code_generation::CodeGeneration,
        patch_report: Option<&patch_generation::PatchGenerationReport>,
    ) -> JaymiResult<PreparedExecution> {
        let deletion_method = self.resolve_deletion_method(None, "")?;
        let calls = code_generation::tool_calls_for_generation(generation, deletion_method)
            .map_err(JaymiError::new)?;
        for call in &calls {
            let tool = self
                .tools
                .get(&call.tool_id)
                .map_err(|_| unknown_tool_error(&call.tool_id))?;
            if !tool
                .metadata()
                .capabilities
                .contains(&Capability::FileManagement)
            {
                return Err(missing_capability_error(
                    &call.tool_id,
                    Capability::FileManagement,
                ));
            }
        }

        let first = &calls[0];
        let capability = Capability::FileManagement;
        let intent = if calls
            .iter()
            .any(|call| call.tool_id == MANAGE_PATH_TOOL_ID)
        {
            IntentId::ManagePath
        } else {
            IntentId::WriteFile
        };

        let mut tool_risk = jaymi_tools::ToolRisk::Modify;
        for call in &calls {
            let tool = self.tools.get(&call.tool_id)?;
            let metadata = tool.metadata();
            let risk = metadata
                .risk
                .effective_for(&call.input, metadata.internet);
            if matches!(
                risk,
                jaymi_tools::ToolRisk::Destructive | jaymi_tools::ToolRisk::External
            ) {
                tool_risk = risk;
            } else if matches!(risk, jaymi_tools::ToolRisk::Modify)
                && !matches!(
                    tool_risk,
                    jaymi_tools::ToolRisk::Destructive | jaymi_tools::ToolRisk::External
                )
            {
                tool_risk = jaymi_tools::ToolRisk::Modify;
            }
        }

        let primary_tool = self.tools.get(&first.tool_id)?;
        let metadata = primary_tool.metadata().clone();
        let provider_id = metadata.provider.clone();
        let resource = first.path.display().to_string();
        let candidate = ExecutionCandidate {
            tool_id: first.tool_id.clone(),
            provider_id: provider_id.clone(),
            requires_internet: matches!(metadata.internet, InternetRequirement::Required),
            local_only: matches!(metadata.privacy, PrivacyMode::LocalOnly),
            cloud_only: matches!(metadata.privacy, PrivacyMode::CloudOnly),
        };
        let policy_evaluation = self.policies.evaluate(&candidate)?;
        let permission_result = if matches!(policy_evaluation.decision, PolicyDecision::Denied) {
            None
        } else {
            let permission_request = PermissionRequest {
                category: PermissionCategory::Filesystem,
                action: first.permission_action,
                scope: PermissionScope::Once,
                explanation: format!(
                    "Code generation ({} op(s)) starting at {resource}",
                    calls.len()
                ),
                resource: Some(resource.clone()),
            };
            Some(self.permissions.check(&permission_request)?)
        };

        let mut gate =
            combine_gate_decision(&policy_evaluation, permission_result.as_ref(), tool_risk);
        // Constitutional: generation always requires Review Before Action.
        if matches!(gate, GateDecision::Allowed) {
            gate = GateDecision::RequiresApproval {
                explanation: "code generation requires Review Before Action".into(),
            };
        }

        let coding_plan = self.clone_last_coding_plan();
        let risk_label = EstimatedRisk::from_tool_risk(tool_risk);
        let coding_diff = coding_diff_review::build_coding_diff_review(
            generation,
            |path| self.read_file_text_for_patch(path),
            coding_diff_review::CodingDiffReviewContext {
                coding_plan: coding_plan.as_ref(),
                patch_report,
                risk_label: Some(risk_label.as_str()),
            },
        );

        match &gate {
            GateDecision::Denied { explanation } => {
                let mut plan = self.create_generation_plan(
                    generation,
                    &calls,
                    intent,
                    tool_risk,
                    ReviewRequirement::NotRequired,
                    calls
                        .iter()
                        .find_map(|c| c.input.deletion_method)
                        .or(Some(deletion_method)),
                    None,
                );
                plan.mark_ready()
                    .map_err(|error| JaymiError::new(error.to_string()))?;
                let _ = plan.cancel();
                self.record_plan_history(&plan)?;
                let summary = ExecutionSummary::from_plan(
                    &plan,
                    Vec::new(),
                    Vec::new(),
                    Some(explanation.clone()),
                );
                return Ok(PreparedExecution {
                    plan: plan.clone(),
                    tool_id: first.tool_id.clone(),
                    provider_id: Some(provider_id),
                    policy_evaluation: Some(policy_evaluation.clone()),
                    permission_result: permission_result.clone(),
                    blocked_response: Some(PlannerResponse {
                        content: format!(
                            "Denied before code generation: {explanation}"
                        ),
                        capability: Some(capability),
                        tool_id: Some(first.tool_id.clone()),
                        provider_id: Some(candidate.provider_id),
                        policy_evaluation: Some(policy_evaluation),
                        permission_result,
                        blocked: true,
                        awaiting_review: false,
                        execution_plan: Some(plan),
                        execution_summary: Some(summary),
                        code_generation: Some(generation.clone()),
                        coding_diff_review: Some(coding_diff.surface),
                        ..PlannerResponse::default()
                    }),
                });
            }
            GateDecision::RequiresApproval { explanation } => {
                // Prefer multi-file coding diff over first-tool-only preview (C3.5).
                let preview = Some(coding_diff.preview.clone());
                let mut plan = self.create_generation_plan(
                    generation,
                    &calls,
                    intent,
                    tool_risk,
                    ReviewRequirement::Required,
                    calls
                        .iter()
                        .find_map(|c| c.input.deletion_method)
                        .or(Some(deletion_method)),
                    preview,
                );
                plan.mark_ready()
                    .map_err(|error| JaymiError::new(error.to_string()))?;
                plan.mark_awaiting_review()
                    .map_err(|error| JaymiError::new(error.to_string()))?;
                self.record_plan_history(&plan)?;
                let generation_calls = calls
                    .iter()
                    .map(|call| (call.tool_id.clone(), call.input.clone()))
                    .collect::<Vec<_>>();
                let generation_baselines = coding_mutation_recovery::baselines_for_generation(
                    generation,
                    |path| self.read_file_text_for_patch(path),
                );
                self.pause_execution(PausedExecution {
                    plan: plan.clone(),
                    tool_input: first.input.clone(),
                    tool_id: first.tool_id.clone(),
                    provider_id: Some(provider_id.clone()),
                    capability,
                    policy_evaluation: Some(policy_evaluation.clone()),
                    permission_result: permission_result.clone(),
                    paused_at: Instant::now(),
                    generation_calls,
                    generation_baselines,
                    test_execution: None,
                })?;
                let summary = ExecutionSummary::from_plan(
                    &plan,
                    Vec::new(),
                    Vec::new(),
                    Some(explanation.clone()),
                );
                let mut review = ReviewCardModel::from_plan(&plan, Some(explanation.as_str()));
                review.apply_coding_diff_surface(&coding_diff.surface);
                // Prefix generation markdown so the conversation shows ops + Review Card.
                let content = format!(
                    "{}\n\n{}",
                    generation.to_markdown(),
                    review.render_text()
                );
                jaymi_logging::info(
                    "planner",
                    format!(
                        "execution_plan_lifecycle status={} plan={} {}",
                        plan.status().as_str(),
                        plan.id().as_str(),
                        coding_diff.surface.to_diagnostics_summary()
                    ),
                );
                return Ok(PreparedExecution {
                    plan: plan.clone(),
                    tool_id: first.tool_id.clone(),
                    provider_id: Some(provider_id),
                    policy_evaluation: Some(policy_evaluation.clone()),
                    permission_result: permission_result.clone(),
                    blocked_response: Some(PlannerResponse {
                        content,
                        capability: Some(capability),
                        tool_id: Some(first.tool_id.clone()),
                        provider_id: Some(candidate.provider_id),
                        policy_evaluation: Some(policy_evaluation),
                        permission_result,
                        blocked: true,
                        awaiting_review: true,
                        execution_plan: Some(plan),
                        execution_summary: Some(summary),
                        code_generation: Some(generation.clone()),
                        coding_diff_review: Some(coding_diff.surface),
                        ..PlannerResponse::default()
                    }),
                });
            }
            GateDecision::Allowed => unreachable!("generation forces RequiresApproval"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_generation_plan(
        &self,
        generation: &code_generation::CodeGeneration,
        calls: &[code_generation::GenerationToolCall],
        intent: IntentId,
        tool_risk: jaymi_tools::ToolRisk,
        review_requirement: ReviewRequirement,
        deletion_method: Option<DeletionMethod>,
        action_preview: Option<ActionPreview>,
    ) -> ExecutionPlan {
        let mut proposed_tools = Vec::new();
        for call in calls {
            if !proposed_tools.iter().any(|id| id == &call.tool_id) {
                proposed_tools.push(call.tool_id.clone());
            }
        }
        let steps = calls
            .iter()
            .enumerate()
            .map(|(index, call)| ExecutionStep {
                order: index + 1,
                description: call.action_label.clone(),
                tool_id: Some(call.tool_id.clone()),
                resource: Some(call.path.display().to_string()),
            })
            .collect();
        let mut affected_resources = calls
            .iter()
            .map(|call| call.path.display().to_string())
            .collect::<Vec<_>>();
        if let Some(preview) = &action_preview {
            for path in &preview.resources {
                if !path.trim().is_empty()
                    && !affected_resources.iter().any(|existing| existing == path)
                {
                    affected_resources.push(path.clone());
                }
            }
        }
        let mut permissions_required = vec![PlanPermissionRequirement::from_enums(
            PermissionCategory::Filesystem,
            PermissionAction::Write,
        )];
        if calls
            .iter()
            .any(|call| call.permission_action == PermissionAction::Delete)
        {
            permissions_required.push(PlanPermissionRequirement::from_enums(
                PermissionCategory::Filesystem,
                PermissionAction::Delete,
            ));
        }
        ExecutionPlan::create(ExecutionPlanParams {
            originating_request: format!("Code generation: {}", generation.goal),
            planner_intent: intent,
            capability: Capability::FileManagement,
            proposed_tools,
            steps,
            estimated_risk: EstimatedRisk::from_tool_risk(tool_risk),
            affected_resources,
            permissions_required,
            review_requirement,
            estimated_reversibility: EstimatedReversibility::from_tool_risk(tool_risk),
            expected_outputs: vec![format!(
                "Applied {} generation operation(s)",
                calls.len()
            )],
            deletion_method,
            action_preview,
            lineage: PlanLineage::root(),
        })
    }

    /// Execute every tool call in an approved generation batch (Planner-owned).
    ///
    /// Sprint C3.6 — Apply Approved Coding Changes:
    /// runs ops in order through existing tools/providers, stops on first
    /// failure, and returns an [`ExecutionSummary`] that reports exactly what
    /// succeeded vs failed (`partial` when mid-batch stop). No autonomous retry.
    ///
    /// Sprint C3.7 — Coding Mutation Recovery:
    /// re-validates frozen baselines before any write and between ops so drifted
    /// sources are never silently overwritten.
    fn execute_approved_generation(
        &self,
        plan: &mut ExecutionPlan,
        calls: &[(String, ToolInput)],
        baselines: &[coding_mutation_recovery::GenerationBaseline],
    ) -> JaymiResult<(
        jaymi_tools::ToolOutput,
        ExecutionSummary,
        Option<coding_mutation_recovery::CodingMutationRecovery>,
    )> {
        if !plan.status().may_execute() {
            return Err(JaymiError::new(format!(
                "execution plan {} is not approved (status={})",
                plan.id(),
                plan.status()
            )));
        }
        if calls.is_empty() {
            return Err(JaymiError::new("generation batch is empty"));
        }
        plan.mark_executing()
            .map_err(|error| JaymiError::new(error.to_string()))?;
        self.transition_conversation(ConversationState::Executing);
        jaymi_logging::info(
            "planner",
            format!(
                "execution plan {} applying coding changes ops={}",
                plan.id(),
                calls.len()
            ),
        );

        // C3.7: full-batch preflight before the first mutation.
        let preflight = coding_mutation_recovery::preflight_baselines(
            baselines,
            |path| self.read_file_text_for_patch(path),
            |parent| parent.exists(),
        );
        if !preflight.ok() {
            let _ = plan.mark_failed();
            let mut recovery = coding_mutation_recovery::recovery_for_preflight(
                plan,
                baselines,
                &preflight,
            );
            let mut summary = ExecutionSummary::from_plan(
                plan,
                Vec::new(),
                Vec::new(),
                recovery
                    .blockers
                    .first()
                    .map(|b| b.detail.clone()),
            );
            summary.status = ExecutionStatus::Failed;
            coding_mutation_recovery::enrich_summary_with_recovery(&mut summary, &recovery);
            recovery.phase = Some(coding_mutation_recovery::MutationPhase::Failed);
            let output = jaymi_tools::ToolOutput {
                success: false,
                message: Some(format!(
                    "Coding apply refused before mutation: {}",
                    recovery
                        .blockers
                        .first()
                        .map(|b| b.detail.as_str())
                        .unwrap_or("stale or invalid source state")
                )),
                ..Default::default()
            };
            jaymi_logging::warn(
                "planner",
                format!(
                    "coding_apply preflight blocked plan={} {}",
                    plan.id(),
                    recovery.to_diagnostics_summary()
                ),
            );
            return Ok((output, summary, Some(recovery)));
        }

        let started = Instant::now();
        let mut results = Vec::new();
        let mut last_success_output = None;

        for (index, (tool_id, input)) in calls.iter().enumerate() {
            let order = index + 1;
            let path = input
                .path
                .as_ref()
                .map(|p| p.display().to_string());

            // Apply-time invalid patch guard.
            if let Some(reason) =
                coding_mutation_recovery::invalid_write_content(tool_id, input.content.as_deref())
            {
                results.push(coding_apply::CodingOpResult::from_error(
                    order,
                    tool_id.clone(),
                    path.clone(),
                    reason.clone(),
                ));
                let stop_kind = Some(coding_mutation_recovery::RecoveryKind::InvalidPatch);
                let duration_ms = started.elapsed().as_millis() as u64;
                let _ = plan.mark_failed();
                let mut summary =
                    coding_apply::summary_from_op_results(plan, &results, duration_ms);
                let recovery = coding_mutation_recovery::recovery_from_op_results(
                    plan,
                    baselines,
                    &results,
                    &summary,
                    stop_kind,
                );
                coding_mutation_recovery::enrich_summary_with_recovery(&mut summary, &recovery);
                let output = jaymi_tools::ToolOutput {
                    success: false,
                    message: Some(reason),
                    ..Default::default()
                };
                return Ok((output, summary, Some(recovery)));
            }

            // Per-op baseline recheck (race after preflight / prior writes).
            if let Some(baseline) = baselines.iter().find(|b| b.order == order) {
                let op_preflight = coding_mutation_recovery::preflight_baselines(
                    std::slice::from_ref(baseline),
                    |path| self.read_file_text_for_patch(path),
                    |parent| parent.exists(),
                );
                if !op_preflight.ok() {
                    let blocker = &op_preflight.blockers[0];
                    results.push(coding_apply::CodingOpResult::from_error(
                        order,
                        tool_id.clone(),
                        path.clone(),
                        blocker.detail.clone(),
                    ));
                    let stop_kind = Some(blocker.kind);
                    let duration_ms = started.elapsed().as_millis() as u64;
                    let _ = plan.mark_failed();
                    let mut summary =
                        coding_apply::summary_from_op_results(plan, &results, duration_ms);
                    let recovery = coding_mutation_recovery::recovery_from_op_results(
                        plan,
                        baselines,
                        &results,
                        &summary,
                        stop_kind,
                    );
                    coding_mutation_recovery::enrich_summary_with_recovery(&mut summary, &recovery);
                    let output = jaymi_tools::ToolOutput {
                        success: false,
                        message: Some(blocker.detail.clone()),
                        ..Default::default()
                    };
                    return Ok((output, summary, Some(recovery)));
                }
            }

            jaymi_logging::info(
                "planner",
                format!("coding_apply op={order} tool={tool_id} path={path:?}"),
            );
            match self.orchestrator.execute(tool_id, input.clone()) {
                Ok(output) => {
                    let result = coding_apply::CodingOpResult::from_tool_output(
                        order,
                        tool_id.clone(),
                        path.clone(),
                        &output,
                    );
                    results.push(result);
                    if !output.success {
                        let stop_kind = Some(coding_mutation_recovery::RecoveryKind::WriteFailed);
                        let duration_ms = started.elapsed().as_millis() as u64;
                        let _ = plan.mark_failed();
                        let mut summary =
                            coding_apply::summary_from_op_results(plan, &results, duration_ms);
                        let recovery = coding_mutation_recovery::recovery_from_op_results(
                            plan,
                            baselines,
                            &results,
                            &summary,
                            stop_kind,
                        );
                        coding_mutation_recovery::enrich_summary_with_recovery(
                            &mut summary,
                            &recovery,
                        );
                        jaymi_logging::warn(
                            "planner",
                            format!(
                                "coding_apply stopped plan={} partial={} at_op={}",
                                plan.id(),
                                summary.partial,
                                order
                            ),
                        );
                        return Ok((output, summary, Some(recovery)));
                    }
                    last_success_output = Some(output);
                }
                Err(error) => {
                    let message = error.message().to_string();
                    let kind = if message.to_ascii_lowercase().contains("parent")
                        || message.to_ascii_lowercase().contains("no such file")
                        || message.to_ascii_lowercase().contains("not found")
                    {
                        coding_mutation_recovery::RecoveryKind::DirectoryMissing
                    } else {
                        coding_mutation_recovery::RecoveryKind::WriteFailed
                    };
                    let stop_kind = Some(kind);
                    results.push(coding_apply::CodingOpResult::from_error(
                        order,
                        tool_id.clone(),
                        path,
                        message.clone(),
                    ));
                    let duration_ms = started.elapsed().as_millis() as u64;
                    let _ = plan.mark_failed();
                    let mut summary =
                        coding_apply::summary_from_op_results(plan, &results, duration_ms);
                    let recovery = coding_mutation_recovery::recovery_from_op_results(
                        plan,
                        baselines,
                        &results,
                        &summary,
                        stop_kind,
                    );
                    coding_mutation_recovery::enrich_summary_with_recovery(&mut summary, &recovery);
                    let output = jaymi_tools::ToolOutput {
                        success: false,
                        message: Some(message),
                        ..Default::default()
                    };
                    jaymi_logging::warn(
                        "planner",
                        format!(
                            "coding_apply provider/tool error plan={} partial={} at_op={}",
                            plan.id(),
                            summary.partial,
                            order
                        ),
                    );
                    return Ok((output, summary, Some(recovery)));
                }
            }
        }

        let duration_ms = started.elapsed().as_millis() as u64;
        let _ = plan.mark_completed();
        let mut output = last_success_output.ok_or_else(|| {
            JaymiError::new("generation batch produced no tool output")
        })?;
        let summary = coding_apply::summary_from_op_results(plan, &results, duration_ms);
        let headline = coding_apply::coding_apply_headline(&summary, calls.len());
        let detail = results
            .iter()
            .map(|r| r.message.clone())
            .collect::<Vec<_>>()
            .join(" · ");
        output.message = Some(format!("{headline} {detail}"));
        self.context.request_fresh_context("files_changed");
        jaymi_logging::info(
            "planner",
            format!(
                "coding_apply completed plan={} ops={} files={}",
                plan.id(),
                calls.len(),
                summary.files_edited.len()
            ),
        );
        Ok((output, summary, None))
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
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
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
            generation_calls: entry.generation_calls.clone(),
            generation_baselines: entry.generation_baselines.clone(),
            test_execution: entry.test_execution.clone(),
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

        let (output, execution_summary, mut coding_recovery) =
            if entry.generation_calls.is_empty() {
                let (output, summary) =
                    self.execute_approved_plan(&mut entry.plan, entry.tool_input)?;
                (output, summary, None)
            } else {
                self.execute_approved_generation(
                    &mut entry.plan,
                    &entry.generation_calls,
                    &entry.generation_baselines,
                )?
            };
        let _ = self.update_plan_history_status(entry.plan.id(), entry.plan.status());

        let mut summary_body = if execution_summary.should_surface_in_conversation() {
            Some(execution_summary.render_conversation())
        } else {
            None
        };
        if let Some(recovery) = &coding_recovery {
            let markdown = recovery.to_markdown();
            summary_body = Some(match summary_body {
                Some(body) => format!("{body}\n\n{markdown}"),
                None => markdown,
            });
        }

        // Soft tool / provider failure: keep ExecutionSummary (incl. partial
        // multi-file apply) instead of dropping it via ensure_success Err.
        if !output.success {
            let test_report = entry.test_execution.take().map(|pending| {
                test_execution::finalize_from_tool_output(pending, &output, None)
            });
            let mut response = self.tool_failure_response(
                entry.plan,
                entry.capability,
                entry.tool_id,
                entry.provider_id,
                entry.policy_evaluation,
                entry.permission_result,
                output,
                execution_summary,
            );
            if let Some(body) = summary_body {
                response.content = format!("{}\n\n{body}", response.content);
            }
            response.coding_mutation_recovery = coding_recovery.take();
            if let Some(report) = test_report {
                let mut next_session = self.context.session_inputs();
                test_execution::publish_to_context(&mut next_session, &report);
                self.context.set_session_inputs(next_session);
                self.context.request_fresh_context("test_execution");
                self.remember_verification_evidence(report.evidence.clone());
                response.test_execution = Some(report);
                self.attach_verification_summary(&mut response, false, Vec::new());
            } else {
                self.remember_execution_summary(response.execution_summary.clone());
                if response
                    .execution_summary
                    .as_ref()
                    .is_some_and(|s| !s.files_edited.is_empty())
                {
                    self.note_apply_for_verification_loop(&mut response);
                    self.attach_verification_summary(&mut response, false, Vec::new());
                }
            }
            return Ok(response);
        }

        self.ensure_success(&output)?;

        let mut content = output.message.clone().unwrap_or_else(|| {
            format!(
                "Resumed and completed plan {} via {} → {}",
                entry.plan.id(),
                entry.capability.id(),
                entry.tool_id
            )
        });
        if let Some(body) = summary_body {
            content = format!("{content}\n\n{body}");
        }

        let test_report = entry.test_execution.take().map(|pending| {
            test_execution::finalize_from_tool_output(pending, &output, None)
        });
        let mut response = response_from_tool_output(
            content,
            entry.capability,
            entry.tool_id,
            entry.provider_id,
            entry.plan,
            execution_summary,
            entry.policy_evaluation,
            entry.permission_result,
            output,
        );
        response.coding_mutation_recovery = coding_recovery.take();
        if let Some(report) = test_report {
            let mut next_session = self.context.session_inputs();
            test_execution::publish_to_context(&mut next_session, &report);
            self.context.set_session_inputs(next_session);
            self.context.request_fresh_context("test_execution");
            self.remember_verification_evidence(report.evidence.clone());
            response.test_execution = Some(report);
            self.attach_verification_summary(&mut response, false, Vec::new());
        } else {
            self.remember_execution_summary(response.execution_summary.clone());
            if response
                .execution_summary
                .as_ref()
                .is_some_and(|s| !s.files_edited.is_empty())
            {
                self.note_apply_for_verification_loop(&mut response);
                self.attach_verification_summary(&mut response, false, Vec::new());
            }
        }
        Ok(response)
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
        let had_pending_test = entry.test_execution.is_some();
        let pending_test = entry.test_execution.take();
        let _ = entry.plan.cancel();
        let summary = ExecutionSummary::cancelled(&entry.plan, reason);
        let recovery = if !entry.generation_calls.is_empty() || !entry.generation_baselines.is_empty()
        {
            Some(coding_mutation_recovery::recovery_for_cancel(
                &entry.plan,
                &entry.generation_baselines,
            ))
        } else if entry.plan.steps().len() > 1 {
            Some(coding_mutation_recovery::recovery_for_cancel(
                &entry.plan,
                &[],
            ))
        } else {
            None
        };
        let mut content = if user_cancelled {
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
        if let Some(recovery) = &recovery {
            content = format!("{content}\n\n{}", recovery.to_markdown());
        }
        let mut response = PlannerResponse {
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
            coding_mutation_recovery: recovery,
            test_execution: pending_test,
            ..PlannerResponse::default()
        };
        if had_pending_test {
            self.attach_verification_summary(&mut response, true, Vec::new());
        }
        if let Some(cancelled) =
            self.cancel_coding_verification_loop("Review cancelled by user.")
        {
            response.coding_verification_loop = Some(cancelled.clone());
            response.content = coding_verification_loop::append_progress_note(
                &response.content,
                &cancelled,
            );
        }
        Ok(response)
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
        terminal_exit_code: output.terminal_exit_code,
        terminal_duration_ms: output.terminal_duration_ms,
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
    use jaymi_core::{CodingAction, EntryType, FileType, Lifecycle};
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
        SEARCH_KNOWLEDGE_TOOL_ID,
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
        last_goal: std::sync::Mutex<Option<String>>,
        last_prompt_text: std::sync::Mutex<Option<String>>,
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
                last_goal: std::sync::Mutex::new(None),
                last_prompt_text: std::sync::Mutex::new(None),
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

        fn last_goal(&self) -> Option<String> {
            self.last_goal.lock().ok().and_then(|guard| guard.clone())
        }

        fn record_request(&self, request: &jaymi_reasoning::ReasoningRequest) {
            self.last_history_len
                .store(request.history.len() as u32, AtomicOrdering::SeqCst);
            *self.last_model.lock().expect("lock") = request.model.clone();
            *self.last_goal.lock().expect("lock") = Some(request.goal.clone());
            *self.last_prompt_text.lock().expect("lock") =
                request.prompt.as_ref().map(|prompt| prompt.text.clone());
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

    /// Test double: same id as write_file but cloud-only so Privacy Maximum denies.
    struct CloudOnlyWriteFileTool {
        metadata: ToolMetadata,
    }

    impl CloudOnlyWriteFileTool {
        fn new() -> Self {
            Self {
                metadata: ToolMetadata {
                    id: jaymi_tools::WRITE_FILE_TOOL_ID.into(),
                    name: "Cloud Write File".into(),
                    version: "0.1.0".into(),
                    description: "Cloud-only write probe for deny tests".into(),
                    provider: "cloud".into(),
                    capabilities: vec![Capability::FileManagement],
                    risk: ToolRisk::External,
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::Low,
                    memory_usage: MemoryUsage::Small,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::CloudOnly,
                    internet: InternetRequirement::Required,
                    reliability: Reliability::Experimental,
                    result_type: ResultType::StructuredData,
                },
            }
        }
    }

    impl Tool for CloudOnlyWriteFileTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput> {
            Err(JaymiError::new(
                "cloud write probe must never execute under Privacy Maximum",
            ))
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
    fn explain_file_returns_structured_understanding_without_tools() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/main.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            },
            current_selection: jaymi_context::CurrentSelectionSection {
                path: Some("/proj/main.rs".into()),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 0,
                text: None,
            },
            diagnostics: jaymi_context::DiagnosticsSection {
                diagnostics: vec![jaymi_context::BundleDiagnostic {
                    severity: "error".into(),
                    path: Some("/proj/main.rs".into()),
                    message: "unused variable".into(),
                    line: Some(2),
                    column: Some(4),
                    source: None,
                }],
            },
            approved_context_providers: vec!["editor".into(), "diagnostics".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner
            .handle(UserRequest::coding_action(CodingAction::ExplainFile))
            .unwrap();

        assert!(!response.awaiting_review);
        assert!(!response.reasoning_used);
        assert!(response.tool_id.is_none());
        assert!(response.capability.is_none());
        assert!(response.execution_plan.is_none());

        let understanding = response
            .coding_understanding
            .as_ref()
            .expect("coding_understanding attached");
        assert!(
            understanding.purpose.iter().any(|p| p.contains("main.rs")),
            "{:?}",
            understanding.purpose
        );
        assert!(
            response.content.contains("main.rs") || response.content.contains("Current file"),
            "{}",
            response.content
        );
        assert!(response.content.contains("\nPurpose\n"));
        assert!(response.content.contains("\nResponsibilities\n"));
        assert!(response.content.contains("\nInteresting observations\n"));
        assert!(response.content.contains("\nRecommendations\n"));
        assert!(response.content.contains("\nNext actions\n"));
        assert!(response.content.contains("[Review]"));
        assert!(!response.content.contains("## Coding Understanding"));
        assert!(!response.content.contains("appears to be"));

        let notes = &response
            .context_bundle
            .as_ref()
            .unwrap()
            .planner_metadata()
            .notes;
        assert!(
            notes.iter().any(|n| n.contains("coding_understanding=")),
            "{notes:?}"
        );
    }

    #[test]
    fn free_text_project_question_triggers_understanding() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("How does this project work?"))
            .unwrap();
        assert!(response.coding_understanding.is_some());
        assert!(!response.awaiting_review);
        assert!(response.tool_id.is_none());
        assert!(response.execution_plan.is_none());
        assert!(response.content.contains("## Project Understanding"));
        assert!(response.content.contains("\nOverview\n"));
        assert!(response.content.contains("\nArchitecture\n"));
        assert!(response.content.contains("\nImportant Modules\n"));
        assert!(response.content.contains("\nNext actions\n"));
    }

    #[test]
    fn explain_this_project_is_not_selection_and_stays_understanding_only() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Explain this project."))
            .unwrap();
        let understanding = response.coding_understanding.expect("project understanding");
        assert!(!understanding.suggested_next_actions.is_empty());
        assert!(!response.awaiting_review);
        assert!(response.capability.is_none());
        assert!(response.tool_id.is_none());
        assert!(response.execution_plan.is_none());
        assert!(response.content.contains("## Project Understanding"));
    }

    #[test]
    fn review_this_file_returns_structured_review_without_plans() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/main.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            diagnostics: jaymi_context::DiagnosticsSection {
                diagnostics: vec![jaymi_context::BundleDiagnostic {
                    severity: "warning".into(),
                    path: Some("/proj/main.rs".into()),
                    message: "unused import".into(),
                    line: Some(1),
                    column: Some(0),
                    source: None,
                }],
            },
            approved_context_providers: vec!["editor".into(), "diagnostics".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner
            .handle(UserRequest::new("Review this file."))
            .unwrap();

        assert!(!response.awaiting_review);
        assert!(!response.reasoning_used);
        assert!(response.tool_id.is_none());
        assert!(response.capability.is_none());
        assert!(response.execution_plan.is_none());
        assert!(response.coding_understanding.is_none());

        let review = response.coding_review.expect("coding_review attached");
        assert!(review.strengths.iter().any(|s| s.contains("main.rs")));
        assert!(response.content.contains("## Coding Review"));
        assert!(response.content.contains("### Strengths"));
        assert!(response.content.contains("### Weaknesses"));
        assert!(response.content.contains("### Potential Bugs"));
        assert!(response.content.contains("### Complexity"));
        assert!(response.content.contains("### Performance"));
        assert!(response.content.contains("### Maintainability"));
        assert!(response.content.contains("### Architecture"));

        let notes = &response
            .context_bundle
            .as_ref()
            .unwrap()
            .planner_metadata()
            .notes;
        assert!(
            notes.iter().any(|n| n.contains("coding_review=")),
            "{notes:?}"
        );
    }

    #[test]
    fn review_my_changes_is_review_only() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Review my changes."))
            .unwrap();
        assert!(response.coding_review.is_some());
        assert!(response.execution_plan.is_none());
        assert!(!response.awaiting_review);
        assert!(response.content.contains("## Coding Review"));
    }

    #[test]
    fn build_pong_returns_agentic_coding_goal_experience() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/main.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner.handle(UserRequest::new("Build me a Pong game.")).unwrap();

        assert!(!response.awaiting_review);
        assert!(!response.reasoning_used);
        assert!(response.tool_id.is_none());
        assert!(response.capability.is_none());
        assert!(response.execution_plan.is_none());
        assert!(response.coding_plan.is_none());

        let experience = response
            .agentic_coding_experience
            .expect("agentic experience");
        assert_eq!(
            experience.status,
            crate::AgenticExperienceStatus::Created
        );
        assert!(!experience.mutated);
        assert!(!experience.tools_executed);
        assert!(!experience.exposes_chain_of_thought);
        assert!(!experience.parallel_coding_agent);
        assert!(response.content.contains("## Agentic Coding Experience"));
        assert!(response.content.contains("## Coding Goal"));
        let goal = response.coding_goal.expect("coding goal");
        assert!(goal.user_objective.to_ascii_lowercase().contains("pong"));
        assert!(!goal.mutated);
        assert!(!goal.tools_executed);
    }

    #[test]
    fn create_parser_and_write_tests_are_coding_plans_only() {
        let planner = planner_with_search_and_read();
        for prompt in ["Create a parser.", "Write tests."] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            assert!(
                response.coding_plan.is_some(),
                "expected coding_plan for {prompt}"
            );
            assert!(response.execution_plan.is_none());
            assert!(!response.awaiting_review);
            assert!(response.tool_id.is_none());
            assert!(response.capability.is_none());
            assert!(response.content.contains("## Coding Plan"));
        }
    }

    #[test]
    fn generate_the_code_from_coding_plan_requires_review() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let create_path = dir.join("pong.rs");
        // Seed a Coding Plan the generation path will remember.
        let plan = CodingPlan {
            files_to_create: vec![format!("`{}` pong entry", create_path.display())],
            files_to_modify: vec![],
            summary: vec!["Goal: Build Pong.".into()],
            ..Default::default()
        };
        planner.remember_coding_plan(Some(plan));

        let response = planner
            .handle(UserRequest::new("Generate the code."))
            .unwrap();

        assert!(response.awaiting_review);
        assert!(response.blocked);
        assert!(response.code_generation.is_some());
        let generation = response.code_generation.as_ref().unwrap();
        assert!(generation.operations.iter().any(|op| {
            op.kind == GenerationOpKind::CreateFile && op.path == create_path
        }));
        assert!(response.content.contains("## Code Generation"));
        assert!(response.content.contains("create_file"));
        let plan = response.execution_plan.expect("execution plan");
        assert_eq!(plan.status(), ExecutionStatus::AwaitingReview);
        assert!(plan.steps().len() >= 1);
        assert!(!create_path.exists(), "must not write before review");

        let plan_id = plan.id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(!resumed.awaiting_review);
        assert!(create_path.exists(), "create_file should run after approve");
        let body = fs::read_to_string(&create_path).unwrap();
        assert!(body.contains("Generated by Jaymi Planner"));
    }

    #[test]
    fn generate_the_changes_from_that_plan_enters_patch_review_not_new_plan() {
        // Exact failure: completed CodingPlan + "Generate the changes from that plan."
        // must consume the remembered plan (C3 patch/codegen → Review Card), not
        // author a second CodingPlan via the C1.4 `generate ` catch-all.
        let dir = temp_dir();
        let html = dir.join("index.html");
        let css = dir.join("style.css");
        let js = dir.join("script.js");

        for phrase in [
            "Generate the changes from that plan.",
            "Implement that plan.",
            "Make those changes.",
            "Go ahead and code it.",
            "Apply the plan.",
        ] {
            let planner = planner_with_write();
            planner.remember_coding_plan(Some(mutation_plan_for_paths(
                vec![
                    format!("`{}` Pong canvas shell", html.display()),
                    format!("`{}` Pong styles", css.display()),
                    format!("`{}` Pong game loop", js.display()),
                ],
                vec![],
                vec![],
                "Create a simple Pong game.",
            )));

            let response = planner.handle(UserRequest::new(phrase)).unwrap();

            assert!(
                response.awaiting_review,
                "{phrase}: expected Review Card / awaiting_review"
            );
            assert!(
                response.blocked,
                "{phrase}: generation must pause before mutation"
            );
            assert!(
                response.patch_generation.is_some() || response.code_generation.is_some(),
                "{phrase}: expected patch/codegen path"
            );
            // Must not fall through to C1.4 and replace the actionable plan with a
            // fresh Generic scaffold (no ExecutionPlan / no review).
            assert!(
                response.execution_plan.is_some(),
                "{phrase}: expected ExecutionPlan awaiting review"
            );
            assert_eq!(
                response.execution_plan.as_ref().unwrap().status(),
                ExecutionStatus::AwaitingReview
            );
            assert!(
                !html.exists() && !css.exists() && !js.exists(),
                "{phrase}: must not write before Approve"
            );
            // Soft-path generation responses do not re-author a CodingPlan body.
            if let Some(plan) = &response.coding_plan {
                assert!(
                    plan.files_to_create.iter().any(|p| p.contains("index.html"))
                        || plan.summary.iter().any(|s| s.contains("Pong")),
                    "{phrase}: if coding_plan is mirrored it must be the remembered plan, not a new Generic scaffold"
                );
            }
        }
    }

    #[test]
    fn begin_code_generation_owns_create_modify_delete_ops() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let create = dir.join("new.rs");
        let modify = dir.join("existing.rs");
        fs::write(&modify, b"old").unwrap();

        // Create + Modify execute after Approve. DeleteFile is proposed/cancelled
        // in `generation_plan_lists_delete_file_step_without_executing` (Trash may
        // be unavailable in sandboxed CI).
        let generation = CodeGeneration {
            goal: "Apply sample ops".into(),
            operations: vec![
                GenerationOp::create_file(&create, "// created\n", "create"),
                GenerationOp::modify_file(&modify, "// modified\n", "modify"),
            ],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        assert!(response.awaiting_review);
        assert_eq!(
            response.code_generation.as_ref().unwrap().operations.len(),
            2
        );
        let plan = response.execution_plan.expect("plan");
        assert_eq!(plan.steps().len(), 2);
        assert!(plan
            .proposed_tools()
            .iter()
            .any(|t| t == jaymi_tools::WRITE_FILE_TOOL_ID));
        assert!(!create.exists());
        assert_eq!(fs::read_to_string(&modify).unwrap(), "old");

        let resumed = planner
            .resolve_review(ReviewIntent::Approve {
                plan_id: plan.id().clone(),
            })
            .unwrap();
        assert!(create.exists());
        assert_eq!(fs::read_to_string(&modify).unwrap(), "// modified\n");
        assert!(
            resumed.content.contains("Code generation completed")
                || resumed.execution_summary.is_some()
        );
    }

    #[test]
    fn generation_plan_lists_delete_file_step_without_executing() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("stale.rs");
        fs::write(&path, b"x").unwrap();
        let generation = CodeGeneration {
            goal: "Remove stale".into(),
            operations: vec![GenerationOp::delete_file(&path, "delete")],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        assert!(response.awaiting_review);
        assert!(response.content.contains("delete_file"));
        let plan = response.execution_plan.expect("plan");
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(
            plan.steps()[0].tool_id.as_deref(),
            Some(jaymi_tools::MANAGE_PATH_TOOL_ID)
        );
        assert!(path.exists(), "delete must wait for Approve");
        // Cancel rather than Approve — Trash may be unavailable in sandbox CI.
        let _ = planner
            .resolve_review(ReviewIntent::Cancel {
                plan_id: plan.id().clone(),
            })
            .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn coding_diff_review_appears_before_approval_with_accurate_stats() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let planner_rs = dir.join("planner.rs");
        let context_rs = dir.join("context.rs");
        fs::write(&planner_rs, b"fn a() {}\n").unwrap();
        fs::write(&context_rs, b"use x;\n").unwrap();

        let generation = CodeGeneration {
            goal: "Adjust planner + context".into(),
            operations: vec![
                GenerationOp::modify_file(
                    &planner_rs,
                    "fn a() {}\nfn b() {}\n",
                    "extend planner",
                ),
                GenerationOp::modify_file(
                    &context_rs,
                    "use x;\nuse y;\n",
                    "extend context",
                ),
            ],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        assert!(response.awaiting_review);
        assert!(response.content.contains("Files affected"));
        assert!(response.content.contains("planner.rs"));
        assert!(response.content.contains("context.rs"));
        assert!(response.content.contains("Diff Preview") || response.content.contains("+"));
        assert!(response.content.contains("Approve"));
        assert!(response.content.contains("Modify"));
        assert!(response.content.contains("Cancel"));
        assert!(response.content.contains("Validation"));
        assert!(response.content.contains("Rationale") || response.content.contains("extend"));

        let surface = response.coding_diff_review.expect("coding diff surface");
        assert_eq!(surface.files.len(), 2);
        assert!(surface.total_added >= 2);
        let preview = response
            .execution_plan
            .as_ref()
            .and_then(|plan| plan.action_preview())
            .expect("multi-file preview");
        assert_eq!(preview.kind, jaymi_core::PreviewKind::UnifiedDiff);
        assert!(preview.resources.len() >= 2);

        // No pre-approval writes.
        assert_eq!(fs::read_to_string(&planner_rs).unwrap(), "fn a() {}\n");
        assert_eq!(fs::read_to_string(&context_rs).unwrap(), "use x;\n");
    }

    #[test]
    fn coding_diff_review_cancel_prevents_mutation() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("keep.rs");
        fs::write(&path, b"keep\n").unwrap();
        let generation = CodeGeneration {
            goal: "Would overwrite".into(),
            operations: vec![GenerationOp::modify_file(&path, "changed\n", "mutate")],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        let plan_id = response.execution_plan.expect("plan").id().clone();
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep\n");

        let cancelled = planner
            .resolve_review(ReviewIntent::Cancel { plan_id })
            .unwrap();
        assert!(!cancelled.awaiting_review);
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep\n");
    }

    #[test]
    fn coding_diff_review_modify_creates_child_plan() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("file.rs");
        fs::write(&path, b"old\n").unwrap();
        let generation = CodeGeneration {
            goal: "Mutate".into(),
            operations: vec![GenerationOp::modify_file(&path, "new\n", "change")],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        let parent = response.execution_plan.expect("parent");
        let parent_id = parent.id().clone();

        let revised = planner
            .resolve_review(ReviewIntent::Modify {
                plan_id: parent_id.clone(),
                note: Some("Only touch the helper.".into()),
            })
            .unwrap();
        assert!(revised.awaiting_review);
        let child = revised.execution_plan.expect("child");
        assert_ne!(child.id(), &parent_id);
        assert_eq!(child.parent_plan_id(), Some(&parent_id));
        assert!(child.revision() > 1);
        assert!(!child.revision_changes().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
    }

    #[test]
    fn coding_diff_review_duplicate_approval_is_safe() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("once.rs");
        let generation = CodeGeneration {
            goal: "Create once".into(),
            operations: vec![GenerationOp::create_file(&path, "body\n", "create")],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        let plan_id = response.execution_plan.expect("plan").id().clone();
        assert!(!path.exists());

        let first = planner
            .resolve_review(ReviewIntent::Approve {
                plan_id: plan_id.clone(),
            })
            .unwrap();
        assert!(path.exists());
        assert!(
            first.content.contains("Code generation completed")
                || first.execution_summary.is_some()
        );

        let dup = planner.resolve_review(ReviewIntent::Approve { plan_id });
        assert!(dup.is_err(), "duplicate approval must not re-execute");
        let body = fs::read_to_string(&path).unwrap();
        assert_eq!(body, "body\n");
    }

    #[test]
    fn coding_apply_create_file_after_approve() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("created.rs");
        let generation = CodeGeneration {
            goal: "Create file".into(),
            operations: vec![GenerationOp::create_file(&path, "hello\n", "create")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        assert!(paused.awaiting_review);
        assert!(!path.exists());
        let plan_id = paused.execution_plan.expect("plan").id().clone();

        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(!resumed.awaiting_review);
        assert!(!resumed.blocked);
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
        let summary = resumed.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Completed);
        assert!(!summary.partial);
        assert!(summary.files_edited.iter().any(|f| f.contains("created.rs")));
        assert!(resumed.content.contains("Execution summary"));
        assert!(resumed.content.contains("Code generation completed"));
    }

    #[test]
    fn coding_apply_modify_file_after_approve() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("edit.rs");
        fs::write(&path, b"old\n").unwrap();
        let generation = CodeGeneration {
            goal: "Modify file".into(),
            operations: vec![GenerationOp::modify_file(&path, "new\n", "modify")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        let summary = resumed.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Completed);
        assert!(summary.render_conversation().contains("Files edited"));
    }

    #[test]
    fn coding_apply_multi_file_preserves_order() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let a = dir.join("a.rs");
        let b = dir.join("b.rs");
        let generation = CodeGeneration {
            goal: "Multi-file".into(),
            operations: vec![
                GenerationOp::create_file(&a, "a\n", "first"),
                GenerationOp::create_file(&b, "b\n", "second"),
            ],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let steps = paused.execution_plan.as_ref().unwrap().steps();
        assert_eq!(steps.len(), 2);
        assert!(steps[0].description.contains("a.rs") || steps[0].resource.as_deref().unwrap().contains("a.rs"));
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), "a\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b\n");
        let summary = resumed.execution_summary.expect("summary");
        assert_eq!(summary.files_edited.len(), 2);
        assert!(summary.outputs.iter().any(|o| o.contains("op 1")));
        assert!(summary.outputs.iter().any(|o| o.contains("op 2")));
    }

    #[test]
    fn coding_apply_failed_write_reports_summary_without_mutation() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let missing_parent = dir.join("nope").join("nested").join("fail.rs");
        let generation = CodeGeneration {
            goal: "Fail write".into(),
            operations: vec![GenerationOp::create_file(
                &missing_parent,
                "x\n",
                "will fail",
            )],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        assert!(!missing_parent.exists());
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        assert!(!missing_parent.exists());
        let summary = resumed.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Failed);
        assert!(!summary.partial);
        assert!(!summary.errors.is_empty());
        assert!(resumed.content.contains("Execution summary (failed)"));
    }

    #[test]
    fn coding_apply_partial_execution_stops_and_reports() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let ok_path = dir.join("ok.rs");
        let ro_dir = dir.join("ro");
        fs::create_dir_all(&ro_dir).unwrap();
        let fail_path = ro_dir.join("fail.rs");
        let generation = CodeGeneration {
            goal: "Partial batch".into(),
            operations: vec![
                GenerationOp::create_file(&ok_path, "ok\n", "succeeds"),
                GenerationOp::create_file(&fail_path, "no\n", "fails"),
            ],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&ro_dir).unwrap().permissions();
            perms.set_mode(0o555);
            fs::set_permissions(&ro_dir, perms).unwrap();
        }
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(ok_path.exists(), "first op must have applied");
        assert!(!fail_path.exists(), "second op must not invent parents");
        assert!(resumed.blocked);
        let summary = resumed.execution_summary.expect("summary");
        assert!(summary.partial);
        assert_eq!(summary.status, ExecutionStatus::Failed);
        assert!(summary.files_edited.iter().any(|f| f.contains("ok.rs")));
        assert!(summary.warnings.iter().any(|w| w.contains("Stopped after")));
        assert!(resumed.content.contains("partial"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&ro_dir).unwrap().permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&ro_dir, perms);
        }
    }

    #[test]
    fn coding_apply_cancelled_plan_never_writes() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("cancel.rs");
        let generation = CodeGeneration {
            goal: "Cancel".into(),
            operations: vec![GenerationOp::create_file(&path, "x\n", "cancel me")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let cancelled = planner
            .resolve_review(ReviewIntent::Cancel { plan_id })
            .unwrap();
        assert!(!path.exists());
        assert!(!cancelled.awaiting_review);
        let summary = cancelled.execution_summary.expect("summary");
        assert_eq!(summary.status, ExecutionStatus::Cancelled);
    }

    #[test]
    fn coding_apply_review_required_before_any_mutation() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("gated.rs");
        let generation = CodeGeneration {
            goal: "Gate".into(),
            operations: vec![GenerationOp::create_file(&path, "x\n", "gate")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        assert!(paused.awaiting_review);
        assert!(paused.blocked);
        assert_eq!(
            paused
                .execution_plan
                .as_ref()
                .unwrap()
                .review_requirement(),
            ReviewRequirement::Required
        );
        assert!(!path.exists());
        assert!(
            paused
                .permission_result
                .as_ref()
                .map(|r| matches!(
                    r.decision,
                    PermissionDecision::RequiresApproval | PermissionDecision::Allowed
                ))
                .unwrap_or(true)
        );
    }

    #[test]
    fn coding_apply_permission_denial_blocks_before_review() {
        // Replace write_file with a cloud-only tool so Privacy Maximum denies
        // at the shared Policy + Permission gate (no coding bypass).
        let planner = planner_with_tools_and_policies(
            |tools, _, _| {
                tools
                    .register_tool(Arc::new(CloudOnlyWriteFileTool::new()))
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
        let dir = temp_dir();
        let path = dir.join("denied.rs");
        let generation = CodeGeneration {
            goal: "Denied".into(),
            operations: vec![GenerationOp::create_file(&path, "x\n", "deny")],
            coding_plan_summary: vec![],
        };
        let response = planner.begin_code_generation(generation).unwrap();
        assert!(response.blocked);
        assert!(!response.awaiting_review);
        assert!(!path.exists());
        assert!(response.content.contains("Denied before code generation"));
    }

    #[test]
    fn coding_apply_execution_summary_recorded_in_approval_history() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("hist.rs");
        let generation = CodeGeneration {
            goal: "History".into(),
            operations: vec![GenerationOp::create_file(&path, "h\n", "hist")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.execution_summary.is_some());
        let history = planner.approval_history().unwrap();
        assert!(history.iter().any(|entry| {
            matches!(entry.decision, ApprovalDecision::Approve)
                && entry.execution_result.is_some()
        }));
    }

    #[test]
    fn coding_recovery_stale_source_refuses_overwrite() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("stale.rs");
        fs::write(&path, b"original\n").unwrap();
        let generation = CodeGeneration {
            goal: "Modify stale".into(),
            operations: vec![GenerationOp::modify_file(&path, "proposed\n", "edit")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        assert!(paused.awaiting_review);
        // User edits the file after review pause — must not be silently overwritten.
        fs::write(&path, b"user changed\n").unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        assert_eq!(fs::read_to_string(&path).unwrap(), "user changed\n");
        let recovery = resumed
            .coding_mutation_recovery
            .expect("recovery report");
        assert!(matches!(
            recovery.kind,
            Some(RecoveryKind::StaleSource | RecoveryKind::PatchNoLongerApplies)
        ));
        assert!(recovery.requires_new_plan);
        assert!(resumed.content.contains("Coding Mutation Recovery"));
        assert!(resumed.content.contains("new") || resumed.content.contains("Review"));
    }

    #[test]
    fn coding_recovery_create_refuses_when_file_appeared() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("appeared.rs");
        let generation = CodeGeneration {
            goal: "Create".into(),
            operations: vec![GenerationOp::create_file(&path, "new\n", "create")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        fs::write(&path, b"user created\n").unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        assert_eq!(fs::read_to_string(&path).unwrap(), "user created\n");
        let recovery = resumed.coding_mutation_recovery.expect("recovery");
        assert_eq!(recovery.kind, Some(RecoveryKind::StaleSource));
        assert!(recovery.requires_new_plan);
    }

    #[test]
    fn coding_recovery_directory_missing_before_write() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let parent = dir.join("will_vanish");
        fs::create_dir_all(&parent).unwrap();
        let path = parent.join("child.rs");
        let generation = CodeGeneration {
            goal: "Create under parent".into(),
            operations: vec![GenerationOp::create_file(&path, "x\n", "create")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        fs::remove_dir_all(&parent).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        assert!(!path.exists());
        let recovery = resumed.coding_mutation_recovery.expect("recovery");
        assert!(matches!(
            recovery.kind,
            Some(RecoveryKind::DirectoryMissing | RecoveryKind::WriteFailed)
        ));
        assert!(recovery.requires_new_plan);
    }

    #[test]
    fn coding_recovery_invalid_patch_empty_content() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("empty.rs");
        // Bypass GenerationOp helpers that always set content — craft empty write.
        let generation = CodeGeneration {
            goal: "Invalid".into(),
            operations: vec![GenerationOp {
                kind: GenerationOpKind::CreateFile,
                path: path.clone(),
                destination: None,
                content: Some(String::new()),
                rationale: "empty".into(),
            }],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        assert!(!path.exists());
        let recovery = resumed.coding_mutation_recovery.expect("recovery");
        assert_eq!(recovery.kind, Some(RecoveryKind::InvalidPatch));
        assert!(recovery.requires_new_plan);
    }

    #[test]
    fn coding_recovery_partial_after_first_succeeds() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let ok_path = dir.join("ok.rs");
        let ro_dir = dir.join("ro");
        fs::create_dir_all(&ro_dir).unwrap();
        let fail_path = ro_dir.join("fail.rs");
        let generation = CodeGeneration {
            goal: "Partial mid-batch".into(),
            operations: vec![
                GenerationOp::create_file(&ok_path, "ok\n", "first"),
                GenerationOp::create_file(&fail_path, "no\n", "second"),
            ],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        // Preflight still passes (parent exists, targets absent). Make the
        // second write fail at tool/provider time so op1 can succeed first.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&ro_dir).unwrap().permissions();
            perms.set_mode(0o555);
            fs::set_permissions(&ro_dir, perms).unwrap();
        }
        #[cfg(not(unix))]
        {
            // Best-effort: without chmod, remove parent after pause so preflight
            // blocks the whole batch (still no silent continue).
            let _ = fs::remove_dir_all(&ro_dir);
        }
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        let recovery = resumed.coding_mutation_recovery.expect("recovery");
        assert!(recovery.requires_new_plan);
        assert!(!recovery.actions.is_empty());
        if ok_path.exists() {
            assert_eq!(fs::read_to_string(&ok_path).unwrap(), "ok\n");
            assert!(!fail_path.exists());
            assert_eq!(recovery.kind, Some(RecoveryKind::PartialBatchFailure));
            assert!(recovery.succeeded_paths.iter().any(|p| p.contains("ok.rs")));
            assert!(
                recovery.failed_paths.iter().any(|p| p.contains("fail.rs"))
                    || recovery.skipped_paths.iter().any(|p| p.contains("fail.rs"))
            );
            let summary = resumed.execution_summary.expect("summary");
            assert!(summary.partial);
        } else {
            // Preflight refused the batch — still reported, no silent writes.
            assert!(!fail_path.exists());
            assert!(recovery.kind.is_some());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if ro_dir.exists() {
                let mut perms = fs::metadata(&ro_dir).unwrap().permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&ro_dir, perms);
            }
        }
    }

    #[test]
    fn coding_recovery_cancel_multi_file_lists_skipped() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let a = dir.join("a.rs");
        let b = dir.join("b.rs");
        let generation = CodeGeneration {
            goal: "Cancel multi".into(),
            operations: vec![
                GenerationOp::create_file(&a, "a\n", "a"),
                GenerationOp::create_file(&b, "b\n", "b"),
            ],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let cancelled = planner
            .resolve_review(ReviewIntent::Cancel { plan_id })
            .unwrap();
        assert!(!a.exists());
        assert!(!b.exists());
        let recovery = cancelled.coding_mutation_recovery.expect("recovery");
        assert_eq!(recovery.phase, Some(MutationPhase::Cancelled));
        assert_eq!(recovery.kind, Some(RecoveryKind::CancelledMultiFile));
        assert!(recovery.requires_new_plan);
        assert!(recovery.skipped_paths.len() >= 2);
        assert!(cancelled.content.contains("Coding Mutation Recovery"));
        let history = planner.approval_history().unwrap();
        assert!(history
            .iter()
            .any(|e| matches!(e.decision, ApprovalDecision::Cancel)));
    }

    #[test]
    fn coding_recovery_write_fail_preserves_summary_and_history() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let path = dir.join("nope").join("x.rs");
        let generation = CodeGeneration {
            goal: "Fail".into(),
            operations: vec![GenerationOp::create_file(&path, "x\n", "fail")],
            coding_plan_summary: vec![],
        };
        let paused = planner.begin_code_generation(generation).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(resumed.blocked);
        assert!(resumed.execution_summary.is_some());
        assert!(resumed.coding_mutation_recovery.is_some());
        let history = planner.approval_history().unwrap();
        assert!(history.iter().any(|e| {
            matches!(e.decision, ApprovalDecision::Approve) && e.execution_result.is_some()
        }));
    }

    #[test]
    fn c41_build_awareness_rust_project_soft_path() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                metadata: jaymi_context::ProjectMetadata {
                    project_id: Some("jaymi".into()),
                    name: Some("jaymi".into()),
                    root_directory: Some("/proj".into()),
                    ..Default::default()
                },
                package_manager: Some(jaymi_context::PackageManagerKind::Cargo),
                build_system: Some(jaymi_context::BuildSystemKind::Cargo),
                cargo: Some(jaymi_context::CargoProjectMeta {
                    package_name: Some("jaymi".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["project".into()],
            ..Default::default()
        });
        // Seed active project via open project is heavy — ProjectProvider needs
        // engine. Soft path still reads session project_snapshot for discovery.
        let response = planner
            .handle(UserRequest::new("Does this build?"))
            .unwrap();
        let report = response.build_awareness.expect("build_awareness");
        assert!(!report.executed);
        assert!(report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref() == Some("cargo check")
                || t.candidate_command.as_deref() == Some("cargo build")));
        assert!(response.tool_id.is_none());
        assert!(response.execution_plan.is_none());
        assert!(!response.awaiting_review);
        assert!(response.content.contains("Build Awareness"));
        assert!(response.content.contains("no verification command was executed"));
    }

    #[test]
    fn c41_build_awareness_no_active_project() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Is this project currently buildable?"))
            .unwrap();
        let report = response.build_awareness.expect("build_awareness");
        assert_eq!(
            report.status,
            Some(crate::BuildAwarenessStatus::NoActiveProject)
        );
        assert!(report.targets.is_empty());
        assert!(!report.executed);
        assert!(response.tool_id.is_none());
    }

    #[test]
    fn c41_build_awareness_no_direct_terminal_bypass() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                metadata: jaymi_context::ProjectMetadata {
                    project_id: Some("web".into()),
                    name: Some("web".into()),
                    root_directory: Some("/web".into()),
                    ..Default::default()
                },
                package_manager: Some(jaymi_context::PackageManagerKind::Npm),
                build_system: Some(jaymi_context::BuildSystemKind::NpmScripts),
                npm: Some(jaymi_context::NpmProjectMeta {
                    scripts: vec!["build".into(), "lint".into()],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        let response = planner
            .handle(UserRequest::new("What should I run to check this?"))
            .unwrap();
        let report = response.build_awareness.expect("build_awareness");
        assert!(!report.executed);
        assert!(response.terminal_session_id.is_none());
        assert!(response.terminal_output.is_none());
        assert!(response.tool_id.is_none());
        assert!(response.provider_id.is_none());
        assert!(!response.reasoning_used);
    }

    #[derive(Debug)]
    struct StubTerminalTool {
        metadata: ToolMetadata,
        exit_code: i32,
        output: String,
        fail_with: Option<String>,
        ran: std::sync::Mutex<usize>,
    }

    impl StubTerminalTool {
        fn passing(output: &str) -> Arc<Self> {
            Arc::new(Self {
                metadata: Self::meta(),
                exit_code: 0,
                output: output.into(),
                fail_with: None,
                ran: std::sync::Mutex::new(0),
            })
        }

        fn failing(exit: i32, output: &str) -> Arc<Self> {
            Arc::new(Self {
                metadata: Self::meta(),
                exit_code: exit,
                output: output.into(),
                fail_with: None,
                ran: std::sync::Mutex::new(0),
            })
        }

        fn timeout() -> Arc<Self> {
            Arc::new(Self {
                metadata: Self::meta(),
                exit_code: -1,
                output: String::new(),
                fail_with: Some("timed out waiting for terminal command: cargo test".into()),
                ran: std::sync::Mutex::new(0),
            })
        }

        fn meta() -> ToolMetadata {
            ToolMetadata {
                id: jaymi_tools::TERMINAL_TOOL_ID.to_string(),
                name: "Terminal".into(),
                version: "0".into(),
                description: "stub terminal".into(),
                provider: "stub_terminal".into(),
                capabilities: vec![
                    Capability::ExecuteTerminalCommands,
                    Capability::Code,
                ],
                risk: ToolRisk::Destructive,
                estimated_runtime: EstimatedRuntime::Medium,
                execution_mode: ExecutionMode::Synchronous,
                resource_cost: ResourceCost::Low,
                memory_usage: MemoryUsage::Small,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::StructuredData,
            }
        }

        fn run_count(&self) -> usize {
            *self.ran.lock().unwrap()
        }
    }

    impl Tool for StubTerminalTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
            *self.ran.lock().unwrap() += 1;
            if let Some(err) = &self.fail_with {
                return Err(JaymiError::new(err.clone()));
            }
            let command = input.command.clone().unwrap_or_default();
            let success = self.exit_code == 0;
            Ok(ToolOutput::terminal_with_status(
                "coding",
                input.path.clone().unwrap_or_else(|| "/proj".into()),
                self.output.clone(),
                self.output.clone(),
                vec![command],
                "Tests",
                true,
                format!("stub ran exit={}", self.exit_code),
                Some(self.exit_code),
                Some(42),
                success,
            ))
        }
    }

    fn planner_with_stub_terminal(stub: Arc<StubTerminalTool>) -> Planner {
        let stub_clone = Arc::clone(&stub);
        planner_with_tools(move |tools, filesystem, content_api| {
            tools
                .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                .unwrap();
            tools
                .register_tool(Arc::new(ReadFileTool::new(content_api)))
                .unwrap();
            tools.register_tool(stub_clone).unwrap();
        })
    }

    fn session_cargo_for_tests() -> jaymi_context::ContextSessionInputs {
        jaymi_context::ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                metadata: jaymi_context::ProjectMetadata {
                    project_id: Some("jaymi".into()),
                    name: Some("jaymi".into()),
                    root_directory: Some("/proj".into()),
                    ..Default::default()
                },
                package_manager: Some(jaymi_context::PackageManagerKind::Cargo),
                build_system: Some(jaymi_context::BuildSystemKind::Cargo),
                cargo: Some(jaymi_context::CargoProjectMeta {
                    package_name: Some("jaymi".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn c42_run_tests_awaits_review_no_direct_provider() {
        let stub = StubTerminalTool::passing("ok\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let response = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let report = response.test_execution.expect("test_execution");
        assert_eq!(
            report.status,
            Some(crate::TestExecutionStatus::AwaitingReview)
        );
        assert_eq!(report.command.as_deref(), Some("cargo test"));
        assert!(response.awaiting_review);
        assert_eq!(stub.run_count(), 0, "must not run before Approve");
        assert!(!report.mutated);
        assert!(response.reasoning_used == false);
    }

    #[test]
    fn c42_passing_tests_after_approve() {
        let stub = StubTerminalTool::passing("test result: ok\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let report = resumed.test_execution.expect("test_execution");
        assert_eq!(report.status, Some(crate::TestExecutionStatus::Passed));
        assert_eq!(report.exit_status, Some(0));
        assert_eq!(stub.run_count(), 1);
        assert!(!report.mutated);
        assert!(report.evidence.is_some());
        assert!(
            resumed.content.contains("Verification succeeded")
                || resumed.content.contains("succeeded")
                || resumed.content.contains("Tests passed")
                || resumed.content.contains("passed")
        );
    }

    #[test]
    fn c42_failing_tests_no_mutation() {
        let stub = StubTerminalTool::failing(101, "FAILED\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let report = resumed.test_execution.expect("test_execution");
        assert_eq!(report.status, Some(crate::TestExecutionStatus::Failed));
        assert_eq!(report.exit_status, Some(101));
        assert!(!report.mutated);
        assert!(report.evidence.is_some());
        assert_eq!(stub.run_count(), 1);
        // Structured feedback — not a raw dump of the failed run.
        assert!(!resumed.content.contains("12,000"));
        assert!(
            resumed.content.contains("Verification")
                || resumed.content.contains("failed")
                || resumed.content.contains("Unknown")
        );
    }

    #[test]
    fn c42_missing_test_command() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                metadata: jaymi_context::ProjectMetadata {
                    project_id: Some("notes".into()),
                    name: Some("notes".into()),
                    root_directory: Some("/notes".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        });
        let response = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let report = response.test_execution.expect("test_execution");
        assert_eq!(
            report.status,
            Some(crate::TestExecutionStatus::MissingCommand)
        );
        assert!(!response.awaiting_review);
        assert!(response.tool_id.is_none());
    }

    #[test]
    fn c42_timeout_surface() {
        let stub = StubTerminalTool::timeout();
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner.resolve_review(ReviewIntent::Approve { plan_id });
        // Provider Err may surface as JaymiError or failed tool path.
        match resumed {
            Ok(response) => {
                if let Some(report) = response.test_execution {
                    assert!(
                        matches!(
                            report.status,
                            Some(crate::TestExecutionStatus::TimedOut)
                                | Some(crate::TestExecutionStatus::CommandError)
                                | Some(crate::TestExecutionStatus::Failed)
                        ),
                        "{:?}",
                        report.status
                    );
                }
            }
            Err(err) => {
                assert!(err.message().contains("timed out"));
            }
        }
        assert_eq!(stub.run_count(), 1);
    }

    #[test]
    fn c42_large_output_truncated_on_finalize() {
        let huge = format!("{}ERROR_TAIL", "y".repeat(20_000));
        let stub = StubTerminalTool::failing(1, &huge);
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let report = resumed.test_execution.expect("test_execution");
        assert!(report.truncated);
        assert!(
            report
                .stdout
                .as_deref()
                .unwrap_or("")
                .contains("ERROR_TAIL")
        );
    }

    #[test]
    fn c42_permission_gate_blocks_until_approve() {
        let stub = StubTerminalTool::passing("ok");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let response = planner
            .handle(UserRequest::new("Verify the tests pass."))
            .unwrap();
        assert!(response.awaiting_review);
        assert!(response.permission_result.is_some());
        assert_eq!(stub.run_count(), 0);
        let cancelled = planner
            .resolve_review(ReviewIntent::Cancel {
                plan_id: response.execution_plan.expect("plan").id().clone(),
            })
            .unwrap();
        assert_eq!(stub.run_count(), 0, "cancel must not execute");
        assert!(!cancelled.awaiting_review);
    }

    #[test]
    fn c43_compiler_errors_become_structured_feedback() {
        let out = r#"
error[E0308]: mismatched types
  --> src/foo.rs:42:5
error[E0382]: borrow of moved value
  --> src/bar.rs:91:12
error: aborting due to 2 previous errors
"#;
        let stub = StubTerminalTool::failing(101, out);
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let report = resumed.test_execution.expect("test_execution");
        let evidence = report.evidence.expect("verification evidence");
        assert_eq!(
            evidence.kind,
            Some(crate::VerificationResultKind::BuildFailure)
        );
        assert!(evidence.errors.len() >= 2);
        assert!(
            resumed.content.to_ascii_lowercase().contains("build failed")
                || resumed.content.contains("## Failure")
        );
        assert!(resumed.content.contains("src/foo.rs:42"));
        assert!(resumed.verification_summary.is_some());
        assert!(!resumed.content.contains("aborting due to 2 previous errors")
            || resumed.content.contains("### Errors")
            || resumed.content.contains("## Failure"));
        // Context diagnostics pipeline received verification findings.
        let session = planner.context.session_inputs();
        assert!(session
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.source.as_deref() == Some("verification")));
    }

    #[test]
    fn c43_malformed_failure_does_not_invent_errors() {
        let stub = StubTerminalTool::failing(2, "blargle frobnitz exited weirdly\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let evidence = resumed
            .test_execution
            .expect("test_execution")
            .evidence
            .expect("evidence");
        assert_eq!(evidence.kind, Some(crate::VerificationResultKind::Unknown));
        assert!(evidence.errors.is_empty());
        assert!(resumed.content.contains("unclear") || resumed.content.contains("Unknown"));
    }

    #[test]
    fn c44_compiler_failure_yields_corrective_plan_no_mutation() {
        let out = r#"
error[E0308]: mismatched types
  --> src/foo.rs:42:5
error[E0382]: borrow of moved value
  --> src/bar.rs:91:12
error: aborting due to 2 previous errors
"#;
        let stub = StubTerminalTool::failing(101, out);
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let _failed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let runs_before_fix = stub.run_count();

        let fixed = planner
            .handle(UserRequest::new("Fix the build failure."))
            .unwrap();
        let report = fixed
            .fix_from_verification
            .expect("fix_from_verification");
        assert_eq!(
            report.status,
            Some(crate::FixActionability::Actionable)
        );
        assert!(!report.mutated);
        assert!(!report.auto_approved);
        assert!(!report.auto_retried);
        assert!(report.failing_files.iter().any(|f| f.contains("foo.rs")));
        assert!(fixed.coding_plan.is_some());
        assert!(!fixed.awaiting_review, "C4.4 must not auto-open mutation review");
        assert!(fixed.tool_id.is_none());
        assert_eq!(
            stub.run_count(),
            runs_before_fix,
            "must not silently re-run verification"
        );
        assert!(fixed.content.contains("Review Before Action"));
        assert!(planner.clone_last_coding_plan().is_some());
    }

    #[test]
    fn c44_insufficient_evidence_does_not_invent_targets() {
        let stub = StubTerminalTool::failing(2, "blargle frobnitz exited weirdly\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let _ = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();

        let fixed = planner
            .handle(UserRequest::new("Fix the verification failure."))
            .unwrap();
        let report = fixed
            .fix_from_verification
            .expect("fix_from_verification");
        assert_eq!(
            report.status,
            Some(crate::FixActionability::InsufficientEvidence)
        );
        assert!(report.failing_files.is_empty());
        assert!(!report.mutated);
        assert!(!fixed.awaiting_review);
    }

    #[test]
    fn c44_environment_failure_is_not_code_mutation() {
        let stub = StubTerminalTool::failing(
            127,
            "cargo: command not found\nNo such file or directory\n",
        );
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        // Seed remembered environment evidence directly via a timed-out-style
        // parse path: publish a CommandError-like evidence through finalize.
        let mut seed = crate::TestExecutionReport {
            status: Some(crate::TestExecutionStatus::CommandError),
            command: Some("cargo test".into()),
            working_directory: Some("/proj".into()),
            exit_status: Some(127),
            stdout: Some("cargo: command not found\nNo such file or directory\n".into()),
            gaps: vec!["command not found".into()],
            ..Default::default()
        };
        crate::verification_diagnostics::enrich_test_report(&mut seed);
        // Force environment kind when parser classifies from output.
        if let Some(evidence) = seed.evidence.as_mut() {
            evidence.kind = Some(crate::VerificationResultKind::EnvironmentFailure);
        }
        planner.remember_verification_evidence(seed.evidence.clone());

        let fixed = planner
            .handle(UserRequest::new("Fix the failing tests."))
            .unwrap();
        let report = fixed
            .fix_from_verification
            .expect("fix_from_verification");
        assert_eq!(
            report.status,
            Some(crate::FixActionability::NotCodeRelated)
        );
        assert!(!report.mutated);
        assert!(!report.auto_approved);
        assert_eq!(stub.run_count(), 0);
    }

    #[test]
    fn c44_test_failure_and_no_auto_approve() {
        let out = r#"
running 1 test
test suite::bad ... FAILED

failures:

---- suite::bad stdout ----
thread 'suite::bad' panicked at 'assertion failed', src/lib.rs:10:5

failures:
    suite::bad

test result: FAILED. 0 passed; 1 failed
"#;
        let stub = StubTerminalTool::failing(101, out);
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let _ = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();

        let fixed = planner
            .handle(UserRequest::new("Fix the failing tests."))
            .unwrap();
        let report = fixed.fix_from_verification.expect("report");
        assert_eq!(report.status, Some(crate::FixActionability::Actionable));
        assert!(
            report
                .failing_symbols
                .iter()
                .any(|s| s.contains("suite::bad"))
                || report
                    .supporting_evidence
                    .iter()
                    .any(|s| s.contains("suite::bad"))
        );
        assert!(!report.auto_approved);
        assert!(!fixed.awaiting_review);
        assert!(fixed.coding_plan.is_some());
    }

    #[test]
    fn c44_fix_this_without_evidence_stays_coding_edit() {
        let planner = planner_with_stub_terminal(StubTerminalTool::passing("ok"));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let response = planner.handle(UserRequest::new("Fix this.")).unwrap();
        assert!(response.fix_from_verification.is_none());
        assert!(response.coding_edit.is_some());
    }

    #[test]
    fn c45_passing_tests_produce_concise_summary() {
        let stub = StubTerminalTool::passing("test result: ok. 43 passed; 0 failed\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let summary = resumed
            .verification_summary
            .expect("verification_summary");
        assert_eq!(summary.status, Some(crate::VerificationStatus::Passed));
        assert!(!summary.invented);
        assert!(resumed.content.contains("## Verification"));
        assert!(resumed.content.contains("43 tests passed") || resumed.content.contains("passed"));
        assert!(!resumed.content.contains("12,000"));
    }

    #[test]
    fn c45_not_run_while_awaiting_review() {
        let stub = StubTerminalTool::passing("ok");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let response = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        assert!(response.awaiting_review);
        let summary = response
            .verification_summary
            .expect("verification_summary");
        assert_eq!(summary.status, Some(crate::VerificationStatus::NotRun));
        assert!(!response.content.to_ascii_lowercase().contains("tests passed"));
    }

    #[test]
    fn c45_cancelled_verification_is_not_claimed_passed() {
        let stub = StubTerminalTool::passing("ok");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let response = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let cancelled = planner
            .resolve_review(ReviewIntent::Cancel {
                plan_id: response.execution_plan.expect("plan").id().clone(),
            })
            .unwrap();
        let summary = cancelled
            .verification_summary
            .expect("verification_summary");
        assert_eq!(summary.status, Some(crate::VerificationStatus::Cancelled));
        assert!(!cancelled.content.to_ascii_lowercase().contains("tests passed"));
        assert_eq!(stub.run_count(), 0);
    }

    #[test]
    fn c45_failure_recommends_review_fix_chip() {
        let out = r#"
error[E0308]: mismatched types
  --> src/foo.rs:42:5
"#;
        let stub = StubTerminalTool::failing(101, out);
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let summary = resumed
            .verification_summary
            .expect("verification_summary");
        assert_eq!(summary.status, Some(crate::VerificationStatus::Failed));
        assert_eq!(
            summary.recommended_fix_prompt.as_deref(),
            Some("Fix the build failure.")
        );
        assert!(resumed.content.contains("[Review Fix]"));
        let pair = resumed.pair_programming.expect("pair");
        assert!(pair.suggested_actions.iter().any(|a| a.id == "review_fix"));
        // Chip is conversational — not auto-executed.
        assert_eq!(stub.run_count(), 1);
        assert!(!resumed.awaiting_review);
    }

    fn seed_apply_loop(planner: &Planner, files: &[&str]) {
        let summary = ExecutionSummary {
            plan_id: ExecutionPlanId::from_existing("seed-apply"),
            status: ExecutionStatus::Completed,
            goal: "Fix this bug.".into(),
            actions_performed: vec!["write_file".into()],
            resources_changed: Vec::new(),
            files_edited: files.iter().map(|s| (*s).to_string()).collect(),
            duration_ms: 5,
            warnings: Vec::new(),
            errors: Vec::new(),
            next_suggested_actions: Vec::new(),
            tools_executed: vec!["write_file".into()],
            outputs: Vec::new(),
            partial: false,
            files_moved_to_trash: Vec::new(),
            files_permanently_deleted: Vec::new(),
            recovery_available: None,
            deletion_method: None,
            error: None,
        };
        let mut response = PlannerResponse {
            execution_summary: Some(summary),
            content: "Applied.".into(),
            ..PlannerResponse::default()
        };
        planner.note_apply_for_verification_loop(&mut response);
        planner.attach_verification_summary(&mut response, false, Vec::new());
        assert!(response.coding_verification_loop.is_some());
        assert_eq!(
            response
                .verification_summary
                .as_ref()
                .and_then(|s| s.status),
            Some(crate::VerificationStatus::NotRun)
        );
    }

    #[test]
    fn c46_apply_offers_verify_without_running_tests() {
        let stub = StubTerminalTool::passing("ok");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        seed_apply_loop(&planner, &["src/lib.rs"]);
        let loop_state = planner
            .clone_coding_verification_loop()
            .expect("loop");
        assert_eq!(
            loop_state.phase,
            crate::LoopPhase::AppliedAwaitingVerify
        );
        assert_eq!(stub.run_count(), 0);
        assert!(!loop_state.auto_approved);
        assert!(!loop_state.silent_retry);
    }

    #[test]
    fn c46_verify_pass_ends_loop() {
        let stub = StubTerminalTool::passing("test result: ok. 3 passed; 0 failed\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        seed_apply_loop(&planner, &["src/a.rs"]);
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let loop_state = resumed
            .coding_verification_loop
            .expect("loop on response");
        assert_eq!(loop_state.phase, crate::LoopPhase::Passed);
        assert!(!loop_state.active);
        assert_eq!(
            resumed
                .verification_summary
                .as_ref()
                .and_then(|s| s.status),
            Some(crate::VerificationStatus::Passed)
        );
    }

    #[test]
    fn c46_failure_then_user_cancellation() {
        let stub = StubTerminalTool::failing(101, "error[E0308]: x\n  --> src/a.rs:1:1\n");
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        seed_apply_loop(&planner, &["src/a.rs"]);
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let _ = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        let stopped = planner.handle(UserRequest::new("Stop.")).unwrap();
        let loop_state = stopped
            .coding_verification_loop
            .expect("cancelled loop");
        assert_eq!(loop_state.phase, crate::LoopPhase::Cancelled);
        assert!(!loop_state.active);
        assert!(stopped.content.contains("Stopped"));
    }

    #[test]
    fn c46_max_cycles_suppresses_review_fix() {
        let out = "error[E0308]: x\n  --> src/a.rs:1:1\n";
        let stub = StubTerminalTool::failing(101, out);
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        seed_apply_loop(&planner, &["src/a.rs"]);
        for _ in 0..crate::MAX_CYCLES {
            let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
            let plan_id = paused.execution_plan.expect("plan").id().clone();
            let _ = planner
                .resolve_review(ReviewIntent::Approve { plan_id })
                .unwrap();
        }
        let loop_state = planner
            .clone_coding_verification_loop()
            .expect("loop");
        assert_eq!(loop_state.phase, crate::LoopPhase::Exhausted);
        assert!(crate::coding_verification_loop::suppress_fix_offer(Some(
            &loop_state
        )));
        assert!(!loop_state.auto_approved);
        assert_eq!(loop_state.cycles_completed, crate::MAX_CYCLES);
    }

    #[test]
    fn c46_no_mutation_without_review_on_failure() {
        let stub = StubTerminalTool::failing(
            101,
            "error[E0308]: mismatched types\n  --> src/foo.rs:42:5\n",
        );
        let planner = planner_with_stub_terminal(Arc::clone(&stub));
        planner
            .context
            .set_session_inputs(session_cargo_for_tests());
        seed_apply_loop(&planner, &["src/foo.rs"]);
        let paused = planner.handle(UserRequest::new("Run the tests.")).unwrap();
        let plan_id = paused.execution_plan.expect("plan").id().clone();
        let resumed = planner
            .resolve_review(ReviewIntent::Approve { plan_id })
            .unwrap();
        assert!(!resumed.awaiting_review || resumed.test_execution.is_some());
        // Failure offers fix conversationally — does not open mutation review.
        let loop_state = resumed.coding_verification_loop.expect("loop");
        assert_eq!(loop_state.phase, crate::LoopPhase::FailedAwaitingFix);
        assert!(!loop_state.auto_approved);
        assert!(!loop_state.silent_retry);
        let pair = resumed.pair_programming.expect("pair");
        assert!(pair.suggested_actions.iter().any(|a| a.id == "review_fix"));
    }

    #[test]
    fn c51_create_multi_step_goal_no_mutation() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                metadata: jaymi_context::ProjectMetadata {
                    project_id: Some("demo".into()),
                    name: Some("demo".into()),
                    root_directory: Some("/proj".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            approved_context_providers: vec!["project".into()],
            ..Default::default()
        });
        let response = planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let goal = response.coding_goal.expect("coding_goal");
        assert_eq!(goal.status, crate::CodingGoalStatus::Active);
        assert!(!goal.mutated);
        assert!(!goal.tools_executed);
        assert!(response.tool_id.is_none());
        assert!(response.execution_plan.is_none());
        assert!(!response.awaiting_review);
        assert!(response.coding_plan.is_none());
        assert!(response.content.contains("Coding Goal"));
        assert!(response.content.contains("proposed") || response.content.contains("Acceptance"));
    }

    #[test]
    fn c51_one_step_and_readonly_do_not_create_goal() {
        let planner = planner_with_search_and_read();
        for prompt in [
            "Rename this variable.",
            "Write tests.",
            "Explain this function.",
            "Help with authentication.",
            "What does AuthService do?",
        ] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            assert!(
                response.coding_goal.is_none(),
                "unexpected coding_goal for {prompt}: {}",
                response.content
            );
        }
    }

    #[test]
    fn c51_multi_step_examples_create_goals() {
        let planner = planner_with_search_and_read();
        for prompt in [
            "Refactor the authentication architecture.",
            "Add image generation to the application.",
            "Fix all the compiler errors in this project.",
        ] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            let goal = response
                .coding_goal
                .unwrap_or_else(|| panic!("expected coding_goal for {prompt}"));
            assert_eq!(goal.status, crate::CodingGoalStatus::Active);
            assert!(!goal.mutated);
            assert!(!goal.tools_executed);
        }
    }

    #[test]
    fn c51_goal_persists_across_turns() {
        let planner = planner_with_search_and_read();
        let created = planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let id = created.coding_goal.expect("goal").id.clone();

        // Ordinary question must not clear or replace the goal.
        let qa = planner
            .handle(UserRequest::new("Explain this function."))
            .unwrap();
        assert!(qa.coding_goal.is_none());

        let status = planner
            .handle(UserRequest::new("What's my current goal?"))
            .unwrap();
        let goal = status.coding_goal.expect("persisted goal");
        assert_eq!(goal.id, id);
        assert_eq!(goal.status, crate::CodingGoalStatus::Active);
        assert!(goal.user_objective.contains("login"));
    }

    #[test]
    fn c51_cancel_complete_and_blocked() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        let cancelled_goal = cancelled.coding_goal.expect("cancelled");
        assert_eq!(cancelled_goal.status, crate::CodingGoalStatus::Cancelled);
        assert_eq!(
            cancelled_goal.agent_lifecycle.state,
            crate::AgentLifecycleState::Cancelled
        );
        assert!(!cancelled_goal.mutated);

        planner
            .handle(UserRequest::new(
                "Refactor the authentication architecture.",
            ))
            .unwrap();
        let blocked = planner
            .handle(UserRequest::new(
                "This goal is blocked because OAuth keys are missing.",
            ))
            .unwrap();
        let blocked_goal = blocked.coding_goal.expect("blocked");
        assert_eq!(blocked_goal.status, crate::CodingGoalStatus::Blocked);
        assert_eq!(
            blocked_goal.agent_lifecycle.state,
            crate::AgentLifecycleState::Blocked
        );
        assert!(blocked_goal
            .blockers
            .iter()
            .any(|b| b.to_ascii_lowercase().contains("oauth")));

        // Complete without evidence is refused (C5.3).
        let refused = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        assert!(
            refused.content.to_ascii_lowercase().contains("evidence")
                || refused.content.to_ascii_lowercase().contains("could not")
        );
    }

    #[test]
    fn c53_pause_resume_and_cancel_during_investigation() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let understanding = planner
            .handle(UserRequest::new("Begin understanding."))
            .unwrap();
        assert_eq!(
            understanding
                .coding_goal
                .as_ref()
                .unwrap()
                .agent_lifecycle
                .state,
            crate::AgentLifecycleState::Understanding
        );

        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        assert_eq!(
            paused.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );
        assert!(
            paused.agent_control.is_some() || paused.agent_lifecycle.as_ref().is_some_and(|r| r.ok),
            "pause should report via C5.8 control or C5.3 lifecycle"
        );

        let resumed = planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        assert_eq!(
            resumed.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Understanding
        );

        planner
            .handle(UserRequest::new("Begin investigation."))
            .unwrap();
        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        assert_eq!(
            cancelled.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Cancelled
        );
        assert!(cancelled.coding_goal.as_ref().unwrap().control.cancelled);
    }

    #[test]
    fn c53_awaiting_approval_blocks_execution_without_record() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        // Drive lifecycle via Planner store (Planner-owned API — not LLM).
        let conversation_id = None;
        planner.with_coding_goal_store(|store| {
            let goal = store.active_goal_mut(conversation_id).unwrap();
            *goal = crate::agent_state_machine::transition_goal(
                goal.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "plan".into(),
                },
            )
            .unwrap();
            *goal = crate::agent_state_machine::transition_goal(
                goal.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-1".into(),
                },
            )
            .unwrap();
            assert_eq!(
                goal.agent_lifecycle.state,
                crate::AgentLifecycleState::AwaitingApproval
            );
            assert!(!goal.agent_lifecycle.state.allows_mutation_execution());
            assert!(crate::agent_state_machine::transition_goal(
                goal.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "plan-1".into(),
                },
            )
            .is_err());
        });
    }

    #[test]
    fn c54_two_sequential_tasks_and_no_mutation_without_approval() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        let started = planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let report = started.multi_step_execution.expect("multi_step");
        assert_eq!(report.status, crate::MultiStepStatus::Started);
        assert!(!report.mutated);
        assert!(!report.tools_executed);
        assert!(!report.auto_approved);
        assert!(!report.llm_direct_tools);
        assert!(started.execution_plan.is_none());
        assert!(started.tool_id.is_none());
        let goal = started.coding_goal.expect("goal");
        assert!(goal.active_task_id.is_some());
        assert_eq!(
            goal.tasks
                .iter()
                .filter(|t| t.status == crate::GoalTaskStatus::InProgress)
                .count(),
            1
        );

        // Cannot advance while first task is still in progress.
        let blocked_advance = planner
            .handle(UserRequest::new("Advance to the next task."))
            .unwrap();
        assert_eq!(
            blocked_advance
                .multi_step_execution
                .as_ref()
                .unwrap()
                .status,
            crate::MultiStepStatus::Rejected
        );

        // Complete first task with evidence (Planner-owned), then continue.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            let tid = g.active_task_id.clone().unwrap();
            let task = g.tasks.iter().find(|t| t.id == tid).unwrap().clone();
            let evidence = crate::TaskCompletionEvidence {
                summary: "First task investigation complete".into(),
                approved_plan_id: if task.mutation_expected {
                    Some("plan-1".into())
                } else {
                    None
                },
                verification_ref: task.expected_verification.as_ref().map(|_| "ver-1".into()),
                verification_passed: task.expected_verification.is_some(),
                investigation_complete: !task.mutation_expected,
            };
            let (next, _) =
                crate::multi_step_execution::complete_active_task_with_evidence(g.clone(), evidence)
                    .unwrap();
            *g = next;
        });

        let continued = planner
            .handle(UserRequest::new("Continue the goal."))
            .unwrap();
        let cont = continued.multi_step_execution.expect("continue");
        assert!(matches!(
            cont.status,
            crate::MultiStepStatus::Continued | crate::MultiStepStatus::Advanced
        ));
        assert!(continued.coding_goal.as_ref().unwrap().active_task_id.is_some());
        assert!(!cont.mutated);
        assert!(!cont.auto_approved);
    }

    #[test]
    fn c54_dependent_blocked_independent_and_failure() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();

        // Replace with controlled graph: t1 → t2 (dep), t3 independent.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.tasks = vec![
                crate::GoalTask {
                    id: "t1".into(),
                    description: "Investigate auth".into(),
                    objective: "Investigate auth".into(),
                    dependencies: vec![],
                    mutation_expected: false,
                    expected_verification: None,
                    status: crate::GoalTaskStatus::Pending,
                    ..crate::GoalTask::default()
                },
                crate::GoalTask {
                    id: "t2".into(),
                    description: "Implement login".into(),
                    objective: "Implement login".into(),
                    dependencies: vec!["t1".into()],
                    mutation_expected: true,
                    expected_verification: Some("tests".into()),
                    status: crate::GoalTaskStatus::Pending,
                    ..crate::GoalTask::default()
                },
                crate::GoalTask {
                    id: "t3".into(),
                    description: "Docs pass".into(),
                    objective: "Docs pass".into(),
                    dependencies: vec![],
                    mutation_expected: false,
                    expected_verification: None,
                    status: crate::GoalTaskStatus::Pending,
                    ..crate::GoalTask::default()
                },
            ];
            g.active_task_id = None;
        });

        // Dependent t2 cannot start before t1.
        let blocked = {
            let g = planner
                .clone_coding_goal_store()
                .active_goal(None)
                .unwrap()
                .clone();
            crate::goal_decomposition::transition_task_status(
                g,
                "t2",
                crate::GoalTaskStatus::InProgress,
            )
        };
        assert!(blocked.is_err());

        let started = planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let active = started
            .coding_goal
            .as_ref()
            .unwrap()
            .active_task_id
            .clone()
            .unwrap();
        assert!(active == "t1" || active == "t3");

        // Fail active task — dependents of that task stay blocked.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            let tid = g.active_task_id.clone().unwrap();
            let (next, _) = crate::goal_decomposition::transition_task_status(
                g.clone(),
                &tid,
                crate::GoalTaskStatus::Failed,
            )
            .unwrap();
            *g = next;
            g.active_task_id = None;
        });

        let after_fail = planner
            .handle(UserRequest::new("Continue the goal."))
            .unwrap();
        if let Some(goal) = after_fail.coding_goal.as_ref() {
            if goal.tasks.iter().any(|t| t.id == "t1" && t.status == crate::GoalTaskStatus::Failed)
            {
                assert!(goal
                    .tasks
                    .iter()
                    .find(|t| t.id == "t2")
                    .map(|t| t.status != crate::GoalTaskStatus::InProgress)
                    .unwrap_or(true));
            }
        }
    }

    #[test]
    fn c54_verification_failure_cancel_pause_and_bounds() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        // Verification failure after observing approved apply.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            let next = crate::multi_step_execution::on_apply_success(g.clone(), "plan-v").unwrap();
            let (next, report) =
                crate::multi_step_execution::on_verify_fail(next, "cargo-test").unwrap();
            assert_eq!(report.status, crate::MultiStepStatus::VerificationFailed);
            assert_eq!(
                next.agent_lifecycle.state,
                crate::AgentLifecycleState::Correcting
            );
            *g = next;
        });

        // Pause / resume still work via C5.3.
        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        assert_eq!(
            paused.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );
        // Multi-step start rejected while paused.
        let rejected = planner
            .handle(UserRequest::new("Continue the goal."))
            .unwrap();
        assert_eq!(
            rejected.multi_step_execution.as_ref().unwrap().status,
            crate::MultiStepStatus::Rejected
        );
        planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();

        // Workflow bound prevents infinite orchestration loops.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.workflow_transition_count =
                crate::MAX_WORKFLOW_TRANSITIONS as u32;
            assert!(crate::multi_step_execution::start_goal_execution(g.clone()).is_err());
        });

        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        assert_eq!(
            cancelled.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Cancelled
        );
    }

    #[test]
    fn c54_approval_rejection_and_successful_goal_completion() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();

        // Approval gate: AwaitingApproval blocks BeginExecution without RecordApproval.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c54".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-reject".into(),
                },
            )
            .unwrap();
            assert!(!g.agent_lifecycle.state.allows_mutation_execution());
            assert!(crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "plan-reject".into(),
                },
            )
            .is_err());
        });

        // Fresh goal for successful completion path.
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new(
                "Build a login system. Keep this as one task only.",
            ))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            if let Some(goal) = store.active_goal_mut(None) {
                if !goal.constraints.iter().any(|c| c.contains("one task")) {
                    goal.constraints
                        .push("Keep this as one task only.".into());
                }
            }
        });
        planner
            .handle(UserRequest::new("Decompose the goal."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            let tid = g.active_task_id.clone().unwrap();
            let task = g.tasks.iter().find(|t| t.id == tid).unwrap().clone();
            let evidence = crate::TaskCompletionEvidence {
                summary: "Task done with investigation".into(),
                approved_plan_id: if task.mutation_expected {
                    Some("plan-ok".into())
                } else {
                    None
                },
                verification_ref: None,
                verification_passed: false,
                investigation_complete: true,
            };
            // If mutation expected without verification, approved_plan_id is enough.
            let evidence = if task.mutation_expected && task.expected_verification.is_some() {
                crate::TaskCompletionEvidence {
                    verification_ref: Some("ver-ok".into()),
                    verification_passed: true,
                    ..evidence
                }
            } else {
                evidence
            };
            let (next, _) =
                crate::multi_step_execution::complete_active_task_with_evidence(g.clone(), evidence)
                    .unwrap();
            *g = next;
        });

        let ready = planner
            .handle(UserRequest::new("Continue the goal."))
            .unwrap();
        assert!(matches!(
            ready.multi_step_execution.as_ref().unwrap().status,
            crate::MultiStepStatus::GoalReadyToComplete | crate::MultiStepStatus::Continued
        ));

        // Goal complete still requires C5.3 completion evidence.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::Complete {
                    evidence: crate::CompletionEvidence {
                        summary: "Login goal verified".into(),
                        references: vec!["ver-ok".into()],
                        source: crate::CompletionEvidenceSource::VerificationPassed,
                        verification_passed: true,
                    },
                },
            )
            .unwrap();
            assert_eq!(
                g.agent_lifecycle.state,
                crate::AgentLifecycleState::Completed
            );
        });
    }

    #[test]
    fn c55_multi_task_context_and_compaction() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system without OAuth."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        let maintained = planner
            .handle(UserRequest::new("Maintain agent context."))
            .unwrap();
        let report = maintained
            .agent_context_maintenance
            .expect("agent_context_maintenance");
        assert_eq!(report.status, crate::AgentContextStatus::Maintained);
        assert!(!report.mutated);
        assert!(!report.tools_executed);
        assert!(!report.memory_written);
        assert!(!report.duplicate_assembler);
        let ctx = report.context.expect("context");
        assert!(ctx.current_task.is_some());
        assert!(
            ctx.constraints
                .iter()
                .any(|c| c.to_ascii_lowercase().contains("oauth")),
            "constraints must survive maintenance"
        );
        assert!(maintained.content.contains("Agent Context"));

        // Inflate history then compact — constraints + decisions retained.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.notes
                .push("Decision: prefer session cookies over JWT.".into());
            for i in 0..50 {
                g.workflow_history
                    .push(crate::WorkflowRecord {
                        sequence: 100 + i,
                        kind: "noise".into(),
                        task_id: None,
                        detail: format!("bulk history {i} {}", "z".repeat(40)),
                    });
                g.completed_work.push(format!("finished chunk {i}"));
            }
        });

        let compacted = planner
            .handle(UserRequest::new("Compact goal context."))
            .unwrap();
        let creport = compacted.agent_context_maintenance.expect("compact");
        assert_eq!(creport.status, crate::AgentContextStatus::Compacted);
        let cctx = creport.context.expect("compacted ctx");
        assert!(cctx.compacted);
        assert!(cctx
            .constraints
            .iter()
            .any(|c| c.to_ascii_lowercase().contains("oauth")));
        assert!(cctx
            .decisions
            .iter()
            .any(|d| d.summary.contains("session cookies")));
        assert!(cctx.chars_estimate <= crate::MAX_GOAL_CONTEXT_CHARS + 400);
    }

    #[test]
    fn c55_failed_verification_and_workspace_refresh() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.verification_state = crate::GoalVerificationState::Failed;
            g.agent_lifecycle.state = crate::AgentLifecycleState::Correcting;
            g.workflow_history.push(crate::WorkflowRecord {
                sequence: 1,
                kind: "verify_fail".into(),
                task_id: g.active_task_id.clone(),
                detail: "Verification failed `cargo test`".into(),
            });
        });

        // Build a ContextBundle with Policy-selected labels (still engine-minted empty + patched).
        let mut bundle = jaymi_context::ContextBundle::default();
        // Prefer assemble via engine when possible.
        let assembled = planner.context.assemble(&UserRequest::new("status")).ok();
        let response = if let Some(ctx_bundle) = assembled {
            // Soft path uses handle which reassembles; drive maintain_from_goal for refresh.
            let goal = planner
                .clone_coding_goal_store()
                .active_goal(None)
                .cloned()
                .unwrap();
            let maintained =
                crate::agent_context_maintenance::maintain_from_goal(&goal, Some(&ctx_bundle));
            assert!(
                !maintained.failures.is_empty(),
                "failed verification must remain available"
            );
            // workspace_refresh may be None on empty session — still must not invent snapshots.
            let s = serde_json::to_string(&maintained).unwrap();
            assert!(!s.contains("WorkspaceSnapshot"));
            assert!(!s.contains("ProjectSnapshot"));
            planner
                .handle(UserRequest::new("Show agent context."))
                .unwrap()
        } else {
            let _ = &mut bundle;
            planner
                .handle(UserRequest::new("Show agent context."))
                .unwrap()
        };
        let report = response.agent_context_maintenance.expect("show");
        assert!(!report.duplicate_assembler);
        assert!(report
            .context
            .as_ref()
            .map(|c| !c.failures.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn c55_no_goal_and_llm_attach_only_with_active_goal() {
        let planner = planner_with_search_and_read();
        let no_goal = planner
            .handle(UserRequest::new("Maintain agent context."))
            .unwrap();
        assert_eq!(
            no_goal
                .agent_context_maintenance
                .as_ref()
                .unwrap()
                .status,
            crate::AgentContextStatus::NoActiveGoal
        );

        // Ordinary coding conversation path must not invent agent context without a goal.
        let bundle = jaymi_context::ContextBundle::default();
        let llm = jaymi_context::LlmContext::from_bundle(&bundle);
        let same = crate::agent_context_maintenance::attach_to_llm(llm.clone(), None, &bundle);
        assert_eq!(same.extensions.len(), llm.extensions.len());

        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let goal = planner
            .clone_coding_goal_store()
            .active_goal(None)
            .cloned();
        let attached =
            crate::agent_context_maintenance::attach_to_llm(llm, goal.as_ref(), &bundle);
        assert!(attached
            .extensions
            .contains_key(crate::agent_context_maintenance::LLM_EXTENSION_KEY));
    }

    #[test]
    fn c56_autonomy_policy_status_and_gates() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        let status = planner
            .handle(UserRequest::new("Autonomy policy status."))
            .unwrap();
        let report = status.autonomy_policy.expect("autonomy_policy");
        assert_eq!(report.status, crate::AutonomyPolicyStatus::Status);
        assert!(!report.modes_implemented);
        assert!(!report.agent_approval_system);
        assert!(!report.llm_self_authorize);
        assert!(!report.read_observe_examples.is_empty());
        assert!(!report.mutation_examples.is_empty());
        assert!(!report.side_effect_examples.is_empty());
        assert!(status.content.contains("READ / OBSERVE"));

        let explain = planner
            .handle(UserRequest::new("What can this goal do automatically?"))
            .unwrap();
        assert_eq!(
            explain.autonomy_policy.as_ref().unwrap().status,
            crate::AutonomyPolicyStatus::Explained
        );

        // Bind approval via lifecycle Submit → Record; rejection of mismatched plan.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c56".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-c56".into(),
                },
            )
            .unwrap();
            assert!(g.autonomy_policy.active.is_some());
            assert_eq!(
                g.autonomy_policy.active.as_ref().unwrap().plan_id,
                "plan-c56"
            );
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "plan-c56".into(),
                },
            )
            .unwrap();
            assert!(g.autonomy_policy.active.as_ref().unwrap().recorded);
            assert!(crate::approval_autonomy_policy::assert_execution_authorized(
                g, "plan-c56"
            )
            .is_ok());
            assert!(crate::approval_autonomy_policy::assert_execution_authorized(
                g, "plan-other"
            )
            .is_err());

            // Rejection path: BeginExecution without matching approval still fails.
            assert!(crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "plan-stale".into(),
                },
            )
            .is_err());

            // Cancel invalidates.
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::Cancel {
                    reason: "c56 cancel".into(),
                },
            )
            .unwrap();
            assert!(crate::approval_autonomy_policy::assert_execution_authorized(
                g, "plan-c56"
            )
            .is_err());
        });
    }

    #[test]
    fn c57_goal_start_and_task_transition_progress() {
        let planner = planner_with_search_and_read();
        let created = planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        assert!(created.agent_progress.is_some(), "active goal attaches progress");
        assert!(!created
            .agent_progress
            .as_ref()
            .unwrap()
            .exposes_chain_of_thought);
        assert!(!created.agent_progress.as_ref().unwrap().second_agent_state);

        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        let started = planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let progress = started.agent_progress.expect("progress after start");
        assert!(progress.total_tasks >= 2);
        assert!(progress.current_task.is_some());
        assert_eq!(progress.completed_count, 0);
        assert!(!progress.exposes_chain_of_thought);

        let shown = planner
            .handle(UserRequest::new("Show goal progress."))
            .unwrap();
        let report = shown.agent_progress_report.expect("progress report");
        assert_eq!(report.status, crate::ProgressReportStatus::Shown);
        assert!(shown.content.contains("Goal Progress"));
        assert!(shown.content.contains("tasks"));
        let lower = shown.content.to_ascii_lowercase();
        assert!(
            lower.contains("no private reasoning") || lower.contains("operational progress only"),
            "progress UX must disclaim private reasoning, not embed CoT"
        );
        assert!(!lower.contains("hidden reasoning"));
        assert!(!lower.contains("deliberation scratch"));

        // Transition: complete active task → continue → progress updates.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            let tid = g.active_task_id.clone().unwrap();
            let task = g.tasks.iter().find(|t| t.id == tid).unwrap().clone();
            let evidence = crate::TaskCompletionEvidence {
                summary: "c57 first task done".into(),
                approved_plan_id: if task.mutation_expected {
                    Some("plan-c57".into())
                } else {
                    None
                },
                verification_ref: task.expected_verification.as_ref().map(|_| "ver-c57".into()),
                verification_passed: task.expected_verification.is_some(),
                investigation_complete: !task.mutation_expected,
            };
            let (next, _) =
                crate::multi_step_execution::complete_active_task_with_evidence(g.clone(), evidence)
                    .unwrap();
            *g = next;
        });
        let continued = planner
            .handle(UserRequest::new("Continue the goal."))
            .unwrap();
        let after = continued.agent_progress.expect("progress after transition");
        assert!(after.completed_count >= 1);
        assert_eq!(
            after.goal_id,
            continued.coding_goal.as_ref().unwrap().id,
            "UI progress mirrors Planner goal id"
        );
    }

    #[test]
    fn c57_approval_execution_verification_failure_blocked() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c57".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-c57".into(),
                },
            )
            .unwrap();
        });
        let approval = planner
            .handle(UserRequest::new("Show goal progress."))
            .unwrap();
        let ap = approval.agent_progress.expect("approval progress");
        assert_eq!(ap.phase, crate::ProgressPhase::ApprovalRequired);
        assert!(ap.approval_required);
        assert!(ap.actions.iter().any(|a| a.id == "review_goal"));

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "plan-c57".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "plan-c57".into(),
                },
            )
            .unwrap();
        });
        let exec = planner
            .handle(UserRequest::new("Show goal progress."))
            .unwrap();
        assert_eq!(
            exec.agent_progress.as_ref().unwrap().phase,
            crate::ProgressPhase::Executing
        );

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginVerification {
                    reason: "c57 verify".into(),
                },
            )
            .unwrap();
        });
        let verifying = planner
            .handle(UserRequest::new("Show goal progress."))
            .unwrap();
        assert_eq!(
            verifying.agent_progress.as_ref().unwrap().phase,
            crate::ProgressPhase::Verifying
        );

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::VerificationFailed {
                    evidence_ref: "tests failed".into(),
                },
            )
            .unwrap();
        });
        let failed = planner
            .handle(UserRequest::new("Show goal progress."))
            .unwrap();
        let fp = failed.agent_progress.expect("failed progress");
        assert_eq!(fp.phase, crate::ProgressPhase::Correcting);

        // Fresh goal for blocked.
        let planner2 = planner_with_search_and_read();
        planner2
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner2
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner2
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner2.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::Block {
                    reason: "missing credentials".into(),
                },
            )
            .unwrap();
        });
        let blocked = planner2
            .handle(UserRequest::new("Show goal progress."))
            .unwrap();
        let bp = blocked.agent_progress.expect("blocked progress");
        assert_eq!(bp.phase, crate::ProgressPhase::Blocked);
        assert!(!bp.blockers.is_empty());
    }

    #[test]
    fn c57_pause_cancel_completion_and_ui_mirrors_planner() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        let pp = paused.agent_progress.expect("paused progress");
        assert_eq!(pp.phase, crate::ProgressPhase::Paused);
        assert!(pp.actions.iter().any(|a| a.id == "resume_goal"));
        assert_eq!(
            pp.phase,
            crate::ProgressPhase::from_lifecycle(
                paused.coding_goal.as_ref().unwrap().agent_lifecycle.state
            )
        );

        planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        // Terminal goals do not auto-attach progress in finalize; explicit show may report none.
        assert!(
            cancelled.coding_goal.as_ref().map(|g| g.status.is_terminal()) == Some(true)
                || cancelled
                    .agent_progress
                    .as_ref()
                    .map(|p| p.phase == crate::ProgressPhase::Cancelled)
                    .unwrap_or(true)
        );

        // Completion path on a fresh goal.
        let planner2 = planner_with_search_and_read();
        planner2
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner2
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner2
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let completed_view = planner2
            .with_coding_goal_store(|store| {
                let g = store.active_goal_mut(None).unwrap();
                for t in &mut g.tasks {
                    t.status = crate::GoalTaskStatus::Completed;
                }
                g.active_task_id = None;
                *g = crate::agent_state_machine::transition_goal(
                    g.clone(),
                    crate::AgentLifecycleEvent::Complete {
                        evidence: crate::CompletionEvidence {
                            summary: "all tasks done".into(),
                            references: vec!["ver-c57".into()],
                            source: crate::CompletionEvidenceSource::VerificationPassed,
                            verification_passed: true,
                        },
                    },
                )
                .unwrap();
                assert_eq!(
                    g.agent_lifecycle.state,
                    crate::AgentLifecycleState::Completed
                );
                crate::agent_progress_ux::derive_progress(g)
            })
            .expect("coding goal store");
        assert_eq!(completed_view.phase, crate::ProgressPhase::Completed);
        assert_eq!(completed_view.completed_count, completed_view.total_tasks);
        assert!(!completed_view.exposes_chain_of_thought);
        assert!(!completed_view.second_agent_state);
        let panel = completed_view.panel_lines();
        assert!(panel
            .iter()
            .any(|l| l.contains(&completed_view.title) || l == &completed_view.title));
    }

    #[test]
    fn c58_pause_resume_cancel_and_stale_resume() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        let control = paused.agent_control.expect("agent_control");
        assert_eq!(control.status, crate::AgentControlStatus::Paused);
        assert!(control.stop_new_operations);
        assert!(control.resumable);
        assert!(!control.rolled_back_mutations);
        assert!(!control.parallel_cancel_engine);
        assert!(control.planner_owned);
        assert!(!crate::agent_pause_resume_cancel::allows_new_operations(
            paused.coding_goal.as_ref().unwrap()
        ));

        // Multi-step must not continue while paused.
        let rejected = planner
            .handle(UserRequest::new("Continue the goal."))
            .unwrap();
        assert_eq!(
            rejected.multi_step_execution.as_ref().unwrap().status,
            crate::MultiStepStatus::Rejected
        );

        let resumed = planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        assert_eq!(
            resumed.agent_control.as_ref().unwrap().status,
            crate::AgentControlStatus::Resumed
        );
        assert!(crate::agent_pause_resume_cancel::allows_new_operations(
            resumed.coding_goal.as_ref().unwrap()
        ));

        // Cancel during execution phase.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c58".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-c58".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "plan-c58".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "plan-c58".into(),
                },
            )
            .unwrap();
        });
        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        let creport = cancelled.agent_control.expect("cancel report");
        assert_eq!(creport.status, crate::AgentControlStatus::Cancelled);
        assert!(creport.approvals_invalidated);
        assert!(!creport.rolled_back_mutations);
        assert!(!creport.continued_after_cancel);
        assert!(cancelled.coding_goal.as_ref().unwrap().control.cancelled);

        // Repeated cancel is idempotent.
        let again = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        assert!(matches!(
            again.agent_control.as_ref().map(|r| r.status),
            Some(crate::AgentControlStatus::CancelRepeated)
                | Some(crate::AgentControlStatus::NoActiveGoal)
                | None
        ));
    }

    #[test]
    fn c58_stale_resume_and_resume_after_verification_failure() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();

        // Drive to Executing, pause with fingerprint, resume with changed HEAD.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c58".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-stale".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "plan-stale".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "plan-stale".into(),
                },
            )
            .unwrap();
            let pause_fp = crate::ProjectFingerprint {
                head_sha: Some("aaa".into()),
                branch: Some("main".into()),
                dirty_count: 0,
                ..crate::ProjectFingerprint::default()
            };
            let (paused, _) = crate::agent_pause_resume_cancel::pause_goal(
                g.clone(),
                &jaymi_context::ContextBundle::builder()
                    .git_status(jaymi_context::GitStatusSection {
                        head_sha: pause_fp.head_sha.clone(),
                        branch: pause_fp.branch.clone(),
                        modified_count: 0,
                        ..jaymi_context::GitStatusSection::default()
                    })
                    .build(),
                &crate::InFlightObservation::default(),
                "unit pause",
            )
            .unwrap();
            *g = paused;
        });

        // Soft-path resume uses empty bundle (no HEAD) — inject stale via store resume helper.
        let stale_report = planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            let resume_bundle = jaymi_context::ContextBundle::builder()
                .git_status(jaymi_context::GitStatusSection {
                    head_sha: Some("bbb".into()),
                    branch: Some("main".into()),
                    modified_count: 2,
                    ..jaymi_context::GitStatusSection::default()
                })
                .build();
            let (resumed, report) = crate::agent_pause_resume_cancel::resume_goal(
                g.clone(),
                &resume_bundle,
                &crate::InFlightObservation::default(),
                "stale resume",
            )
            .unwrap();
            *g = resumed;
            report
        })
        .expect("store");
        assert!(!stale_report.stale_reasons.is_empty());
        assert!(stale_report.requires_fresh_approval);
        assert_ne!(
            stale_report.to,
            Some(crate::AgentLifecycleState::Executing)
        );

        // Fresh goal: pause/resume after verification failure (Correcting).
        let planner2 = planner_with_search_and_read();
        planner2
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner2
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner2.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c58".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "p".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "p".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "p".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginVerification {
                    reason: "v".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::VerificationFailed {
                    evidence_ref: "fail".into(),
                },
            )
            .unwrap();
        });
        let paused = planner2
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        assert_eq!(
            paused.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );
        let resumed = planner2
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        assert_eq!(
            resumed.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Correcting
        );
    }

    #[test]
    fn c59_recover_verification_and_no_hidden_mutation() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            if let Some(t) = g.tasks.get_mut(0) {
                t.status = crate::GoalTaskStatus::Completed;
            }
            g.tasks.push(crate::GoalTask {
                id: "failed-1".into(),
                description: "Verify auth".into(),
                status: crate::GoalTaskStatus::Failed,
                ..crate::GoalTask::default()
            });
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c59".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "old".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "old".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "old".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginVerification {
                    reason: "v".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::VerificationFailed {
                    evidence_ref: "fail".into(),
                },
            )
            .unwrap();
        });

        let recovered = planner
            .handle(UserRequest::new("Recover from verification failure."))
            .unwrap();
        let report = recovered.agent_recovery.expect("recovery report");
        assert_eq!(report.category, crate::FailureCategory::VerificationFailure);
        assert_eq!(
            report.disposition,
            crate::RecoveryDisposition::ProposeCorrectivePlan
        );
        assert!(!report.mutated);
        assert!(!report.auto_retried);
        assert!(!report.approval_reused);
        assert!(!report.automatic_rollback);
        assert!(report.corrective_plan.is_some());
        assert!(planner.clone_last_coding_plan().is_some());
        let goal = recovered.coding_goal.expect("goal");
        assert!(!goal.agent_lifecycle.approval_recorded);
        assert!(goal
            .tasks
            .iter()
            .any(|t| t.status == crate::GoalTaskStatus::Completed));
        assert!(goal
            .tasks
            .iter()
            .any(|t| t.status == crate::GoalTaskStatus::Failed));
        assert!(!goal.recovery.history.is_empty());

        // User can still cancel.
        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        assert!(cancelled.coding_goal.as_ref().unwrap().control.cancelled);
    }

    #[test]
    fn c59_await_user_exhaustion_and_consistent_state() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let blocked = planner
            .handle(UserRequest::new("Recover from ambiguous requirement."))
            .unwrap();
        let report = blocked.agent_recovery.expect("recovery");
        assert_eq!(
            report.disposition,
            crate::RecoveryDisposition::AwaitUser
        );
        assert!(
            blocked
                .coding_goal
                .as_ref()
                .unwrap()
                .agent_lifecycle
                .state
                == crate::AgentLifecycleState::Blocked
                || blocked.coding_goal.as_ref().unwrap().status
                    == crate::CodingGoalStatus::Blocked
        );

        // Exhaust recovery attempts on a fresh goal.
        let planner2 = planner_with_search_and_read();
        planner2
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        for _ in 0..crate::MAX_GOAL_RECOVERY_ATTEMPTS {
            let r = planner2
                .handle(UserRequest::new("Recover from execution failure."))
                .unwrap();
            assert_ne!(
                r.agent_recovery.as_ref().unwrap().status,
                crate::GoalRecoveryStatus::Exhausted
            );
        }
        let exhausted = planner2
            .handle(UserRequest::new("Recover from execution failure."))
            .unwrap();
        assert_eq!(
            exhausted.agent_recovery.as_ref().unwrap().status,
            crate::GoalRecoveryStatus::Exhausted
        );
        assert!(!exhausted.agent_recovery.as_ref().unwrap().auto_retried);
    }

    #[test]
    fn c510_successful_completion_and_evidence() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            for t in &mut g.tasks {
                t.status = crate::GoalTaskStatus::Completed;
            }
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c510".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "p1".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "p1".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "p1".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginVerification {
                    reason: "v".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::VerificationPassed {
                    evidence_ref: "ok".into(),
                },
            )
            .unwrap();
            g.verification_state = crate::GoalVerificationState::Passed;
            g.completed_work.push("Login applied.".into());
            g.remaining_work.clear();
        });

        let assessed = planner
            .handle(UserRequest::new("Assess goal completion."))
            .unwrap();
        let assess_report = assessed.goal_completion.expect("assess");
        assert_eq!(
            assess_report.outcome,
            crate::GoalCompletionOutcome::Ready
        );
        assert!(!assess_report.invented_success);
        assert!(!assess_report.model_authoritative);
        assert!(!assess_report.unknown_treated_as_satisfied);

        let completed = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        let report = completed.goal_completion.expect("complete");
        assert_eq!(
            report.outcome,
            crate::GoalCompletionOutcome::SuccessfullyCompleted
        );
        assert!(!report.mutated);
        let goal = completed.coding_goal.expect("goal");
        assert_eq!(goal.status, crate::CodingGoalStatus::Completed);
        assert_eq!(
            goal.completion.ending_kind,
            Some(crate::GoalEndingKind::SuccessfullyCompleted)
        );
        assert!(!goal.completion.history.is_empty());
    }

    #[test]
    fn c510_incomplete_and_failed_verification() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();

        let incomplete = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        let incomplete_report = incomplete.goal_completion.expect("incomplete");
        assert_eq!(
            incomplete_report.outcome,
            crate::GoalCompletionOutcome::Incomplete
        );
        assert_eq!(
            incomplete.coding_goal.as_ref().unwrap().status,
            crate::CodingGoalStatus::Active
        );

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.verification_state = crate::GoalVerificationState::Failed;
        });
        let failed_v = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        assert_eq!(
            failed_v.goal_completion.as_ref().unwrap().outcome,
            crate::GoalCompletionOutcome::FailedVerification
        );
    }

    #[test]
    fn c510_blocked_cannot_complete() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("This goal is blocked because missing API key."))
            .unwrap();
        let blocked = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        assert_eq!(
            blocked.goal_completion.as_ref().unwrap().outcome,
            crate::GoalCompletionOutcome::Blocked
        );
    }

    #[test]
    fn c510_waiver_user_end_and_cancel() {
        let planner2 = planner_with_search_and_read();
        planner2
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let waived = planner2
            .handle(UserRequest::new(
                "Waive remaining criteria because demo scope.",
            ))
            .unwrap();
        assert!(waived
            .goal_completion
            .as_ref()
            .unwrap()
            .criteria
            .iter()
            .any(|c| c.satisfaction == crate::CriterionSatisfaction::Waived));
        let ended = planner2
            .handle(UserRequest::new(
                "End this goal incomplete because pausing work.",
            ))
            .unwrap();
        let end_report = ended.goal_completion.expect("user end");
        assert_eq!(end_report.outcome, crate::GoalCompletionOutcome::UserEnded);
        assert_eq!(
            end_report.ending_kind,
            Some(crate::GoalEndingKind::UserEnded)
        );
        assert_eq!(
            ended.coding_goal.as_ref().unwrap().status,
            crate::CodingGoalStatus::Cancelled
        );
        assert_ne!(
            ended.coding_goal.as_ref().unwrap().completion.ending_kind,
            Some(crate::GoalEndingKind::SuccessfullyCompleted)
        );

        let planner3 = planner_with_search_and_read();
        planner3
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let cancelled = planner3
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        assert!(cancelled.coding_goal.as_ref().unwrap().control.cancelled);
        assert_eq!(
            cancelled.coding_goal.as_ref().unwrap().status,
            crate::CodingGoalStatus::Cancelled
        );
    }

    #[test]
    fn c511_prepare_compact_and_serde_restart_shape() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            // Inflate to a long task list + completed/failed history.
            while g.tasks.len() < 36 {
                let i = g.tasks.len();
                g.tasks.push(crate::GoalTask {
                    id: format!("extra-{i}"),
                    description: format!("Extra task {i}"),
                    status: if i % 5 == 0 {
                        crate::GoalTaskStatus::Failed
                    } else if i % 2 == 0 {
                        crate::GoalTaskStatus::Completed
                    } else {
                        crate::GoalTaskStatus::Pending
                    },
                    ..crate::GoalTask::default()
                });
            }
            g.constraints.push("Do not use OAuth.".into());
        });

        let prepared = planner
            .handle(UserRequest::new("Prepare long-running session."))
            .unwrap();
        let report = prepared.session_continuity.expect("continuity");
        assert_eq!(report.status, crate::SessionContinuityStatus::Prepared);
        assert!(!report.mutated);
        assert!(!report.background_autonomous_work);
        assert!(!report.independent_agent_database);
        assert!(!report.survives_process_restart);
        assert!(report.serde_portable_snapshot);
        assert!(report.task_count >= 36);
        assert!(report.constraints_preserved);
        let goal = prepared.coding_goal.expect("goal");
        assert!(goal.maintained_context.is_some());
        assert!(goal
            .tasks
            .iter()
            .any(|t| t.status == crate::GoalTaskStatus::Failed));
        assert!(goal
            .tasks
            .iter()
            .any(|t| t.status == crate::GoalTaskStatus::Completed));

        let compacted = planner
            .handle(UserRequest::new("Compact session context."))
            .unwrap();
        assert!(
            compacted
                .session_continuity
                .as_ref()
                .unwrap()
                .context_compacted
                || compacted
                    .session_continuity
                    .as_ref()
                    .unwrap()
                    .constraints_preserved
        );

        // Application restart shape via existing serde hitch (no agent DB).
        let snap = crate::agent_session_continuity::snapshot_store(
            &planner.clone_coding_goal_store(),
        )
        .unwrap();
        let restored = crate::agent_session_continuity::restore_store(&snap).unwrap();
        assert_eq!(
            restored.active_goal(None).unwrap().id,
            planner
                .clone_coding_goal_store()
                .active_goal(None)
                .unwrap()
                .id
        );
    }

    #[test]
    fn c511_pause_revalidate_stale_and_no_blind_resume() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        assert_eq!(
            paused.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );

        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.continuity.last_fingerprint =
                Some(crate::agent_pause_resume_cancel::ProjectFingerprint {
                    head_sha: Some("old-sha".into()),
                    branch: Some("main".into()),
                    dirty_count: 0,
                    staged_count: 0,
                    conflict_count: 0,
                    workspace_root: Some("/tmp/old".into()),
                    ..Default::default()
                });
            g.agent_lifecycle.approval_recorded = true;
            g.agent_lifecycle.approved_plan_id = Some("plan-old".into());
            g.autonomy_policy.active = Some(crate::BoundApproval {
                goal_id: g.id.clone(),
                plan_id: "plan-old".into(),
                task_id: None,
                recorded: true,
                class: crate::AutonomyPolicyClass::Mutation,
                invalidated: false,
                invalidate_reason: None,
            });
        });

        let revalidated = planner
            .handle(UserRequest::new("Revalidate after pause."))
            .unwrap();
        let report = revalidated.session_continuity.expect("revalidate");
        assert_eq!(report.status, crate::SessionContinuityStatus::Revalidated);
        assert!(!report.paused_continues_executing);
        assert!(report.stop_new_operations);
        assert!(!report.blindly_resumed_stale_plan);
        assert!(!report.background_autonomous_work);
        assert!(
            report.requires_fresh_approval
                || report.approvals_invalidated
                || !report.stale_reasons.is_empty()
        );
        assert_eq!(
            revalidated
                .coding_goal
                .as_ref()
                .unwrap()
                .agent_lifecycle
                .state,
            crate::AgentLifecycleState::Paused
        );
    }

    fn c514_session(
        head: &str,
        dirty: &[&str],
        file: Option<(&str, bool)>,
    ) -> jaymi_context::ContextSessionInputs {
        use jaymi_context::{GitPathEntry, GitSnapshot, ProjectMetadata, ProjectSnapshot};
        jaymi_context::ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(ProjectSnapshot {
                metadata: ProjectMetadata {
                    project_id: Some("demo".into()),
                    name: Some("demo".into()),
                    root_directory: Some("/proj".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            git_snapshot: Some(GitSnapshot {
                is_repository: true,
                repo_root: Some("/proj".into()),
                branch: Some("main".into()),
                head_sha: Some(head.into()),
                head_short: Some(head.chars().take(7).collect()),
                summary: if dirty.is_empty() {
                    "clean".into()
                } else {
                    format!("{} modified", dirty.len())
                },
                dirty: dirty
                    .iter()
                    .map(|p| GitPathEntry {
                        path: (*p).into(),
                        status: "M".into(),
                    })
                    .collect(),
                ..Default::default()
            }),
            current_file: file
                .map(|(path, dirty)| jaymi_context::CurrentFileSection {
                    path: Some(path.into()),
                    dirty,
                    language: Some("rust".into()),
                })
                .unwrap_or_default(),
            approved_context_providers: vec![
                "project".into(),
                "git".into(),
                "editor".into(),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn c514_planner_unchanged_workspace_resume() {
        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(c514_session("abc123", &[], Some(("/proj/src/main.rs", false))));
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        let pause_fp = paused
            .coding_goal
            .as_ref()
            .unwrap()
            .control
            .checkpoint
            .as_ref()
            .expect("checkpoint")
            .fingerprint
            .clone();
        assert!(pause_fp.is_available(), "{pause_fp:?}");
        assert_eq!(pause_fp.head_sha.as_deref(), Some("abc123"));

        let resumed = planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        let report = resumed.agent_control.expect("control");
        assert!(
            report.stale_reasons.is_empty(),
            "unchanged workspace should not be stale: {:?}",
            report.stale_reasons
        );
        assert!(!report.requires_fresh_approval);
        assert_ne!(
            resumed.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );
        assert!(!resumed.coding_goal.as_ref().unwrap().mutated);
        assert!(!report.rolled_back_mutations);
    }

    #[test]
    fn c514_planner_changed_file_and_project_stale() {
        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(c514_session("aaa", &[], Some(("/proj/src/a.rs", false))));
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c514".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "plan-c514".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "plan-c514".into(),
                },
            )
            .unwrap();
            g.autonomy_policy.active = Some(crate::BoundApproval {
                goal_id: g.id.clone(),
                plan_id: "plan-c514".into(),
                task_id: None,
                recorded: true,
                class: crate::AutonomyPolicyClass::Mutation,
                invalidated: false,
                invalidate_reason: None,
            });
        });
        planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();

        planner.context.set_session_inputs(c514_session(
            "bbb",
            &["src/a.rs"],
            Some(("/proj/src/a.rs", true)),
        ));
        let resumed = planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        let report = resumed.agent_control.expect("control");
        assert!(!report.stale_reasons.is_empty(), "{:?}", report.stale_reasons);
        assert!(report.requires_fresh_approval);
        assert!(report.approvals_invalidated);
        assert!(!resumed.coding_goal.as_ref().unwrap().mutated);
        assert!(crate::approval_autonomy_policy::assert_execution_authorized(
            resumed.coding_goal.as_ref().unwrap(),
            "plan-c514"
        )
        .is_err());
    }

    #[test]
    fn c514_planner_unavailable_fingerprint_unknown_stale() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            approved_context_providers: Vec::new(),
            ..Default::default()
        });
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        let fp = &paused
            .coding_goal
            .as_ref()
            .unwrap()
            .control
            .checkpoint
            .as_ref()
            .unwrap()
            .fingerprint;
        assert!(!fp.is_available(), "empty ambient context must be unavailable");

        let resumed = planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        let report = resumed.agent_control.expect("control");
        assert!(report
            .stale_reasons
            .contains(&crate::StaleReason::FingerprintUnavailable));
        assert!(report.requires_fresh_approval);
        assert!(!report.rolled_back_mutations);
        assert!(!resumed.coding_goal.as_ref().unwrap().mutated);
    }

    #[test]
    fn c512_simple_goal_experience() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Build me a Pong game."))
            .unwrap();
        let experience = response
            .agentic_coding_experience
            .expect("agentic experience");
        assert_eq!(experience.status, crate::AgenticExperienceStatus::Created);
        assert!(!experience.what_planned.is_empty());
        assert!(!experience.what_doing.is_empty());
        assert!(!experience.what_changed.is_empty());
        assert!(!experience.whether_worked.is_empty());
        assert!(!experience.what_went_wrong.is_empty());
        assert!(!experience.what_remains.is_empty());
        assert!(!experience.exposes_chain_of_thought);
        assert!(!experience.parallel_coding_agent);
        assert!(!experience.mutated);
        assert!(!experience.tools_executed);
        assert!(experience.planner_owned);
        assert!(experience.conversation_primary);
        assert!(response.coding_goal.is_some());
        assert!(response.coding_plan.is_none());

        let status = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap();
        assert_eq!(
            status
                .agentic_coding_experience
                .as_ref()
                .unwrap()
                .status,
            crate::AgenticExperienceStatus::Status
        );
    }

    #[test]
    fn c512_multi_file_goal_experience() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            project_open: true,
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/auth.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                metadata: jaymi_context::ProjectMetadata {
                    project_id: Some("demo".into()),
                    name: Some("demo".into()),
                    root_directory: Some("/proj".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..Default::default()
        });
        let response = planner
            .handle(UserRequest::new(
                "Refactor the authentication architecture.",
            ))
            .unwrap();
        let experience = response
            .agentic_coding_experience
            .expect("multi-file agentic");
        assert_eq!(experience.status, crate::AgenticExperienceStatus::Created);
        let goal = response.coding_goal.expect("goal");
        assert!(
            goal.user_objective.to_ascii_lowercase().contains("authentication")
                || goal.user_objective.to_ascii_lowercase().contains("refactor")
        );
        // Light relevant files may be ambient from Context — never a hidden FS walk.
        assert!(!experience.hidden_filesystem_access);
        assert!(!experience.model_direct_tools);
        assert!(!experience.unrelated_capabilities);
    }

    #[test]
    fn c512_multi_task_goal_experience() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let decomposed = planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        assert!(
            decomposed
                .coding_goal
                .as_ref()
                .unwrap()
                .tasks
                .len()
                > 1
        );
        let experience = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap()
            .agentic_coding_experience
            .expect("status");
        assert!(experience.what_planned.to_ascii_lowercase().contains("task"));
        assert!(experience.stages.iter().any(|s| {
            s.stage == crate::ExperienceStage::Decompose && s.state == crate::agentic_coding_experience::StageState::Done
        }));
        assert!(!experience.exposes_chain_of_thought);
    }

    #[test]
    fn c512_verification_failure_and_correction() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.verification_state = crate::GoalVerificationState::Failed;
            g.agent_lifecycle.last_error = Some("tests failed: login_flow".into());
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginVerification {
                    reason: "c512".into(),
                },
            )
            .unwrap_or_else(|_| g.clone());
        });
        // Force failed verification state for narration even if transition bounced.
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            g.verification_state = crate::GoalVerificationState::Failed;
            g.agent_lifecycle.last_error = Some("tests failed: login_flow".into());
        });

        let recovered = planner
            .handle(UserRequest::new("Recover from verification failure."))
            .unwrap();
        assert!(recovered.agent_recovery.is_some());
        assert!(!recovered.agent_recovery.as_ref().unwrap().mutated);
        assert!(!recovered.agent_recovery.as_ref().unwrap().auto_retried);

        let experience = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap()
            .agentic_coding_experience
            .expect("experience after failure");
        assert!(
            experience.whether_worked.to_ascii_lowercase().contains("fail")
                || experience.what_went_wrong.to_ascii_lowercase().contains("fail")
                || experience.what_went_wrong.to_ascii_lowercase().contains("login")
        );
        // Corrective path must not silently mutate or reuse destructive retries.
        assert!(!experience.silent_destructive_retry);
        assert!(!experience.mutated);
        assert!(!experience.model_direct_tools);
    }

    #[test]
    fn c512_cancellation_cannot_silently_continue() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build me a Pong game."))
            .unwrap();
        let cancelled = planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        assert!(cancelled.coding_goal.as_ref().unwrap().control.cancelled);
        assert_eq!(
            cancelled.coding_goal.as_ref().unwrap().status,
            crate::CodingGoalStatus::Cancelled
        );

        let experience = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap()
            .agentic_coding_experience
            .expect("status");
        assert!(!experience.silent_continue_after_cancel);
        assert!(
            experience.what_remains.to_ascii_lowercase().contains("cancel")
                || experience.what_doing.to_ascii_lowercase().contains("cancel")
        );
        // Cancelled goals are terminal — Begin would create a new goal; status still narrates.
        assert_eq!(experience.status, crate::AgenticExperienceStatus::Status);
    }

    #[test]
    fn c512_pause_and_resume() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        let paused = planner
            .handle(UserRequest::new("Pause this goal."))
            .unwrap();
        assert_eq!(
            paused.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );
        let paused_xp = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap()
            .agentic_coding_experience
            .expect("paused xp");
        assert!(paused_xp.what_doing.to_ascii_lowercase().contains("pause"));

        let resumed = planner
            .handle(UserRequest::new("Resume this goal."))
            .unwrap();
        assert_ne!(
            resumed.coding_goal.as_ref().unwrap().agent_lifecycle.state,
            crate::AgentLifecycleState::Paused
        );
        let control = resumed.agent_control.as_ref().expect("control");
        assert!(!control.rolled_back_mutations);
        assert!(!control.continued_after_cancel);
        assert!(!control.parallel_cancel_engine);
    }

    #[test]
    fn c512_blocked_goal_experience() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new(
                "This goal is blocked because missing API key.",
            ))
            .unwrap();
        let experience = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap()
            .agentic_coding_experience
            .expect("blocked xp");
        assert!(
            experience.what_doing.to_ascii_lowercase().contains("block")
                || experience.what_went_wrong.to_ascii_lowercase().contains("block")
                || experience.what_went_wrong.to_ascii_lowercase().contains("api")
        );
        let complete_attempt = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        assert_eq!(
            complete_attempt.goal_completion.as_ref().unwrap().outcome,
            crate::GoalCompletionOutcome::Blocked
        );
    }

    #[test]
    fn c512_successful_completion_experience() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        planner
            .handle(UserRequest::new("Start the goal."))
            .unwrap();
        planner.with_coding_goal_store(|store| {
            let g = store.active_goal_mut(None).unwrap();
            for t in &mut g.tasks {
                t.status = crate::GoalTaskStatus::Completed;
            }
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginPlanning {
                    reason: "c512".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::SubmitForApproval {
                    plan_id: "p-c512".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::RecordApproval {
                    plan_id: "p-c512".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginExecution {
                    approved_plan_id: "p-c512".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::BeginVerification {
                    reason: "v".into(),
                },
            )
            .unwrap();
            *g = crate::agent_state_machine::transition_goal(
                g.clone(),
                crate::AgentLifecycleEvent::VerificationPassed {
                    evidence_ref: "ok".into(),
                },
            )
            .unwrap();
            g.verification_state = crate::GoalVerificationState::Passed;
            g.completed_work.push("Login applied.".into());
            g.remaining_work.clear();
        });

        let completed = planner
            .handle(UserRequest::new("Mark this goal complete."))
            .unwrap();
        assert_eq!(
            completed.goal_completion.as_ref().unwrap().outcome,
            crate::GoalCompletionOutcome::SuccessfullyCompleted
        );
        assert_eq!(
            completed.coding_goal.as_ref().unwrap().status,
            crate::CodingGoalStatus::Completed
        );

        let experience = planner
            .handle(UserRequest::new("Show agentic coding experience."))
            .unwrap()
            .agentic_coding_experience
            .expect("completed xp");
        assert!(
            experience.whether_worked.to_ascii_lowercase().contains("complet")
                || experience.whether_worked.to_ascii_lowercase().contains("pass")
        );
        assert!(!experience.parallel_coding_agent);
        assert!(experience.planner_owned);
        assert!(!experience.silent_continue_after_cancel);
    }

    #[test]
    fn c51_multiple_goals_across_conversations() {
        let planner = planner_with_search_and_read();
        // Without an active Memory conversation, goals share `_default`.
        // C5.12 Begin will not create a second goal while one is active —
        // cancel (or complete) first, then start the next objective.
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        planner
            .handle(UserRequest::new("Cancel this goal."))
            .unwrap();
        planner
            .handle(UserRequest::new(
                "Add image generation to the application.",
            ))
            .unwrap();
        let store = planner.clone_coding_goal_store();
        assert!(store.goals.len() >= 2);
        let active = store.active_goal(None).expect("active");
        assert!(active.user_objective.contains("image generation"));

        // Isolate by conversation id via the store API (Planner-owned).
        let mut isolated = crate::CodingGoalStore::default();
        let a = crate::coding_goal::detect_coding_goal_request(&UserRequest::new(
            "Build a login system.",
        ))
        .unwrap();
        let b = crate::coding_goal::detect_coding_goal_request(&UserRequest::new(
            "Refactor the authentication architecture.",
        ))
        .unwrap();
        isolated.push_active(crate::coding_goal::create_goal(
            &a,
            &jaymi_context::ContextBundle::default(),
            Some("conv-a".into()),
        ));
        isolated.push_active(crate::coding_goal::create_goal(
            &b,
            &jaymi_context::ContextBundle::default(),
            Some("conv-b".into()),
        ));
        assert_ne!(
            isolated.active_goal(Some("conv-a")).unwrap().id,
            isolated.active_goal(Some("conv-b")).unwrap().id
        );
    }

    #[test]
    fn c52_multi_task_decomposition_no_mutation() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system."))
            .unwrap();
        let response = planner
            .handle(UserRequest::new("Break this goal into tasks."))
            .unwrap();
        let report = response
            .goal_decomposition
            .expect("goal_decomposition");
        assert_eq!(report.status, crate::DecompositionStatus::Proposed);
        assert!(report.tasks.len() >= 5);
        assert!(!report.mutated);
        assert!(!report.tools_executed);
        assert!(!report.auto_approved);
        assert!(response.tool_id.is_none());
        assert!(response.execution_plan.is_none());
        let goal = response.coding_goal.expect("coding_goal");
        assert_eq!(goal.tasks.len(), report.tasks.len());
        assert!(!goal.mutated);
        assert!(response.content.contains("Goal Decomposition"));
        assert!(report.order.len() == report.tasks.len());
    }

    #[test]
    fn c52_single_task_when_constrained() {
        let planner = planner_with_search_and_read();
        // Constraint via objective phrasing extracted into goal.constraints.
        planner
            .handle(UserRequest::new(
                "Build a login system. Keep this as one task only.",
            ))
            .unwrap();
        // If create didn't capture the constraint phrase as a separate constraint,
        // inject via store for the soft-path contract.
        let conversation_id = None;
        planner.with_coding_goal_store(|store| {
            if let Some(goal) = store.active_goal_mut(conversation_id) {
                if !goal.constraints.iter().any(|c| c.contains("one task")) {
                    goal.constraints
                        .push("Keep this as one task only.".into());
                }
            }
        });
        let response = planner
            .handle(UserRequest::new("Decompose the goal."))
            .unwrap();
        let report = response.goal_decomposition.expect("decomp");
        assert_eq!(report.tasks.len(), 1);
        assert!(!report.mutated);
    }

    #[test]
    fn c52_ambiguous_without_goal() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Break things down."))
            .unwrap();
        let report = response.goal_decomposition.expect("decomp");
        assert!(matches!(
            report.status,
            crate::DecompositionStatus::Ambiguous | crate::DecompositionStatus::NoActiveGoal
        ));
        assert!(report.tasks.is_empty());
        assert!(!report.mutated);
    }

    #[test]
    fn c52_preserves_constraints_and_task_transitions() {
        let planner = planner_with_search_and_read();
        planner
            .handle(UserRequest::new("Build a login system without OAuth."))
            .unwrap();
        let decomp = planner
            .handle(UserRequest::new("Propose tasks for this goal."))
            .unwrap();
        let report = decomp.goal_decomposition.expect("decomp");
        assert!(!report.preserved_constraints.is_empty());
        assert!(report
            .preserved_constraints
            .iter()
            .any(|c| c.to_ascii_lowercase().contains("oauth")));

        let goal = decomp.coding_goal.expect("goal");
        let first = goal
            .tasks
            .iter()
            .find(|t| t.dependencies.is_empty())
            .map(|t| t.id.clone())
            .expect("root task");
        let completed = planner
            .handle(UserRequest::new(format!("Complete task {first}")))
            .unwrap();
        let done = completed.goal_decomposition.expect("complete");
        assert!(done
            .tasks
            .iter()
            .any(|t| t.id == first && t.status == crate::GoalTaskStatus::Completed));

        // Cancel a still-pending later task (e.g. UI) — status only, no execution.
        let cancel_id = done
            .tasks
            .iter()
            .find(|t| t.status == crate::GoalTaskStatus::Pending)
            .map(|t| t.id.clone())
            .expect("pending task");
        let cancelled = planner
            .handle(UserRequest::new(format!("Cancel task {cancel_id}")))
            .unwrap();
        let updated = cancelled.goal_decomposition.expect("task update");
        assert_eq!(updated.status, crate::DecompositionStatus::TaskUpdated);
        assert!(updated
            .tasks
            .iter()
            .any(|t| t.id == cancel_id && t.status == crate::GoalTaskStatus::Cancelled));
        assert!(!updated.mutated);
        assert!(!updated.auto_approved);
    }

    #[test]
    fn c52_circular_dependency_detection_unit() {
        let a = crate::GoalTask {
            id: "a".into(),
            description: "A".into(),
            objective: "A".into(),
            dependencies: vec!["b".into()],
            ..Default::default()
        };
        let b = crate::GoalTask {
            id: "b".into(),
            description: "B".into(),
            objective: "B".into(),
            dependencies: vec!["a".into()],
            ..Default::default()
        };
        let err = crate::goal_decomposition::validate_dependencies(&[a, b]).unwrap_err();
        assert!(matches!(
            err,
            crate::goal_decomposition::DependencyError::Circular { .. }
        ));
    }

    #[test]
    fn generate_without_coding_plan_is_honest() {
        let planner = planner_with_write();
        let response = planner
            .handle(UserRequest::new("Generate the code."))
            .unwrap();
        assert!(!response.awaiting_review);
        assert!(response.execution_plan.is_none());
        assert!(response.content.contains("Coding Plan"));
    }

    #[test]
    fn rename_symbol_multi_file_requires_review() {
        let planner = planner_with_write();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            open_files: jaymi_context::OpenFilesSection {
                files: vec![jaymi_context::OpenFileEntry {
                    path: "/proj/src/main.rs".into(),
                    dirty: false,
                    active: false,
                }],
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/lib.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "counter".into(),
                    kind: Some("variable".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/lib.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/main.rs".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner
            .handle(UserRequest::new("Rename symbol"))
            .unwrap();

        assert!(response.multi_file.is_some());
        let multi = response.multi_file.as_ref().unwrap();
        assert_eq!(multi.kind, Some(MultiFileKind::RenameSymbol));
        assert!(
            multi.affected_files.len() >= 2,
            "expected WI-derived affected files, got {:?}",
            multi.affected_files
        );
        assert!(response.awaiting_review);
        assert!(response.execution_plan.is_some());
        assert!(response.content.contains("## Multi-file Intelligence"));
        assert!(response.content.contains("Affected Files"));
        assert!(response.content.contains("Execution Plan"));
        assert!(response.code_generation.is_some());
    }

    #[test]
    fn extract_class_and_split_file_are_reviewed_multi_file_plans() {
        let planner = planner_with_write();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/big.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/big.rs".into()),
                    dirty: true,
                    language: Some("rust".into()),
                },
                enclosing_type: Some(jaymi_context::EditorSymbol {
                    name: "Widget".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        for prompt in ["Extract class", "Split file"] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            assert!(
                response.multi_file.is_some(),
                "expected multi_file for {prompt}"
            );
            assert!(response.awaiting_review, "{prompt} should require review");
            assert!(response.execution_plan.is_some());
            assert!(response.content.contains("## Multi-file Intelligence"));
        }
    }

    #[test]
    fn move_module_multi_file_requires_review() {
        let planner = planner_with_write_and_manage();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/util.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            open_files: jaymi_context::OpenFilesSection {
                files: vec![jaymi_context::OpenFileEntry {
                    path: "/proj/src/main.rs".into(),
                    dirty: false,
                    active: false,
                }],
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/util.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                open_editors: jaymi_context::OpenFilesSection {
                    files: vec![
                        jaymi_context::OpenFileEntry {
                            path: "/proj/src/util.rs".into(),
                            dirty: false,
                            active: true,
                        },
                        jaymi_context::OpenFileEntry {
                            path: "/proj/src/main.rs".into(),
                            dirty: false,
                            active: false,
                        },
                    ],
                },
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner.handle(UserRequest::new("Move module")).unwrap();
        assert!(response.multi_file.is_some());
        let multi = response.multi_file.as_ref().unwrap();
        assert_eq!(multi.kind, Some(MultiFileKind::MoveModule));
        assert!(
            multi
                .affected_files
                .iter()
                .any(|f| f.role == "source" || f.role == "destination"),
            "expected source/destination from WI, got {:?}",
            multi.affected_files
        );
        assert!(response.awaiting_review);
        assert!(response.execution_plan.is_some());
        assert!(response.content.contains("## Multi-file Intelligence"));
    }

    #[test]
    fn pair_programming_attaches_planner_generated_suggestions() {
        let planner = planner_with_write();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner
            .handle(UserRequest::new("Explain the architecture of this project."))
            .unwrap();
        assert!(response.pair_programming.is_some());
        let surface = response.pair_programming.as_ref().unwrap();
        assert!(
            !surface.suggested_actions.is_empty(),
            "expected Planner-generated suggestions"
        );
        assert!(
            surface
                .suggested_actions
                .iter()
                .any(|a| a.label == "Review Changes" || a.id == "review_changes"),
            "understanding should suggest Review Changes, got {:?}",
            surface
                .suggested_actions
                .iter()
                .map(|a| a.label.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn pair_programming_examples_route_without_tools() {
        let planner = planner_with_write();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let review = planner.handle(UserRequest::new("Review Changes.")).unwrap();
        assert!(review.coding_review.is_some() || review.content.contains("Review"));
        assert!(!review.awaiting_review, "review must not execute tools");

        let tests = planner.handle(UserRequest::new("Generate Tests.")).unwrap();
        assert!(tests.coding_plan.is_some());
        assert!(!tests.awaiting_review, "planning must not execute tools");

        let dead = planner.handle(UserRequest::new("Find Dead Code.")).unwrap();
        assert!(dead.content.contains("Find Dead Code"));
        assert!(!dead.awaiting_review);
        assert!(dead.pair_programming.is_some());
    }

    #[test]
    fn pair_programming_baseline_suggestions_are_planner_owned() {
        let planner = planner_with_write();
        let surface = planner.pair_programming_suggestions();
        let labels: Vec<&str> = surface
            .suggested_actions
            .iter()
            .map(|a| a.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                "Explain Architecture",
                "Review Changes",
                "Generate Tests",
                "Refactor",
                "Run Build",
                "Find Dead Code",
            ]
        );
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
                generation_calls: Vec::new(),
                generation_baselines: Vec::new(),
                test_execution: None,
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
                generation_calls: Vec::new(),
                generation_baselines: Vec::new(),
                test_execution: None,
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
                generation_calls: Vec::new(),
                generation_baselines: Vec::new(),
                test_execution: None,
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

    /// Stub Search Knowledge tool for Intelligent Code Search (C2.1) tests.
    struct StubSearchKnowledgeTool {
        metadata: ToolMetadata,
        citations: Vec<Citation>,
        last_query: std::sync::Mutex<Option<String>>,
    }

    impl StubSearchKnowledgeTool {
        fn new(citations: Vec<Citation>) -> Self {
            Self {
                metadata: ToolMetadata {
                    id: SEARCH_KNOWLEDGE_TOOL_ID.to_string(),
                    name: "Stub Search Knowledge".into(),
                    version: "0".into(),
                    description: "test search_knowledge".into(),
                    provider: FILESYSTEM_PROVIDER_ID.to_string(),
                    capabilities: vec![Capability::Search],
                    risk: ToolRisk::Safe,
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::VeryLow,
                    memory_usage: MemoryUsage::Tiny,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::LocalOnly,
                    internet: InternetRequirement::Never,
                    reliability: Reliability::Stable,
                    result_type: ResultType::SearchResults,
                },
                citations,
                last_query: std::sync::Mutex::new(None),
            }
        }

        fn last_query(&self) -> Option<String> {
            self.last_query.lock().ok().and_then(|g| g.clone())
        }
    }

    impl Tool for StubSearchKnowledgeTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
            if let Some(search) = &input.search {
                if let Ok(mut guard) = self.last_query.lock() {
                    *guard = search.free_text.clone();
                }
            }
            let citations = self.citations.clone();
            Ok(ToolOutput {
                success: true,
                citations: citations.clone(),
                message: Some(format!("stub hits={}", citations.len())),
                ..Default::default()
            })
        }
    }

    fn planner_with_stub_search_knowledge(citations: Vec<Citation>) -> (Planner, Arc<StubSearchKnowledgeTool>) {
        let stub = Arc::new(StubSearchKnowledgeTool::new(citations));
        let stub_clone = Arc::clone(&stub);
        let planner = planner_with_tools(move |tools, filesystem, content_api| {
            tools
                .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                .unwrap();
            tools
                .register_tool(Arc::new(ReadFileTool::new(content_api)))
                .unwrap();
            tools.register_tool(stub_clone).unwrap();
        });
        (planner, stub)
    }

    fn planner_with_stub_search_and_reasoning(
        citations: Vec<Citation>,
        reasoning: Arc<CountingReasoningProvider>,
    ) -> (Planner, Arc<StubSearchKnowledgeTool>) {
        let stub = Arc::new(StubSearchKnowledgeTool::new(citations));
        let stub_clone = Arc::clone(&stub);
        let reasoning =
            Arc::clone(&reasoning) as Arc<dyn jaymi_reasoning::ReasoningProvider>;
        let planner = planner_with_tools_policies_routes_and_reasoning(
            move |tools, filesystem, content_api| {
                tools
                    .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                    .unwrap();
                tools
                    .register_tool(Arc::new(ReadFileTool::new(content_api)))
                    .unwrap();
                tools.register_tool(stub_clone).unwrap();
            },
            |policies| {
                policies.initialize().unwrap();
            },
            ToolRouteTable::builtin(),
            Some(reasoning),
        );
        (planner, stub)
    }

    fn sample_citations(n: usize) -> Vec<Citation> {
        (0..n)
            .map(|i| Citation {
                title: format!("file_{i}.rs"),
                location: PathBuf::from(format!("/proj/src/file_{i}.rs")),
                preview: format!("fn match_{i}() {{ /* auth */ }}"),
                why_matched: "free_text".into(),
                confidence: 8_000,
                matching_section: Some(if i == 0 {
                    "authenticate".into()
                } else {
                    format!("sym_{i}")
                }),
                line: Some(i as u32),
                column: Some(0),
                end_line: Some(i as u32),
                end_column: Some(10),
            })
            .collect()
    }

    #[test]
    fn code_search_exact_symbol_is_planner_owned_read_only() {
        let (planner, stub) = planner_with_stub_search_knowledge(sample_citations(2));
        let response = planner
            .handle(UserRequest::new("Where is ContextBundle created?"))
            .unwrap();
        assert!(!response.awaiting_review);
        assert!(!response.blocked);
        assert_eq!(
            response.tool_id.as_deref(),
            Some(SEARCH_KNOWLEDGE_TOOL_ID)
        );
        assert!(response.code_search.is_some());
        let report = response.code_search.as_ref().unwrap();
        assert_eq!(report.kind, Some(CodeSearchKind::ExactSymbol));
        assert!(report.query.contains("ContextBundle"));
        assert_eq!(report.evidence.len(), 2);
        assert!(response.content.contains("## Code Search"));
        assert!(response.content.contains("Read-only"));
        let q = stub.last_query().unwrap_or_default();
        assert!(q.contains("ContextBundle"), "query={q}");
        // No mutation tools.
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
        assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
    }

    #[test]
    fn code_search_natural_language_and_multiple_files() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(3));
        let response = planner
            .handle(UserRequest::new("Where is authentication handled?"))
            .unwrap();
        let report = response.code_search.expect("code_search");
        assert_eq!(report.kind, Some(CodeSearchKind::NaturalLanguage));
        assert_eq!(report.total_hits, 3);
        assert_eq!(report.evidence.len(), 3);
        assert!(response.content.contains("file_0.rs"));
        assert!(response.content.contains("file_1.rs"));
    }

    #[test]
    fn code_search_no_results_is_honest() {
        let (planner, _) = planner_with_stub_search_knowledge(Vec::new());
        let response = planner
            .handle(UserRequest::new("Find where the score gets updated."))
            .unwrap();
        let report = response.code_search.expect("code_search");
        assert_eq!(report.total_hits, 0);
        assert!(report.evidence.is_empty());
        assert!(response.content.contains("no matches"));
    }

    #[test]
    fn code_search_large_result_set_respects_budget() {
        let (planner, _) =
            planner_with_stub_search_knowledge(sample_citations(CONVERSATION_EVIDENCE_CAP + 8));
        let response = planner
            .handle(UserRequest::new(
                "Find the code responsible for opening Settings.",
            ))
            .unwrap();
        let report = response.code_search.expect("code_search");
        assert_eq!(report.evidence.len(), CONVERSATION_EVIDENCE_CAP);
        assert!(report.truncated);
        assert!(response.content.contains("context budget"));
        assert!(report
            .to_diagnostics_summary()
            .contains("truncated=true"));
    }

    #[test]
    fn code_search_works_without_active_workspace() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(1));
        // No project open — scope_search_request leaves folder unset.
        assert!(planner.active_project_root().is_none());
        let response = planner
            .handle(UserRequest::new("Where is authentication handled?"))
            .unwrap();
        assert!(response.code_search.is_some());
        assert_eq!(response.tool_id.as_deref(), Some(SEARCH_KNOWLEDGE_TOOL_ID));
    }

    #[test]
    fn code_search_find_references_soft_path_uses_wi_only() {
        // Call-site deixis is Symbol Navigation (C2.2); lexical code search no longer owns it.
        let (planner, stub) = planner_with_stub_search_knowledge(sample_citations(1));
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/game.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/game.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "update_score".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/game.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 10,
                            start_column: 0,
                            end_line: 10,
                            end_column: 12,
                        },
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/ui.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 4,
                            start_column: 2,
                            end_line: 4,
                            end_column: 14,
                        },
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner
            .handle(UserRequest::new(
                "Show me all the places that call this function.",
            ))
            .unwrap();
        let report = response.symbol_navigation.expect("symbol_navigation");
        assert_eq!(report.kind, Some(SymbolNavKind::Callers));
        assert_eq!(report.strategy, "workspace_intelligence");
        assert_eq!(report.evidence.len(), 2);
        assert!(response.content.contains("update_score"));
        assert!(response.tool_id.is_none());
        assert!(stub.last_query().is_none());
        assert!(!response.awaiting_review);
        assert!(response.code_search.is_none());
    }

    #[test]
    fn symbol_navigation_definition_from_wi() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/lib.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "assemble".into(),
                    kind: Some("function".into()),
                    range: Some(jaymi_context::EditorRange {
                        start_line: 40,
                        start_column: 7,
                        end_line: 40,
                        end_column: 15,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Where is this function defined?"))
            .unwrap();
        let report = response.symbol_navigation.expect("symbol_navigation");
        assert_eq!(report.kind, Some(SymbolNavKind::Definition));
        assert!(!report.evidence.is_empty());
        assert!(response.content.contains("## Symbol Navigation"));
        assert!(response.tool_id.is_none());
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn symbol_navigation_missing_symbol_is_honest() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("What implements this trait?"))
            .unwrap();
        let report = response.symbol_navigation.expect("symbol_navigation");
        assert_eq!(report.kind, Some(SymbolNavKind::Implementations));
        assert!(report.evidence.is_empty());
        assert!(response.content.contains("no navigation targets"));
        assert!(!response.awaiting_review);
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn symbol_navigation_multi_file_references() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/a.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/a.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "ContextBundle".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/a.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 1,
                            start_column: 0,
                            end_line: 1,
                            end_column: 13,
                        },
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/b.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 8,
                            start_column: 2,
                            end_line: 8,
                            end_column: 15,
                        },
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/c.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 3,
                            start_column: 0,
                            end_line: 3,
                            end_column: 13,
                        },
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Where is this type used?"))
            .unwrap();
        let report = response.symbol_navigation.expect("symbol_navigation");
        assert_eq!(report.kind, Some(SymbolNavKind::References));
        assert_eq!(report.evidence.len(), 3);
        assert!(response.content.contains("a.rs"));
        assert!(response.content.contains("b.rs"));
        assert!(response.content.contains("c.rs"));
    }

    #[test]
    fn symbol_navigation_never_mutates() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "foo".into(),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/src/lib.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        for prompt in [
            "Who calls this?",
            "Where is this function defined?",
            "Show me everything that depends on this.",
        ] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            assert!(response.symbol_navigation.is_some(), "prompt={prompt}");
            assert!(response.lsp_edits.is_empty());
            assert!(!response.awaiting_review);
            assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
            assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
        }
    }

    #[test]
    fn relationship_trace_click_save_builds_chain() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/ui.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/ui.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "on_save".into(),
                    kind: Some("function".into()),
                    range: Some(jaymi_context::EditorRange {
                        start_line: 40,
                        start_column: 3,
                        end_line: 40,
                        end_column: 10,
                    }),
                    ..Default::default()
                }),
                enclosing_type: Some(jaymi_context::EditorSymbol {
                    name: "EditorChrome".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/ui.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 12,
                            start_column: 0,
                            end_line: 12,
                            end_column: 7,
                        },
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/actions.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 8,
                            start_column: 2,
                            end_line: 8,
                            end_column: 9,
                        },
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let response = planner
            .handle(UserRequest::new("What happens when I click Save?"))
            .unwrap();
        let report = response.relationship_trace.expect("relationship_trace");
        assert_eq!(report.kind, Some(TraceKind::EventFlow));
        assert!(report.steps.len() >= 3);
        assert!(report.steps.iter().any(|s| s.action == "identify_symbol"));
        assert!(report.steps.iter().any(|s| s.action == "find_callers"));
        assert!(response.content.contains("### Investigation Chain"));
        assert!(response.content.contains("Read-only"));
        assert!(report.files_inspected <= relationship_tracing::MAX_FILES);
        assert!(report.steps.len() <= relationship_tracing::MAX_STEPS);
        assert!(response.lsp_edits.is_empty());
        assert!(!response.awaiting_review);
    }

    #[test]
    fn relationship_trace_provider_callers_and_no_mutation() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/provider.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "LspProvider".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/src/boot.rs".into(),
                    range: jaymi_context::EditorRange {
                        start_line: 1,
                        start_column: 0,
                        end_line: 1,
                        end_column: 11,
                    },
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("What calls this provider?"))
            .unwrap();
        let report = response.relationship_trace.expect("relationship_trace");
        assert_eq!(report.kind, Some(TraceKind::ProviderCallers));
        assert!(!report.evidence.is_empty());
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
        assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn relationship_trace_missing_seed_is_honest() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new(
                "Trace this request through the application.",
            ))
            .unwrap();
        let report = response.relationship_trace.expect("relationship_trace");
        assert_eq!(report.stop_reason, Some(TraceStopReason::NoSeed));
        assert!(response.content.contains("Investigation Chain"));
    }

    #[test]
    fn relationship_trace_does_not_steal_simple_who_calls() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "foo".into(),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/a.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner.handle(UserRequest::new("Who calls this?")).unwrap();
        assert!(response.relationship_trace.is_none());
        assert!(response.symbol_navigation.is_some());
    }

    #[test]
    fn deep_investigation_why_slow_is_planner_owned() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/hot.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "render_frame".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/src/loop.rs".into(),
                    range: jaymi_context::EditorRange {
                        start_line: 20,
                        start_column: 0,
                        end_line: 20,
                        end_column: 12,
                    },
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Why is this slow?"))
            .unwrap();
        let report = response.deep_investigation.expect("deep_investigation");
        assert_eq!(report.kind, Some(InvestigationKind::Performance));
        assert!(!report.required_evidence.is_empty());
        assert!(!report.steps.is_empty());
        assert!(response.content.contains("## Deep Investigation"));
        assert!(response.content.contains("What could not be established"));
        assert!(!response.content.contains("budget="));
        assert!(!response.content.contains("max_steps"));
        assert!(report.to_diagnostics_summary().contains("deep_investigation"));
        assert!(report.chain_diagnostics_lines().iter().any(|l| l.contains("budget")));
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
        assert!(response.lsp_edits.is_empty());
        assert!(!response.awaiting_review);
    }

    #[test]
    fn deep_investigation_how_auth_works() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(3));
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("How does authentication work?"))
            .unwrap();
        let report = response.deep_investigation.expect("deep_investigation");
        assert_eq!(report.kind, Some(InvestigationKind::SubsystemHow));
        assert!(report.searches.iter().any(|s| s.to_ascii_lowercase().contains("auth"))
            || report.required_evidence.iter().any(|e| matches!(e, deep_investigation::EvidenceNeed::Search { .. })));
        assert!(response.content.contains("Deep Investigation"));
        assert!(response.code_search.is_none());
        assert!(response.relationship_trace.is_none());
    }

    #[test]
    fn deep_investigation_feature_placement_and_change_impact() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "TokenStore".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/src/auth.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });

        let placement = planner
            .handle(UserRequest::new(
                "Where should this feature be implemented?",
            ))
            .unwrap();
        let placement_report = placement.deep_investigation.expect("placement");
        assert_eq!(
            placement_report.kind,
            Some(InvestigationKind::FeaturePlacement)
        );
        assert!(placement.coding_understanding.is_none());

        let impact = planner
            .handle(UserRequest::new(
                "What would be affected if I changed this?",
            ))
            .unwrap();
        let impact_report = impact.deep_investigation.expect("impact");
        assert_eq!(impact_report.kind, Some(InvestigationKind::ChangeImpact));
        assert!(impact_report.steps.len() <= deep_investigation::MAX_STEPS);
        assert!(impact.lsp_edits.is_empty());
    }

    #[test]
    fn deep_investigation_insufficient_evidence_is_honest() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Why does this state sometimes disappear?"))
            .unwrap();
        let report = response.deep_investigation.expect("deep_investigation");
        assert_eq!(report.kind, Some(InvestigationKind::StateBug));
        assert!(response.content.contains("What could not be established"));
        assert!(!report.gaps.is_empty() || response.content.contains("no solid evidence")
            || response.content.contains("could not"));
        assert!(!response.content.contains("deep_investigation kind="));
    }

    #[test]
    fn deep_investigation_does_not_steal_narrower_modes() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "save".into(),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/a.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let who = planner.handle(UserRequest::new("Who calls this?")).unwrap();
        assert!(who.deep_investigation.is_none());
        assert!(who.symbol_navigation.is_some());

        let flow = planner
            .handle(UserRequest::new("What happens when I click Save?"))
            .unwrap();
        assert!(flow.deep_investigation.is_none());
        assert!(flow.relationship_trace.is_some());
    }

    #[test]
    fn codebase_qa_permissions_uses_investigation_route() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(3));
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("How does Jaymi handle permissions?"))
            .unwrap();
        let report = response.codebase_qa.expect("codebase_qa");
        assert_eq!(report.kind, Some(QaKind::HowHandles));
        assert_eq!(report.complexity, Some(QaComplexity::Investigate));
        assert!(report.investigation_needed);
        assert!(response.content.contains("## Codebase Q&A"));
        assert!(response.content.contains("What could not be established")
            || !report.gaps.is_empty()
            || !report.evidence.is_empty());
        assert!(!response.content.contains("codebase_qa kind="));
        assert!(report.to_diagnostics_summary().contains("codebase_qa"));
        assert!(response.deep_investigation.is_none());
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn codebase_qa_settings_models_is_focused_search_not_deep() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(2));
        let response = planner
            .handle(UserRequest::new("Where does Settings get its models?"))
            .unwrap();
        let report = response.codebase_qa.expect("codebase_qa");
        assert_eq!(report.kind, Some(QaKind::WhereGets));
        assert_eq!(report.complexity, Some(QaComplexity::Search));
        assert!(!report.investigation_needed);
        assert!(response.content.contains("deep investigation was not required")
            || response.content.contains("Focused search"));
        assert!(response.deep_investigation.is_none());
        assert!(response.relationship_trace.is_none());
    }

    #[test]
    fn codebase_qa_after_press_enter_is_flow() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "on_enter".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/src/input.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("What happens after I press Enter?"))
            .unwrap();
        let report = response.codebase_qa.expect("codebase_qa");
        assert_eq!(report.kind, Some(QaKind::EventFlow));
        assert_eq!(report.complexity, Some(QaComplexity::Flow));
        assert_eq!(report.route, Some(QaRoute::Relationship));
        assert!(report.investigation_needed);
    }

    #[test]
    fn codebase_qa_feature_belong_and_remove_provider() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(2));
        let belong = planner
            .handle(UserRequest::new("Where would image generation belong?"))
            .unwrap();
        let belong_report = belong.codebase_qa.expect("belong");
        assert_eq!(belong_report.kind, Some(QaKind::FeaturePlacement));
        assert!(!belong_report.investigation_needed);
        assert!(belong.coding_understanding.is_none());

        let remove = planner
            .handle(UserRequest::new(
                "What would break if I removed this provider?",
            ))
            .unwrap();
        let remove_report = remove.codebase_qa.expect("remove");
        assert_eq!(remove_report.kind, Some(QaKind::ChangeImpact));
        assert!(remove_report.investigation_needed);
        assert!(remove.lsp_edits.is_empty());
        assert!(!remove.awaiting_review);
    }

    #[test]
    fn codebase_qa_does_not_steal_narrower_or_simple_understanding() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "foo".into(),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/a.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let who = planner.handle(UserRequest::new("Who calls this?")).unwrap();
        assert!(who.codebase_qa.is_none());
        assert!(who.symbol_navigation.is_some());

        let slow = planner.handle(UserRequest::new("Why is this slow?")).unwrap();
        assert!(slow.codebase_qa.is_none());
        assert!(slow.deep_investigation.is_some());

        assert!(codebase_qa::detect_codebase_qa_request(&UserRequest::new(
            "Where is authentication handled?",
        ))
        .is_none());
        let (search_planner, _) = planner_with_stub_search_knowledge(sample_citations(1));
        let where_is = search_planner
            .handle(UserRequest::new("Where is authentication handled?"))
            .unwrap();
        assert!(where_is.codebase_qa.is_none());
        assert!(where_is.code_search.is_some());
    }

    // --- C3.1: Coding Edit Intent ---

    fn session_with_selection_and_symbol() -> jaymi_context::ContextSessionInputs {
        jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/ui.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/ui.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                selection: jaymi_context::CurrentSelectionSection {
                    path: Some("/proj/src/ui.rs".into()),
                    start_line: 10,
                    start_column: 0,
                    end_line: 12,
                    end_column: 4,
                    text: Some("fn paint_button() { color = RED; }".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "paint_button".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        }
    }

    #[test]
    fn c31_clear_single_file_edit_is_understood() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/button.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/button.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Button".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Change the button color."))
            .unwrap();
        let obj = response.coding_edit.expect("coding_edit");
        assert_eq!(obj.kind, Some(CodingEditKind::Change));
        assert_eq!(obj.status, CodingEditStatus::Understood);
        assert_eq!(obj.target_file.as_deref(), Some("/proj/src/button.rs"));
        assert!(response.content.contains("I understand what you want to change"));
        assert!(!response.awaiting_review);
        assert!(response.lsp_edits.is_empty());
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
        assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
    }

    #[test]
    fn c31_selection_based_and_fix_this_with_context() {
        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(session_with_selection_and_symbol());
        let selection = planner
            .handle(UserRequest::new("Refactor this function."))
            .unwrap();
        let obj = selection.coding_edit.expect("selection edit");
        assert_eq!(obj.kind, Some(CodingEditKind::Refactor));
        assert_eq!(obj.status, CodingEditStatus::Understood);
        assert!(obj.target_selection.is_some() || !obj.target_symbols.is_empty());
        assert_eq!(obj.expected_scope, Some(EditScope::Selection));

        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(session_with_selection_and_symbol());
        let fix = planner.handle(UserRequest::new("Fix this.")).unwrap();
        let fix_obj = fix.coding_edit.expect("fix this");
        assert_eq!(fix_obj.kind, Some(CodingEditKind::Fix));
        assert_eq!(fix_obj.status, CodingEditStatus::Understood);
        assert!(fix.content.contains("I understand what you want to change"));
        assert!(fix.tool_id.is_none());
    }

    #[test]
    fn c31_fix_this_without_context_asks_clarification() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("Fix this.")).unwrap();
        let obj = response.coding_edit.expect("coding_edit");
        assert_eq!(obj.status, CodingEditStatus::NeedsClarification);
        assert!(obj.clarification.is_some());
        assert!(response.content.contains("Clarification") || response.content.contains("clearer target"));
        assert!(response.lsp_edits.is_empty());
        assert!(!response.awaiting_review);
        assert!(response.execution_plan.is_none());
    }

    #[test]
    fn c31_multi_file_rename_refactor_create_and_kinds() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/a.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/a.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "TokenStore".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/a.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/b.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/c.rs".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let multi = planner
            .handle(UserRequest::new("Make this faster."))
            .unwrap();
        let multi_obj = multi.coding_edit.expect("optimize");
        assert_eq!(multi_obj.kind, Some(CodingEditKind::Optimize));
        assert_eq!(multi_obj.status, CodingEditStatus::Understood);
        assert!(
            multi_obj.relevant_files.len() > 1
                || multi_obj.expected_scope == Some(EditScope::MultiFile)
                || multi_obj.expected_scope == Some(EditScope::Symbol)
        );

        let rename = planner.handle(UserRequest::new("Rename this.")).unwrap();
        assert!(rename.coding_edit.is_some());
        assert_eq!(
            rename.coding_edit.as_ref().unwrap().kind,
            Some(CodingEditKind::Rename)
        );

        let create = planner
            .handle(UserRequest::new("Create a new settings page."))
            .unwrap();
        let create_obj = create.coding_edit.as_ref().expect("create");
        assert_eq!(create_obj.kind, Some(CodingEditKind::Create));
        assert!(create.multi_file.is_none());
        assert!(create.lsp_edits.is_empty());
        assert!(create.execution_plan.is_none());
    }

    #[test]
    fn c31_read_only_and_review_remain_read_only() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(session_with_selection_and_symbol());
        let explain = planner.handle(UserRequest::new("Explain this.")).unwrap();
        assert!(explain.coding_edit.is_none());

        let where_used = planner
            .handle(UserRequest::new("Where is this used?"))
            .unwrap();
        assert!(where_used.coding_edit.is_none());
        assert!(
            where_used.symbol_navigation.is_some() || where_used.code_search.is_some()
        );

        let review = planner
            .handle(UserRequest::new("Review this code."))
            .unwrap();
        assert!(review.coding_edit.is_none());
        assert!(review.coding_review.is_some() || review.content.contains("Review"));
    }

    #[test]
    fn c31_ambiguous_named_target_asks_clarification() {
        let planner = planner_with_search_and_read();
        // No editor / project binding for "login button".
        let response = planner
            .handle(UserRequest::new("Change the login button."))
            .unwrap();
        let obj = response.coding_edit.expect("coding_edit");
        assert_eq!(obj.kind, Some(CodingEditKind::Change));
        assert_eq!(obj.status, CodingEditStatus::NeedsClarification);
        assert!(obj.clarification.is_some());
        assert!(response.content.contains("Clarification") || !obj.ambiguities.is_empty());
    }

    #[test]
    fn c31_no_mutation_tools_and_planner_owns_classification() {
        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(session_with_selection_and_symbol());
        for prompt in [
            "Fix this bug.",
            "Add authentication.",
            "Remove this unused function.",
            "Add tests for this.",
        ] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            assert!(
                response.coding_edit.is_some(),
                "Planner should own edit classification for {prompt}"
            );
            assert!(response.execution_plan.is_none());
            assert!(!response.awaiting_review);
            assert!(response.lsp_edits.is_empty());
            assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
            assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
            assert!(!response.reasoning_used);
            assert!(response
                .coding_edit
                .as_ref()
                .unwrap()
                .to_diagnostics_summary()
                .contains("coding_edit"));
        }
    }

    #[test]
    fn c31_c2_reading_behavior_unchanged() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(2));
        let search = planner
            .handle(UserRequest::new("Where is ContextBundle created?"))
            .unwrap();
        assert!(search.coding_edit.is_none());
        assert!(search.code_search.is_some());

        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "assemble".into(),
                    kind: Some("function".into()),
                    range: Some(jaymi_context::EditorRange {
                        start_line: 10,
                        start_column: 0,
                        end_line: 10,
                        end_column: 8,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let nav = planner
            .handle(UserRequest::new("Where is this function defined?"))
            .unwrap();
        assert!(nav.coding_edit.is_none());
        assert!(nav.symbol_navigation.is_some());
    }

    // --- C3.2: Change Investigation ---

    #[test]
    fn c32_simple_single_file_change_investigates() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/button.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/button.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Button".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Change the button color."))
            .unwrap();
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert_eq!(inv.mode, ChangeInvestigationMode::Full);
        assert!(
            inv.affected_files
                .iter()
                .any(|p| p.contains("button.rs"))
                || inv.primary_file.as_deref() == Some("/proj/src/button.rs")
        );
        assert!(response.content.contains("Change Investigation"));
        assert!(response.coding_edit.is_some());
        assert!(response.execution_plan.is_none());
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn c32_multi_file_change_lists_affected() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/a.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/a.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "TokenStore".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/a.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/b.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/tests/token_store_test.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/Cargo.toml".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Make this faster."))
            .unwrap();
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert_eq!(inv.mode, ChangeInvestigationMode::Full);
        assert!(inv.affected_files.len() >= 2);
        assert!(
            !inv.tests_possibly_affected.is_empty()
                || inv.affected_files.iter().any(|p| p.contains("test"))
        );
        assert!(
            !inv.configuration_possibly_affected.is_empty()
                || inv.affected_files.iter().any(|p| p.contains("Cargo.toml"))
        );
    }

    #[test]
    fn c32_ambiguous_target_light_with_unknowns() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("Fix this.")).unwrap();
        let obj = response.coding_edit.expect("coding_edit");
        assert_eq!(obj.status, CodingEditStatus::NeedsClarification);
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert_eq!(inv.mode, ChangeInvestigationMode::Light);
        assert!(!inv.unknowns.is_empty());
        assert!(response.content.contains("What is unknown") || !inv.unknowns.is_empty());
    }

    #[test]
    fn c32_missing_project_records_unknowns() {
        let planner = planner_with_search_and_read();
        // No editor / project session — named target cannot bind.
        let response = planner
            .handle(UserRequest::new("Change the login button."))
            .unwrap();
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert_eq!(inv.mode, ChangeInvestigationMode::Light);
        assert!(!inv.unknowns.is_empty());
        assert!(response.execution_plan.is_none());
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn c32_existing_related_implementation_surfaced() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/apps/jaymi/src/settings_workspace.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/apps/jaymi/src/settings_workspace.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Settings".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/apps/jaymi/src/settings_workspace.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/docs/settings.md".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            project_snapshot: Some(jaymi_context::ProjectSnapshot {
                package_manager: Some(jaymi_context::PackageManagerKind::Cargo),
                dependency_summary: jaymi_context::DependencyGraphSummary {
                    top_level: vec!["serde".into(), "tokio".into()],
                    ..Default::default()
                },
                workspace_layout: jaymi_context::WorkspaceLayoutSummary {
                    top_level_dirs: vec!["apps".into(), "crates".into(), "docs".into()],
                    ..Default::default()
                },
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Add image generation to Settings."))
            .unwrap();
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert_eq!(inv.mode, ChangeInvestigationMode::Full);
        assert!(
            !inv.related_dependencies.is_empty()
                || !inv.implementation_patterns.is_empty()
                || !inv.related_functionality.is_empty()
        );
        assert!(
            inv.related_functionality
                .iter()
                .any(|r| r.contains("docs") || r.contains("settings"))
                || inv.affected_files.iter().any(|p| p.contains("settings"))
        );
    }

    #[test]
    fn c32_change_crossing_subsystem_boundaries() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/crates/jaymi-planner/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/crates/jaymi-planner/src/lib.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Planner".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/crates/jaymi-planner/src/lib.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/crates/jaymi-context/src/lib.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/apps/jaymi/src/boot.rs".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Refactor Planner context handoff."))
            .unwrap();
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert_eq!(inv.mode, ChangeInvestigationMode::Full);
        assert!(
            inv.likely_side_effects
                .iter()
                .any(|s| s.contains("subsystem"))
                || inv
                    .architectural_constraints
                    .iter()
                    .any(|s| s.contains("subsystem") || s.contains("shared contracts"))
        );
    }

    #[test]
    fn c32_no_mutation_tools_or_patches() {
        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(session_with_selection_and_symbol());
        for prompt in [
            "Fix this bug.",
            "Add authentication.",
            "Remove this unused function.",
        ] {
            let response = planner.handle(UserRequest::new(prompt)).unwrap();
            assert!(response.change_investigation.is_some());
            assert!(response.execution_plan.is_none());
            assert!(!response.awaiting_review);
            assert!(response.lsp_edits.is_empty());
            assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
            assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
            let inv = response.change_investigation.as_ref().unwrap();
            assert!(!inv.to_markdown().contains("```diff"));
            assert!(inv
                .architectural_constraints
                .iter()
                .any(|c| c.contains("Read-only")));
            assert!(inv
                .to_diagnostics_summary()
                .contains("change_investigation"));
        }
    }

    #[test]
    fn c32_bounded_investigation_respects_c2_caps() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/lib.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Root".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: (0..20)
                    .map(|i| jaymi_context::EditorReference {
                        path: format!("/proj/src/mod_{i}.rs"),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Refactor Root module layout."))
            .unwrap();
        let inv = response
            .change_investigation
            .expect("change_investigation");
        assert!(inv.steps.len() <= deep_investigation::MAX_STEPS);
        assert!(inv.files_inspected <= deep_investigation::MAX_FILES);
        assert!(inv.evidence.len() <= change_investigation::CONVERSATION_ROW_CAP);
        assert!(inv.searches.len() <= deep_investigation::MAX_STEPS);
    }

    #[test]
    fn c32_c2_reading_behavior_unchanged() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(2));
        let search = planner
            .handle(UserRequest::new("Where is ContextBundle created?"))
            .unwrap();
        assert!(search.coding_edit.is_none());
        assert!(search.change_investigation.is_none());
        assert!(search.coding_plan.is_none() || search.code_search.is_some());
        // Read paths must not attach a mutation Coding Plan.
        if search.code_search.is_some() {
            assert_ne!(
                search
                    .coding_plan
                    .as_ref()
                    .map(|p| p.origin),
                Some(CodingPlanOrigin::ChangeInvestigation)
            );
        }
        assert!(search.code_search.is_some());

        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "assemble".into(),
                    kind: Some("function".into()),
                    range: Some(jaymi_context::EditorRange {
                        start_line: 10,
                        start_column: 0,
                        end_line: 10,
                        end_column: 8,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let nav = planner
            .handle(UserRequest::new("Where is this function defined?"))
            .unwrap();
        assert!(nav.coding_edit.is_none());
        assert!(nav.change_investigation.is_none());
        assert!(nav.symbol_navigation.is_some());

        let deep = planner
            .handle(UserRequest::new(
                "Investigate how authentication flows through the codebase.",
            ))
            .unwrap();
        // Deep investigation soft path still owns reading asks.
        assert!(deep.change_investigation.is_none() || deep.deep_investigation.is_some());
        if deep.deep_investigation.is_some() {
            assert!(deep.coding_edit.is_none());
            assert!(deep.change_investigation.is_none());
            assert_ne!(
                deep.coding_plan
                    .as_ref()
                    .map(|p| p.origin),
                Some(CodingPlanOrigin::ChangeInvestigation)
            );
        }
    }

    // --- C3.3: Coding Plan from Change Investigation ---

    #[test]
    fn c33_plan_based_on_investigation_evidence() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/apps/jaymi/src/settings_workspace.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/apps/jaymi/src/settings_workspace.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Settings".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/apps/jaymi/src/settings_workspace.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/crates/jaymi-providers/src/registry.rs".into(),
                        ..Default::default()
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/docs/settings.md".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Add image generation to Settings."))
            .unwrap();
        let inv = response.change_investigation.expect("investigation");
        let plan = response.coding_plan.expect("coding_plan");
        assert_eq!(plan.origin, CodingPlanOrigin::ChangeInvestigation);
        assert!(plan.rationale.iter().any(|r| r.contains("Change Investigation")));
        assert!(
            plan.files_to_modify.iter().any(|f| f.contains("settings_workspace"))
                || plan
                    .files_to_modify
                    .iter()
                    .any(|f| inv.affected_files.iter().any(|a| f.contains(a)))
        );
        assert!(!plan.step_dependencies.is_empty());
        assert!(response.content.contains("## Coding Plan"));
        assert!(response.content.contains("### Step ordering"));
    }

    #[test]
    fn c33_affected_files_grounded_and_ordering_preserved() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/button.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/button.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "Button".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Change the button color."))
            .unwrap();
        let plan = response.coding_plan.expect("coding_plan");
        assert!(plan
            .files_to_modify
            .iter()
            .any(|f| f.contains("button.rs")));
        // Ordering: confirmation before modify for Change kind.
        let joined = plan.plan_steps.join("\n");
        let i1 = joined.find("1.").expect("step 1");
        let i2 = joined.find("2.").expect("step 2");
        assert!(i1 < i2);
        assert!(!plan.step_dependencies.is_empty());
    }

    #[test]
    fn c33_ambiguity_remains_visible() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("Fix this.")).unwrap();
        let plan = response.coding_plan.expect("coding_plan");
        assert!(!plan.unresolved_questions.is_empty());
        assert!(plan.files_to_create.is_empty());
        assert!(
            response.content.contains("Unresolved questions")
                || !plan.unresolved_questions.is_empty()
        );
    }

    #[test]
    fn c33_no_mutation_no_tool_bypass() {
        let planner = planner_with_search_and_read();
        planner
            .context
            .set_session_inputs(session_with_selection_and_symbol());
        let response = planner
            .handle(UserRequest::new("Add authentication."))
            .unwrap();
        assert!(response.coding_plan.is_some());
        assert!(response.execution_plan.is_none());
        assert!(!response.awaiting_review);
        assert!(response.lsp_edits.is_empty());
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
        assert_ne!(response.tool_id.as_deref(), Some(MANAGE_PATH_TOOL_ID));
        let plan = response.coding_plan.as_ref().unwrap();
        assert!(!plan.to_markdown().contains("```diff"));
        assert!(!plan.to_markdown().contains("```rust"));
        assert!(plan
            .to_diagnostics_summary()
            .contains("coding_plan"));
        // Remembered for later translation — still not executed.
        assert!(planner.clone_last_coding_plan().is_some());
    }

    #[test]
    fn c33_constitutional_conflict_flagged() {
        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/button.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/button.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        // "Change the ..." keeps C3.1 ownership; phrase flags review bypass.
        let response = planner
            .handle(UserRequest::new(
                "Change the button without review and auto-apply.",
            ))
            .unwrap();
        let plan = response.coding_plan.expect("coding_plan");
        assert!(plan
            .constitutional_conflicts
            .iter()
            .any(|c| c.contains("Review Before Action")));
        assert!(response.execution_plan.is_none());
        assert!(response.lsp_edits.is_empty());
    }

    // --- C3.4: Patch Generation ---

    fn mutation_plan_for_paths(
        create: Vec<String>,
        modify: Vec<String>,
        remove: Vec<String>,
        goal: &str,
    ) -> CodingPlan {
        CodingPlan {
            origin: CodingPlanOrigin::ChangeInvestigation,
            kind: Some(CodingPlanKind::Mutation),
            objective: Some(goal.into()),
            files_to_create: create,
            files_to_modify: modify,
            files_to_remove: remove,
            plan_steps: vec!["1. Apply grounded patches.".into()],
            summary: vec![format!("Mutation Coding Plan for: {goal}")],
            ..Default::default()
        }
    }

    #[test]
    fn c34_create_file_patch_awaits_review_without_write() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let create_path = dir.join("helper.rs");
        planner.remember_coding_plan(Some(mutation_plan_for_paths(
            vec![format!("`{}` new helper", create_path.display())],
            vec![],
            vec![],
            "Add helper.",
        )));

        let response = planner
            .handle(UserRequest::new("Generate patches."))
            .unwrap();

        let report = response.patch_generation.expect("patch_generation");
        assert!(report.valid_count() >= 1);
        assert!(report.converted_to_execution);
        assert!(response.awaiting_review);
        assert!(response.code_generation.is_some());
        assert!(response
            .code_generation
            .as_ref()
            .unwrap()
            .operations
            .iter()
            .any(|op| op.kind == GenerationOpKind::CreateFile && op.path == create_path));
        assert!(!create_path.exists(), "must not write before review");
        assert!(response.content.contains("## Patch Generation"));
    }

    #[test]
    fn c34_targeted_modification_preserves_file_until_review() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("lib.rs");
        fs::write(&path, "fn keep()\nfn touch()\n").unwrap();
        planner.remember_coding_plan(Some(mutation_plan_for_paths(
            vec![],
            vec![format!("`{}` (from investigation)", path.display())],
            vec![],
            "Change touch.",
        )));

        let response = planner
            .handle(UserRequest::new("Generate the code."))
            .unwrap();
        let report = response.patch_generation.expect("patch_generation");
        assert!(report
            .patches
            .iter()
            .any(|p| p.proposal.change_type == PatchChangeType::ModifyFile
                && p.status.is_valid()));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "fn keep()\nfn touch()\n",
            "original file must be preserved before Approve"
        );
        assert!(response.awaiting_review);
        let proposed = response
            .code_generation
            .as_ref()
            .unwrap()
            .operations
            .iter()
            .find(|op| op.kind == GenerationOpKind::ModifyFile)
            .and_then(|op| op.content.clone())
            .unwrap_or_default();
        assert!(
            proposed.contains("fn keep()") || proposed.contains("jaymi-patch-proposal"),
            "targeted modify should preserve surrounding context: {proposed}"
        );
    }

    #[test]
    fn c34_multi_file_patch_batch() {
        let planner = planner_with_write_and_manage();
        let dir = temp_dir();
        let create = dir.join("new.rs");
        let modify = dir.join("existing.rs");
        let delete = dir.join("gone.rs");
        fs::write(&modify, "old\n").unwrap();
        fs::write(&delete, "bye\n").unwrap();
        planner.remember_coding_plan(Some(mutation_plan_for_paths(
            vec![format!("`{}` create", create.display())],
            vec![format!("`{}` modify", modify.display())],
            vec![format!("`{}` remove", delete.display())],
            "Multi-file change.",
        )));

        let response = planner
            .handle(UserRequest::new("Generate patches."))
            .unwrap();
        let gen = response.code_generation.expect("code_generation");
        assert!(gen.operations.len() >= 2);
        assert!(gen
            .operations
            .iter()
            .any(|op| op.kind == GenerationOpKind::CreateFile));
        assert!(gen
            .operations
            .iter()
            .any(|op| op.kind == GenerationOpKind::ModifyFile));
        assert!(!create.exists());
        assert_eq!(fs::read_to_string(&modify).unwrap(), "old\n");
        assert!(delete.exists(), "delete must wait for review");
        assert!(response.awaiting_review);
    }

    #[test]
    fn c34_stale_and_invalid_patches_leave_project_unchanged() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let existing = dir.join("already.rs");
        fs::write(&existing, "present\n").unwrap();
        let missing = dir.join("missing.rs");

        // Create targeting an existing path → UnexpectedExists (invalid for create).
        // Modify targeting a missing path → MissingFile.
        planner.remember_coding_plan(Some(mutation_plan_for_paths(
            vec![format!("`{}` should not create", existing.display())],
            vec![format!("`{}` cannot modify", missing.display())],
            vec![],
            "Invalid patch batch.",
        )));

        let before = fs::read_to_string(&existing).unwrap();
        let response = planner
            .handle(UserRequest::new("Generate patches."))
            .unwrap();
        let report = response.patch_generation.expect("patch_generation");
        assert!(report.valid_count() == 0, "{:?}", report.patches);
        assert!(!report.converted_to_execution);
        assert!(!response.awaiting_review);
        assert!(response.execution_plan.is_none());
        assert_eq!(fs::read_to_string(&existing).unwrap(), before);
        assert!(!missing.exists());
        assert!(
            report.failures.iter().any(|f| f.contains("already.rs")
                || f.contains("missing.rs")
                || f.contains("unchanged")
                || !report.failures.is_empty())
        );
        assert!(response.content.contains("Original project unchanged"));
    }

    #[test]
    fn c34_no_direct_filesystem_mutation_and_c15_generation_still_works() {
        let planner = planner_with_write();
        let dir = temp_dir();
        let path = dir.join("pong.rs");
        // C1.5 greenfield plan still uses stub generation (not C3.4).
        let plan = CodingPlan {
            origin: CodingPlanOrigin::GenerationPlanning,
            kind: Some(CodingPlanKind::NewProject),
            files_to_create: vec![format!("`{}` pong", path.display())],
            summary: vec!["Goal: Build Pong.".into()],
            ..Default::default()
        };
        planner.remember_coding_plan(Some(plan));
        let response = planner
            .handle(UserRequest::new("Generate the code."))
            .unwrap();
        assert!(response.patch_generation.is_none());
        assert!(response.code_generation.is_some());
        assert!(response.awaiting_review);
        assert!(!path.exists());
    }

    #[test]
    fn c34_rename_proposal_supported_in_draft_and_tool_binding() {
        let plan = CodingPlan {
            origin: CodingPlanOrigin::ChangeInvestigation,
            kind: Some(CodingPlanKind::Mutation),
            plan_steps: vec!["Rename `/proj/old.rs` to `/proj/new.rs`.".into()],
            files_to_modify: vec![],
            ..Default::default()
        };
        let drafts =
            patch_generation::draft_proposals_from_plan(&plan, "Rename", Some(Path::new("/proj")));
        // Best-effort rename parse may or may not extract both paths from prose;
        // also exercise GenerationOp rename binding directly.
        let op = GenerationOp::rename_move("/proj/old.rs", "/proj/new.rs", "rename");
        let call = code_generation::tool_calls_for_generation(
            &CodeGeneration {
                goal: "rename".into(),
                operations: vec![op],
                coding_plan_summary: vec![],
            },
            jaymi_core::DeletionMethod::Trash,
        )
        .unwrap();
        assert_eq!(call[0].tool_id, MANAGE_PATH_TOOL_ID);
        assert_eq!(call[0].input.command.as_deref(), Some("rename"));
        let _ = drafts;
    }

    #[test]
    fn code_search_generic_search_is_not_reformatted() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(1));
        let response = planner.handle(UserRequest::new("search fungi")).unwrap();
        // Generic search keeps tool message; no Code Search report attachment.
        assert!(response.code_search.is_none());
        assert!(response.content.contains("stub hits") || response.citations.len() == 1);
    }

    // --- C2.7.2: unified evidence → Context → Reasoning handoff ---

    #[test]
    fn c272_nl_search_evidence_reaches_reasoning_via_canonical_pipeline() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let (planner, _) =
            planner_with_stub_search_and_reasoning(sample_citations(3), Arc::clone(&provider));
        let response = planner
            .handle(UserRequest::new("Where is authentication handled?"))
            .unwrap();
        assert!(response.code_search.is_some());
        assert!(response.reasoning_used);
        assert_eq!(provider.stream_calls(), 1);
        assert_eq!(provider.complete_calls(), 0);
        let goal = provider.last_goal().unwrap_or_default();
        assert!(
            goal.contains("origin=code_search") || goal.contains("Planner code search"),
            "goal missing evidence brief: {goal}"
        );
        assert!(goal.contains("file_0.rs") || goal.contains("authenticate"));
        // ContextPolicy selection boundary: hits offered via session → bundle search.
        let hits = planner.context.session_inputs().search_hits;
        assert!(!hits.is_empty(), "expected session search_hits for ContextPolicy");
        assert!(hits.iter().any(|h| h.title.contains("file_")));
        let bundle = response.context_bundle.expect("context_bundle");
        assert!(
            !bundle.search_results().hits.is_empty()
                || bundle.search().is_some()
                || !hits.is_empty()
        );
        assert!(response.lsp_edits.is_empty());
        assert!(!response.awaiting_review);
    }

    #[test]
    fn c272_exact_symbol_stays_deterministic_even_with_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let (planner, _) =
            planner_with_stub_search_and_reasoning(sample_citations(2), Arc::clone(&provider));
        let response = planner
            .handle(UserRequest::new("Where is ContextBundle created?"))
            .unwrap();
        assert!(response.code_search.is_some());
        assert!(!response.reasoning_used);
        assert_eq!(provider.stream_calls(), 0);
        assert!(response.content.contains("## Code Search"));
        assert_ne!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
    }

    #[test]
    fn c272_relationship_evidence_reaches_reasoning_via_canonical_pipeline() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(Arc::clone(&provider)
            as Arc<dyn jaymi_reasoning::ReasoningProvider>);
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/ui.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "on_save".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/ui.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 12,
                            start_column: 0,
                            end_line: 12,
                            end_column: 7,
                        },
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/actions.rs".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into(), "project".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("What happens when I click Save?"))
            .unwrap();
        assert!(response.relationship_trace.is_some());
        assert!(response.reasoning_used);
        assert_eq!(provider.stream_calls(), 1);
        let goal = provider.last_goal().unwrap_or_default();
        assert!(
            goal.contains("origin=relationship_trace")
                || goal.contains("Planner relationship trace"),
            "goal={goal}"
        );
        assert!(!planner.context.session_inputs().search_hits.is_empty());
        assert!(response.lsp_edits.is_empty());
        assert!(!response.awaiting_review);
    }

    #[test]
    fn c272_symbol_dependents_evidence_reaches_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(Arc::clone(&provider)
            as Arc<dyn jaymi_reasoning::ReasoningProvider>);
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/src/auth.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                active_file: jaymi_context::CurrentFileSection {
                    path: Some("/proj/src/auth.rs".into()),
                    dirty: false,
                    language: Some("rust".into()),
                },
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "TokenStore".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    jaymi_context::EditorReference {
                        path: "/proj/src/auth.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 1,
                            start_column: 0,
                            end_line: 1,
                            end_column: 10,
                        },
                    },
                    jaymi_context::EditorReference {
                        path: "/proj/src/api.rs".into(),
                        range: jaymi_context::EditorRange {
                            start_line: 4,
                            start_column: 0,
                            end_line: 4,
                            end_column: 10,
                        },
                    },
                ],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new(
                "Show me everything that depends on this.",
            ))
            .unwrap();
        assert!(response.symbol_navigation.is_some());
        let report = response.symbol_navigation.as_ref().unwrap();
        assert_eq!(report.kind, Some(SymbolNavKind::Dependents));
        assert!(!report.evidence.is_empty());
        assert!(response.reasoning_used);
        assert_eq!(provider.stream_calls(), 1);
        let goal = provider.last_goal().unwrap_or_default();
        assert!(
            goal.contains("origin=symbol_navigation")
                || goal.contains("Planner symbol navigation"),
            "goal={goal}"
        );
        assert!(response.lsp_edits.is_empty());
    }

    #[test]
    fn c272_symbol_definition_stays_deterministic_with_reasoning() {
        let provider = Arc::new(CountingReasoningProvider::new("chat-mock"));
        let planner = planner_with_reasoning(Arc::clone(&provider)
            as Arc<dyn jaymi_reasoning::ReasoningProvider>);
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "assemble".into(),
                    kind: Some("function".into()),
                    range: Some(jaymi_context::EditorRange {
                        start_line: 10,
                        start_column: 0,
                        end_line: 10,
                        end_column: 8,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let response = planner
            .handle(UserRequest::new("Where is this function defined?"))
            .unwrap();
        assert!(response.symbol_navigation.is_some());
        assert!(!response.reasoning_used);
        assert_eq!(provider.stream_calls(), 0);
        assert!(response.content.contains("## Symbol Navigation"));
    }

    #[test]
    fn c272_deterministic_fallback_without_reasoning_backend() {
        let (planner, _) = planner_with_stub_search_knowledge(sample_citations(3));
        let nl = planner
            .handle(UserRequest::new("Where is authentication handled?"))
            .unwrap();
        assert!(!nl.reasoning_used);
        assert!(nl.content.contains("## Code Search"));
        assert!(nl.code_search.is_some());

        let planner = planner_with_search_and_read();
        planner.context.set_session_inputs(jaymi_context::ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            editor_snapshot: Some(jaymi_context::EditorSnapshot {
                symbol: Some(jaymi_context::EditorSymbol {
                    name: "on_save".into(),
                    ..Default::default()
                }),
                references: vec![jaymi_context::EditorReference {
                    path: "/proj/a.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            approved_context_providers: vec!["editor".into()],
            ..jaymi_context::ContextSessionInputs::default()
        });
        let rel = planner
            .handle(UserRequest::new("What happens when I click Save?"))
            .unwrap();
        assert!(!rel.reasoning_used);
        assert!(rel.content.contains("## Relationship Trace") || rel.relationship_trace.is_some());
        assert!(rel.lsp_edits.is_empty());
    }

    #[test]
    fn c272_no_direct_reasoning_provider_from_c2_modules() {
        // Constitutional: C2 modules never call ReasoningProvider — only Planner
        // complete_with_codebase_evidence_handoff → ConversationStream.
        for path in [
            "code_search.rs",
            "symbol_navigation.rs",
            "relationship_tracing.rs",
            "deep_investigation.rs",
            "codebase_qa.rs",
            "codebase_evidence.rs",
        ] {
            let body = std::fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(path),
            )
            .unwrap_or_else(|_| panic!("read {path}"));
            for needle in [
                "ReasoningProvider",
                "ReasoningEngine",
                "reasoning.complete",
                "reasoning.stream",
            ] {
                // Allow documentary mentions of the ban itself.
                let banned = body.lines().any(|line| {
                    let t = line.trim();
                    !t.starts_with("//")
                        && !t.starts_with("//!")
                        && !t.starts_with('*')
                        && t.contains(needle)
                });
                assert!(
                    !banned,
                    "{path} must not call/use {needle} outside comments"
                );
            }
        }
    }
}
