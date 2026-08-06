//! First-class Execution Plans — every meaningful action becomes a plan.
//!
//! Pipeline ownership:
//!
//! ```text
//! User Request → Planner → Context → Execution Plan → Review → Tool → Summary
//! ```
//!
//! The Planner alone creates plans. Tools never generate plans. Providers
//! never know plans exist.
//!
//! Plan *content* is immutable after creation. Only [`ExecutionStatus`] may
//! progress through the controlled lifecycle API.

use std::sync::atomic::{AtomicU64, Ordering};

use jaymi_capabilities::Capability;
use jaymi_core::IntentId;
use jaymi_permissions::{PermissionAction, PermissionCategory};
use serde::{Deserialize, Serialize};

static PLAN_SEQ: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for an [`ExecutionPlan`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionPlanId(String);

impl ExecutionPlanId {
    /// Allocate a new unique plan id.
    pub fn new() -> Self {
        let n = PLAN_SEQ.fetch_add(1, Ordering::Relaxed);
        Self(format!("exec-plan-{n}"))
    }

    /// Borrow the underlying id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstruct an id from a previously allocated value (timeout / diagnostics).
    pub fn from_existing(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl Default for ExecutionPlanId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ExecutionPlanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle status of an execution plan.
///
/// Valid transitions:
/// - Draft → Ready
/// - Ready → AwaitingReview | Approved | Cancelled
/// - AwaitingReview → Approved | Cancelled
/// - Approved → Executing | Cancelled
/// - Executing → Completed | Failed | Cancelled
///
/// Completed, Cancelled, and Failed are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Plan created; content frozen; not yet ready to gate.
    Draft,
    /// Content complete; ready for review gate or auto-approval.
    Ready,
    /// Review is required before tools may run.
    AwaitingReview,
    /// Review passed (or was not required); tools may run.
    Approved,
    /// Tools are running.
    Executing,
    /// Tools finished successfully.
    Completed,
    /// Plan was cancelled before or during execution.
    Cancelled,
    /// Tools failed during execution.
    Failed,
}

impl ExecutionStatus {
    /// Stable label for diagnostics and serialization keys.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::AwaitingReview => "awaiting_review",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    /// True when no further transitions are allowed.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }

    /// True when tools may begin executing from this status.
    pub fn may_execute(self) -> bool {
        matches!(self, Self::Approved)
    }
}

impl std::fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Estimated risk of carrying out the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimatedRisk {
    /// Read-only or otherwise low-impact work.
    Low,
    /// Mutating but typically recoverable work.
    Medium,
    /// Destructive, irreversible, or high-blast-radius work.
    High,
}

impl EstimatedRisk {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Derive estimated risk from a Tool's risk classification.
    pub fn from_tool_risk(risk: jaymi_tools::ToolRisk) -> Self {
        match risk {
            jaymi_tools::ToolRisk::Safe | jaymi_tools::ToolRisk::Workspace => Self::Low,
            jaymi_tools::ToolRisk::Modify => Self::Medium,
            jaymi_tools::ToolRisk::Destructive | jaymi_tools::ToolRisk::External => Self::High,
        }
    }

    /// Derive risk from the permission action being requested.
    #[deprecated(note = "use from_tool_risk — review derives from ToolRisk")]
    pub fn from_permission_action(action: PermissionAction) -> Self {
        match action {
            PermissionAction::Read | PermissionAction::Import => Self::Low,
            PermissionAction::Write | PermissionAction::Network | PermissionAction::Export => {
                Self::Medium
            }
            PermissionAction::Execute | PermissionAction::Delete => Self::High,
        }
    }
}

/// How easily the planned effects can be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimatedReversibility {
    /// Effects leave no lasting change (pure reads).
    FullyReversible,
    /// Effects can usually be undone (writes that overwrite known content).
    PartiallyReversible,
    /// Effects are difficult or impossible to undo (delete, execute).
    Irreversible,
}

impl EstimatedReversibility {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FullyReversible => "fully_reversible",
            Self::PartiallyReversible => "partially_reversible",
            Self::Irreversible => "irreversible",
        }
    }

    /// Derive reversibility from a Tool's risk classification.
    pub fn from_tool_risk(risk: jaymi_tools::ToolRisk) -> Self {
        match risk {
            jaymi_tools::ToolRisk::Safe | jaymi_tools::ToolRisk::Workspace => {
                Self::FullyReversible
            }
            jaymi_tools::ToolRisk::Modify => Self::PartiallyReversible,
            jaymi_tools::ToolRisk::Destructive | jaymi_tools::ToolRisk::External => {
                Self::Irreversible
            }
        }
    }

    /// Derive reversibility from the permission action being requested.
    #[deprecated(note = "use from_tool_risk")]
    pub fn from_permission_action(action: PermissionAction) -> Self {
        match action {
            PermissionAction::Read | PermissionAction::Import => Self::FullyReversible,
            PermissionAction::Write | PermissionAction::Export | PermissionAction::Network => {
                Self::PartiallyReversible
            }
            PermissionAction::Execute | PermissionAction::Delete => Self::Irreversible,
        }
    }
}

/// Whether human review must gate tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRequirement {
    /// Tools may proceed once the plan is Ready / Approved without review.
    NotRequired,
    /// Plan must reach AwaitingReview and receive explicit approval.
    Required,
}

impl ReviewRequirement {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "not_required",
            Self::Required => "required",
        }
    }
}

/// One ordered step inside an execution plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionStep {
    /// 1-based order within the plan.
    pub order: usize,
    /// Short description of what this step does.
    pub description: String,
    /// Tool that would perform this step, when known.
    pub tool_id: Option<String>,
    /// Resource path or identifier affected by this step, when known.
    pub resource: Option<String>,
}

/// Permission label recorded on a plan (Planner-owned; not a Permission Engine type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPermissionRequirement {
    /// Permission category label (`filesystem`, `terminal`, …).
    pub category: String,
    /// Action label (`read`, `write`, …).
    pub action: String,
}

impl PlanPermissionRequirement {
    /// Build from Permission Engine enums.
    pub fn from_enums(category: PermissionCategory, action: PermissionAction) -> Self {
        Self {
            category: permission_category_label(category).to_string(),
            action: permission_action_label(action).to_string(),
        }
    }

    /// Compact `category:action` label.
    pub fn label(&self) -> String {
        format!("{}:{}", self.category, self.action)
    }
}

/// Lineage metadata frozen into an Execution Plan at creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanLineage {
    /// Parent plan when this plan is a revision produced by Modify.
    pub parent_plan_id: Option<ExecutionPlanId>,
    /// 1-based revision number (`1` = original proposal).
    pub revision: u32,
    /// User note that produced this revision, when any.
    pub modification_note: Option<String>,
    /// Human-readable changes vs the parent plan.
    pub revision_changes: Vec<String>,
}

impl PlanLineage {
    /// Root lineage for a newly proposed plan.
    pub fn root() -> Self {
        Self {
            parent_plan_id: None,
            revision: 1,
            modification_note: None,
            revision_changes: Vec::new(),
        }
    }

    /// Child lineage for a plan revised from `parent`.
    pub fn revision_of(
        parent: &ExecutionPlan,
        note: Option<String>,
        changes: Vec<String>,
    ) -> Self {
        Self {
            parent_plan_id: Some(parent.id().clone()),
            revision: parent.revision().saturating_add(1).max(2),
            modification_note: note,
            revision_changes: changes,
        }
    }
}

/// Inputs used once at plan creation. Content cannot change afterward.
#[derive(Debug, Clone)]
pub struct ExecutionPlanParams {
    /// Original user / Planner request description.
    pub originating_request: String,
    /// Canonical Planner intent.
    pub planner_intent: IntentId,
    /// Capability selected for the work.
    pub capability: Capability,
    /// Tools proposed for execution (selection order).
    pub proposed_tools: Vec<String>,
    /// Ordered execution steps.
    pub steps: Vec<ExecutionStep>,
    /// Estimated risk.
    pub estimated_risk: EstimatedRisk,
    /// Resources the plan would touch.
    pub affected_resources: Vec<String>,
    /// Permissions required before execution.
    pub permissions_required: Vec<PlanPermissionRequirement>,
    /// Whether review gates execution.
    pub review_requirement: ReviewRequirement,
    /// Estimated reversibility of effects.
    pub estimated_reversibility: EstimatedReversibility,
    /// What successful execution is expected to produce.
    pub expected_outputs: Vec<String>,
    /// Revision lineage (immutable once created).
    pub lineage: PlanLineage,
}

/// First-class, Planner-owned execution plan.
///
/// Content fields are private and have getters only — immutable after
/// [`ExecutionPlan::create`]. Status progresses exclusively through
/// transition methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    id: ExecutionPlanId,
    originating_request: String,
    planner_intent: IntentId,
    capability: Capability,
    proposed_tools: Vec<String>,
    steps: Vec<ExecutionStep>,
    estimated_risk: EstimatedRisk,
    affected_resources: Vec<String>,
    permissions_required: Vec<PlanPermissionRequirement>,
    review_requirement: ReviewRequirement,
    estimated_reversibility: EstimatedReversibility,
    expected_outputs: Vec<String>,
    lineage: PlanLineage,
    status: ExecutionStatus,
}

impl ExecutionPlan {
    /// Create a new plan in [`ExecutionStatus::Draft`]. Content is frozen.
    pub fn create(params: ExecutionPlanParams) -> Self {
        let mut lineage = params.lineage;
        if lineage.revision == 0 {
            lineage.revision = 1;
        }
        Self {
            id: ExecutionPlanId::new(),
            originating_request: params.originating_request,
            planner_intent: params.planner_intent,
            capability: params.capability,
            proposed_tools: params.proposed_tools,
            steps: params.steps,
            estimated_risk: params.estimated_risk,
            affected_resources: params.affected_resources,
            permissions_required: params.permissions_required,
            review_requirement: params.review_requirement,
            estimated_reversibility: params.estimated_reversibility,
            expected_outputs: params.expected_outputs,
            lineage,
            status: ExecutionStatus::Draft,
        }
    }

    /// Unique plan id.
    pub fn id(&self) -> &ExecutionPlanId {
        &self.id
    }

    /// Originating request text.
    pub fn originating_request(&self) -> &str {
        &self.originating_request
    }

    /// Planner intent that produced this plan.
    pub fn planner_intent(&self) -> IntentId {
        self.planner_intent
    }

    /// Capability selected for the work.
    pub fn capability(&self) -> Capability {
        self.capability
    }

    /// Proposed tool ids.
    pub fn proposed_tools(&self) -> &[String] {
        &self.proposed_tools
    }

    /// Primary (first) proposed tool id, when any.
    pub fn primary_tool_id(&self) -> Option<&str> {
        self.proposed_tools.first().map(String::as_str)
    }

    /// Ordered execution steps.
    pub fn steps(&self) -> &[ExecutionStep] {
        &self.steps
    }

    /// Estimated risk.
    pub fn estimated_risk(&self) -> EstimatedRisk {
        self.estimated_risk
    }

    /// Affected resources.
    pub fn affected_resources(&self) -> &[String] {
        &self.affected_resources
    }

    /// Permissions required.
    pub fn permissions_required(&self) -> &[PlanPermissionRequirement] {
        &self.permissions_required
    }

    /// Review requirement.
    pub fn review_requirement(&self) -> ReviewRequirement {
        self.review_requirement
    }

    /// Estimated reversibility.
    pub fn estimated_reversibility(&self) -> EstimatedReversibility {
        self.estimated_reversibility
    }

    /// Expected outputs.
    pub fn expected_outputs(&self) -> &[String] {
        &self.expected_outputs
    }

    /// Full lineage metadata.
    pub fn lineage(&self) -> &PlanLineage {
        &self.lineage
    }

    /// 1-based revision number.
    pub fn revision(&self) -> u32 {
        self.lineage.revision.max(1)
    }

    /// Parent plan id when this is a Modify revision.
    pub fn parent_plan_id(&self) -> Option<&ExecutionPlanId> {
        self.lineage.parent_plan_id.as_ref()
    }

    /// Human-readable changes vs the parent plan.
    pub fn revision_changes(&self) -> &[String] {
        &self.lineage.revision_changes
    }

    /// User note that produced this revision.
    pub fn modification_note(&self) -> Option<&str> {
        self.lineage.modification_note.as_deref()
    }

    /// Current execution status.
    pub fn status(&self) -> ExecutionStatus {
        self.status
    }

    /// Draft → Ready.
    pub fn mark_ready(&mut self) -> Result<(), PlanTransitionError> {
        self.transition(ExecutionStatus::Ready)
    }

    /// Ready → AwaitingReview (only when review is required).
    pub fn mark_awaiting_review(&mut self) -> Result<(), PlanTransitionError> {
        if self.review_requirement != ReviewRequirement::Required {
            return Err(PlanTransitionError::ReviewNotRequired);
        }
        self.transition(ExecutionStatus::AwaitingReview)
    }

    /// Ready (no review) or AwaitingReview → Approved.
    pub fn approve(&mut self) -> Result<(), PlanTransitionError> {
        match self.status {
            ExecutionStatus::Ready if self.review_requirement == ReviewRequirement::NotRequired => {
                self.status = ExecutionStatus::Approved;
                Ok(())
            }
            ExecutionStatus::AwaitingReview => {
                self.status = ExecutionStatus::Approved;
                Ok(())
            }
            ExecutionStatus::Approved => Ok(()),
            other => Err(PlanTransitionError::Invalid {
                from: other,
                to: ExecutionStatus::Approved,
            }),
        }
    }

    /// Approved → Executing.
    pub fn mark_executing(&mut self) -> Result<(), PlanTransitionError> {
        self.transition(ExecutionStatus::Executing)
    }

    /// Executing → Completed.
    pub fn mark_completed(&mut self) -> Result<(), PlanTransitionError> {
        self.transition(ExecutionStatus::Completed)
    }

    /// Executing → Failed.
    pub fn mark_failed(&mut self) -> Result<(), PlanTransitionError> {
        self.transition(ExecutionStatus::Failed)
    }

    /// Cancel from a non-terminal status.
    pub fn cancel(&mut self) -> Result<(), PlanTransitionError> {
        if self.status.is_terminal() {
            return Err(PlanTransitionError::Terminal(self.status));
        }
        self.status = ExecutionStatus::Cancelled;
        Ok(())
    }

    /// Advance status when the edge is allowed by the lifecycle.
    fn transition(&mut self, to: ExecutionStatus) -> Result<(), PlanTransitionError> {
        let from = self.status;
        let allowed = matches!(
            (from, to),
            (ExecutionStatus::Draft, ExecutionStatus::Ready)
                | (ExecutionStatus::Ready, ExecutionStatus::AwaitingReview)
                | (ExecutionStatus::Ready, ExecutionStatus::Approved)
                | (ExecutionStatus::Ready, ExecutionStatus::Cancelled)
                | (ExecutionStatus::AwaitingReview, ExecutionStatus::Approved)
                | (ExecutionStatus::AwaitingReview, ExecutionStatus::Cancelled)
                | (ExecutionStatus::Approved, ExecutionStatus::Executing)
                | (ExecutionStatus::Approved, ExecutionStatus::Cancelled)
                | (ExecutionStatus::Executing, ExecutionStatus::Completed)
                | (ExecutionStatus::Executing, ExecutionStatus::Failed)
                | (ExecutionStatus::Executing, ExecutionStatus::Cancelled)
        );
        if allowed {
            self.status = to;
            Ok(())
        } else {
            Err(PlanTransitionError::Invalid { from, to })
        }
    }

    /// Short diagnostic summary.
    pub fn summary(&self) -> String {
        format!(
            "execution plan {} · intent={} · capability={} · tools=[{}] · risk={} · review={} · status={}",
            self.id,
            self.planner_intent.as_str(),
            self.capability.id(),
            self.proposed_tools.join(","),
            self.estimated_risk.as_str(),
            self.review_requirement.as_str(),
            self.status.as_str()
        )
    }
}

/// Error when a lifecycle transition is illegal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanTransitionError {
    /// Transition edge is not in the lifecycle.
    Invalid {
        /// Current status.
        from: ExecutionStatus,
        /// Requested status.
        to: ExecutionStatus,
    },
    /// Plan is already terminal.
    Terminal(ExecutionStatus),
    /// AwaitingReview was requested but review is not required.
    ReviewNotRequired,
}

impl std::fmt::Display for PlanTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid { from, to } => {
                write!(f, "invalid execution plan transition {from} → {to}")
            }
            Self::Terminal(status) => {
                write!(f, "execution plan is terminal ({status})")
            }
            Self::ReviewNotRequired => {
                write!(f, "execution plan does not require review")
            }
        }
    }
}

impl std::error::Error for PlanTransitionError {}

/// Outcome recorded after a plan finishes (or is blocked/cancelled).
///
/// The Planner creates summaries. Tools contribute structured metadata via
/// [`jaymi_tools::ToolExecutionMetadata`]. Summaries surface in conversation
/// and can be stored for memory retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Plan that produced this summary.
    pub plan_id: ExecutionPlanId,
    /// Final status at summary time.
    pub status: ExecutionStatus,
    /// Goal / originating user request.
    pub goal: String,
    /// Concrete actions that ran (from tools + plan steps).
    pub actions_performed: Vec<String>,
    /// Resources touched or changed.
    pub resources_changed: Vec<String>,
    /// Files created, overwritten, renamed, or deleted.
    pub files_edited: Vec<String>,
    /// Wall-clock duration for the gated execution (milliseconds).
    pub duration_ms: u64,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
    /// Error messages (failed / cancelled / denied).
    pub errors: Vec<String>,
    /// Suggested next steps for the user or Planner.
    pub next_suggested_actions: Vec<String>,
    /// Tools that were invoked (may be empty when blocked/cancelled).
    pub tools_executed: Vec<String>,
    /// Human-readable outcome lines.
    pub outputs: Vec<String>,
    /// True when some work succeeded but not the full expected outcome.
    pub partial: bool,
    /// Convenience: first error, when any (diagnostics / older call sites).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ExecutionSummary {
    /// Build a gated / cancelled / failed summary without tool metadata.
    pub fn from_plan(
        plan: &ExecutionPlan,
        tools_executed: Vec<String>,
        outputs: Vec<String>,
        error: Option<String>,
    ) -> Self {
        let errors: Vec<String> = error.into_iter().collect();
        Self {
            plan_id: plan.id().clone(),
            status: plan.status(),
            goal: plan.originating_request().to_string(),
            actions_performed: Vec::new(),
            resources_changed: plan.affected_resources().to_vec(),
            files_edited: Vec::new(),
            duration_ms: 0,
            warnings: Vec::new(),
            error: errors.first().cloned(),
            errors,
            next_suggested_actions: default_next_actions(plan.status(), false),
            tools_executed,
            outputs,
            partial: false,
        }
    }

    /// Build a summary after a tool invocation using tool metadata + duration.
    pub fn from_tool_result(
        plan: &ExecutionPlan,
        tool_id: impl Into<String>,
        output: &jaymi_tools::ToolOutput,
        duration_ms: u64,
    ) -> Self {
        let tool_id = tool_id.into();
        let meta = &output.metadata;
        let duration_ms = meta.duration_ms.unwrap_or(duration_ms);
        let mut actions = meta.actions_performed.clone();
        if actions.is_empty() {
            for step in plan.steps() {
                actions.push(step.description.clone());
            }
        }
        let mut resources = meta.resources_changed.clone();
        if resources.is_empty() {
            resources = plan.affected_resources().to_vec();
        }
        let mut files = meta.files_edited.clone();
        if files.is_empty() {
            if let Some(path) = &output.listed_path {
                if plan.capability() == Capability::FileManagement && output.success {
                    files.push(path.display().to_string());
                }
            }
        }
        let mut errors = Vec::new();
        if !output.success {
            if let Some(message) = &output.message {
                errors.push(message.clone());
            } else {
                errors.push(format!("tool '{tool_id}' reported failure"));
            }
        }
        let mut outputs: Vec<String> = output.message.clone().into_iter().collect();
        outputs.extend(plan.expected_outputs().iter().cloned());

        let mut next = meta.next_suggested_actions.clone();
        if next.is_empty() {
            next = default_next_actions(plan.status(), meta.partial);
        }

        Self {
            plan_id: plan.id().clone(),
            status: plan.status(),
            goal: plan.originating_request().to_string(),
            actions_performed: actions,
            resources_changed: resources,
            files_edited: files,
            duration_ms,
            warnings: meta.warnings.clone(),
            error: errors.first().cloned(),
            errors,
            next_suggested_actions: next,
            tools_executed: vec![tool_id],
            outputs,
            partial: meta.partial,
        }
    }

    /// Build a cancelled summary (review cancel / timeout / invalidate).
    pub fn cancelled(plan: &ExecutionPlan, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let mut summary = Self::from_plan(plan, Vec::new(), Vec::new(), Some(reason.clone()));
        summary.status = ExecutionStatus::Cancelled;
        summary.next_suggested_actions = vec![
            "Revise the request and try again".into(),
            "Ask Jaymi to explain what would have changed".into(),
        ];
        summary.errors = vec![reason.clone()];
        summary.error = Some(reason);
        summary
    }

    /// True when this summary should appear in the conversation transcript.
    pub fn should_surface_in_conversation(&self) -> bool {
        matches!(
            self.status,
            ExecutionStatus::Completed
                | ExecutionStatus::Failed
                | ExecutionStatus::Cancelled
        ) || self.partial
            || (!self.errors.is_empty()
                && !matches!(self.status, ExecutionStatus::AwaitingReview))
    }

    /// Conversation-facing render (natural language sections).
    pub fn render_conversation(&self) -> String {
        let mut lines = Vec::new();
        let headline = if self.partial {
            "Execution summary (partial)"
        } else {
            match self.status {
                ExecutionStatus::Completed => "Execution summary",
                ExecutionStatus::Failed => "Execution summary (failed)",
                ExecutionStatus::Cancelled => "Execution summary (cancelled)",
                _ => "Execution summary",
            }
        };
        lines.push(headline.to_string());
        lines.push(format!("Goal: {}", self.goal));
        if !self.actions_performed.is_empty() {
            lines.push("Actions performed:".into());
            for action in &self.actions_performed {
                lines.push(format!("• {action}"));
            }
        } else {
            lines.push("Actions performed: (none)".into());
        }
        if !self.resources_changed.is_empty() {
            lines.push(format!(
                "Resources changed: {}",
                self.resources_changed.join(", ")
            ));
        }
        if !self.files_edited.is_empty() {
            lines.push(format!("Files edited: {}", self.files_edited.join(", ")));
        }
        lines.push(format!("Duration: {} ms", self.duration_ms));
        if !self.warnings.is_empty() {
            lines.push("Warnings:".into());
            for warning in &self.warnings {
                lines.push(format!("• {warning}"));
            }
        }
        if !self.errors.is_empty() {
            lines.push("Errors:".into());
            for error in &self.errors {
                lines.push(format!("• {error}"));
            }
        }
        if !self.next_suggested_actions.is_empty() {
            lines.push("Next suggested actions:".into());
            for action in &self.next_suggested_actions {
                lines.push(format!("• {action}"));
            }
        }
        lines.join("\n")
    }

    /// Content blob suitable for Memory Engine storage.
    pub fn memory_content(&self) -> String {
        self.render_conversation()
    }

    /// Short summary line for Memory `summary` field.
    pub fn memory_summary_line(&self) -> String {
        let outcome = if self.partial {
            "partial"
        } else {
            self.status.as_str()
        };
        format!(
            "Execution {outcome}: {} ({})",
            truncate_chars(&self.goal, 80),
            self.plan_id
        )
    }

    /// JSON metadata for Memory retrieval filters.
    pub fn memory_metadata_json(&self) -> String {
        format!(
            r#"{{"plan_id":"{}","status":"{}","partial":{},"duration_ms":{},"tools":[{}],"files_edited":[{}]}}"#,
            escape_json(self.plan_id.as_str()),
            self.status.as_str(),
            self.partial,
            self.duration_ms,
            self.tools_executed
                .iter()
                .map(|tool| format!("\"{}\"", escape_json(tool)))
                .collect::<Vec<_>>()
                .join(","),
            self.files_edited
                .iter()
                .map(|path| format!("\"{}\"", escape_json(path)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    /// Short diagnostic line.
    pub fn summary(&self) -> String {
        format!(
            "execution summary {} · status={} · partial={} · tools=[{}] · duration_ms={} · outputs={}",
            self.plan_id,
            self.status.as_str(),
            self.partial,
            self.tools_executed.join(","),
            self.duration_ms,
            self.outputs.len()
        )
    }
}

fn default_next_actions(status: ExecutionStatus, partial: bool) -> Vec<String> {
    if partial {
        return vec![
            "Review what completed and retry the remainder".into(),
            "Ask Jaymi to continue from the partial result".into(),
        ];
    }
    match status {
        ExecutionStatus::Completed => vec![
            "Review the result".into(),
            "Ask a follow-up question".into(),
        ],
        ExecutionStatus::Failed => vec![
            "Inspect the error and retry".into(),
            "Ask Jaymi for an alternative approach".into(),
        ],
        ExecutionStatus::Cancelled => vec![
            "Revise the request and try again".into(),
        ],
        _ => Vec::new(),
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let shortened: String = value.chars().take(max).collect();
        format!("{shortened}…")
    }
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn permission_category_label(category: PermissionCategory) -> &'static str {
    match category {
        PermissionCategory::Filesystem => "filesystem",
        PermissionCategory::Terminal => "terminal",
        PermissionCategory::Internet => "internet",
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

/// IntentId needs serde for plan serialization.
mod intent_serde {
    // IntentId is defined in jaymi-core; we rely on remote impl below.
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::IntentId;

    fn sample_params() -> ExecutionPlanParams {
        ExecutionPlanParams {
            originating_request: "List /tmp".into(),
            planner_intent: IntentId::ListDirectory,
            capability: Capability::Search,
            proposed_tools: vec!["search_files".into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "List directory entries".into(),
                tool_id: Some("search_files".into()),
                resource: Some("/tmp".into()),
            }],
            estimated_risk: EstimatedRisk::Low,
            affected_resources: vec!["/tmp".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "read".into(),
            }],
            review_requirement: ReviewRequirement::NotRequired,
            estimated_reversibility: EstimatedReversibility::FullyReversible,
            expected_outputs: vec!["directory listing".into()],
        lineage: Default::default(),
        }
    }

    #[test]
    fn plan_creation() {
        let plan = ExecutionPlan::create(sample_params());
        assert_eq!(plan.status(), ExecutionStatus::Draft);
        assert_eq!(plan.planner_intent(), IntentId::ListDirectory);
        assert_eq!(plan.capability(), Capability::Search);
        assert_eq!(plan.proposed_tools(), &["search_files".to_string()]);
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(plan.estimated_risk(), EstimatedRisk::Low);
        assert_eq!(plan.affected_resources(), &["/tmp".to_string()]);
        assert_eq!(plan.permissions_required().len(), 1);
        assert_eq!(plan.review_requirement(), ReviewRequirement::NotRequired);
        assert_eq!(
            plan.estimated_reversibility(),
            EstimatedReversibility::FullyReversible
        );
        assert_eq!(plan.expected_outputs(), &["directory listing".to_string()]);
        assert!(plan.id().as_str().starts_with("exec-plan-"));
    }

    #[test]
    fn lifecycle_transitions() {
        let mut plan = ExecutionPlan::create(sample_params());
        assert!(plan.mark_ready().is_ok());
        assert_eq!(plan.status(), ExecutionStatus::Ready);
        assert!(plan.approve().is_ok());
        assert_eq!(plan.status(), ExecutionStatus::Approved);
        assert!(plan.mark_executing().is_ok());
        assert!(plan.mark_completed().is_ok());
        assert_eq!(plan.status(), ExecutionStatus::Completed);
        assert!(plan.cancel().is_err());
        assert!(plan.mark_ready().is_err());
    }

    #[test]
    fn lifecycle_review_path() {
        let mut params = sample_params();
        params.review_requirement = ReviewRequirement::Required;
        params.estimated_risk = EstimatedRisk::High;
        let mut plan = ExecutionPlan::create(params);
        plan.mark_ready().unwrap();
        assert!(plan.approve().is_err());
        plan.mark_awaiting_review().unwrap();
        assert_eq!(plan.status(), ExecutionStatus::AwaitingReview);
        plan.approve().unwrap();
        plan.mark_executing().unwrap();
        plan.mark_failed().unwrap();
        assert_eq!(plan.status(), ExecutionStatus::Failed);
    }

    #[test]
    fn serialization() {
        let mut plan = ExecutionPlan::create(sample_params());
        plan.mark_ready().unwrap();
        plan.approve().unwrap();
        let json = serde_json::to_string(&plan).expect("serialize");
        let restored: ExecutionPlan = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, plan);
        assert_eq!(restored.status(), ExecutionStatus::Approved);
        assert_eq!(restored.originating_request(), "List /tmp");
    }

    #[test]
    fn immutability() {
        let plan = ExecutionPlan::create(sample_params());
        let id = plan.id().clone();
        let request = plan.originating_request().to_string();
        let intent = plan.planner_intent();
        let capability = plan.capability();
        let tools = plan.proposed_tools().to_vec();
        let steps = plan.steps().to_vec();
        let risk = plan.estimated_risk();
        let resources = plan.affected_resources().to_vec();
        let perms = plan.permissions_required().to_vec();
        let review = plan.review_requirement();
        let reversibility = plan.estimated_reversibility();
        let outputs = plan.expected_outputs().to_vec();

        let mut plan = plan;
        plan.mark_ready().unwrap();
        plan.approve().unwrap();
        plan.mark_executing().unwrap();
        plan.mark_completed().unwrap();

        // Status may change; every content field must remain identical.
        assert_eq!(plan.id(), &id);
        assert_eq!(plan.originating_request(), request);
        assert_eq!(plan.planner_intent(), intent);
        assert_eq!(plan.capability(), capability);
        assert_eq!(plan.proposed_tools(), tools.as_slice());
        assert_eq!(plan.steps(), steps.as_slice());
        assert_eq!(plan.estimated_risk(), risk);
        assert_eq!(plan.affected_resources(), resources.as_slice());
        assert_eq!(plan.permissions_required(), perms.as_slice());
        assert_eq!(plan.review_requirement(), review);
        assert_eq!(plan.estimated_reversibility(), reversibility);
        assert_eq!(plan.expected_outputs(), outputs.as_slice());
        assert_eq!(plan.status(), ExecutionStatus::Completed);
    }

    #[test]
    fn summary_render_includes_required_sections() {
        let mut plan = ExecutionPlan::create(sample_params());
        plan.mark_ready().unwrap();
        plan.approve().unwrap();
        plan.mark_executing().unwrap();
        plan.mark_completed().unwrap();
        let summary = ExecutionSummary::from_plan(
            &plan,
            vec!["search_files".into()],
            vec!["directory listing".into()],
            None,
        );
        let text = summary.render_conversation();
        assert!(text.contains("Goal:"));
        assert!(text.contains("Actions performed"));
        assert!(text.contains("Duration:"));
        assert!(text.contains("Next suggested actions"));
        assert!(summary.should_surface_in_conversation());
    }
}
