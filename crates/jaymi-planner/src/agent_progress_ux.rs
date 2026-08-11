//! Agent Progress UX (Sprint C5.7).
//!
//! Give users clear visibility into what an active Coding Goal is doing
//! **without** exposing chain-of-thought or private reasoning.
//!
//! ```text
//! CodingGoal + lifecycle + tasks  (Planner-owned authority)
//!         │
//!         ▼
//!   AgentProgressView  (derive-only read model — not a second agent state)
//!         │
//!         ▼
//! Conversation markdown + SuggestedAction chips
//! Experience / Coding Workspace mirror (never invent)
//! ```
//!
//! Diagnostics remain a separate operational surface.

use crate::agent_state_machine::AgentLifecycleState;
use crate::coding_goal::{CodingGoal, CodingGoalStatus, GoalVerificationState};
use crate::goal_decomposition::GoalTaskStatus;
use crate::pair_programming::{PairPhase, SuggestedAction};
use jaymi_core::UserRequest;

/// High-level operational phase for progress UX (not agent authority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    NotStarted,
    Understanding,
    Investigating,
    Planning,
    ApprovalRequired,
    Executing,
    Verifying,
    Correcting,
    Blocked,
    Paused,
    Failed,
    Cancelled,
    Completed,
}

impl ProgressPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Understanding => "understanding",
            Self::Investigating => "investigating",
            Self::Planning => "planning",
            Self::ApprovalRequired => "approval_required",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
            Self::Correcting => "correcting",
            Self::Blocked => "blocked",
            Self::Paused => "paused",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "Not started",
            Self::Understanding => "Understanding",
            Self::Investigating => "Investigating",
            Self::Planning => "Planning",
            Self::ApprovalRequired => "Approval required",
            Self::Executing => "Executing",
            Self::Verifying => "Verifying",
            Self::Correcting => "Correcting",
            Self::Blocked => "Blocked",
            Self::Paused => "Paused",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Completed => "Completed",
        }
    }

    pub fn from_lifecycle(state: AgentLifecycleState) -> Self {
        match state {
            AgentLifecycleState::Idle => Self::NotStarted,
            AgentLifecycleState::Understanding => Self::Understanding,
            AgentLifecycleState::Investigating => Self::Investigating,
            AgentLifecycleState::Planning => Self::Planning,
            AgentLifecycleState::AwaitingApproval => Self::ApprovalRequired,
            AgentLifecycleState::Executing => Self::Executing,
            AgentLifecycleState::Verifying => Self::Verifying,
            AgentLifecycleState::Correcting => Self::Correcting,
            AgentLifecycleState::Blocked => Self::Blocked,
            AgentLifecycleState::Paused => Self::Paused,
            AgentLifecycleState::Failed => Self::Failed,
            AgentLifecycleState::Cancelled => Self::Cancelled,
            AgentLifecycleState::Completed => Self::Completed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressTaskMark {
    Completed,
    Current,
    Pending,
    Failed,
    Blocked,
    Cancelled,
    AwaitingApproval,
}

impl ProgressTaskMark {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Current => "current",
            Self::Pending => "pending",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
            Self::AwaitingApproval => "awaiting_approval",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::Completed => "✓",
            Self::Current => "●",
            Self::Pending => "○",
            Self::Failed => "✗",
            Self::Blocked => "!",
            Self::Cancelled => "–",
            Self::AwaitingApproval => "◌",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProgressTaskLine {
    pub task_id: String,
    pub description: String,
    pub mark: ProgressTaskMark,
}

/// Planner-authored progress action chip (Conversation First).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProgressAction {
    pub id: String,
    pub label: String,
    pub conversation_text: String,
}

/// Derive-only progress read model — **not** a second agent state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentProgressView {
    pub goal_id: String,
    pub title: String,
    pub goal_status: String,
    pub phase: ProgressPhase,
    pub phase_label: String,
    pub current_task: Option<ProgressTaskLine>,
    pub completed_tasks: Vec<ProgressTaskLine>,
    pub pending_tasks: Vec<ProgressTaskLine>,
    pub other_tasks: Vec<ProgressTaskLine>,
    pub completed_count: usize,
    pub total_tasks: usize,
    pub approval_required: bool,
    pub verification_status: String,
    pub blockers: Vec<String>,
    pub failures: Vec<String>,
    pub actions: Vec<ProgressAction>,
    /// Always false — progress UX never embeds private reasoning.
    pub exposes_chain_of_thought: bool,
    /// Always false — not a second agent authority.
    pub second_agent_state: bool,
}

impl Default for AgentProgressView {
    fn default() -> Self {
        Self {
            goal_id: String::new(),
            title: String::new(),
            goal_status: CodingGoalStatus::Active.as_str().into(),
            phase: ProgressPhase::NotStarted,
            phase_label: ProgressPhase::NotStarted.label().into(),
            current_task: None,
            completed_tasks: Vec::new(),
            pending_tasks: Vec::new(),
            other_tasks: Vec::new(),
            completed_count: 0,
            total_tasks: 0,
            approval_required: false,
            verification_status: GoalVerificationState::NotStarted.as_str().into(),
            blockers: Vec::new(),
            failures: Vec::new(),
            actions: Vec::new(),
            exposes_chain_of_thought: false,
            second_agent_state: false,
        }
    }
}

impl AgentProgressView {
    /// Conversation-facing progress (matches the sprint example layout).
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Goal Progress\n\n");
        out.push_str(&self.title);
        out.push('\n');
        if self.total_tasks > 0 {
            out.push_str(&format!(
                "\n{}/{} tasks · {}\n",
                self.completed_count, self.total_tasks, self.phase_label
            ));
        } else {
            out.push_str(&format!("\nPhase: {}\n", self.phase_label));
        }

        if !self.completed_tasks.is_empty() {
            out.push('\n');
            for t in &self.completed_tasks {
                out.push_str(&format!("{} {}\n", t.mark.glyph(), t.description));
            }
        }
        if let Some(cur) = &self.current_task {
            out.push('\n');
            out.push_str(&format!("{} {}\n", cur.mark.glyph(), cur.description));
        }
        if !self.pending_tasks.is_empty() {
            out.push('\n');
            for t in self.pending_tasks.iter().take(6) {
                out.push_str(&format!("{} {}\n", t.mark.glyph(), t.description));
            }
            if self.pending_tasks.len() > 6 {
                out.push_str(&format!(
                    "○ … {} more pending\n",
                    self.pending_tasks.len() - 6
                ));
            }
        }
        for t in &self.other_tasks {
            out.push_str(&format!("{} {}\n", t.mark.glyph(), t.description));
        }

        if self.approval_required {
            out.push_str("\n**Approval required** — Review Before Action before mutation.\n");
        }
        if self.verification_status != GoalVerificationState::NotStarted.as_str() {
            out.push_str(&format!("\nVerification: {}\n", self.verification_status));
        }
        if !self.blockers.is_empty() {
            out.push_str("\nBlockers:\n");
            for b in &self.blockers {
                out.push_str(&format!("- {b}\n"));
            }
        }
        if !self.failures.is_empty() {
            out.push_str("\nFailures:\n");
            for f in &self.failures {
                out.push_str(&format!("- {f}\n"));
            }
        }
        if !self.actions.is_empty() {
            out.push_str("\n");
            for a in &self.actions {
                out.push_str(&format!("[{}] ", a.label));
            }
            out.push('\n');
        }
        out.push_str(
            "\n_Operational progress only — no private reasoning or chain-of-thought._\n",
        );
        out
    }

    /// Compact lines for Coding Workspace progress chrome (not Diagnostics).
    pub fn panel_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(self.title.clone());
        if self.total_tasks > 0 {
            lines.push(format!(
                "{}/{} · {}",
                self.completed_count, self.total_tasks, self.phase_label
            ));
        } else {
            lines.push(format!("phase={}", self.phase_label));
        }
        if let Some(cur) = &self.current_task {
            lines.push(format!("{} {}", cur.mark.glyph(), cur.description));
        }
        if self.approval_required {
            lines.push("approval_required".into());
        }
        if !self.blockers.is_empty() {
            lines.push(format!("blockers={}", self.blockers.len()));
        }
        if !self.failures.is_empty() {
            lines.push(format!("failures={}", self.failures.len()));
        }
        lines
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "agent_progress goal={} phase={} tasks={}/{} approval={} verification={} cot={} second_state={}",
            self.goal_id,
            self.phase.as_str(),
            self.completed_count,
            self.total_tasks,
            self.approval_required,
            self.verification_status,
            self.exposes_chain_of_thought,
            self.second_agent_state,
        )
    }

    /// Map progress actions into pair-programming SuggestedAction chips.
    pub fn to_suggested_actions(&self) -> Vec<SuggestedAction> {
        self.actions
            .iter()
            .map(|a| {
                let phase = match a.id.as_str() {
                    "pause_goal" | "resume_goal" | "cancel_goal" => PairPhase::Ready,
                    "continue_goal" | "start_goal" => PairPhase::Planning,
                    "review_goal" => PairPhase::Review,
                    _ => PairPhase::Ready,
                };
                SuggestedAction::conversational(
                    a.id.clone(),
                    a.label.clone(),
                    phase,
                    a.conversation_text.clone(),
                    false,
                )
            })
            .collect()
    }
}

fn short_title(objective: &str) -> String {
    let t = objective.trim();
    if t.chars().count() <= 72 {
        // Prefer gerund-ish headline without trailing punctuation.
        return t
            .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?')
            .to_string();
    }
    let mut s: String = t.chars().take(71).collect();
    s.push('…');
    s
}

fn mark_for_task(status: GoalTaskStatus, is_current: bool) -> ProgressTaskMark {
    if is_current
        && matches!(
            status,
            GoalTaskStatus::InProgress | GoalTaskStatus::AwaitingApproval
        )
    {
        return ProgressTaskMark::Current;
    }
    match status {
        GoalTaskStatus::Completed => ProgressTaskMark::Completed,
        GoalTaskStatus::InProgress => ProgressTaskMark::Current,
        GoalTaskStatus::Pending => ProgressTaskMark::Pending,
        GoalTaskStatus::Failed => ProgressTaskMark::Failed,
        GoalTaskStatus::Blocked => ProgressTaskMark::Blocked,
        GoalTaskStatus::Cancelled => ProgressTaskMark::Cancelled,
        GoalTaskStatus::AwaitingApproval => ProgressTaskMark::AwaitingApproval,
    }
}

fn actions_for(goal: &CodingGoal, phase: ProgressPhase) -> Vec<ProgressAction> {
    let mut actions = Vec::new();
    let terminal = matches!(
        phase,
        ProgressPhase::Completed | ProgressPhase::Cancelled
    );
    if !terminal {
        if phase == ProgressPhase::Paused {
            actions.push(ProgressAction {
                id: "resume_goal".into(),
                label: "Resume".into(),
                conversation_text: "Resume this goal.".into(),
            });
        } else if !matches!(phase, ProgressPhase::NotStarted) {
            actions.push(ProgressAction {
                id: "pause_goal".into(),
                label: "Pause".into(),
                conversation_text: "Pause this goal.".into(),
            });
        }
        actions.push(ProgressAction {
            id: "cancel_goal".into(),
            label: "Stop".into(),
            conversation_text: "Cancel this goal.".into(),
        });
    }
    if phase == ProgressPhase::ApprovalRequired {
        actions.push(ProgressAction {
            id: "review_goal".into(),
            label: "Review".into(),
            conversation_text: "Review my changes.".into(),
        });
    }
    if matches!(
        phase,
        ProgressPhase::NotStarted | ProgressPhase::Understanding | ProgressPhase::Investigating
    ) && goal.tasks.iter().any(|t| t.status == GoalTaskStatus::Pending)
        && goal.active_task_id.is_none()
    {
        actions.push(ProgressAction {
            id: "start_goal".into(),
            label: "Start".into(),
            conversation_text: "Start the goal.".into(),
        });
    }
    if goal.active_task_id.is_none()
        && goal
            .tasks
            .iter()
            .any(|t| t.status == GoalTaskStatus::Pending)
        && !matches!(phase, ProgressPhase::Paused | ProgressPhase::Cancelled)
    {
        actions.push(ProgressAction {
            id: "continue_goal".into(),
            label: "Continue".into(),
            conversation_text: "Continue the goal.".into(),
        });
    } else if goal.active_task_id.is_some()
        && matches!(
            phase,
            ProgressPhase::Investigating
                | ProgressPhase::Planning
                | ProgressPhase::Executing
                | ProgressPhase::Verifying
                | ProgressPhase::Correcting
        )
    {
        actions.push(ProgressAction {
            id: "continue_goal".into(),
            label: "Continue".into(),
            conversation_text: "Continue the goal.".into(),
        });
    }
    // Dedupe by id.
    let mut seen = std::collections::HashSet::new();
    actions.retain(|a| seen.insert(a.id.clone()));
    actions
}

/// Derive progress from the authoritative Coding Goal (no second state).
pub fn derive_progress(goal: &CodingGoal) -> AgentProgressView {
    let phase = ProgressPhase::from_lifecycle(goal.agent_lifecycle.state);
    let active = goal.active_task_id.clone();

    let mut completed = Vec::new();
    let mut pending = Vec::new();
    let mut other = Vec::new();
    let mut current = None;

    for t in &goal.tasks {
        let is_current = active.as_deref() == Some(t.id.as_str());
        let line = ProgressTaskLine {
            task_id: t.id.clone(),
            description: t.description.clone(),
            mark: mark_for_task(t.status, is_current),
        };
        match t.status {
            GoalTaskStatus::Completed => completed.push(line),
            GoalTaskStatus::Pending => pending.push(line),
            GoalTaskStatus::InProgress | GoalTaskStatus::AwaitingApproval if is_current => {
                current = Some(line);
            }
            GoalTaskStatus::InProgress | GoalTaskStatus::AwaitingApproval => {
                // Unexpected second in-progress — still show as current-ish.
                if current.is_none() {
                    current = Some(line);
                } else {
                    other.push(line);
                }
            }
            GoalTaskStatus::Failed | GoalTaskStatus::Blocked | GoalTaskStatus::Cancelled => {
                other.push(line);
            }
        }
    }

    // Fallback: goal.current_task text when tasks empty.
    if current.is_none() && goal.tasks.is_empty() {
        if let Some(ct) = &goal.current_task {
            current = Some(ProgressTaskLine {
                task_id: "current".into(),
                description: ct.clone(),
                mark: ProgressTaskMark::Current,
            });
        }
    }

    let mut failures = Vec::new();
    for t in &goal.tasks {
        if t.status == GoalTaskStatus::Failed {
            failures.push(format!("{}: {}", t.id, t.description));
        }
    }
    for rec in goal.workflow_history.iter().rev().take(4) {
        if rec.kind.contains("fail") {
            failures.push(rec.detail.clone());
        }
    }
    failures.truncate(6);

    let approval_required = matches!(
        goal.agent_lifecycle.state,
        AgentLifecycleState::AwaitingApproval
    ) || goal
        .tasks
        .iter()
        .any(|t| t.status == GoalTaskStatus::AwaitingApproval);

    let total = goal.tasks.len();
    let completed_count = completed.len();

    AgentProgressView {
        goal_id: goal.id.clone(),
        title: short_title(&goal.user_objective),
        goal_status: goal.status.as_str().into(),
        phase,
        phase_label: phase.label().into(),
        current_task: current,
        completed_tasks: completed,
        pending_tasks: pending,
        other_tasks: other,
        completed_count,
        total_tasks: total,
        approval_required,
        verification_status: goal.verification_state.as_str().into(),
        blockers: goal.blockers.clone(),
        failures,
        actions: actions_for(goal, phase),
        exposes_chain_of_thought: false,
        second_agent_state: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgressRequestKind {
    Show,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressAssessment {
    pub kind: ProgressRequestKind,
    pub matched_rules: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressReportStatus {
    Shown,
    NoActiveGoal,
    Rejected,
}

impl ProgressReportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shown => "shown",
            Self::NoActiveGoal => "no_active_goal",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentProgressReport {
    pub status: ProgressReportStatus,
    pub progress: Option<AgentProgressView>,
    pub notes: Vec<String>,
}

impl Default for AgentProgressReport {
    fn default() -> Self {
        Self {
            status: ProgressReportStatus::Rejected,
            progress: None,
            notes: Vec::new(),
        }
    }
}

impl AgentProgressReport {
    pub fn to_markdown(&self) -> String {
        match &self.progress {
            Some(p) => p.to_markdown(),
            None => {
                let mut out = String::from("## Goal Progress\n\n");
                out.push_str("No active Coding Goal.\n");
                for n in &self.notes {
                    out.push_str("- ");
                    out.push_str(n);
                    out.push('\n');
                }
                out
            }
        }
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "agent_progress_ux status={} {}",
            self.status.as_str(),
            self.progress
                .as_ref()
                .map(|p| p.to_diagnostics_summary())
                .unwrap_or_else(|| "progress=—".into()),
        )
    }
}

pub fn detect_progress_request(request: &UserRequest) -> Option<ProgressAssessment> {
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
        || crate::approval_autonomy_policy::detect_autonomy_policy_request(request).is_some()
        || crate::agent_failure_recovery::detect_recovery_request(request).is_some()
        || crate::coding_goal_completion::detect_completion_request(request).is_some()
        || crate::agent_session_continuity::detect_continuity_request(request).is_some()
        || crate::agentic_coding_experience::detect_agentic_experience_request(request).is_some()
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::fix_from_verification::steals_from_other_detect(request)
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "show goal progress"
            | "show progress"
            | "goal progress"
            | "what's the goal progress"
            | "what is the goal progress"
            | "how is the goal going"
            | "progress on this goal"
    ) {
        return Some(ProgressAssessment {
            kind: ProgressRequestKind::Show,
            matched_rules: vec!["show_progress"],
        });
    }
    if matches!(
        trimmed.as_str(),
        "goal progress status" | "progress status"
    ) {
        return Some(ProgressAssessment {
            kind: ProgressRequestKind::Status,
            matched_rules: vec!["status_phrase"],
        });
    }
    None
}

pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    _assessment: &ProgressAssessment,
) -> AgentProgressReport {
    let Some(goal) = goal else {
        return AgentProgressReport {
            status: ProgressReportStatus::NoActiveGoal,
            notes: vec![
                "Create a Coding Goal first (for example, “Build a login system.”).".into(),
            ],
            ..Default::default()
        };
    };
    let progress = derive_progress(goal);
    debug_assert!(!progress.exposes_chain_of_thought);
    debug_assert!(!progress.second_agent_state);
    AgentProgressReport {
        status: ProgressReportStatus::Shown,
        progress: Some(progress),
        notes: vec![
            "Derived from Planner-owned Coding Goal / lifecycle / tasks.".into(),
            "Experience/UI must mirror — not invent — this view.".into(),
        ],
    }
}

/// Enrich pair-programming suggestions with goal progress actions (Planner-authored).
pub fn enrich_pair_programming(
    surface: &mut crate::pair_programming::PairProgrammingSurface,
    progress: &AgentProgressView,
) {
    let mut actions = progress.to_suggested_actions();
    if actions.is_empty() {
        return;
    }
    // Prepend goal actions so they are visible; keep existing suggestions after.
    actions.append(&mut surface.suggested_actions);
    // Dedupe by id.
    let mut seen = std::collections::HashSet::new();
    actions.retain(|a| seen.insert(a.id.clone()));
    surface.suggested_actions = actions;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_state_machine::{transition_goal, AgentLifecycleEvent};
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::{apply_decomposition, propose_tasks, GoalTask};
    use crate::multi_step_execution::start_goal_execution;
    use jaymi_context::ContextBundle;

    fn goal_started() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(
            "Build a login system without OAuth.",
        ))
        .unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let (tasks, order) = propose_tasks(&g).unwrap();
        let g = apply_decomposition(g, tasks, order);
        start_goal_execution(g).unwrap().0
    }

    #[test]
    fn goal_start_shows_current_and_counts() {
        let g = goal_started();
        let p = derive_progress(&g);
        assert!(p.total_tasks >= 2);
        assert!(p.current_task.is_some());
        assert_eq!(p.phase, ProgressPhase::Investigating);
        assert!(!p.exposes_chain_of_thought);
        assert!(!p.second_agent_state);
        let md = p.to_markdown();
        assert!(md.contains("●") || md.contains(p.current_task.as_ref().unwrap().description.as_str()));
        assert!(md.contains(&format!("{}/{}", p.completed_count, p.total_tasks)));
    }

    #[test]
    fn task_transition_and_approval_phase() {
        let mut g = goal_started();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginPlanning {
                reason: "plan".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-1".into(),
            },
        )
        .unwrap();
        let p = derive_progress(&g);
        assert_eq!(p.phase, ProgressPhase::ApprovalRequired);
        assert!(p.approval_required);
        assert!(p.actions.iter().any(|a| a.id == "review_goal" || a.id == "pause_goal"));
    }

    #[test]
    fn execution_verification_failure_blocked() {
        let mut g = goal_started();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginPlanning { reason: "p".into() },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "p1".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "p1".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "p1".into(),
            },
        )
        .unwrap();
        assert_eq!(derive_progress(&g).phase, ProgressPhase::Executing);

        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginVerification {
                reason: "v".into(),
            },
        )
        .unwrap();
        assert_eq!(derive_progress(&g).phase, ProgressPhase::Verifying);

        g = transition_goal(
            g,
            AgentLifecycleEvent::VerificationFailed {
                evidence_ref: "fail".into(),
            },
        )
        .unwrap();
        assert_eq!(derive_progress(&g).phase, ProgressPhase::Correcting);

        let mut g2 = goal_started();
        g2 = transition_goal(
            g2,
            AgentLifecycleEvent::Block {
                reason: "waiting on API key".into(),
            },
        )
        .unwrap();
        let p = derive_progress(&g2);
        assert_eq!(p.phase, ProgressPhase::Blocked);
        assert!(p.blockers.iter().any(|b| b.contains("API key")));
    }

    #[test]
    fn pause_cancel_completion() {
        let mut g = goal_started();
        g = transition_goal(
            g,
            AgentLifecycleEvent::Pause {
                reason: "user".into(),
            },
        )
        .unwrap();
        let p = derive_progress(&g);
        assert_eq!(p.phase, ProgressPhase::Paused);
        assert!(p.actions.iter().any(|a| a.id == "resume_goal"));

        g = transition_goal(
            g,
            AgentLifecycleEvent::Cancel {
                reason: "stop".into(),
            },
        )
        .unwrap();
        assert_eq!(derive_progress(&g).phase, ProgressPhase::Cancelled);

        let mut g2 = goal_started();
        // Mark all tasks complete for display.
        for t in &mut g2.tasks {
            t.status = GoalTaskStatus::Completed;
        }
        g2.active_task_id = None;
        g2 = transition_goal(
            g2,
            AgentLifecycleEvent::Complete {
                evidence: crate::agent_state_machine::CompletionEvidence {
                    summary: "done".into(),
                    references: vec!["ver".into()],
                    source: crate::agent_state_machine::CompletionEvidenceSource::VerificationPassed,
                    verification_passed: true,
                },
            },
        )
        .unwrap();
        let p = derive_progress(&g2);
        assert_eq!(p.phase, ProgressPhase::Completed);
        assert_eq!(p.completed_count, p.total_tasks);
    }

    #[test]
    fn no_chain_of_thought_in_markdown() {
        let g = goal_started();
        let md = derive_progress(&g).to_markdown().to_ascii_lowercase();
        assert!(!md.contains("chain-of-thought") || md.contains("no private"));
        assert!(!md.contains("hidden reasoning"));
        assert!(!md.contains("deliberation scratch"));
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_progress_request(&UserRequest::new("Show goal progress.")).is_some());
        assert!(detect_progress_request(&UserRequest::new("How is the goal going?")).is_some());
        assert!(detect_progress_request(&UserRequest::new("Build a login system.")).is_none());
    }

    #[test]
    fn panel_lines_separate_from_cot() {
        let g = goal_started();
        let lines = derive_progress(&g).panel_lines();
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|l| !l.contains("thinking:")));
    }

    #[test]
    fn ui_reflects_planner_task_marks() {
        let mut g = goal_started();
        let tid = g.active_task_id.clone().unwrap();
        // Complete current, leave others pending.
        if let Some(t) = g.tasks.iter_mut().find(|t| t.id == tid) {
            t.status = GoalTaskStatus::Completed;
        }
        g.active_task_id = None;
        g.tasks.push(GoalTask {
            id: "failed-1".into(),
            description: "Broken step".into(),
            status: GoalTaskStatus::Failed,
            ..GoalTask::default()
        });
        let p = derive_progress(&g);
        assert!(p.completed_tasks.iter().any(|t| t.task_id == tid));
        assert!(p.other_tasks.iter().any(|t| t.mark == ProgressTaskMark::Failed));
    }
}
