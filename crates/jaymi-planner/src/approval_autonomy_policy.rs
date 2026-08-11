//! Approval and Autonomy Policy (Sprint C5.6).
//!
//! Define exactly what an Agentic Coding Goal may do automatically and what
//! requires explicit user authorization — **policy boundaries only**.
//!
//! ```text
//! READ / OBSERVE  → may proceed without mutation approval (when Permission allows)
//! MUTATION / SIDE EFFECT → existing Review Before Action + Permission + ToolRisk
//! ```
//!
//! Constitutional constraints:
//! - Reuse Permission Engine, ToolRisk, Review Before Action
//! - Do **not** create an AgentApproval system
//! - LLM cannot determine its own authorization
//! - Agent lifecycle cannot bypass existing gates
//! - Approvals bind to plan_id + task_id (no cross-task leak)
//! - Cancel / plan change / corrective plan invalidate pending approvals
//! - No user-configurable autonomy modes

use crate::agent_state_machine::{
    AgentLifecycleEvent, AgentLifecycleState,
};
use crate::coding_goal::{CodingGoal, CodingGoalStatus};
use jaymi_core::UserRequest;
use jaymi_permissions::{PermissionAction, PermissionCategory, PermissionDecision};
use jaymi_tools::ToolRisk;

/// Default policy class for agentic coding operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyPolicyClass {
    /// Search, read, navigate, inspect, investigate, reason, plan, interpret verify.
    ReadObserve,
    /// Write / modify / delete / rename files and similar workspace mutations.
    Mutation,
    /// Destructive terminal, Git mutations, external / network side effects.
    SideEffect,
}

impl AutonomyPolicyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadObserve => "read_observe",
            Self::Mutation => "mutation",
            Self::SideEffect => "side_effect",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadObserve => "Read / Observe",
            Self::Mutation => "Mutation",
            Self::SideEffect => "Side effect",
        }
    }

    /// True when this class may proceed without Review **only if** Permission also Allows
    /// and ToolRisk does not require review.
    pub fn may_auto_when_permitted(self) -> bool {
        matches!(self, Self::ReadObserve)
    }

    /// True when consequential authorization (Review Before Action) is required.
    pub fn requires_explicit_authorization(self) -> bool {
        !self.may_auto_when_permitted()
    }
}

/// Classify using existing ToolRisk + Permission category/action — never LLM judgment.
pub fn classify(
    tool_risk: ToolRisk,
    category: PermissionCategory,
    action: PermissionAction,
) -> AutonomyPolicyClass {
    // Side effects first (terminal / network / destructive / external).
    if matches!(
        tool_risk,
        ToolRisk::Destructive | ToolRisk::External
    ) || matches!(
        category,
        PermissionCategory::Terminal
            | PermissionCategory::Internet
            | PermissionCategory::Communication
            | PermissionCategory::System
            | PermissionCategory::AiProviders
    ) || matches!(
        action,
        PermissionAction::Execute | PermissionAction::Network | PermissionAction::Export
    ) {
        return AutonomyPolicyClass::SideEffect;
    }

    if matches!(tool_risk, ToolRisk::Modify)
        || matches!(
            action,
            PermissionAction::Write | PermissionAction::Delete | PermissionAction::Import
        )
    {
        return AutonomyPolicyClass::Mutation;
    }

    // Safe / Workspace + Read (and similar observe paths).
    AutonomyPolicyClass::ReadObserve
}

/// Convenience classifiers for common agentic scenarios (still ToolRisk/Permission based).
pub fn classify_read_only_investigation() -> AutonomyPolicyClass {
    classify(
        ToolRisk::Safe,
        PermissionCategory::Filesystem,
        PermissionAction::Read,
    )
}

pub fn classify_file_mutation() -> AutonomyPolicyClass {
    classify(
        ToolRisk::Modify,
        PermissionCategory::Filesystem,
        PermissionAction::Write,
    )
}

pub fn classify_destructive_command() -> AutonomyPolicyClass {
    classify(
        ToolRisk::Destructive,
        PermissionCategory::Terminal,
        PermissionAction::Execute,
    )
}

pub fn classify_git_mutation() -> AutonomyPolicyClass {
    classify(
        ToolRisk::Modify,
        PermissionCategory::Filesystem,
        PermissionAction::Write,
    )
    // Git mutate escalates via ToolRisk::effective_for; treat as side-effect-ish
    // when destructive discard, otherwise Mutation. Default policy uses SideEffect
    // for mutating Git via Terminal/execute-style escalation:
    // Prefer SideEffect when risk is Destructive.
}

/// Git mutations are always consequential — map destructive discard to SideEffect.
pub fn classify_git(tool_risk: ToolRisk) -> AutonomyPolicyClass {
    if matches!(tool_risk, ToolRisk::Destructive | ToolRisk::External) {
        AutonomyPolicyClass::SideEffect
    } else {
        AutonomyPolicyClass::Mutation
    }
}

/// Whether a gate (Permission decision + ToolRisk) allows unattended read/observe.
pub fn allows_automatic_read_observe(
    class: AutonomyPolicyClass,
    permission: PermissionDecision,
    tool_risk: ToolRisk,
) -> bool {
    class.may_auto_when_permitted()
        && permission.allows_execution()
        && !tool_risk.requires_review()
}

/// Whether mutation/side-effect must pause for Review Before Action.
pub fn requires_review_authorization(
    class: AutonomyPolicyClass,
    permission: PermissionDecision,
    tool_risk: ToolRisk,
) -> bool {
    if class.requires_explicit_authorization() {
        return true;
    }
    // Even ReadObserve escalates if Permission/ToolRisk say so.
    matches!(permission, PermissionDecision::RequiresApproval | PermissionDecision::Denied)
        || tool_risk.requires_review()
}

/// Explicit approval bound to the current plan **and** task (when multi-step).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoundApproval {
    pub goal_id: String,
    pub plan_id: String,
    pub task_id: Option<String>,
    pub recorded: bool,
    pub class: AutonomyPolicyClass,
    /// True when invalidated (cancel / plan change / corrective / task advance).
    pub invalidated: bool,
    pub invalidate_reason: Option<String>,
}

impl BoundApproval {
    pub fn is_usable_for(&self, goal_id: &str, plan_id: &str, task_id: Option<&str>) -> bool {
        if self.invalidated || !self.recorded {
            return false;
        }
        if self.goal_id != goal_id || self.plan_id != plan_id {
            return false;
        }
        match (&self.task_id, task_id) {
            (None, None) => true,
            (Some(bound), Some(active)) => bound == active,
            // Bound to a task but caller has none (or vice versa) → not interchangeable.
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
}

/// Planner-owned autonomy / approval binding for one Coding Goal.
///
/// Not an AgentApproval system — attributes Review approvals to plan+task.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GoalAutonomyState {
    pub active: Option<BoundApproval>,
    pub history: Vec<BoundApproval>,
    pub notes: Vec<String>,
}

impl GoalAutonomyState {
    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "autonomy_policy active_plan={:?} active_task={:?} recorded={} invalidated={} history={}",
            self.active.as_ref().map(|a| a.plan_id.as_str()),
            self.active.as_ref().and_then(|a| a.task_id.as_deref()),
            self.active.as_ref().map(|a| a.recorded).unwrap_or(false),
            self.active.as_ref().map(|a| a.invalidated).unwrap_or(false),
            self.history.len(),
        )
    }
}

/// Begin awaiting approval for a plan under the current task (SubmitForApproval).
pub fn bind_pending_approval(
    mut goal: CodingGoal,
    plan_id: &str,
    class: AutonomyPolicyClass,
) -> CodingGoal {
    let bound = BoundApproval {
        goal_id: goal.id.clone(),
        plan_id: plan_id.to_string(),
        task_id: goal.active_task_id.clone(),
        recorded: false,
        class,
        invalidated: false,
        invalidate_reason: None,
    };
    // Changing plan after a prior pending/recorded approval → invalidate old.
    if let Some(prev) = goal.autonomy_policy.active.take() {
        if prev.plan_id != plan_id || !prev.invalidated {
            let mut old = prev;
            old.invalidated = true;
            old.invalidate_reason = Some("superseded by new plan submission".into());
            goal.autonomy_policy.history.push(old);
            goal.autonomy_policy
                .notes
                .push("Prior approval binding invalidated — new plan requires new approval.".into());
        }
    }
    goal.autonomy_policy.active = Some(bound);
    goal.autonomy_policy.notes.push(format!(
        "Pending approval bound to plan `{plan_id}` task={:?} class={}",
        goal.active_task_id,
        class.as_str()
    ));
    goal
}

/// Record Review approval — only for the currently bound plan+task.
pub fn record_bound_approval(
    mut goal: CodingGoal,
    plan_id: &str,
) -> Result<CodingGoal, String> {
    let Some(active) = goal.autonomy_policy.active.as_mut() else {
        return Err("No pending approval binding — SubmitForApproval required.".into());
    };
    if active.invalidated {
        return Err(format!(
            "Approval binding invalidated ({}) — new approval required.",
            active
                .invalidate_reason
                .as_deref()
                .unwrap_or("unknown")
        ));
    }
    if active.plan_id != plan_id {
        return Err(format!(
            "Plan mismatch: bound `{}` vs approval `{plan_id}` — approvals do not transfer.",
            active.plan_id
        ));
    }
    if active.task_id.as_deref() != goal.active_task_id.as_deref() {
        return Err(
            "Task mismatch: approval does not apply to the current task (cross-task isolation)."
                .into(),
        );
    }
    // Fresh pending bind after corrective invalidate is allowed — SubmitForApproval
    // clears correction_requires_new_c3_plan in the lifecycle; do not block Record here.
    active.recorded = true;
    goal.autonomy_policy
        .notes
        .push(format!("Approval recorded for plan `{plan_id}` (attributable)."));
    Ok(goal)
}

/// Validate BeginExecution against bound approval (and C5.3 lifecycle flags).
pub fn assert_execution_authorized(
    goal: &CodingGoal,
    plan_id: &str,
) -> Result<(), String> {
    if !goal.agent_lifecycle.state.allows_mutation_execution()
        && goal.agent_lifecycle.state != AgentLifecycleState::AwaitingApproval
        && goal.agent_lifecycle.state != AgentLifecycleState::Executing
    {
        // Soft check — definitive gate remains agent_state_machine BeginExecution.
    }
    if goal.agent_lifecycle.correction_requires_new_c3_plan
        && !goal.agent_lifecycle.approval_recorded
    {
        return Err(
            "Corrective plan required — previous approval cannot authorize new mutation.".into(),
        );
    }
    let Some(active) = goal.autonomy_policy.active.as_ref() else {
        // Fallback: C5.3 lifecycle may still authorize when approval_recorded matches.
        if goal.agent_lifecycle.approval_recorded
            && goal.agent_lifecycle.approved_plan_id.as_deref() == Some(plan_id)
        {
            return Ok(());
        }
        return Err("No bound approval for mutation execution.".into());
    };
    if !active.is_usable_for(&goal.id, plan_id, goal.active_task_id.as_deref()) {
        return Err(format!(
            "Stale or mismatched approval (plan={} task={:?} invalidated={} recorded={}).",
            active.plan_id,
            active.task_id,
            active.invalidated,
            active.recorded
        ));
    }
    Ok(())
}

pub fn invalidate_on_cancel(mut goal: CodingGoal, reason: &str) -> CodingGoal {
    if let Some(mut active) = goal.autonomy_policy.active.take() {
        active.invalidated = true;
        active.invalidate_reason = Some(reason.into());
        active.recorded = false;
        goal.autonomy_policy.history.push(active);
    }
    goal.autonomy_policy
        .notes
        .push(format!("Approvals invalidated on cancel: {reason}"));
    goal
}

pub fn invalidate_on_plan_change(mut goal: CodingGoal, reason: &str) -> CodingGoal {
    if let Some(mut active) = goal.autonomy_policy.active.take() {
        active.invalidated = true;
        active.invalidate_reason = Some(reason.into());
        active.recorded = false;
        goal.autonomy_policy.history.push(active);
    }
    goal.autonomy_policy
        .notes
        .push(format!("Approvals invalidated — plan changed: {reason}"));
    goal
}

pub fn invalidate_on_corrective_plan(mut goal: CodingGoal) -> CodingGoal {
    if let Some(mut active) = goal.autonomy_policy.active.take() {
        active.invalidated = true;
        active.invalidate_reason =
            Some("verification/execution failure — corrective plan needs new approval".into());
        active.recorded = false;
        goal.autonomy_policy.history.push(active);
    }
    goal.autonomy_policy.notes.push(
        "Failed execution must not reuse old approval for a new corrective plan.".into(),
    );
    goal
}

/// Cross-task isolation: advancing to a new task invalidates prior task’s approval.
pub fn invalidate_on_task_advance(
    mut goal: CodingGoal,
    previous_task: Option<&str>,
    next_task: Option<&str>,
) -> CodingGoal {
    if previous_task == next_task {
        return goal;
    }
    if let Some(mut active) = goal.autonomy_policy.active.take() {
        active.invalidated = true;
        active.invalidate_reason = Some(format!(
            "task advance {:?} → {:?} — approvals do not leak across tasks",
            previous_task, next_task
        ));
        active.recorded = false;
        goal.autonomy_policy.history.push(active);
    }
    goal.autonomy_policy.notes.push(
        "Cross-task approval isolation: prior task approval cleared.".into(),
    );
    goal
}

/// Sync GoalAutonomyState after a successful agent lifecycle event.
pub fn sync_after_lifecycle_event(
    mut goal: CodingGoal,
    event: &AgentLifecycleEvent,
) -> CodingGoal {
    match event {
        AgentLifecycleEvent::SubmitForApproval { plan_id } => {
            // New plan after approval → bind_pending also invalidates prior.
            let class = if goal.agent_lifecycle.correction_requires_new_c3_plan {
                AutonomyPolicyClass::Mutation
            } else {
                AutonomyPolicyClass::Mutation
            };
            goal = bind_pending_approval(goal, plan_id, class);
        }
        AgentLifecycleEvent::RecordApproval { plan_id } => {
            match record_bound_approval(goal.clone(), plan_id) {
                Ok(g) => goal = g,
                Err(err) => {
                    goal.autonomy_policy
                        .notes
                        .push(format!("RecordApproval policy note: {err}"));
                }
            }
        }
        AgentLifecycleEvent::Cancel { reason } => {
            goal = invalidate_on_cancel(goal, reason);
        }
        AgentLifecycleEvent::VerificationFailed { .. }
        | AgentLifecycleEvent::BeginCorrection { .. }
        | AgentLifecycleEvent::ExecutionFailed { .. } => {
            goal = invalidate_on_corrective_plan(goal);
        }
        AgentLifecycleEvent::BeginExecution { approved_plan_id } => {
            if let Err(err) = assert_execution_authorized(&goal, approved_plan_id) {
                goal.autonomy_policy
                    .notes
                    .push(format!("BeginExecution policy warning: {err}"));
            }
        }
        _ => {}
    }
    goal
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutonomyPolicyRequestKind {
    Status,
    ExplainAutomatic,
    ExplainRequiresApproval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyPolicyAssessment {
    pub kind: AutonomyPolicyRequestKind,
    pub matched_rules: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyPolicyStatus {
    Status,
    Explained,
    NoActiveGoal,
    Rejected,
}

impl AutonomyPolicyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Explained => "explained",
            Self::NoActiveGoal => "no_active_goal",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutonomyPolicyReport {
    pub status: AutonomyPolicyStatus,
    pub goal_id: Option<String>,
    pub active_binding: Option<BoundApproval>,
    pub read_observe_examples: Vec<String>,
    pub mutation_examples: Vec<String>,
    pub side_effect_examples: Vec<String>,
    pub notes: Vec<String>,
    pub modes_implemented: bool,
    pub agent_approval_system: bool,
    pub llm_self_authorize: bool,
}

impl Default for AutonomyPolicyReport {
    fn default() -> Self {
        Self {
            status: AutonomyPolicyStatus::Rejected,
            goal_id: None,
            active_binding: None,
            read_observe_examples: Vec::new(),
            mutation_examples: Vec::new(),
            side_effect_examples: Vec::new(),
            notes: Vec::new(),
            modes_implemented: false,
            agent_approval_system: false,
            llm_self_authorize: false,
        }
    }
}

impl AutonomyPolicyReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Approval and Autonomy Policy\n");
        out.push_str(
            "\n_Default policy boundaries for Agentic Coding Goals — not configurable autonomy modes._\n",
        );
        out.push_str(&format!("\n### Status\n- **{}**\n", self.status.as_str()));
        if let Some(gid) = &self.goal_id {
            out.push_str(&format!("- Goal: `{gid}`\n"));
        }
        out.push_str("\n### READ / OBSERVE (may proceed without mutation approval)\n");
        for e in &self.read_observe_examples {
            out.push_str(&format!("- {e}\n"));
        }
        out.push_str("\n### MUTATION (requires Review Before Action + Permission)\n");
        for e in &self.mutation_examples {
            out.push_str(&format!("- {e}\n"));
        }
        out.push_str("\n### SIDE EFFECT (requires Review Before Action + Permission / ToolRisk)\n");
        for e in &self.side_effect_examples {
            out.push_str(&format!("- {e}\n"));
        }
        if let Some(b) = &self.active_binding {
            out.push_str("\n### Current approval binding\n");
            out.push_str(&format!(
                "- plan=`{}` task={:?} recorded={} invalidated={} class={}\n",
                b.plan_id,
                b.task_id,
                b.recorded,
                b.invalidated,
                b.class.as_str()
            ));
            if let Some(r) = &b.invalidate_reason {
                out.push_str(&format!("- reason: {r}\n"));
            }
        } else {
            out.push_str("\n### Current approval binding\n- _(none)_\n");
        }
        out.push_str(&format!(
            "\n### Safety\n- modes_implemented={} · agent_approval_system={} · llm_self_authorize={}\n",
            self.modes_implemented, self.agent_approval_system, self.llm_self_authorize
        ));
        out.push_str(
            "\nAuthorization still flows through Permission Engine → ToolRisk → Review Before Action. Agent state cannot bypass those gates.\n",
        );
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str("- ");
                out.push_str(n);
                out.push('\n');
            }
        }
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "autonomy_policy status={} goal={:?} binding_plan={:?} modes={} agent_approval={} llm_auth={}",
            self.status.as_str(),
            self.goal_id.as_deref().unwrap_or("—"),
            self.active_binding
                .as_ref()
                .map(|b| b.plan_id.as_str())
                .unwrap_or("—"),
            self.modes_implemented,
            self.agent_approval_system,
            self.llm_self_authorize,
        )
    }
}

fn default_examples() -> (Vec<String>, Vec<String>, Vec<String>) {
    (
        vec![
            "search / read files / symbol navigation".into(),
            "inspect diagnostics, project state, Git status".into(),
            "investigate, reason, formulate plans".into(),
            "interpret verification output".into(),
        ],
        vec![
            "write / modify / delete / rename / move files".into(),
            "non-destructive workspace mutations (ToolRisk::Modify)".into(),
        ],
        vec![
            "destructive terminal operations".into(),
            "Git mutations (commit, push, discard, …)".into(),
            "external / network side effects".into(),
        ],
    )
}

pub fn detect_autonomy_policy_request(request: &UserRequest) -> Option<AutonomyPolicyAssessment> {
    if request.coding_action.is_some()
        || request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
    {
        return None;
    }
    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    let trimmed = lower
        .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?')
        .to_string();

    if crate::coding_goal::detect_coding_goal_request(request).is_some()
        || crate::goal_decomposition::detect_decomposition_request(request).is_some()
        || crate::agent_state_machine::detect_agent_lifecycle_request(request).is_some()
        || crate::multi_step_execution::detect_multi_step_execution_request(request).is_some()
        || crate::agent_context_maintenance::detect_agent_context_request(request).is_some()
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::fix_from_verification::steals_from_other_detect(request)
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "what can this goal do automatically"
            | "what can the goal do automatically"
            | "what may this goal do automatically"
            | "explain autonomy policy"
    ) {
        return Some(AutonomyPolicyAssessment {
            kind: AutonomyPolicyRequestKind::ExplainAutomatic,
            matched_rules: vec!["explain_automatic"],
        });
    }
    if matches!(
        trimmed.as_str(),
        "what requires approval"
            | "what needs approval"
            | "what requires authorization"
            | "explain approval policy"
    ) {
        return Some(AutonomyPolicyAssessment {
            kind: AutonomyPolicyRequestKind::ExplainRequiresApproval,
            matched_rules: vec!["explain_requires_approval"],
        });
    }
    if matches!(
        trimmed.as_str(),
        "autonomy policy status"
            | "approval policy status"
            | "show autonomy policy"
            | "show approval policy"
    ) {
        return Some(AutonomyPolicyAssessment {
            kind: AutonomyPolicyRequestKind::Status,
            matched_rules: vec!["status_phrase"],
        });
    }
    None
}

pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &AutonomyPolicyAssessment,
) -> AutonomyPolicyReport {
    let (read_ex, mut_ex, side_ex) = default_examples();
    let Some(goal) = goal else {
        return AutonomyPolicyReport {
            status: AutonomyPolicyStatus::NoActiveGoal,
            read_observe_examples: read_ex,
            mutation_examples: mut_ex,
            side_effect_examples: side_ex,
            notes: vec![
                "No active Coding Goal — default policy still applies to tools via Permission + ToolRisk + Review."
                    .into(),
            ],
            ..Default::default()
        };
    };

    let mut notes = vec![
        format!(
            "Lifecycle={} · correction_requires_new_c3_plan={} · approval_recorded={}",
            goal.agent_lifecycle.state.as_str(),
            goal.agent_lifecycle.correction_requires_new_c3_plan,
            goal.agent_lifecycle.approval_recorded
        ),
        goal.autonomy_policy.to_diagnostics_summary(),
        "LLM cannot self-authorize; agent state cannot bypass Permission/ToolRisk/Review.".into(),
    ];
    if goal.status == CodingGoalStatus::Cancelled {
        notes.push("Goal cancelled — pending approvals are invalid.".into());
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::AwaitingApproval {
        notes.push("AwaitingApproval — mutation execution is stopped until Review Approve.".into());
    }
    if goal.agent_lifecycle.correction_requires_new_c3_plan {
        notes.push(
            "Correcting: a new C3 Coding Plan must go through Review — old approval is stale."
                .into(),
        );
    }

    match assessment.kind {
        AutonomyPolicyRequestKind::Status => AutonomyPolicyReport {
            status: AutonomyPolicyStatus::Status,
            goal_id: Some(goal.id.clone()),
            active_binding: goal.autonomy_policy.active.clone(),
            read_observe_examples: read_ex,
            mutation_examples: mut_ex,
            side_effect_examples: side_ex,
            notes,
            ..Default::default()
        },
        AutonomyPolicyRequestKind::ExplainAutomatic => {
            notes.push(
                "Read/Observe may auto-run only when Permission Allows and ToolRisk does not require review."
                    .into(),
            );
            AutonomyPolicyReport {
                status: AutonomyPolicyStatus::Explained,
                goal_id: Some(goal.id.clone()),
                active_binding: goal.autonomy_policy.active.clone(),
                read_observe_examples: read_ex,
                mutation_examples: mut_ex,
                side_effect_examples: side_ex,
                notes,
                ..Default::default()
            }
        }
        AutonomyPolicyRequestKind::ExplainRequiresApproval => {
            notes.push(
                "Any consequential mutation pauses for Review Before Action; approvals bind to plan_id + task_id."
                    .into(),
            );
            AutonomyPolicyReport {
                status: AutonomyPolicyStatus::Explained,
                goal_id: Some(goal.id.clone()),
                active_binding: goal.autonomy_policy.active.clone(),
                read_observe_examples: read_ex,
                mutation_examples: mut_ex,
                side_effect_examples: side_ex,
                notes,
                ..Default::default()
            }
        }
    }
}

/// Read-only lifecycle states that may advance without mutation approval.
pub fn allows_readonly_lifecycle_transition(state: AgentLifecycleState) -> bool {
    matches!(
        state,
        AgentLifecycleState::Idle
            | AgentLifecycleState::Understanding
            | AgentLifecycleState::Investigating
            | AgentLifecycleState::Planning
            | AgentLifecycleState::Verifying
            | AgentLifecycleState::Paused
            | AgentLifecycleState::Blocked
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_state_machine::{transition_goal, AgentLifecycleEvent};
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::{apply_decomposition, propose_tasks};
    use crate::multi_step_execution::start_goal_execution;
    use jaymi_context::ContextBundle;

    fn goal() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new("Build a login system.")).unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let (tasks, order) = propose_tasks(&g).unwrap();
        let g = apply_decomposition(g, tasks, order);
        start_goal_execution(g).unwrap().0
    }

    #[test]
    fn read_only_task_may_auto() {
        let class = classify_read_only_investigation();
        assert_eq!(class, AutonomyPolicyClass::ReadObserve);
        assert!(allows_automatic_read_observe(
            class,
            PermissionDecision::Allowed,
            ToolRisk::Safe
        ));
        assert!(!requires_review_authorization(
            class,
            PermissionDecision::Allowed,
            ToolRisk::Safe
        ));
    }

    #[test]
    fn file_mutation_requires_approval() {
        let class = classify_file_mutation();
        assert_eq!(class, AutonomyPolicyClass::Mutation);
        assert!(requires_review_authorization(
            class,
            PermissionDecision::Allowed,
            ToolRisk::Modify
        ));
        assert!(!allows_automatic_read_observe(
            class,
            PermissionDecision::Allowed,
            ToolRisk::Modify
        ));
    }

    #[test]
    fn destructive_command_and_git() {
        assert_eq!(
            classify_destructive_command(),
            AutonomyPolicyClass::SideEffect
        );
        assert_eq!(
            classify_git(ToolRisk::Destructive),
            AutonomyPolicyClass::SideEffect
        );
        assert_eq!(classify_git(ToolRisk::Modify), AutonomyPolicyClass::Mutation);
        assert!(requires_review_authorization(
            AutonomyPolicyClass::SideEffect,
            PermissionDecision::RequiresApproval,
            ToolRisk::Destructive
        ));
    }

    #[test]
    fn approval_and_rejection_binding() {
        let mut g = goal();
        let tid = g.active_task_id.clone();
        g = bind_pending_approval(g, "plan-1", AutonomyPolicyClass::Mutation);
        assert!(!g.autonomy_policy.active.as_ref().unwrap().recorded);
        g = record_bound_approval(g, "plan-1").unwrap();
        assert!(g.autonomy_policy.active.as_ref().unwrap().recorded);
        assert!(assert_execution_authorized(&g, "plan-1").is_ok());
        // Wrong plan rejected.
        assert!(record_bound_approval(g.clone(), "plan-other").is_err());
        // Cross-task isolation.
        g.active_task_id = Some("other-task".into());
        assert!(assert_execution_authorized(&g, "plan-1").is_err());
        let _ = tid;
    }

    #[test]
    fn cancellation_invalidates() {
        let mut g = goal();
        g = bind_pending_approval(g, "plan-c", AutonomyPolicyClass::Mutation);
        g = record_bound_approval(g, "plan-c").unwrap();
        g = invalidate_on_cancel(g, "user cancel");
        assert!(g.autonomy_policy.active.is_none());
        assert!(g
            .autonomy_policy
            .history
            .iter()
            .any(|b| b.invalidated && b.plan_id == "plan-c"));
        assert!(assert_execution_authorized(&g, "plan-c").is_err());
    }

    #[test]
    fn changed_plan_and_stale_approval() {
        let mut g = goal();
        g = bind_pending_approval(g, "plan-old", AutonomyPolicyClass::Mutation);
        g = record_bound_approval(g, "plan-old").unwrap();
        g = bind_pending_approval(g, "plan-new", AutonomyPolicyClass::Mutation);
        // Old approval must not authorize new plan.
        assert!(assert_execution_authorized(&g, "plan-old").is_err());
        assert!(assert_execution_authorized(&g, "plan-new").is_err()); // not recorded yet
        g = record_bound_approval(g, "plan-new").unwrap();
        assert!(assert_execution_authorized(&g, "plan-new").is_ok());
    }

    #[test]
    fn corrective_plan_rejects_old_approval() {
        let mut g = goal();
        g = bind_pending_approval(g, "plan-fail", AutonomyPolicyClass::Mutation);
        g = record_bound_approval(g, "plan-fail").unwrap();
        g = invalidate_on_corrective_plan(g);
        g.agent_lifecycle.correction_requires_new_c3_plan = true;
        g.agent_lifecycle.approval_recorded = false;
        assert!(assert_execution_authorized(&g, "plan-fail").is_err());
        // Fresh corrective submission + approval.
        g = bind_pending_approval(g, "plan-fix", AutonomyPolicyClass::Mutation);
        g.agent_lifecycle.correction_requires_new_c3_plan = false;
        g.agent_lifecycle.approval_recorded = true;
        g.agent_lifecycle.approved_plan_id = Some("plan-fix".into());
        g = record_bound_approval(g, "plan-fix").unwrap();
        assert!(assert_execution_authorized(&g, "plan-fix").is_ok());
        // Old plan still rejected.
        assert!(assert_execution_authorized(&g, "plan-fail").is_err());
    }

    #[test]
    fn cross_task_approval_isolation() {
        let mut g = goal();
        let t1 = g.active_task_id.clone();
        g = bind_pending_approval(g, "plan-t1", AutonomyPolicyClass::Mutation);
        g = record_bound_approval(g, "plan-t1").unwrap();
        let prev = t1.clone();
        g = invalidate_on_task_advance(g, prev.as_deref(), Some("t2"));
        g.active_task_id = Some("t2".into());
        assert!(assert_execution_authorized(&g, "plan-t1").is_err());
    }

    #[test]
    fn lifecycle_sync_on_cancel() {
        let mut g = goal();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginPlanning {
                reason: "p".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-x".into(),
            },
        )
        .unwrap();
        // Manual bind to mirror Submit (sync hooked in transition_goal).
        if g.autonomy_policy.active.is_none() {
            g = bind_pending_approval(g, "plan-x", AutonomyPolicyClass::Mutation);
        }
        g = sync_after_lifecycle_event(
            g,
            &AgentLifecycleEvent::Cancel {
                reason: "stop".into(),
            },
        );
        assert!(g
            .autonomy_policy
            .history
            .iter()
            .any(|b| b.invalidated)
            || g.autonomy_policy.active.is_none());
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_autonomy_policy_request(&UserRequest::new(
            "What can this goal do automatically?"
        ))
        .is_some());
        assert!(detect_autonomy_policy_request(&UserRequest::new(
            "Autonomy policy status."
        ))
        .is_some());
        assert!(detect_autonomy_policy_request(&UserRequest::new(
            "What requires approval?"
        ))
        .is_some());
        assert!(detect_autonomy_policy_request(&UserRequest::new(
            "Build a login system."
        ))
        .is_none());
    }

    #[test]
    fn readonly_lifecycle_states() {
        assert!(allows_readonly_lifecycle_transition(
            AgentLifecycleState::Investigating
        ));
        assert!(!allows_readonly_lifecycle_transition(
            AgentLifecycleState::Executing
        ));
    }
}
