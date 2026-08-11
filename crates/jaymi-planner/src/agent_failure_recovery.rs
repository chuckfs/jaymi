//! Agent Failure and Recovery (Sprint C5.9).
//!
//! Generalize C4 recovery from individual verification failures to entire
//! multi-step Coding Goals — **Planner-owned disposition**, never silent mutation.
//!
//! Constitutional constraints:
//! - Never silently mutate or silently retry mutation
//! - Never reuse approval for a materially changed plan
//! - Preserve completed work and failed task state
//! - No automatic rollback
//! - Bound recovery attempts (`MAX_GOAL_RECOVERY_ATTEMPTS`)

use crate::agent_state_machine::{
    transition_goal, AgentLifecycleEvent, AgentLifecycleState,
};
use crate::approval_autonomy_policy::invalidate_on_corrective_plan;
use crate::coding_goal::{CodingGoal, CodingGoalStatus, GoalVerificationState};
use crate::coding_plan::{CodingPlan, CodingPlanKind, CodingPlanOrigin};
use crate::goal_decomposition::GoalTaskStatus;
use jaymi_core::UserRequest;

/// Hard cap on Planner-recorded recovery proposals / dispositions per goal.
pub const MAX_GOAL_RECOVERY_ATTEMPTS: u32 = 3;

/// Categories of multi-step Coding Goal failure (C5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    InvestigationFailure,
    PlanningFailure,
    ApprovalRejection,
    ExecutionFailure,
    PartialExecution,
    VerificationFailure,
    EnvironmentFailure,
    MissingDependency,
    AmbiguousRequirement,
    StaleProjectState,
}

impl FailureCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvestigationFailure => "investigation_failure",
            Self::PlanningFailure => "planning_failure",
            Self::ApprovalRejection => "approval_rejection",
            Self::ExecutionFailure => "execution_failure",
            Self::PartialExecution => "partial_execution",
            Self::VerificationFailure => "verification_failure",
            Self::EnvironmentFailure => "environment_failure",
            Self::MissingDependency => "missing_dependency",
            Self::AmbiguousRequirement => "ambiguous_requirement",
            Self::StaleProjectState => "stale_project_state",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::InvestigationFailure => "Investigation failure",
            Self::PlanningFailure => "Planning failure",
            Self::ApprovalRejection => "Approval rejection",
            Self::ExecutionFailure => "Execution failure",
            Self::PartialExecution => "Partial execution",
            Self::VerificationFailure => "Verification failure",
            Self::EnvironmentFailure => "Environment failure",
            Self::MissingDependency => "Missing dependency",
            Self::AmbiguousRequirement => "Ambiguous requirement",
            Self::StaleProjectState => "Stale project state",
        }
    }
}

/// What Jaymi may do next — never includes silent mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDisposition {
    /// May recover via READ-ONLY investigation / diagnosis.
    ReadonlyInvestigate,
    /// Propose a **new** C3 Coding Plan (Review Before Action required).
    ProposeCorrectivePlan,
    /// User must decide → Blocked / AwaitingUser semantics.
    AwaitUser,
    /// Goal cannot safely continue → Failed.
    StopFailed,
    /// Cancelled / completed — refuse recovery.
    Terminal,
    /// Hit `MAX_GOAL_RECOVERY_ATTEMPTS`.
    Exhausted,
}

impl RecoveryDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadonlyInvestigate => "readonly_investigate",
            Self::ProposeCorrectivePlan => "propose_corrective_plan",
            Self::AwaitUser => "await_user",
            Self::StopFailed => "stop_failed",
            Self::Terminal => "terminal",
            Self::Exhausted => "exhausted",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadonlyInvestigate => "Recover with read-only investigation",
            Self::ProposeCorrectivePlan => "Propose a new reviewed Coding Plan",
            Self::AwaitUser => "Awaiting user decision",
            Self::StopFailed => "Stop — goal failed",
            Self::Terminal => "Terminal — no recovery",
            Self::Exhausted => "Recovery attempts exhausted",
        }
    }
}

/// One recorded recovery attempt (preserved on the Coding Goal).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalRecoveryRecord {
    pub sequence: u32,
    pub category: FailureCategory,
    pub disposition: RecoveryDisposition,
    pub summary: String,
    pub corrective_plan_ref: Option<String>,
    pub stamped_at: String,
    pub mutated: bool,
    pub auto_retried: bool,
    pub approval_reused: bool,
}

/// Durable recovery surface on the Coding Goal (C5.9).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GoalRecoveryState {
    pub attempt_count: u32,
    pub history: Vec<GoalRecoveryRecord>,
    pub last_category: Option<FailureCategory>,
    pub last_disposition: Option<RecoveryDisposition>,
    pub last_corrective_plan_ref: Option<String>,
    pub notes: Vec<String>,
}

impl GoalRecoveryState {
    pub fn at_limit(&self) -> bool {
        self.attempt_count >= MAX_GOAL_RECOVERY_ATTEMPTS
    }
}

/// Optional hint when classifying (tests / soft-path phrases).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureHint {
    Investigation,
    Planning,
    ApprovalRejection,
    Execution,
    PartialExecution,
    Verification,
    Environment,
    MissingDependency,
    Ambiguous,
    StaleProject,
}

/// Classify failure from goal state (+ optional hint).
pub fn classify_failure(goal: &CodingGoal, hint: Option<FailureHint>) -> FailureCategory {
    if let Some(hint) = hint {
        return match hint {
            FailureHint::Investigation => FailureCategory::InvestigationFailure,
            FailureHint::Planning => FailureCategory::PlanningFailure,
            FailureHint::ApprovalRejection => FailureCategory::ApprovalRejection,
            FailureHint::Execution => FailureCategory::ExecutionFailure,
            FailureHint::PartialExecution => FailureCategory::PartialExecution,
            FailureHint::Verification => FailureCategory::VerificationFailure,
            FailureHint::Environment => FailureCategory::EnvironmentFailure,
            FailureHint::MissingDependency => FailureCategory::MissingDependency,
            FailureHint::Ambiguous => FailureCategory::AmbiguousRequirement,
            FailureHint::StaleProject => FailureCategory::StaleProjectState,
        };
    }

    if goal.control.requires_fresh_approval
        || goal
            .control
            .notes
            .iter()
            .any(|n| n.to_ascii_lowercase().contains("stale"))
    {
        return FailureCategory::StaleProjectState;
    }

    if goal.verification_state == GoalVerificationState::Failed
        || goal.agent_lifecycle.state == AgentLifecycleState::Correcting
        || goal.agent_lifecycle.correction_requires_new_c3_plan
    {
        return FailureCategory::VerificationFailure;
    }

    if goal.agent_lifecycle.state == AgentLifecycleState::Failed
        || goal
            .agent_lifecycle
            .history
            .iter()
            .rev()
            .take(4)
            .any(|r| r.event.contains("execution_failed") || r.event == "fail")
    {
        if goal
            .workflow_history
            .iter()
            .any(|w| {
                w.kind.contains("partial")
                    || w.detail.to_ascii_lowercase().contains("partial")
            })
            || goal
                .notes
                .iter()
                .any(|n| n.to_ascii_lowercase().contains("partial"))
        {
            return FailureCategory::PartialExecution;
        }
        return FailureCategory::ExecutionFailure;
    }

    if goal.blockers.iter().any(|b| {
        let l = b.to_ascii_lowercase();
        l.contains("depend") || l.contains("missing") || l.contains("require")
    }) || goal
        .tasks
        .iter()
        .any(|t| t.status == GoalTaskStatus::Blocked)
    {
        return FailureCategory::MissingDependency;
    }

    if goal.blockers.iter().any(|b| {
        let l = b.to_ascii_lowercase();
        l.contains("ambiguous") || l.contains("unclear") || l.contains("which ")
    }) {
        return FailureCategory::AmbiguousRequirement;
    }

    if goal
        .notes
        .iter()
        .any(|n| n.to_ascii_lowercase().contains("reject"))
    {
        return FailureCategory::ApprovalRejection;
    }

    if goal.agent_lifecycle.state == AgentLifecycleState::Planning
        || goal
            .notes
            .iter()
            .any(|n| n.to_ascii_lowercase().contains("planning fail"))
    {
        return FailureCategory::PlanningFailure;
    }

    if goal.agent_lifecycle.state == AgentLifecycleState::Investigating
        || goal
            .notes
            .iter()
            .any(|n| n.to_ascii_lowercase().contains("investigation fail"))
    {
        return FailureCategory::InvestigationFailure;
    }

    if goal.notes.iter().any(|n| {
        let l = n.to_ascii_lowercase();
        l.contains("environment") || l.contains("toolchain") || l.contains("timeout")
    }) {
        return FailureCategory::EnvironmentFailure;
    }

    if goal.tasks.iter().any(|t| t.status == GoalTaskStatus::Failed) {
        return FailureCategory::ExecutionFailure;
    }

    FailureCategory::AmbiguousRequirement
}

/// Map category → disposition (constitutional: no silent mutate / retry).
pub fn disposition_for(goal: &CodingGoal, category: FailureCategory) -> RecoveryDisposition {
    if goal.control.cancelled
        || goal.status == CodingGoalStatus::Cancelled
        || goal.agent_lifecycle.state == AgentLifecycleState::Cancelled
        || goal.status == CodingGoalStatus::Completed
        || goal.agent_lifecycle.state == AgentLifecycleState::Completed
    {
        return RecoveryDisposition::Terminal;
    }
    if goal.recovery.at_limit() {
        return RecoveryDisposition::Exhausted;
    }

    match category {
        FailureCategory::InvestigationFailure => RecoveryDisposition::ReadonlyInvestigate,
        FailureCategory::PlanningFailure => RecoveryDisposition::ProposeCorrectivePlan,
        FailureCategory::ApprovalRejection => RecoveryDisposition::AwaitUser,
        FailureCategory::ExecutionFailure => RecoveryDisposition::ProposeCorrectivePlan,
        FailureCategory::PartialExecution => RecoveryDisposition::ProposeCorrectivePlan,
        FailureCategory::VerificationFailure => RecoveryDisposition::ProposeCorrectivePlan,
        FailureCategory::EnvironmentFailure => RecoveryDisposition::AwaitUser,
        FailureCategory::MissingDependency => RecoveryDisposition::AwaitUser,
        FailureCategory::AmbiguousRequirement => RecoveryDisposition::AwaitUser,
        FailureCategory::StaleProjectState => RecoveryDisposition::ProposeCorrectivePlan,
    }
}

fn stamp_now() -> String {
    crate::coding_goal::stamp_now()
}

/// Author a **proposal-only** corrective Coding Plan (new plan — not prior approval).
pub fn propose_corrective_plan(goal: &CodingGoal, category: FailureCategory) -> CodingPlan {
    let failed_tasks: Vec<&str> = goal
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                GoalTaskStatus::Failed | GoalTaskStatus::Blocked
            )
        })
        .map(|t| t.description.as_str())
        .collect();
    let completed: Vec<&str> = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Completed)
        .map(|t| t.description.as_str())
        .collect();

    let mut plan = CodingPlan {
        origin: CodingPlanOrigin::ChangeInvestigation,
        kind: Some(CodingPlanKind::Mutation),
        objective: Some(format!(
            "Corrective plan for goal `{}` after {}",
            goal.id,
            category.label()
        )),
        rationale: vec![
            format!("Failure category: {}", category.label()),
            "This is a **new** Coding Plan — prior approvals must not be reused.".into(),
            "Review Before Action is required before any mutation.".into(),
            "Completed work is preserved; failed tasks remain marked failed.".into(),
            "No automatic rollback of successful mutations.".into(),
        ],
        plan_steps: vec![
            "Re-read failed task evidence and completed work (read-only).".into(),
            "Propose minimal corrective edits grounded in investigation.".into(),
            "Submit for Review Before Action (fresh approval).".into(),
            "Apply only after approval; verify with existing C4 path.".into(),
        ],
        step_dependencies: vec![
            "Investigation before mutation proposal.".into(),
            "Fresh approval before apply — prior plan binding invalidated.".into(),
        ],
        files_to_modify: goal.relevant_files.iter().take(8).cloned().collect(),
        symbols_affected: goal.relevant_symbols.iter().take(8).cloned().collect(),
        estimated_risk: vec![
            "Corrective mutation — requires explicit user authorization.".into(),
        ],
        summary: vec![format!(
            "Propose corrective plan for {} (attempt {}/{}).",
            category.as_str(),
            goal.recovery.attempt_count.saturating_add(1),
            MAX_GOAL_RECOVERY_ATTEMPTS
        )],
        ..CodingPlan::default()
    };

    if !completed.is_empty() {
        plan.rationale
            .push(format!("Preserved completed work: {}", completed.join("; ")));
    }
    if !failed_tasks.is_empty() {
        plan.rationale.push(format!(
            "Failed / blocked tasks (preserved): {}",
            failed_tasks.join("; ")
        ));
        plan.unresolved_questions.push(
            "Confirm whether failed tasks should be re-attempted or redesigned.".into(),
        );
    }
    if category == FailureCategory::PartialExecution {
        plan.rationale.push(
            "Partial application remains honest — succeeded paths are not silently undone."
                .into(),
        );
    }
    plan
}

fn plan_ref_for(goal: &CodingGoal) -> String {
    format!(
        "corrective-{}-{}",
        goal.id,
        goal.recovery.attempt_count.saturating_add(1)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalRecoveryStatus {
    Applied,
    Rejected,
    Exhausted,
    Status,
    NoActiveGoal,
}

impl GoalRecoveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Exhausted => "exhausted",
            Self::Status => "status",
            Self::NoActiveGoal => "no_active_goal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalRecoveryReport {
    pub status: GoalRecoveryStatus,
    pub goal_id: Option<String>,
    pub category: FailureCategory,
    pub disposition: RecoveryDisposition,
    pub from: Option<AgentLifecycleState>,
    pub to: Option<AgentLifecycleState>,
    pub explanation: String,
    pub corrective_plan: Option<CodingPlan>,
    pub corrective_plan_ref: Option<String>,
    pub attempt_index: u32,
    pub max_attempts: u32,
    pub preserved_completed_tasks: usize,
    pub preserved_failed_tasks: usize,
    pub mutated: bool,
    pub auto_retried: bool,
    pub approval_reused: bool,
    pub automatic_rollback: bool,
    pub parallel_repair_engine: bool,
    pub planner_owned: bool,
    pub notes: Vec<String>,
}

impl Default for GoalRecoveryReport {
    fn default() -> Self {
        Self {
            status: GoalRecoveryStatus::Rejected,
            goal_id: None,
            category: FailureCategory::AmbiguousRequirement,
            disposition: RecoveryDisposition::Terminal,
            from: None,
            to: None,
            explanation: String::new(),
            corrective_plan: None,
            corrective_plan_ref: None,
            attempt_index: 0,
            max_attempts: MAX_GOAL_RECOVERY_ATTEMPTS,
            preserved_completed_tasks: 0,
            preserved_failed_tasks: 0,
            mutated: false,
            auto_retried: false,
            approval_reused: false,
            automatic_rollback: false,
            parallel_repair_engine: false,
            planner_owned: true,
            notes: Vec::new(),
        }
    }
}

impl GoalRecoveryReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Goal Failure & Recovery\n\n");
        out.push_str(&format!("**{}**\n\n", self.disposition.label()));
        out.push_str(&format!("- Category: {}\n", self.category.label()));
        out.push_str(&format!("- Status: `{}`\n", self.status.as_str()));
        if let Some(id) = &self.goal_id {
            out.push_str(&format!("- Goal: `{id}`\n"));
        }
        if let (Some(from), Some(to)) = (self.from, self.to) {
            out.push_str(&format!("- Lifecycle: {} → {}\n", from.label(), to.label()));
        }
        out.push_str(&format!(
            "- Recovery attempt: {}/{}\n",
            self.attempt_index, self.max_attempts
        ));
        out.push_str(&format!(
            "- Preserved completed tasks: {}\n",
            self.preserved_completed_tasks
        ));
        out.push_str(&format!(
            "- Preserved failed tasks: {}\n",
            self.preserved_failed_tasks
        ));
        if !self.explanation.is_empty() {
            out.push_str("\n### What happened\n");
            out.push_str(&self.explanation);
            out.push('\n');
        }
        out.push_str("\n### Guards\n");
        out.push_str(&format!(
            "- Silent mutation: {}\n",
            if self.mutated { "YES (bug)" } else { "no" }
        ));
        out.push_str(&format!(
            "- Silent mutation retry: {}\n",
            if self.auto_retried { "YES (bug)" } else { "no" }
        ));
        out.push_str(&format!(
            "- Approval reused for changed plan: {}\n",
            if self.approval_reused { "YES (bug)" } else { "no" }
        ));
        out.push_str(&format!(
            "- Automatic rollback: {}\n",
            if self.automatic_rollback { "YES (bug)" } else { "no" }
        ));
        if self.corrective_plan.is_some() {
            out.push_str(
                "\n### Corrective Coding Plan\n- Proposed below — **Review Before Action** required before mutation.\n",
            );
        }
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str(&format!("- {n}\n"));
            }
        }
        out.push_str(
            "\n_Planner-owned recovery — no parallel repair engine; C2/C3 reused for corrective plans only._\n",
        );
        if let Some(plan) = &self.corrective_plan {
            out.push('\n');
            out.push_str(&plan.to_markdown());
        }
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "agent_recovery status={} category={} disposition={} attempt={}/{} mutated={} auto_retry={} approval_reuse={} rollback={} parallel_engine={} planner_owned={}",
            self.status.as_str(),
            self.category.as_str(),
            self.disposition.as_str(),
            self.attempt_index,
            self.max_attempts,
            self.mutated,
            self.auto_retried,
            self.approval_reused,
            self.automatic_rollback,
            self.parallel_repair_engine,
            self.planner_owned,
        )
    }
}

fn record_attempt(
    goal: &mut CodingGoal,
    category: FailureCategory,
    disposition: RecoveryDisposition,
    summary: &str,
    plan_ref: Option<String>,
) {
    goal.recovery.attempt_count = goal.recovery.attempt_count.saturating_add(1);
    goal.recovery.last_category = Some(category);
    goal.recovery.last_disposition = Some(disposition);
    if let Some(r) = &plan_ref {
        goal.recovery.last_corrective_plan_ref = Some(r.clone());
    }
    goal.recovery.history.push(GoalRecoveryRecord {
        sequence: goal.recovery.attempt_count,
        category,
        disposition,
        summary: summary.into(),
        corrective_plan_ref: plan_ref,
        stamped_at: stamp_now(),
        mutated: false,
        auto_retried: false,
        approval_reused: false,
    });
    if goal.recovery.history.len() > 16 {
        let skip = goal.recovery.history.len() - 16;
        goal.recovery.history.drain(0..skip);
    }
}

/// Apply recovery disposition onto the goal (Planner-owned; no mutation).
pub fn apply_recovery(
    mut goal: CodingGoal,
    category: FailureCategory,
    disposition: RecoveryDisposition,
) -> (CodingGoal, GoalRecoveryReport) {
    let completed_before = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Completed)
        .count();
    let failed_before = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Failed)
        .count();
    let from = goal.agent_lifecycle.state;

    let mut report = GoalRecoveryReport {
        status: GoalRecoveryStatus::Applied,
        goal_id: Some(goal.id.clone()),
        category,
        disposition,
        from: Some(from),
        to: None,
        explanation: String::new(),
        corrective_plan: None,
        corrective_plan_ref: None,
        attempt_index: goal.recovery.attempt_count.saturating_add(1),
        max_attempts: MAX_GOAL_RECOVERY_ATTEMPTS,
        preserved_completed_tasks: completed_before,
        preserved_failed_tasks: failed_before,
        mutated: false,
        auto_retried: false,
        approval_reused: false,
        automatic_rollback: false,
        parallel_repair_engine: false,
        planner_owned: true,
        notes: Vec::new(),
    };

    match disposition {
        RecoveryDisposition::Terminal => {
            report.status = GoalRecoveryStatus::Rejected;
            report.explanation =
                "Goal is cancelled or completed — recovery will not continue.".into();
            report.to = Some(from);
            return (goal, report);
        }
        RecoveryDisposition::Exhausted => {
            report.status = GoalRecoveryStatus::Exhausted;
            report.explanation = format!(
                "Recovery attempt limit ({MAX_GOAL_RECOVERY_ATTEMPTS}) reached — stopping further automatic recovery offers."
            );
            if !matches!(
                goal.agent_lifecycle.state,
                AgentLifecycleState::Failed
                    | AgentLifecycleState::Cancelled
                    | AgentLifecycleState::Completed
            ) {
                if let Ok(updated) = transition_goal(
                    goal.clone(),
                    AgentLifecycleEvent::Fail {
                        reason: "Recovery attempts exhausted.".into(),
                    },
                ) {
                    goal = updated;
                }
            }
            report.to = Some(goal.agent_lifecycle.state);
            goal.recovery.notes.push("Recovery exhausted.".into());
            record_attempt(&mut goal, category, disposition, &report.explanation, None);
            report.attempt_index = goal.recovery.attempt_count;
            return (goal, report);
        }
        RecoveryDisposition::ReadonlyInvestigate => {
            report.explanation = format!(
                "{} — may recover through **read-only** investigation; no mutation in this turn.",
                category.label()
            );
            if matches!(
                goal.agent_lifecycle.state,
                AgentLifecycleState::Idle
                    | AgentLifecycleState::Understanding
                    | AgentLifecycleState::Planning
                    | AgentLifecycleState::Failed
                    | AgentLifecycleState::Correcting
                    | AgentLifecycleState::Blocked
            ) {
                if let Ok(updated) = transition_goal(
                    goal.clone(),
                    AgentLifecycleEvent::BeginInvestigation {
                        reason: "C5.9 read-only recovery investigation".into(),
                    },
                ) {
                    goal = updated;
                }
            }
            report.notes.push(
                "READ-ONLY path — search/read/diagnose only; never silent mutate.".into(),
            );
        }
        RecoveryDisposition::ProposeCorrectivePlan => {
            report.explanation = format!(
                "{} — proposing a **new** corrective Coding Plan. Fresh Review Before Action required; prior approvals are not reused.",
                category.label()
            );
            goal = invalidate_on_corrective_plan(goal);
            if goal.agent_lifecycle.state != AgentLifecycleState::Correcting {
                if let Ok(updated) = transition_goal(
                    goal.clone(),
                    AgentLifecycleEvent::BeginCorrection {
                        reason: format!("C5.9 corrective plan after {}", category.as_str()),
                    },
                ) {
                    goal = updated;
                } else if matches!(
                    goal.agent_lifecycle.state,
                    AgentLifecycleState::Verifying | AgentLifecycleState::Executing
                ) {
                    let ev = if category == FailureCategory::VerificationFailure {
                        AgentLifecycleEvent::VerificationFailed {
                            evidence_ref: "c59-recovery".into(),
                        }
                    } else {
                        AgentLifecycleEvent::ExecutionFailed {
                            reason: "c59 recovery".into(),
                        }
                    };
                    if let Ok(updated) = transition_goal(goal.clone(), ev) {
                        goal = updated;
                    }
                    if goal.agent_lifecycle.state != AgentLifecycleState::Correcting {
                        if let Ok(updated) = transition_goal(
                            goal.clone(),
                            AgentLifecycleEvent::BeginCorrection {
                                reason: "c59 after fail".into(),
                            },
                        ) {
                            goal = updated;
                        }
                    }
                }
            }
            let plan = propose_corrective_plan(&goal, category);
            let pref = plan_ref_for(&goal);
            goal.recovery.last_corrective_plan_ref = Some(pref.clone());
            goal.agent_lifecycle.approval_recorded = false;
            report.corrective_plan = Some(plan);
            report.corrective_plan_ref = Some(pref);
            report.notes.push(
                "Corrective plan is proposal-only — Review Before Action before mutation.".into(),
            );
            report.notes.push(
                "Materially changed plan — prior approval binding invalidated.".into(),
            );
        }
        RecoveryDisposition::AwaitUser => {
            report.explanation = format!(
                "{} — user decision required (AwaitingUser → Blocked). No silent mutation.",
                category.label()
            );
            let reason = match category {
                FailureCategory::ApprovalRejection => {
                    "Approval was rejected — decide how to proceed.".to_string()
                }
                FailureCategory::MissingDependency => {
                    "Missing dependency blocks safe progress.".to_string()
                }
                FailureCategory::AmbiguousRequirement => {
                    "Requirement is ambiguous — clarify before continuing.".to_string()
                }
                FailureCategory::EnvironmentFailure => {
                    "Environment / toolchain issue needs user attention.".to_string()
                }
                other => format!("{} — awaiting user decision.", other.label()),
            };
            if let Ok(updated) = transition_goal(
                goal.clone(),
                AgentLifecycleEvent::Block {
                    reason: reason.clone(),
                },
            ) {
                goal = updated;
            } else {
                goal.status = CodingGoalStatus::Blocked;
                if !goal.blockers.iter().any(|b| b == &reason) {
                    goal.blockers.push(reason);
                }
            }
            report.notes.push(
                "AwaitingUser mapped to Blocked — UI/Experience may mirror blockers.".into(),
            );
        }
        RecoveryDisposition::StopFailed => {
            report.explanation = format!(
                "{} — cannot safely continue; transitioning to Failed.",
                category.label()
            );
            if let Ok(updated) = transition_goal(
                goal.clone(),
                AgentLifecycleEvent::Fail {
                    reason: format!("C5.9 stop after {}", category.as_str()),
                },
            ) {
                goal = updated;
            }
            report.notes.push("Goal Failed — no automatic rollback.".into());
        }
    }

    report.to = Some(goal.agent_lifecycle.state);
    let completed_after = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Completed)
        .count();
    let failed_after = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Failed)
        .count();
    debug_assert_eq!(completed_before, completed_after);
    debug_assert!(failed_after >= failed_before);
    report.preserved_completed_tasks = completed_after;
    report.preserved_failed_tasks = failed_after;

    record_attempt(
        &mut goal,
        category,
        disposition,
        &report.explanation,
        report.corrective_plan_ref.clone(),
    );
    report.attempt_index = goal.recovery.attempt_count;

    debug_assert!(!report.mutated);
    debug_assert!(!report.auto_retried);
    debug_assert!(!report.approval_reused);
    debug_assert!(!report.automatic_rollback);

    (goal, report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryRequestKind {
    Recover,
    Status,
    WhyFailed,
    ProposeCorrective,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryAssessment {
    pub kind: RecoveryRequestKind,
    pub matched_rules: Vec<&'static str>,
    pub hint: Option<FailureHint>,
}

/// Detect goal recovery phrases (Conversation First; phrase-only — avoid detect cycles).
pub fn detect_recovery_request(request: &UserRequest) -> Option<RecoveryAssessment> {
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

    if trimmed.starts_with("pause ")
        || trimmed.starts_with("resume ")
        || trimmed.starts_with("cancel ")
        || trimmed.starts_with("stop this goal")
        || trimmed.starts_with("stop working on this goal")
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "recover this goal"
            | "recover from this failure"
            | "recover from failure"
            | "recover the goal"
            | "goal recovery"
    ) || trimmed.starts_with("recover this goal")
        || trimmed.starts_with("recover from this failure")
        || trimmed.starts_with("recover from failure")
    {
        return Some(RecoveryAssessment {
            kind: RecoveryRequestKind::Recover,
            matched_rules: vec!["c59_recover"],
            hint: None,
        });
    }

    if matches!(
        trimmed.as_str(),
        "why did this goal fail"
            | "why did the goal fail"
            | "what happened to this goal"
            | "explain the goal failure"
    ) || trimmed.starts_with("why did this goal fail")
        || trimmed.starts_with("why did the goal fail")
    {
        return Some(RecoveryAssessment {
            kind: RecoveryRequestKind::WhyFailed,
            matched_rules: vec!["c59_why"],
            hint: None,
        });
    }

    if matches!(
        trimmed.as_str(),
        "goal recovery status" | "recovery status" | "failure recovery status"
    ) {
        return Some(RecoveryAssessment {
            kind: RecoveryRequestKind::Status,
            matched_rules: vec!["c59_status"],
            hint: None,
        });
    }

    if matches!(
        trimmed.as_str(),
        "propose a corrective plan"
            | "propose corrective plan"
            | "create a corrective plan"
            | "new corrective plan"
    ) || trimmed.starts_with("propose a corrective")
    {
        return Some(RecoveryAssessment {
            kind: RecoveryRequestKind::ProposeCorrective,
            matched_rules: vec!["c59_propose"],
            hint: None,
        });
    }

    let hinted = [
        ("recover from investigation failure", FailureHint::Investigation),
        ("recover from planning failure", FailureHint::Planning),
        ("recover from approval rejection", FailureHint::ApprovalRejection),
        ("recover from execution failure", FailureHint::Execution),
        ("recover from partial execution", FailureHint::PartialExecution),
        ("recover from verification failure", FailureHint::Verification),
        ("recover from environment failure", FailureHint::Environment),
        ("recover from missing dependency", FailureHint::MissingDependency),
        ("recover from ambiguous requirement", FailureHint::Ambiguous),
        ("recover from stale project state", FailureHint::StaleProject),
    ];
    for (phrase, hint) in hinted {
        if trimmed == phrase || trimmed.starts_with(phrase) {
            return Some(RecoveryAssessment {
                kind: RecoveryRequestKind::Recover,
                matched_rules: vec!["c59_recover_hinted"],
                hint: Some(hint),
            });
        }
    }

    None
}

pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &RecoveryAssessment,
) -> (GoalRecoveryReport, Option<CodingGoal>) {
    let Some(goal) = goal else {
        return (
            GoalRecoveryReport {
                status: GoalRecoveryStatus::NoActiveGoal,
                explanation: "No active Coding Goal.".into(),
                notes: vec!["Create or activate a Coding Goal before recovery.".into()],
                ..Default::default()
            },
            None,
        );
    };

    if assessment.kind == RecoveryRequestKind::Status
        || assessment.kind == RecoveryRequestKind::WhyFailed
    {
        let category = classify_failure(goal, assessment.hint);
        let disposition = disposition_for(goal, category);
        let report = GoalRecoveryReport {
            status: GoalRecoveryStatus::Status,
            goal_id: Some(goal.id.clone()),
            category,
            disposition,
            from: Some(goal.agent_lifecycle.state),
            to: Some(goal.agent_lifecycle.state),
            explanation: format!(
                "Current classification: {} → {}.",
                category.label(),
                disposition.label()
            ),
            attempt_index: goal.recovery.attempt_count,
            max_attempts: MAX_GOAL_RECOVERY_ATTEMPTS,
            preserved_completed_tasks: goal
                .tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Completed)
                .count(),
            preserved_failed_tasks: goal
                .tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Failed)
                .count(),
            notes: goal
                .recovery
                .history
                .iter()
                .rev()
                .take(4)
                .map(|r| {
                    format!(
                        "#{} {} → {}",
                        r.sequence,
                        r.category.as_str(),
                        r.disposition.as_str()
                    )
                })
                .collect(),
            planner_owned: true,
            ..Default::default()
        };
        return (report, Some(goal.clone()));
    }

    let mut category = classify_failure(goal, assessment.hint);
    let mut disposition = disposition_for(goal, category);
    if assessment.kind == RecoveryRequestKind::ProposeCorrective
        && !matches!(
            disposition,
            RecoveryDisposition::Terminal | RecoveryDisposition::Exhausted
        )
    {
        if matches!(
            category,
            FailureCategory::AmbiguousRequirement | FailureCategory::ApprovalRejection
        ) {
            category = FailureCategory::PlanningFailure;
        }
        disposition = RecoveryDisposition::ProposeCorrectivePlan;
    }

    let (updated, report) = apply_recovery(goal.clone(), category, disposition);
    (report, Some(updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_state_machine::{transition_goal, AgentLifecycleEvent};
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::{apply_decomposition, propose_tasks, GoalTask};
    use crate::multi_step_execution::start_goal_execution;
    use jaymi_context::ContextBundle;

    fn started() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(
            "Build a login system without OAuth.",
        ))
        .unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let (tasks, order) = propose_tasks(&g).unwrap();
        let g = apply_decomposition(g, tasks, order);
        start_goal_execution(g).unwrap().0
    }

    fn with_completed_and_failed(mut g: CodingGoal) -> CodingGoal {
        if let Some(t) = g.tasks.get_mut(0) {
            t.status = GoalTaskStatus::Completed;
        }
        g.tasks.push(GoalTask {
            id: "failed-1".into(),
            description: "Broken apply step".into(),
            status: GoalTaskStatus::Failed,
            ..GoalTask::default()
        });
        g
    }

    #[test]
    fn every_category_has_disposition_and_no_hidden_mutation() {
        let categories = [
            FailureCategory::InvestigationFailure,
            FailureCategory::PlanningFailure,
            FailureCategory::ApprovalRejection,
            FailureCategory::ExecutionFailure,
            FailureCategory::PartialExecution,
            FailureCategory::VerificationFailure,
            FailureCategory::EnvironmentFailure,
            FailureCategory::MissingDependency,
            FailureCategory::AmbiguousRequirement,
            FailureCategory::StaleProjectState,
        ];
        for cat in categories {
            let g = with_completed_and_failed(started());
            let completed = g
                .tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Completed)
                .count();
            let failed = g
                .tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Failed)
                .count();
            let disp = disposition_for(&g, cat);
            let (updated, report) = apply_recovery(g, cat, disp);
            assert!(!report.mutated, "{cat:?}");
            assert!(!report.auto_retried, "{cat:?}");
            assert!(!report.approval_reused, "{cat:?}");
            assert!(!report.automatic_rollback, "{cat:?}");
            assert!(!report.parallel_repair_engine, "{cat:?}");
            assert_eq!(
                updated
                    .tasks
                    .iter()
                    .filter(|t| t.status == GoalTaskStatus::Completed)
                    .count(),
                completed
            );
            assert!(
                updated
                    .tasks
                    .iter()
                    .filter(|t| t.status == GoalTaskStatus::Failed)
                    .count()
                    >= failed
            );
            assert!(!updated.recovery.history.is_empty());
        }
    }

    #[test]
    fn corrective_plan_requires_review_and_invalidates_approval() {
        let mut g = started();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginPlanning { reason: "p".into() },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "old-plan".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "old-plan".into(),
            },
        )
        .unwrap();
        assert!(g.agent_lifecycle.approval_recorded);
        let (updated, report) = apply_recovery(
            g,
            FailureCategory::VerificationFailure,
            RecoveryDisposition::ProposeCorrectivePlan,
        );
        assert!(report.corrective_plan.is_some());
        assert!(!updated.agent_lifecycle.approval_recorded);
        assert!(!report.approval_reused);
        assert!(report
            .corrective_plan
            .as_ref()
            .unwrap()
            .rationale
            .iter()
            .any(|r| r.to_ascii_lowercase().contains("review")));
    }

    #[test]
    fn no_infinite_retry_exhausts() {
        let mut g = with_completed_and_failed(started());
        for _ in 0..MAX_GOAL_RECOVERY_ATTEMPTS {
            let (next, report) = apply_recovery(
                g,
                FailureCategory::ExecutionFailure,
                RecoveryDisposition::ProposeCorrectivePlan,
            );
            assert_ne!(report.status, GoalRecoveryStatus::Exhausted);
            g = next;
        }
        assert!(g.recovery.at_limit());
        let g3 = {
            let mut x = started();
            x.recovery.attempt_count = MAX_GOAL_RECOVERY_ATTEMPTS;
            x
        };
        assert_eq!(
            disposition_for(&g3, FailureCategory::ExecutionFailure),
            RecoveryDisposition::Exhausted
        );
        let (_u, r) = apply_recovery(
            g3,
            FailureCategory::ExecutionFailure,
            RecoveryDisposition::Exhausted,
        );
        assert_eq!(r.status, GoalRecoveryStatus::Exhausted);
        assert!(!r.auto_retried);
    }

    #[test]
    fn cancel_blocks_recovery() {
        let mut g = started();
        g = transition_goal(
            g,
            AgentLifecycleEvent::Cancel {
                reason: "stop".into(),
            },
        )
        .unwrap();
        g.control.cancelled = true;
        assert_eq!(
            disposition_for(&g, FailureCategory::VerificationFailure),
            RecoveryDisposition::Terminal
        );
    }

    #[test]
    fn await_user_maps_to_blocked() {
        let g = started();
        let (updated, report) = apply_recovery(
            g,
            FailureCategory::AmbiguousRequirement,
            RecoveryDisposition::AwaitUser,
        );
        assert_eq!(report.disposition, RecoveryDisposition::AwaitUser);
        assert!(
            updated.agent_lifecycle.state == AgentLifecycleState::Blocked
                || updated.status == CodingGoalStatus::Blocked
        );
        assert!(!updated.blockers.is_empty());
    }

    #[test]
    fn partial_execution_honest_no_rollback() {
        let mut g = with_completed_and_failed(started());
        g.notes.push("partial apply: 2 succeeded, 1 failed".into());
        let (_updated, report) = apply_recovery(
            g,
            FailureCategory::PartialExecution,
            RecoveryDisposition::ProposeCorrectivePlan,
        );
        assert!(!report.automatic_rollback);
        assert!(report
            .corrective_plan
            .as_ref()
            .unwrap()
            .rationale
            .iter()
            .any(|r| r.to_ascii_lowercase().contains("partial")
                || r.to_ascii_lowercase().contains("not silently")));
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_recovery_request(&UserRequest::new("Recover this goal.")).is_some());
        assert!(detect_recovery_request(&UserRequest::new(
            "Recover from verification failure."
        ))
        .is_some());
        assert!(detect_recovery_request(&UserRequest::new("Pause this goal.")).is_none());
        assert!(detect_recovery_request(&UserRequest::new("Build a login system.")).is_none());
    }

    #[test]
    fn recovery_history_preserved() {
        let g = started();
        let (g, _) = apply_recovery(
            g,
            FailureCategory::InvestigationFailure,
            RecoveryDisposition::ReadonlyInvestigate,
        );
        let (g, _) = apply_recovery(
            g,
            FailureCategory::VerificationFailure,
            RecoveryDisposition::ProposeCorrectivePlan,
        );
        assert_eq!(g.recovery.history.len(), 2);
        assert_eq!(g.recovery.attempt_count, 2);
    }
}
