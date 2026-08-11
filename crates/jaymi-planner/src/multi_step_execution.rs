//! Multi-Step Coding Execution (Sprint C5.4).
//!
//! Connect Coding Goals, Goal Tasks, C3 mutation, and C4 verification into a
//! Planner-owned multi-step workflow — **orchestration, not unrestricted autonomy**.
//!
//! ```text
//! Model proposes → Planner decides → Review authorizes → Tools execute → Verification evaluates
//! ```
//!
//! Constitutional constraints:
//! - Reuse C2 investigation, C3 mutation, C4 verification — no parallel stacks
//! - Every mutation still requires existing Review/Approval
//! - No second ExecutionPlan / Review system; no silent approve; no auto-correct mutate
//! - No LLM direct tool execution; no hidden filesystem access
//! - One in-progress task; bounded workflow transitions
//! - Failed tasks do not silently unlock dependents
//! - Task complete requires evidence; goal complete still uses C5.3 evidence rules
//! - Not long-running persistence / autonomy modes / advanced progress UX

use crate::agent_state_machine::{
    transition_goal, AgentLifecycleEvent, AgentLifecycleState,
};
use crate::coding_goal::{CodingGoal, GoalVerificationState};
use crate::goal_decomposition::{transition_task_status, GoalTask, GoalTaskStatus};
use jaymi_core::UserRequest;

/// Only one task may be InProgress at a time.
pub const MAX_IN_PROGRESS_TASKS: usize = 1;
/// Soft-path “continue/advance” starts at most one task per turn.
pub const MAX_TASK_ADVANCES_PER_TURN: usize = 1;
/// Hard cap on recorded workflow steps for one goal.
pub const MAX_WORKFLOW_TRANSITIONS: usize = 128;

/// One recorded orchestration step (history — not an ExecutionPlan).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowRecord {
    pub sequence: u32,
    pub kind: String,
    pub task_id: Option<String>,
    pub detail: String,
}

/// Evidence required before a task may be marked Completed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskCompletionEvidence {
    pub summary: String,
    /// Approved apply / plan id when mutation was expected.
    pub approved_plan_id: Option<String>,
    /// Verification evidence ref when verification was expected.
    pub verification_ref: Option<String>,
    pub verification_passed: bool,
    /// Investigation-only tasks may complete with this flag + summary.
    pub investigation_complete: bool,
}

impl TaskCompletionEvidence {
    pub fn is_sufficient_for(&self, task: &GoalTask) -> bool {
        if self.summary.trim().is_empty() {
            return false;
        }
        // Investigation-only tasks (no mutation): investigation flag or verification.
        if !task.mutation_expected {
            return self.investigation_complete || self.verification_passed;
        }
        // Mutation expected: need an approved plan reference.
        if self.approved_plan_id.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            return false;
        }
        // If verification is expected, require a passing verification ref.
        if task.expected_verification.is_some() {
            return self.verification_passed && self.verification_ref.is_some();
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MultiStepRequestKind {
    StartGoal,
    ContinueGoal,
    AdvanceNextTask,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiStepAssessment {
    pub kind: MultiStepRequestKind,
    pub matched_rules: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiStepStatus {
    Started,
    Continued,
    Advanced,
    BlockedDependency,
    NeedsDecomposition,
    NeedsApproval,
    TaskFailed,
    VerificationFailed,
    GoalReadyToComplete,
    NoActiveGoal,
    Rejected,
    Status,
}

impl MultiStepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Continued => "continued",
            Self::Advanced => "advanced",
            Self::BlockedDependency => "blocked_dependency",
            Self::NeedsDecomposition => "needs_decomposition",
            Self::NeedsApproval => "needs_approval",
            Self::TaskFailed => "task_failed",
            Self::VerificationFailed => "verification_failed",
            Self::GoalReadyToComplete => "goal_ready_to_complete",
            Self::NoActiveGoal => "no_active_goal",
            Self::Rejected => "rejected",
            Self::Status => "status",
        }
    }
}

/// Soft-path / hook report (conversation + diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MultiStepExecutionReport {
    pub status: MultiStepStatus,
    pub goal_id: Option<String>,
    pub active_task_id: Option<String>,
    pub next_hint: Option<String>,
    pub notes: Vec<String>,
    pub history_len: usize,
    pub mutated: bool,
    pub tools_executed: bool,
    pub auto_approved: bool,
    pub llm_direct_tools: bool,
}

impl Default for MultiStepExecutionReport {
    fn default() -> Self {
        Self {
            status: MultiStepStatus::Rejected,
            goal_id: None,
            active_task_id: None,
            next_hint: None,
            notes: Vec::new(),
            history_len: 0,
            mutated: false,
            tools_executed: false,
            auto_approved: false,
            llm_direct_tools: false,
        }
    }
}

impl MultiStepExecutionReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Multi-Step Coding Execution\n");
        out.push_str(
            "\n_Planner orchestration — mutations still require Review; verification stays evidence-based._\n",
        );
        out.push_str(&format!("\n### Status\n- **{}**\n", self.status.as_str()));
        if let Some(gid) = &self.goal_id {
            out.push_str(&format!("- Goal: `{gid}`\n"));
        }
        if let Some(tid) = &self.active_task_id {
            out.push_str(&format!("- Active task: `{tid}`\n"));
        }
        if let Some(hint) = &self.next_hint {
            out.push_str(&format!("\n### Next\n- {hint}\n"));
        }
        out.push_str(&format!(
            "\n### Safety\n- mutated={} · tools_executed={} · auto_approved={} · llm_direct_tools={}\n",
            self.mutated, self.tools_executed, self.auto_approved, self.llm_direct_tools
        ));
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str("- ");
                out.push_str(n);
                out.push('\n');
            }
        }
        out.push_str(
            "\nReuse existing C2 investigation, C3 Coding Plan → Review → Apply, and C4 verification. This module does not approve or execute tools.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "multi_step_execution status={} goal={:?} active_task={:?} history={} mutated={} auto_approved={}",
            self.status.as_str(),
            self.goal_id.as_deref().unwrap_or("—"),
            self.active_task_id.as_deref().unwrap_or("—"),
            self.history_len,
            self.mutated,
            self.auto_approved,
        )
    }
}

fn push_history(goal: &mut CodingGoal, kind: &str, task_id: Option<&str>, detail: &str) {
    if goal.workflow_history.len() >= MAX_WORKFLOW_TRANSITIONS {
        goal.notes.push(format!(
            "Workflow history at cap ({MAX_WORKFLOW_TRANSITIONS}); oldest entries retained."
        ));
        return;
    }
    let sequence = goal.workflow_transition_count.saturating_add(1);
    goal.workflow_transition_count = sequence;
    goal.workflow_history.push(WorkflowRecord {
        sequence,
        kind: kind.into(),
        task_id: task_id.map(|s| s.to_string()),
        detail: detail.into(),
    });
}

fn in_progress_count(goal: &CodingGoal) -> usize {
    goal.tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::InProgress)
        .count()
}

/// Tasks whose dependencies are all Completed and status is Pending.
pub fn ready_tasks(goal: &CodingGoal) -> Vec<&GoalTask> {
    goal.tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Pending)
        .filter(|t| {
            t.dependencies.iter().all(|dep| {
                goal.tasks
                    .iter()
                    .find(|x| &x.id == dep)
                    .map(|x| x.status == GoalTaskStatus::Completed)
                    .unwrap_or(false)
            })
        })
        .collect()
}

pub fn select_next_task(goal: &CodingGoal) -> Option<&GoalTask> {
    ready_tasks(goal).into_iter().next()
}

fn hint_for_task(task: &GoalTask) -> String {
    if !task.reuse_hints.is_empty() {
        return format!(
            "For `{}`: {} — then Review Before Action before any mutation.",
            task.id,
            task.reuse_hints.join("; ")
        );
    }
    if task.mutation_expected {
        format!(
            "Investigate (C2) → author a C3 Coding Plan for `{}` → Review → Apply → Verify (C4).",
            task.id
        )
    } else {
        format!(
            "Investigate (C2) for `{}` — no mutation expected until a later task.",
            task.id
        )
    }
}

/// Start orchestrating an active decomposed goal (first ready task → InProgress).
pub fn start_goal_execution(
    mut goal: CodingGoal,
) -> Result<(CodingGoal, MultiStepExecutionReport), String> {
    if goal.workflow_transition_count as usize >= MAX_WORKFLOW_TRANSITIONS {
        return Err(format!(
            "Workflow transition limit ({MAX_WORKFLOW_TRANSITIONS}) reached."
        ));
    }
    if goal.tasks.is_empty() {
        let mut report = MultiStepExecutionReport {
            status: MultiStepStatus::NeedsDecomposition,
            goal_id: Some(goal.id.clone()),
            notes: vec![
                "Decompose the goal into tasks before starting multi-step execution.".into(),
            ],
            history_len: goal.workflow_history.len(),
            ..Default::default()
        };
        report.next_hint = Some("Say “Break this goal into tasks.”".into());
        return Ok((goal, report));
    }

    // Begin understanding if still Idle.
    if goal.agent_lifecycle.state == AgentLifecycleState::Idle {
        goal = transition_goal(
            goal,
            AgentLifecycleEvent::BeginUnderstanding {
                reason: "C5.4 start the goal".into(),
            },
        )
        .map_err(|e| e.to_string_message())?;
    }

    if in_progress_count(&goal) >= MAX_IN_PROGRESS_TASKS {
        return Err("A task is already in progress — finish or cancel it first.".into());
    }

    let Some(next_id) = select_next_task(&goal).map(|t| t.id.clone()) else {
        // All pending blocked by incomplete deps, or nothing left.
        let pending = goal
            .tasks
            .iter()
            .any(|t| t.status == GoalTaskStatus::Pending);
        if pending {
            let mut report = MultiStepExecutionReport {
                status: MultiStepStatus::BlockedDependency,
                goal_id: Some(goal.id.clone()),
                notes: vec![
                    "No ready tasks — dependents remain blocked until prerequisites complete."
                        .into(),
                ],
                history_len: goal.workflow_history.len(),
                ..Default::default()
            };
            report.next_hint =
                Some("Complete or unblock prerequisite tasks before advancing.".into());
            return Ok((goal, report));
        }
        let mut report = MultiStepExecutionReport {
            status: MultiStepStatus::GoalReadyToComplete,
            goal_id: Some(goal.id.clone()),
            notes: vec![
                "All tasks finished — complete the goal with verification evidence (C5.3)."
                    .into(),
            ],
            history_len: goal.workflow_history.len(),
            ..Default::default()
        };
        report.next_hint = Some(
            "Provide completion evidence, then mark the goal complete.".into(),
        );
        return Ok((goal, report));
    };

    let previous_task = goal.active_task_id.clone();
    let (mut goal, task) =
        transition_task_status(goal, &next_id, GoalTaskStatus::InProgress).map_err(|e| e)?;
    goal.active_task_id = Some(task.id.clone());
    // C5.6 — approvals do not leak across tasks.
    if previous_task.as_deref() != Some(task.id.as_str()) {
        goal = crate::approval_autonomy_policy::invalidate_on_task_advance(
            goal,
            previous_task.as_deref(),
            Some(task.id.as_str()),
        );
    }
    push_history(
        &mut goal,
        "start_task",
        Some(&task.id),
        &format!("Started task: {}", task.description),
    );

    // Align lifecycle toward investigation when appropriate.
    if matches!(
        goal.agent_lifecycle.state,
        AgentLifecycleState::Idle | AgentLifecycleState::Understanding
    ) {
        if let Ok(g) = transition_goal(
            goal.clone(),
            AgentLifecycleEvent::BeginInvestigation {
                reason: format!("C5.4 task {}", task.id),
            },
        ) {
            goal = g;
        }
    }

    let hint = hint_for_task(&task);
    let report = MultiStepExecutionReport {
        status: MultiStepStatus::Started,
        goal_id: Some(goal.id.clone()),
        active_task_id: Some(task.id),
        next_hint: Some(hint),
        notes: vec![
            "Started the first ready task — C3 mutations still require Review Before Action.".into(),
            format!(
                "In-progress limit: {MAX_IN_PROGRESS_TASKS}; workflow cap: {MAX_WORKFLOW_TRANSITIONS}."
            ),
        ],
        history_len: goal.workflow_history.len(),
        ..Default::default()
    };
    Ok((goal, report))
}

/// Continue / advance: if a task is in progress, report hints; else start next ready task.
pub fn continue_or_advance(
    goal: CodingGoal,
    advance: bool,
) -> Result<(CodingGoal, MultiStepExecutionReport), String> {
    if goal.workflow_transition_count as usize >= MAX_WORKFLOW_TRANSITIONS {
        return Err(format!(
            "Workflow transition limit ({MAX_WORKFLOW_TRANSITIONS}) reached."
        ));
    }

    if let Some(active) = goal
        .tasks
        .iter()
        .find(|t| t.status == GoalTaskStatus::InProgress)
        .cloned()
    {
        if !advance {
            let mut report = MultiStepExecutionReport {
                status: MultiStepStatus::Continued,
                goal_id: Some(goal.id.clone()),
                active_task_id: Some(active.id.clone()),
                next_hint: Some(hint_for_task(&active)),
                notes: vec![
                    "Active task still in progress — finish investigate/plan/review/apply/verify before advancing."
                        .into(),
                ],
                history_len: goal.workflow_history.len(),
                ..Default::default()
            };
            if goal.agent_lifecycle.state == AgentLifecycleState::AwaitingApproval {
                report.status = MultiStepStatus::NeedsApproval;
                report.notes.push(
                    "AwaitingApproval — mutation execution is stopped until Review Approve."
                        .into(),
                );
            }
            return Ok((goal, report));
        }
        return Err(
            "Cannot advance while a task is still InProgress — complete or fail it first.".into(),
        );
    }

    // Advance / continue with no in-progress → start next ready.
    let (mut goal, mut report) = start_goal_execution(goal)?;
    if report.status == MultiStepStatus::Started {
        report.status = if advance {
            MultiStepStatus::Advanced
        } else {
            MultiStepStatus::Continued
        };
        push_history(
            &mut goal,
            if advance { "advance_task" } else { "continue_task" },
            report.active_task_id.as_deref(),
            "Advanced to next ready task",
        );
        report.history_len = goal.workflow_history.len();
    }
    Ok((goal, report))
}

/// Walk agent lifecycle to Executing after an already-approved C3 apply.
///
/// This observes Review authorization that already happened — it does **not**
/// invent a second Review or silently approve a new plan.
fn sync_lifecycle_after_approved_apply(
    mut goal: CodingGoal,
    plan_id: &str,
) -> Result<CodingGoal, String> {
    if matches!(
        goal.agent_lifecycle.state,
        AgentLifecycleState::Executing | AgentLifecycleState::Verifying
    ) {
        return Ok(goal);
    }

    if goal.agent_lifecycle.state == AgentLifecycleState::Idle {
        goal = transition_goal(
            goal,
            AgentLifecycleEvent::BeginUnderstanding {
                reason: "C5.4 observe approved apply".into(),
            },
        )
        .map_err(|e| e.to_string_message())?;
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::Understanding {
        goal = transition_goal(
            goal,
            AgentLifecycleEvent::BeginInvestigation {
                reason: "C5.4 observe approved apply".into(),
            },
        )
        .map_err(|e| e.to_string_message())?;
    }
    if matches!(
        goal.agent_lifecycle.state,
        AgentLifecycleState::Investigating | AgentLifecycleState::Correcting
    ) {
        goal = transition_goal(
            goal,
            AgentLifecycleEvent::BeginPlanning {
                reason: "C5.4 observe approved apply".into(),
            },
        )
        .map_err(|e| e.to_string_message())?;
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::Planning {
        goal = transition_goal(
            goal,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: plan_id.into(),
            },
        )
        .map_err(|e| e.to_string_message())?;
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::AwaitingApproval {
        if !goal.agent_lifecycle.approval_recorded
            || goal.agent_lifecycle.approved_plan_id.as_deref() != Some(plan_id)
        {
            goal = transition_goal(
                goal,
                AgentLifecycleEvent::RecordApproval {
                    plan_id: plan_id.into(),
                },
            )
            .map_err(|e| e.to_string_message())?;
        }
        goal = transition_goal(
            goal,
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: plan_id.into(),
            },
        )
        .map_err(|e| e.to_string_message())?;
    }
    Ok(goal)
}

fn ensure_verifying(mut goal: CodingGoal, reason: &str) -> Result<CodingGoal, String> {
    if goal.agent_lifecycle.state == AgentLifecycleState::Verifying {
        return Ok(goal);
    }
    if goal.agent_lifecycle.state != AgentLifecycleState::Executing {
        return Err(format!(
            "Cannot verify from {} — need Executing after an approved apply.",
            goal.agent_lifecycle.state.as_str()
        ));
    }
    goal = transition_goal(
        goal,
        AgentLifecycleEvent::BeginVerification {
            reason: reason.into(),
        },
    )
    .map_err(|e| e.to_string_message())?;
    Ok(goal)
}

/// After an approved apply (Planner hook) — sync lifecycle; do not complete task yet if verify expected.
pub fn on_apply_success(
    mut goal: CodingGoal,
    approved_plan_id: &str,
) -> Result<CodingGoal, String> {
    goal = sync_lifecycle_after_approved_apply(goal, approved_plan_id)?;

    goal.verification_state = GoalVerificationState::InProgress;
    let tid = goal.active_task_id.clone();
    push_history(
        &mut goal,
        "apply_success",
        tid.as_deref(),
        &format!("Approved apply recorded for plan `{approved_plan_id}`"),
    );

    // Mark task AwaitingApproval cleared → keep InProgress until verify/evidence.
    if let Some(tid) = tid {
        if let Some(task) = goal.tasks.iter_mut().find(|t| t.id == tid) {
            if task.status == GoalTaskStatus::AwaitingApproval {
                task.status = GoalTaskStatus::InProgress;
            }
            task.notes
                .push(format!("Apply succeeded for plan `{approved_plan_id}`."));
        }
    }
    goal.mutated = false; // orchestration flag — actual FS mutation was via C3 tools already
    goal.tools_executed = false;
    Ok(goal)
}

pub fn on_apply_failure(mut goal: CodingGoal, reason: &str) -> Result<CodingGoal, String> {
    goal = transition_goal(
        goal,
        AgentLifecycleEvent::ExecutionFailed {
            reason: reason.into(),
        },
    )
    .map_err(|e| e.to_string_message())?;
    if let Some(tid) = goal.active_task_id.clone() {
        let (g, _) = transition_task_status(goal, &tid, GoalTaskStatus::Failed)?;
        goal = g;
        goal.active_task_id = None;
        push_history(
            &mut goal,
            "apply_failure",
            Some(&tid),
            reason,
        );
    }
    Ok(goal)
}

/// Verification passed — may complete the active task when evidence suffices.
pub fn on_verify_pass(
    mut goal: CodingGoal,
    evidence_ref: &str,
) -> Result<(CodingGoal, MultiStepExecutionReport), String> {
    goal = ensure_verifying(goal, evidence_ref)?;
    goal = transition_goal(
        goal,
        AgentLifecycleEvent::VerificationPassed {
            evidence_ref: evidence_ref.into(),
        },
    )
    .map_err(|e| e.to_string_message())?;
    goal.verification_state = GoalVerificationState::Passed;

    let Some(tid) = goal.active_task_id.clone() else {
        push_history(&mut goal, "verify_pass", None, evidence_ref);
        let report = MultiStepExecutionReport {
            status: MultiStepStatus::Continued,
            goal_id: Some(goal.id.clone()),
            notes: vec!["Verification passed with no active task bound.".into()],
            history_len: goal.workflow_history.len(),
            ..Default::default()
        };
        return Ok((goal, report));
    };

    let task = goal
        .tasks
        .iter()
        .find(|t| t.id == tid)
        .cloned()
        .ok_or_else(|| format!("Active task `{tid}` missing"))?;

    let evidence = TaskCompletionEvidence {
        summary: format!("Verification passed for task {}", task.description),
        approved_plan_id: goal.agent_lifecycle.approved_plan_id.clone(),
        verification_ref: Some(evidence_ref.into()),
        verification_passed: true,
        investigation_complete: !task.mutation_expected,
    };

    if !evidence.is_sufficient_for(&task) {
        push_history(
            &mut goal,
            "verify_pass_insufficient",
            Some(&tid),
            "Verification passed but task completion evidence incomplete",
        );
        let report = MultiStepExecutionReport {
            status: MultiStepStatus::Continued,
            goal_id: Some(goal.id.clone()),
            active_task_id: Some(tid),
            notes: vec![
                "Verification passed, but task was not auto-completed — evidence incomplete for this task kind."
                    .into(),
            ],
            history_len: goal.workflow_history.len(),
            ..Default::default()
        };
        return Ok((goal, report));
    }

    let (mut goal, _) = transition_task_status(goal, &tid, GoalTaskStatus::Completed)?;
    goal.active_task_id = None;
    push_history(
        &mut goal,
        "task_completed",
        Some(&tid),
        &format!("Task completed with verification `{evidence_ref}`"),
    );

    let ready = !ready_tasks(&goal).is_empty();
    let all_done = goal
        .tasks
        .iter()
        .all(|t| t.status.is_terminal() && t.status != GoalTaskStatus::Failed);

    let mut report = MultiStepExecutionReport {
        status: if all_done && goal.tasks.iter().all(|t| t.status == GoalTaskStatus::Completed) {
            MultiStepStatus::GoalReadyToComplete
        } else if ready {
            MultiStepStatus::Advanced
        } else {
            MultiStepStatus::BlockedDependency
        },
        goal_id: Some(goal.id.clone()),
        active_task_id: None,
        next_hint: if ready {
            Some("Say “Continue the goal” or “Advance to the next task” to start the next ready task.".into())
        } else if all_done {
            Some("All tasks completed — complete the goal with C5.3 completion evidence.".into())
        } else {
            Some("Remaining tasks are blocked on failed/incomplete prerequisites.".into())
        },
        notes: vec![
            "Task marked complete with verification evidence — dependents may unlock.".into(),
            "Next task is NOT auto-started (no unrestricted autonomy).".into(),
        ],
        history_len: goal.workflow_history.len(),
        ..Default::default()
    };
    let _ = &mut report;
    Ok((goal, report))
}

pub fn on_verify_fail(
    mut goal: CodingGoal,
    evidence_ref: &str,
) -> Result<(CodingGoal, MultiStepExecutionReport), String> {
    goal = ensure_verifying(goal, evidence_ref)?;
    goal = transition_goal(
        goal,
        AgentLifecycleEvent::VerificationFailed {
            evidence_ref: evidence_ref.into(),
        },
    )
    .map_err(|e| e.to_string_message())?;
    goal.verification_state = GoalVerificationState::Failed;

    let tid = goal.active_task_id.clone();
    if let Some(tid) = &tid {
        let (g, _) = transition_task_status(goal, tid, GoalTaskStatus::Failed)?;
        goal = g;
        goal.active_task_id = None;
        // Dependents stay blocked — Failed ≠ Completed.
        push_history(
            &mut goal,
            "verify_fail",
            Some(tid),
            &format!("Verification failed `{evidence_ref}` — dependents remain blocked"),
        );
    }

    let report = MultiStepExecutionReport {
        status: MultiStepStatus::VerificationFailed,
        goal_id: Some(goal.id.clone()),
        active_task_id: None,
        next_hint: Some(
            "Correcting: prepare a new reviewed C3 Coding Plan (C4.4) — no auto-mutation.".into(),
        ),
        notes: vec![
            "Verification failure does not unlock dependent tasks.".into(),
            "No automatic corrective mutation.".into(),
        ],
        history_len: goal.workflow_history.len(),
        ..Default::default()
    };
    Ok((goal, report))
}

/// Complete an investigation-only (or evidenced) active task without auto-starting the next.
pub fn complete_active_task_with_evidence(
    goal: CodingGoal,
    evidence: TaskCompletionEvidence,
) -> Result<(CodingGoal, MultiStepExecutionReport), String> {
    let tid = goal
        .active_task_id
        .clone()
        .ok_or_else(|| "No active task to complete.".to_string())?;
    let task = goal
        .tasks
        .iter()
        .find(|t| t.id == tid)
        .cloned()
        .ok_or_else(|| format!("Unknown task `{tid}`"))?;
    if !evidence.is_sufficient_for(&task) {
        return Err(
            "Insufficient task completion evidence (need apply/verification as required).".into(),
        );
    }
    let (mut goal, _) = transition_task_status(goal, &tid, GoalTaskStatus::Completed)?;
    goal.active_task_id = None;
    push_history(
        &mut goal,
        "task_completed_manual",
        Some(&tid),
        &evidence.summary,
    );
    let report = MultiStepExecutionReport {
        status: MultiStepStatus::Advanced,
        goal_id: Some(goal.id.clone()),
        next_hint: Some(
            "Task completed — say “Continue the goal” to start the next ready task.".into(),
        ),
        notes: vec!["Task completed with evidence — next task not auto-started.".into()],
        history_len: goal.workflow_history.len(),
        ..Default::default()
    };
    Ok((goal, report))
}

pub fn detect_multi_step_execution_request(
    request: &UserRequest,
) -> Option<MultiStepAssessment> {
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
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::fix_from_verification::steals_from_other_detect(request)
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "start the goal"
            | "start this goal"
            | "begin the goal"
            | "start multi-step execution"
            | "start the coding goal"
    ) || trimmed.starts_with("start the goal")
        || trimmed.starts_with("start this goal")
    {
        return Some(MultiStepAssessment {
            kind: MultiStepRequestKind::StartGoal,
            matched_rules: vec!["start_goal"],
        });
    }

    if matches!(
        trimmed.as_str(),
        "continue the goal"
            | "continue this goal"
            | "keep going on this goal"
            | "resume work on this goal"
            | "continue multi-step"
    ) || trimmed.starts_with("continue the goal")
        || trimmed.starts_with("continue this goal")
        || trimmed.starts_with("keep going on this goal")
    {
        return Some(MultiStepAssessment {
            kind: MultiStepRequestKind::ContinueGoal,
            matched_rules: vec!["continue_goal"],
        });
    }

    if matches!(
        trimmed.as_str(),
        "advance to the next task"
            | "start the next task"
            | "next task"
            | "continue to the next task"
            | "advance next task"
    ) || trimmed.starts_with("advance to the next")
        || trimmed.starts_with("start the next task")
    {
        return Some(MultiStepAssessment {
            kind: MultiStepRequestKind::AdvanceNextTask,
            matched_rules: vec!["advance_next_task"],
        });
    }

    if matches!(
        trimmed.as_str(),
        "goal progress"
            | "what's the next task"
            | "what is the next task"
            | "multi-step status"
            | "workflow status"
    ) {
        return Some(MultiStepAssessment {
            kind: MultiStepRequestKind::Status,
            matched_rules: vec!["multi_step_status"],
        });
    }

    None
}

pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &MultiStepAssessment,
) -> (MultiStepExecutionReport, Option<CodingGoal>) {
    let Some(goal) = goal else {
        return (
            MultiStepExecutionReport {
                status: MultiStepStatus::NoActiveGoal,
                notes: vec!["No active Coding Goal.".into()],
                ..Default::default()
            },
            None,
        );
    };

    if !crate::agent_pause_resume_cancel::allows_new_operations(goal) {
        let paused = goal.agent_lifecycle.state == AgentLifecycleState::Paused;
        return (
            MultiStepExecutionReport {
                status: MultiStepStatus::Rejected,
                goal_id: Some(goal.id.clone()),
                notes: vec![if paused {
                    "Goal is paused — say “Resume this goal.” first.".into()
                } else {
                    "Goal is cancelled — will not continue.".into()
                }],
                next_hint: if paused {
                    Some("Resume this goal.".into())
                } else {
                    None
                },
                ..Default::default()
            },
            None,
        );
    }

    match assessment.kind {
        MultiStepRequestKind::Status => {
            let active = goal
                .tasks
                .iter()
                .find(|t| t.status == GoalTaskStatus::InProgress);
            let report = MultiStepExecutionReport {
                status: MultiStepStatus::Status,
                goal_id: Some(goal.id.clone()),
                active_task_id: active.map(|t| t.id.clone()),
                next_hint: active.map(hint_for_task).or_else(|| {
                    select_next_task(goal).map(|t| {
                        format!("Next ready task: {} (`{}`)", t.description, t.id)
                    })
                }),
                notes: vec![format!(
                    "Workflow transitions: {}/{}; in-progress: {}/{}",
                    goal.workflow_transition_count,
                    MAX_WORKFLOW_TRANSITIONS,
                    in_progress_count(goal),
                    MAX_IN_PROGRESS_TASKS
                )],
                history_len: goal.workflow_history.len(),
                ..Default::default()
            };
            (report, Some(goal.clone()))
        }
        MultiStepRequestKind::StartGoal => match start_goal_execution(goal.clone()) {
            Ok((g, r)) => (r, Some(g)),
            Err(e) => (
                MultiStepExecutionReport {
                    status: MultiStepStatus::Rejected,
                    goal_id: Some(goal.id.clone()),
                    notes: vec![e],
                    ..Default::default()
                },
                None,
            ),
        },
        MultiStepRequestKind::ContinueGoal => match continue_or_advance(goal.clone(), false) {
            Ok((g, r)) => (r, Some(g)),
            Err(e) => (
                MultiStepExecutionReport {
                    status: MultiStepStatus::Rejected,
                    goal_id: Some(goal.id.clone()),
                    notes: vec![e],
                    ..Default::default()
                },
                None,
            ),
        },
        MultiStepRequestKind::AdvanceNextTask => match continue_or_advance(goal.clone(), true) {
            Ok((g, r)) => (r, Some(g)),
            Err(e) => (
                MultiStepExecutionReport {
                    status: MultiStepStatus::Rejected,
                    goal_id: Some(goal.id.clone()),
                    notes: vec![e],
                    ..Default::default()
                },
                None,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::{apply_decomposition, propose_tasks};
    use jaymi_context::ContextBundle;

    fn goal_with_tasks() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(
            "Add image generation to the application.",
        ))
        .unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let (tasks, order) = propose_tasks(&g).unwrap();
        apply_decomposition(g, tasks, order)
    }

    #[test]
    fn two_sequential_tasks() {
        let g = goal_with_tasks();
        assert!(g.tasks.len() >= 2);
        let (g, r) = start_goal_execution(g).unwrap();
        assert_eq!(r.status, MultiStepStatus::Started);
        let t1 = g.active_task_id.clone().unwrap();
        let evidence = TaskCompletionEvidence {
            summary: "Investigation done".into(),
            approved_plan_id: None,
            verification_ref: None,
            verification_passed: false,
            investigation_complete: true,
        };
        // First task in add_feature is investigation (no mutation).
        let (g, _) = complete_active_task_with_evidence(g, evidence).unwrap();
        assert!(g.active_task_id.is_none());
        let (g, r2) = continue_or_advance(g, true).unwrap();
        assert!(matches!(
            r2.status,
            MultiStepStatus::Advanced | MultiStepStatus::Started | MultiStepStatus::Continued
        ));
        assert!(g.active_task_id.is_some());
        assert_ne!(g.active_task_id.as_deref(), Some(t1.as_str()));
    }

    #[test]
    fn dependent_tasks_blocked_until_prereq_completes() {
        let g = goal_with_tasks();
        let second = g.tasks[1].id.clone();
        let err = transition_task_status(g.clone(), &second, GoalTaskStatus::InProgress);
        assert!(err.is_err());
        let (g, _) = start_goal_execution(g).unwrap();
        assert_eq!(g.tasks[0].status, GoalTaskStatus::InProgress);
    }

    #[test]
    fn failed_task_does_not_unlock_dependents() {
        let g = goal_with_tasks();
        let (g, _) = start_goal_execution(g).unwrap();
        let tid = g.active_task_id.clone().unwrap();
        // Force fail
        let (g, _) = transition_task_status(g, &tid, GoalTaskStatus::Failed).unwrap();
        assert!(select_next_task(&g).is_none() || {
            // later tasks may be ready only if they don't depend on t1
            let ready = ready_tasks(&g);
            ready.iter().all(|t| !t.dependencies.contains(&tid))
        });
        // Direct dependent of first task must not be ready
        let first_dependents: Vec<_> = g
            .tasks
            .iter()
            .filter(|t| t.dependencies.contains(&tid))
            .collect();
        for d in first_dependents {
            assert_ne!(d.status, GoalTaskStatus::InProgress);
            assert!(ready_tasks(&g).iter().all(|r| r.id != d.id));
        }
    }

    #[test]
    fn verification_failure_path() {
        let g = goal_with_tasks();
        let (g, _) = start_goal_execution(g).unwrap();
        let g = on_apply_success(g, "plan-verify-fail").unwrap();
        assert_eq!(g.agent_lifecycle.state, AgentLifecycleState::Executing);
        let (g, report) = on_verify_fail(g, "ver-fail-1").unwrap();
        assert_eq!(report.status, MultiStepStatus::VerificationFailed);
        assert_eq!(
            g.agent_lifecycle.state,
            AgentLifecycleState::Correcting
        );
        // Dependents must not unlock after verification failure.
        assert!(ready_tasks(&g).is_empty() || {
            let failed_ids: Vec<_> = g
                .tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Failed)
                .map(|t| t.id.clone())
                .collect();
            ready_tasks(&g)
                .iter()
                .all(|t| t.dependencies.iter().all(|d| !failed_ids.contains(d)))
        });
    }

    #[test]
    fn no_advance_while_in_progress() {
        let g = goal_with_tasks();
        let (g, _) = start_goal_execution(g).unwrap();
        assert!(continue_or_advance(g, true).is_err());
    }

    #[test]
    fn workflow_bound() {
        let mut g = goal_with_tasks();
        g.workflow_transition_count = MAX_WORKFLOW_TRANSITIONS as u32;
        assert!(start_goal_execution(g).is_err());
    }

    #[test]
    fn detect_phrases() {
        assert_eq!(
            detect_multi_step_execution_request(&UserRequest::new("Start the goal."))
                .unwrap()
                .kind,
            MultiStepRequestKind::StartGoal
        );
        assert_eq!(
            detect_multi_step_execution_request(&UserRequest::new("Continue the goal."))
                .unwrap()
                .kind,
            MultiStepRequestKind::ContinueGoal
        );
        assert!(detect_multi_step_execution_request(&UserRequest::new(
            "Resume this goal."
        ))
        .is_none()); // C5.3 owns resume
    }
}
