//! Agentic Coding Experience (Sprint C5.12).
//!
//! Integrate C5.1–C5.11 into one coherent conversational coding experience.
//!
//! ```text
//! User: "Build me a Pong game."
//!   → Coding Goal (C5.1)
//!   → decompose / investigate / plan / approve / implement / verify
//!   → diagnose → reviewed corrections → continue → evidence-based completion
//! ```
//!
//! Constitutional constraints:
//! - Reuse C2/C3/C4; Planner remains the sole orchestrator
//! - No parallel coding agent architecture
//! - Conversation is the primary interface
//! - No chain-of-thought; no hidden filesystem / model-to-tool execution
//! - Every consequential mutation retains Review Before Action
//! - Corrective mutations require a **new** review
//! - Verification + evidence-based completion determine success
//! - Cancelled goals cannot silently continue; no silent destructive retries
//! - No web research / image generation / messaging / computer control

use crate::agent_progress_ux::{derive_progress, AgentProgressView, ProgressPhase};
use crate::agent_state_machine::AgentLifecycleState;
use crate::coding_goal::{
    create_goal, detect_coding_goal_request, CodingGoal, CodingGoalRequestKind, CodingGoalStatus,
    GoalVerificationState,
};
use crate::goal_decomposition::GoalTaskStatus;
use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

/// Stages of the agentic coding experience (narrative only — not a second agent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceStage {
    Understand,
    Goal,
    Decompose,
    Investigate,
    Plan,
    Approve,
    Implement,
    Verify,
    Diagnose,
    Correct,
    Continue,
    Complete,
}

impl ExperienceStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Understand => "understand",
            Self::Goal => "goal",
            Self::Decompose => "decompose",
            Self::Investigate => "investigate",
            Self::Plan => "plan",
            Self::Approve => "approve",
            Self::Implement => "implement",
            Self::Verify => "verify",
            Self::Diagnose => "diagnose",
            Self::Correct => "correct",
            Self::Continue => "continue",
            Self::Complete => "complete",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Understand => "Understand",
            Self::Goal => "Coding Goal",
            Self::Decompose => "Decompose",
            Self::Investigate => "Investigate",
            Self::Plan => "Plan",
            Self::Approve => "Request approval",
            Self::Implement => "Implement",
            Self::Verify => "Verify",
            Self::Diagnose => "Diagnose failures",
            Self::Correct => "Propose reviewed corrections",
            Self::Continue => "Continue",
            Self::Complete => "Report completion",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageState {
    Pending,
    Current,
    Done,
    Blocked,
    Skipped,
}

impl StageState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Current => "current",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Skipped => "skipped",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pending => "○",
            Self::Current => "→",
            Self::Done => "✓",
            Self::Blocked => "!",
            Self::Skipped => "–",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExperienceStageView {
    pub stage: ExperienceStage,
    pub state: StageState,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgenticExperienceStatus {
    Created,
    Status,
    NoActiveGoal,
    Rejected,
}

impl AgenticExperienceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Status => "status",
            Self::NoActiveGoal => "no_active_goal",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgenticExperienceRequestKind {
    Begin,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgenticExperienceAssessment {
    pub kind: AgenticExperienceRequestKind,
    pub matched_rules: Vec<&'static str>,
    pub objective: String,
}

/// Coherent experience report composing C5.1–C5.11 (Planner-owned narration).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgenticCodingExperienceReport {
    pub status: AgenticExperienceStatus,
    pub goal_id: Option<String>,
    pub objective: String,
    pub current_stage: ExperienceStage,
    pub stages: Vec<ExperienceStageView>,
    pub phase_label: String,
    pub what_planned: String,
    pub what_doing: String,
    pub approval_needed: bool,
    pub what_changed: String,
    pub whether_worked: String,
    pub what_went_wrong: String,
    pub what_remains: String,
    pub progress: Option<AgentProgressView>,
    pub mutated: bool,
    pub tools_executed: bool,
    pub exposes_chain_of_thought: bool,
    pub parallel_coding_agent: bool,
    pub silent_continue_after_cancel: bool,
    pub silent_destructive_retry: bool,
    pub hidden_filesystem_access: bool,
    pub model_direct_tools: bool,
    pub unrelated_capabilities: bool,
    pub planner_owned: bool,
    pub conversation_primary: bool,
    pub notes: Vec<String>,
}

impl Default for AgenticCodingExperienceReport {
    fn default() -> Self {
        Self {
            status: AgenticExperienceStatus::Rejected,
            goal_id: None,
            objective: String::new(),
            current_stage: ExperienceStage::Understand,
            stages: Vec::new(),
            phase_label: String::new(),
            what_planned: String::new(),
            what_doing: String::new(),
            approval_needed: false,
            what_changed: String::new(),
            whether_worked: String::new(),
            what_went_wrong: String::new(),
            what_remains: String::new(),
            progress: None,
            mutated: false,
            tools_executed: false,
            exposes_chain_of_thought: false,
            parallel_coding_agent: false,
            silent_continue_after_cancel: false,
            silent_destructive_retry: false,
            hidden_filesystem_access: false,
            model_direct_tools: false,
            unrelated_capabilities: false,
            planner_owned: true,
            conversation_primary: true,
            notes: Vec::new(),
        }
    }
}

impl AgenticCodingExperienceReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Agentic Coding Experience\n\n");
        out.push_str("_One coherent path for a coding objective — Planner-orchestrated, Conversation First._\n\n");
        if !self.objective.is_empty() {
            out.push_str(&format!("**Objective:** {}\n\n", self.objective));
        }
        if let Some(id) = &self.goal_id {
            out.push_str(&format!("Goal: `{id}` · Phase: **{}**\n\n", self.phase_label));
        }

        out.push_str("### Path\n");
        for s in &self.stages {
            out.push_str(&format!(
                "{} **{}** — {}\n",
                s.state.glyph(),
                s.stage.label(),
                s.detail
            ));
        }

        out.push_str("\n### At a glance\n");
        out.push_str(&format!("- **Plan:** {}\n", self.what_planned));
        out.push_str(&format!("- **Now:** {}\n", self.what_doing));
        out.push_str(&format!(
            "- **Approval:** {}\n",
            if self.approval_needed {
                "needed before mutation (Review Before Action)"
            } else {
                "not blocking right now"
            }
        ));
        out.push_str(&format!("- **Changed:** {}\n", self.what_changed));
        out.push_str(&format!("- **Worked?:** {}\n", self.whether_worked));
        out.push_str(&format!("- **Went wrong:** {}\n", self.what_went_wrong));
        out.push_str(&format!("- **Remains:** {}\n", self.what_remains));

        if let Some(progress) = &self.progress {
            out.push('\n');
            out.push_str(&progress.to_markdown());
        }

        out.push_str("\n### Next (Conversation First)\n");
        out.push_str("- Break this goal into tasks.\n");
        out.push_str("- Start the goal. / Continue the goal.\n");
        out.push_str("- Show goal progress. / Pause this goal. / Cancel this goal.\n");
        out.push_str("- Assess goal completion. / Recover this goal.\n");

        out.push_str("\n### Guards\n");
        out.push_str(&format!(
            "- Parallel coding agent: {}\n",
            flag(self.parallel_coding_agent)
        ));
        out.push_str(&format!(
            "- Chain-of-thought exposed: {}\n",
            flag(self.exposes_chain_of_thought)
        ));
        out.push_str(&format!(
            "- Silent continue after cancel: {}\n",
            flag(self.silent_continue_after_cancel)
        ));
        out.push_str(&format!(
            "- Silent destructive retry: {}\n",
            flag(self.silent_destructive_retry)
        ));
        out.push_str(&format!(
            "- Hidden filesystem / model→tool: {}\n",
            flag(self.hidden_filesystem_access || self.model_direct_tools)
        ));
        out.push_str(&format!(
            "- Mutation / tools this turn: {} / {}\n",
            flag(self.mutated),
            flag(self.tools_executed)
        ));
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str(&format!("- {n}\n"));
            }
        }
        out.push_str(
            "\n_Reuses C2/C3/C4 + C5.1–C5.11. No web research, image generation, messaging, or computer control._\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "agentic_experience status={} stage={} phase={} approval={} mutated={} tools={} cot={} parallel={} cancel_continue={} destructive_retry={} hidden_fs={} model_tools={} unrelated={} planner_owned={} conversation={}",
            self.status.as_str(),
            self.current_stage.as_str(),
            self.phase_label,
            self.approval_needed,
            self.mutated,
            self.tools_executed,
            self.exposes_chain_of_thought,
            self.parallel_coding_agent,
            self.silent_continue_after_cancel,
            self.silent_destructive_retry,
            self.hidden_filesystem_access,
            self.model_direct_tools,
            self.unrelated_capabilities,
            self.planner_owned,
            self.conversation_primary,
        )
    }
}

fn flag(bad: bool) -> &'static str {
    if bad {
        "YES (bug)"
    } else {
        "no"
    }
}

/// Derive the experience narrative from an existing Coding Goal (compose-only).
pub fn narrate_experience(goal: &CodingGoal) -> AgenticCodingExperienceReport {
    let progress = derive_progress(goal);
    let phase = progress.phase;
    let stages = build_stages(goal, phase);
    let current_stage = stages
        .iter()
        .find(|s| s.state == StageState::Current)
        .map(|s| s.stage)
        .unwrap_or_else(|| stage_from_phase(phase));

    let cancelled = goal.status == CodingGoalStatus::Cancelled
        || goal.control.cancelled
        || goal.agent_lifecycle.state == AgentLifecycleState::Cancelled;

    let what_planned = if goal.tasks.is_empty() {
        format!(
            "Accomplish “{}” as a multi-step Coding Goal (tasks not decomposed yet).",
            truncate(&goal.user_objective, 120)
        )
    } else {
        format!(
            "{} proposed task(s); current focus: {}.",
            goal.tasks.len(),
            goal.current_task
                .as_deref()
                .or_else(|| goal.active_task_id.as_deref())
                .unwrap_or("none")
        )
    };

    let what_doing = match phase {
        ProgressPhase::NotStarted => "Goal recorded — waiting to start / decompose.".into(),
        ProgressPhase::Understanding => "Understanding the objective.".into(),
        ProgressPhase::Investigating => "Investigating the codebase (C2).".into(),
        ProgressPhase::Planning => "Authoring a Coding Plan (C3) — proposal only.".into(),
        ProgressPhase::ApprovalRequired => {
            "Waiting for Review Before Action before any mutation.".into()
        }
        ProgressPhase::Executing => "Implementing an approved plan/task.".into(),
        ProgressPhase::Verifying => "Running verification (C4).".into(),
        ProgressPhase::Correcting => {
            "Preparing a new reviewed corrective plan — prior approval not reused.".into()
        }
        ProgressPhase::Blocked => "Blocked — needs your decision.".into(),
        ProgressPhase::Paused => "Paused — not executing while you are away.".into(),
        ProgressPhase::Failed => "Failed — see failures; recovery available.".into(),
        ProgressPhase::Cancelled => "Cancelled — will not silently continue.".into(),
        ProgressPhase::Completed => "Completed (evidence-based).".into(),
    };

    let approval_needed = progress.approval_required
        || goal.agent_lifecycle.state == AgentLifecycleState::AwaitingApproval
        || goal.control.requires_fresh_approval;

    let what_changed = if goal.completed_work.is_empty() {
        if goal.mutated || goal.tools_executed {
            "Mutation/tools flagged on goal — inspect apply summaries.".into()
        } else {
            "No completed-work entries yet (plans may exist as proposals only).".into()
        }
    } else {
        format!(
            "{} completed-work item(s); latest: {}",
            goal.completed_work.len(),
            truncate(goal.completed_work.last().map(|s| s.as_str()).unwrap_or(""), 100)
        )
    };

    let whether_worked = match goal.verification_state {
        GoalVerificationState::Passed => {
            if goal.status == CodingGoalStatus::Completed
                || goal.completion.ending_kind
                    == Some(crate::coding_goal_completion::GoalEndingKind::SuccessfullyCompleted)
            {
                "Verification passed and goal marked successfully completed.".into()
            } else {
                "Verification passed — completion still requires evidence-based check.".into()
            }
        }
        GoalVerificationState::Failed => {
            "Verification failed — success not claimed.".into()
        }
        GoalVerificationState::InProgress => "Verification in progress.".into(),
        GoalVerificationState::Blocked => "Verification blocked.".into(),
        GoalVerificationState::Unknown => "Verification outcome unknown — not treated as success.".into(),
        GoalVerificationState::NotStarted => "Verification not started yet.".into(),
    };

    let mut wrongs = Vec::new();
    for f in &progress.failures {
        wrongs.push(f.clone());
    }
    for b in &goal.blockers {
        wrongs.push(format!("blocker: {b}"));
    }
    if let Some(err) = &goal.agent_lifecycle.last_error {
        wrongs.push(err.clone());
    }
    let what_went_wrong = if wrongs.is_empty() {
        "Nothing recorded as failed right now.".into()
    } else {
        wrongs.into_iter().take(4).collect::<Vec<_>>().join("; ")
    };

    let remains = if cancelled {
        "Goal cancelled — remaining work will not run.".into()
    } else if goal.status == CodingGoalStatus::Completed {
        "Nothing — goal successfully completed.".into()
    } else if !goal.remaining_work.is_empty() {
        truncate(&goal.remaining_work.join("; "), 160)
    } else {
        let pending = goal
            .tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    GoalTaskStatus::Pending | GoalTaskStatus::InProgress | GoalTaskStatus::Blocked
                )
            })
            .count();
        if pending > 0 {
            format!("{pending} open task(s) remain.")
        } else {
            "Assess goal completion when verification evidence is ready.".into()
        }
    };

    AgenticCodingExperienceReport {
        status: AgenticExperienceStatus::Status,
        goal_id: Some(goal.id.clone()),
        objective: goal.user_objective.clone(),
        current_stage,
        stages,
        phase_label: progress.phase_label.clone(),
        what_planned,
        what_doing,
        approval_needed,
        what_changed,
        whether_worked,
        what_went_wrong,
        what_remains: remains,
        progress: Some(progress),
        mutated: false,
        tools_executed: false,
        exposes_chain_of_thought: false,
        parallel_coding_agent: false,
        silent_continue_after_cancel: false,
        silent_destructive_retry: false,
        hidden_filesystem_access: false,
        model_direct_tools: false,
        unrelated_capabilities: false,
        planner_owned: true,
        conversation_primary: true,
        notes: vec![
            "Narrative composes C5.1–C5.11 — does not invent a parallel agent.".into(),
            "C2/C3/C4 remain the investigation / plan / verify engines.".into(),
        ],
    }
}

fn stage_from_phase(phase: ProgressPhase) -> ExperienceStage {
    match phase {
        ProgressPhase::NotStarted | ProgressPhase::Understanding => ExperienceStage::Understand,
        ProgressPhase::Investigating => ExperienceStage::Investigate,
        ProgressPhase::Planning => ExperienceStage::Plan,
        ProgressPhase::ApprovalRequired => ExperienceStage::Approve,
        ProgressPhase::Executing => ExperienceStage::Implement,
        ProgressPhase::Verifying => ExperienceStage::Verify,
        ProgressPhase::Correcting => ExperienceStage::Correct,
        ProgressPhase::Blocked => ExperienceStage::Diagnose,
        ProgressPhase::Paused => ExperienceStage::Continue,
        ProgressPhase::Failed => ExperienceStage::Diagnose,
        ProgressPhase::Cancelled => ExperienceStage::Goal,
        ProgressPhase::Completed => ExperienceStage::Complete,
    }
}

fn build_stages(goal: &CodingGoal, phase: ProgressPhase) -> Vec<ExperienceStageView> {
    let has_tasks = !goal.tasks.is_empty();
    let has_plan = goal.agent_lifecycle.approved_plan_id.is_some()
        || matches!(
            goal.agent_lifecycle.state,
            AgentLifecycleState::Planning
                | AgentLifecycleState::AwaitingApproval
                | AgentLifecycleState::Executing
                | AgentLifecycleState::Verifying
                | AgentLifecycleState::Correcting
        );
    let investigated = matches!(
        goal.agent_lifecycle.state,
        AgentLifecycleState::Investigating
            | AgentLifecycleState::Planning
            | AgentLifecycleState::AwaitingApproval
            | AgentLifecycleState::Executing
            | AgentLifecycleState::Verifying
            | AgentLifecycleState::Correcting
            | AgentLifecycleState::Completed
    ) || phase == ProgressPhase::Investigating;
    let verifying_done = goal.verification_state == GoalVerificationState::Passed
        || goal.status == CodingGoalStatus::Completed;
    let correcting = phase == ProgressPhase::Correcting
        || goal.agent_lifecycle.state == AgentLifecycleState::Correcting
        || !goal.recovery.history.is_empty();
    let completed = goal.status == CodingGoalStatus::Completed
        || phase == ProgressPhase::Completed;

    let order = [
        ExperienceStage::Understand,
        ExperienceStage::Goal,
        ExperienceStage::Decompose,
        ExperienceStage::Investigate,
        ExperienceStage::Plan,
        ExperienceStage::Approve,
        ExperienceStage::Implement,
        ExperienceStage::Verify,
        ExperienceStage::Diagnose,
        ExperienceStage::Correct,
        ExperienceStage::Continue,
        ExperienceStage::Complete,
    ];

    let current = stage_from_phase(phase);
    let mut out = Vec::new();
    for stage in order {
        let (state, detail) = match stage {
            ExperienceStage::Understand => {
                if matches!(
                    phase,
                    ProgressPhase::NotStarted | ProgressPhase::Understanding
                ) && goal.agent_lifecycle.state == AgentLifecycleState::Idle
                {
                    (
                        StageState::Current,
                        "Capture what you want built.".into(),
                    )
                } else if goal.agent_lifecycle.state != AgentLifecycleState::Idle
                    || !goal.user_objective.is_empty()
                {
                    (
                        StageState::Done,
                        "Objective understood as a Coding Goal.".into(),
                    )
                } else {
                    (StageState::Pending, "Not started.".into())
                }
            }
            ExperienceStage::Goal => (
                StageState::Done,
                format!("Coding Goal `{}` is Planner-owned.", goal.id),
            ),
            ExperienceStage::Decompose => {
                if has_tasks {
                    (
                        StageState::Done,
                        format!("{} task(s) proposed.", goal.tasks.len()),
                    )
                } else if current == ExperienceStage::Decompose {
                    (
                        StageState::Current,
                        "Say **Break this goal into tasks.**".into(),
                    )
                } else {
                    (
                        StageState::Pending,
                        "Decompose when ready (Conversation First).".into(),
                    )
                }
            }
            ExperienceStage::Investigate => {
                if investigated && phase != ProgressPhase::Investigating {
                    (StageState::Done, "Investigation evidence available / reusable (C2).".into())
                } else if phase == ProgressPhase::Investigating {
                    (StageState::Current, "Investigating with C2 tools.".into())
                } else {
                    (StageState::Pending, "Investigate before mutating.".into())
                }
            }
            ExperienceStage::Plan => {
                if has_plan && phase != ProgressPhase::Planning {
                    (StageState::Done, "Coding Plan authored (proposal).".into())
                } else if phase == ProgressPhase::Planning {
                    (StageState::Current, "Planning changes (C3).".into())
                } else {
                    (StageState::Pending, "Plan from investigation.".into())
                }
            }
            ExperienceStage::Approve => {
                if phase == ProgressPhase::ApprovalRequired || goal.control.requires_fresh_approval {
                    (
                        StageState::Current,
                        "Review Before Action required.".into(),
                    )
                } else if goal.agent_lifecycle.approval_recorded {
                    (StageState::Done, "Approval recorded for current plan/task.".into())
                } else {
                    (StageState::Pending, "Approve before mutation.".into())
                }
            }
            ExperienceStage::Implement => {
                if phase == ProgressPhase::Executing {
                    (StageState::Current, "Implementing approved ops.".into())
                } else if !goal.completed_work.is_empty()
                    || matches!(
                        phase,
                        ProgressPhase::Verifying
                            | ProgressPhase::Correcting
                            | ProgressPhase::Completed
                    )
                {
                    (StageState::Done, "Implementation step(s) recorded.".into())
                } else {
                    (StageState::Pending, "Implement only after approval.".into())
                }
            }
            ExperienceStage::Verify => match goal.verification_state {
                GoalVerificationState::Passed => {
                    (StageState::Done, "Verification passed.".into())
                }
                GoalVerificationState::Failed => {
                    (StageState::Blocked, "Verification failed.".into())
                }
                GoalVerificationState::InProgress => {
                    (StageState::Current, "Verifying (C4).".into())
                }
                _ if phase == ProgressPhase::Verifying => {
                    (StageState::Current, "Verifying (C4).".into())
                }
                _ => (StageState::Pending, "Verify after apply.".into()),
            },
            ExperienceStage::Diagnose => {
                if goal.verification_state == GoalVerificationState::Failed
                    || phase == ProgressPhase::Failed
                    || phase == ProgressPhase::Blocked
                {
                    (
                        if matches!(phase, ProgressPhase::Failed | ProgressPhase::Blocked)
                            || (goal.verification_state == GoalVerificationState::Failed
                                && phase != ProgressPhase::Correcting)
                        {
                            StageState::Current
                        } else {
                            StageState::Done
                        },
                        "Diagnose failures from verification evidence.".into(),
                    )
                } else {
                    (StageState::Skipped, "No failure to diagnose.".into())
                }
            }
            ExperienceStage::Correct => {
                if correcting {
                    if phase == ProgressPhase::Correcting {
                        (
                            StageState::Current,
                            "New reviewed corrective plan required.".into(),
                        )
                    } else {
                        (
                            StageState::Done,
                            "Corrective recovery history preserved.".into(),
                        )
                    }
                } else {
                    (StageState::Skipped, "No correction in flight.".into())
                }
            }
            ExperienceStage::Continue => {
                if phase == ProgressPhase::Paused {
                    (
                        StageState::Current,
                        "Paused — resume after revalidation if needed.".into(),
                    )
                } else if has_tasks
                    && goal.tasks.iter().any(|t| {
                        matches!(t.status, GoalTaskStatus::Pending | GoalTaskStatus::InProgress)
                    })
                {
                    (
                        StageState::Pending,
                        "Continue remaining tasks when ready.".into(),
                    )
                } else {
                    (StageState::Skipped, "No continue step required.".into())
                }
            }
            ExperienceStage::Complete => {
                if completed || verifying_done && phase == ProgressPhase::Completed {
                    (
                        StageState::Done,
                        "Evidence-based completion recorded.".into(),
                    )
                } else if phase == ProgressPhase::Completed {
                    (StageState::Current, "Reporting completion.".into())
                } else {
                    (
                        StageState::Pending,
                        "Complete only with verification / waiver evidence.".into(),
                    )
                }
            }
        };
        // Prefer a single Current: if this stage matches current from phase and not already set specially.
        let state = if stage == current
            && !matches!(state, StageState::Done | StageState::Skipped | StageState::Blocked)
        {
            StageState::Current
        } else if stage == current && state == StageState::Pending {
            StageState::Current
        } else {
            state
        };
        out.push(ExperienceStageView {
            stage,
            state,
            detail,
        });
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

/// Detect agentic experience begin/status phrases.
pub fn detect_agentic_experience_request(
    request: &UserRequest,
) -> Option<AgenticExperienceAssessment> {
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

    // Yield control / recovery / completion / continuity / progress / lifecycle.
    if trimmed.starts_with("pause ")
        || trimmed.starts_with("resume ")
        || trimmed.starts_with("cancel ")
        || trimmed.starts_with("stop this goal")
        || trimmed.starts_with("recover ")
        || trimmed.starts_with("mark this goal")
        || trimmed.starts_with("complete this goal")
        || trimmed.starts_with("waive ")
        || trimmed.starts_with("end this goal")
        || trimmed.starts_with("assess goal completion")
        || trimmed.starts_with("revalidate ")
        || trimmed.starts_with("prepare long")
        || trimmed.starts_with("session continuity")
        || trimmed.starts_with("show goal progress")
        || trimmed.starts_with("break this goal")
        || trimmed.starts_with("start the goal")
        || trimmed.starts_with("continue the goal")
        || trimmed.starts_with("this goal is blocked")
        || trimmed.starts_with("block this goal")
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "show agentic coding experience"
            | "agentic coding experience"
            | "how is this coding goal going end-to-end"
            | "how is this coding goal going end to end"
            | "coding experience status"
    ) || trimmed.starts_with("show agentic coding experience")
        || trimmed.starts_with("how is this coding goal going")
    {
        return Some(AgenticExperienceAssessment {
            kind: AgenticExperienceRequestKind::Status,
            matched_rules: vec!["c512_status"],
            objective: content.to_string(),
        });
    }

    // Begin: multi-step / greenfield goal create (reuse C5.1 detect).
    if let Some(assessment) = detect_coding_goal_request(request) {
        if assessment.kind == CodingGoalRequestKind::Create {
            return Some(AgenticExperienceAssessment {
                kind: AgenticExperienceRequestKind::Begin,
                matched_rules: {
                    let mut r = vec!["c512_begin"];
                    r.extend(assessment.matched_rules.iter().copied());
                    r
                },
                objective: assessment.objective,
            });
        }
    }

    None
}

/// Apply experience assessment: create goal (Begin) or narrate (Status).
pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &AgenticExperienceAssessment,
    context: &ContextBundle,
    conversation_id: Option<String>,
) -> (AgenticCodingExperienceReport, Option<CodingGoal>) {
    match assessment.kind {
        AgenticExperienceRequestKind::Status => {
            let Some(goal) = goal else {
                return (
                    AgenticCodingExperienceReport {
                        status: AgenticExperienceStatus::NoActiveGoal,
                        objective: assessment.objective.clone(),
                        what_doing: "No active Coding Goal — say what you want built.".into(),
                        notes: vec![
                            "Create a multi-step goal (e.g. Build me a Pong game.) first.".into(),
                        ],
                        ..Default::default()
                    },
                    None,
                );
            };
            let mut report = narrate_experience(goal);
            report.status = AgenticExperienceStatus::Status;
            if goal.status.is_terminal() {
                report.notes.push(
                    "Narrating the most recent goal for this conversation (active pointer cleared on terminal end)."
                        .into(),
                );
            }
            (report, Some(goal.clone()))
        }
        AgenticExperienceRequestKind::Begin => {
            if let Some(existing) = goal {
                if !existing.status.is_terminal() {
                    let mut report = narrate_experience(existing);
                    report.status = AgenticExperienceStatus::Status;
                    report.notes.push(
                        "An active Coding Goal already exists — showing experience status instead of creating another."
                            .into(),
                    );
                    report.notes.push(
                        "Cancel or complete the current goal before starting a new one.".into(),
                    );
                    return (report, Some(existing.clone()));
                }
            }

            let Some(goal_assessment) =
                detect_coding_goal_request(&UserRequest::new(&assessment.objective))
            else {
                return (
                    AgenticCodingExperienceReport {
                        status: AgenticExperienceStatus::Rejected,
                        objective: assessment.objective.clone(),
                        notes: vec!["Could not classify as a Coding Goal create.".into()],
                        ..Default::default()
                    },
                    None,
                );
            };
            let created = create_goal(&goal_assessment, context, conversation_id);
            let mut report = narrate_experience(&created);
            report.status = AgenticExperienceStatus::Created;
            report.notes.insert(
                0,
                "Created a Coding Goal — next: break into tasks, then start (Conversation First)."
                    .into(),
            );
            report.notes.push(
                "No files were changed and no tools ran creating this goal.".into(),
            );
            report.mutated = false;
            report.tools_executed = false;
            (report, Some(created))
        }
    }
}

/// Prefer the active goal; otherwise the latest goal for the conversation
/// (including terminal cancelled/completed — active pointer is cleared on end).
pub fn resolve_goal_for_experience(
    store: &crate::coding_goal::CodingGoalStore,
    conversation_id: Option<&str>,
) -> Option<CodingGoal> {
    if let Some(active) = store.active_goal(conversation_id) {
        return Some(active.clone());
    }
    let mut goals = store.goals_for_conversation(conversation_id);
    if goals.is_empty() {
        return None;
    }
    goals.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)));
    goals.last().map(|g| (*g).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_goal::create_goal;

    #[test]
    fn detect_pong_and_login_begin() {
        assert!(detect_agentic_experience_request(&UserRequest::new(
            "Build me a Pong game."
        ))
        .is_some());
        assert!(detect_agentic_experience_request(&UserRequest::new("Build Pong.")).is_some());
        assert!(detect_agentic_experience_request(&UserRequest::new(
            "Build a login system."
        ))
        .is_some());
        assert!(detect_agentic_experience_request(&UserRequest::new(
            "Show agentic coding experience."
        ))
        .is_some());
        assert!(detect_agentic_experience_request(&UserRequest::new(
            "Pause this goal."
        ))
        .is_none());
        assert!(detect_agentic_experience_request(&UserRequest::new(
            "Write tests."
        ))
        .is_none());
    }

    #[test]
    fn begin_creates_goal_without_mutation() {
        let a = detect_agentic_experience_request(&UserRequest::new("Build me a Pong game."))
            .expect("detect");
        let (report, goal) = apply_assessment(None, &a, &ContextBundle::default(), None);
        assert_eq!(report.status, AgenticExperienceStatus::Created);
        assert!(!report.mutated);
        assert!(!report.tools_executed);
        assert!(!report.exposes_chain_of_thought);
        assert!(!report.parallel_coding_agent);
        let goal = goal.expect("goal");
        assert!(goal.user_objective.to_ascii_lowercase().contains("pong"));
        assert!(!goal.mutated);
        assert!(!goal.tools_executed);
    }

    #[test]
    fn narrate_covers_ux_acceptance_fields() {
        let a = crate::coding_goal::detect_coding_goal_request(&UserRequest::new(
            "Build a login system.",
        ))
        .unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let report = narrate_experience(&g);
        assert!(!report.what_planned.is_empty());
        assert!(!report.what_doing.is_empty());
        assert!(!report.what_changed.is_empty());
        assert!(!report.whether_worked.is_empty());
        assert!(!report.what_went_wrong.is_empty());
        assert!(!report.what_remains.is_empty());
        assert!(report.progress.is_some());
        assert!(!report.stages.is_empty());
    }
}
