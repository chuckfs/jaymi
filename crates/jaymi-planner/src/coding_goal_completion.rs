//! Coding Goal Completion (Sprint C5.10).
//!
//! Evidence-based determination of when a Coding Goal is actually complete.
//! Planner owns completion state — the model may propose, but cannot declare
//! authoritative completion.
//!
//! Constitutional constraints:
//! - Unknown must never be represented as Satisfied
//! - Failed verification blocks successful completion unless waived
//! - Blocked goals cannot become Completed
//! - Do not invent success; completion summaries use actual evidence
//! - User may explicitly end an incomplete goal (distinct from Completed)
//! - Preserve complete goal history; no silent mutation

use crate::agent_state_machine::{
    CompletionEvidence, CompletionEvidenceSource,
};
use crate::coding_goal::{
    cancel_goal, complete_goal, AcceptanceCriterion, AcceptanceCriterionSource, CodingGoal,
    CodingGoalStatus, GoalVerificationState,
};
use crate::goal_decomposition::GoalTaskStatus;
use jaymi_core::UserRequest;

/// Satisfaction of one acceptance criterion (never invent Satisfied).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionSatisfaction {
    Satisfied,
    Unsatisfied,
    Unknown,
    Waived,
}

impl CriterionSatisfaction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::Unsatisfied => "unsatisfied",
            Self::Unknown => "unknown",
            Self::Waived => "waived",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Satisfied => "Satisfied",
            Self::Unsatisfied => "Unsatisfied",
            Self::Unknown => "Unknown",
            Self::Waived => "Waived",
        }
    }

    pub fn counts_as_met(self) -> bool {
        matches!(self, Self::Satisfied | Self::Waived)
    }
}

/// Evaluation of a single acceptance criterion against goal evidence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CriterionEvaluation {
    pub index: usize,
    pub text: String,
    pub source: AcceptanceCriterionSource,
    pub satisfaction: CriterionSatisfaction,
    pub evidence_refs: Vec<String>,
    pub note: String,
    pub waived_reason: Option<String>,
}

/// How a goal ended (when terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEndingKind {
    /// Evidence-based successful completion.
    SuccessfullyCompleted,
    /// User explicitly ended while incomplete (Cancelled coarse status).
    UserEnded,
}

impl GoalEndingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SuccessfullyCompleted => "successfully_completed",
            Self::UserEnded => "user_ended",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SuccessfullyCompleted => "Successfully completed",
            Self::UserEnded => "User-ended (incomplete)",
        }
    }
}

/// Planner outcome of a completion assessment / attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalCompletionOutcome {
    Ready,
    Incomplete,
    FailedVerification,
    Blocked,
    SuccessfullyCompleted,
    UserEnded,
    AlreadyTerminal,
    NoActiveGoal,
    Rejected,
}

impl GoalCompletionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Incomplete => "incomplete",
            Self::FailedVerification => "failed_verification",
            Self::Blocked => "blocked",
            Self::SuccessfullyCompleted => "successfully_completed",
            Self::UserEnded => "user_ended",
            Self::AlreadyTerminal => "already_terminal",
            Self::NoActiveGoal => "no_active_goal",
            Self::Rejected => "rejected",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready to complete",
            Self::Incomplete => "Incomplete",
            Self::FailedVerification => "Failed verification blocks completion",
            Self::Blocked => "Blocked — cannot complete",
            Self::SuccessfullyCompleted => "Successfully completed",
            Self::UserEnded => "User-ended (incomplete)",
            Self::AlreadyTerminal => "Already terminal",
            Self::NoActiveGoal => "No active goal",
            Self::Rejected => "Rejected",
        }
    }
}

/// Soft-path / report status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalCompletionStatus {
    Assessed,
    Completed,
    UserEnded,
    Incomplete,
    Rejected,
    Status,
    NoActiveGoal,
}

impl GoalCompletionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Assessed => "assessed",
            Self::Completed => "completed",
            Self::UserEnded => "user_ended",
            Self::Incomplete => "incomplete",
            Self::Rejected => "rejected",
            Self::Status => "status",
            Self::NoActiveGoal => "no_active_goal",
        }
    }
}

/// One preserved completion / waiver / end event on the Coding Goal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalCompletionRecord {
    pub sequence: u32,
    pub kind: String,
    pub outcome: GoalCompletionOutcome,
    pub summary: String,
    pub ending_kind: Option<GoalEndingKind>,
    pub stamped_at: String,
}

/// Durable completion surface on the Coding Goal (C5.10).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GoalCompletionState {
    /// Last criterion evaluations (Planner-owned).
    pub criteria: Vec<CriterionEvaluation>,
    /// Explicit waiver of failed / unresolved verification.
    pub verification_waived: bool,
    pub verification_waiver_reason: Option<String>,
    pub history: Vec<GoalCompletionRecord>,
    pub last_outcome: Option<GoalCompletionOutcome>,
    pub ending_kind: Option<GoalEndingKind>,
    pub notes: Vec<String>,
}

impl GoalCompletionState {
    fn push_history(&mut self, kind: &str, outcome: GoalCompletionOutcome, summary: &str) {
        let sequence = self.history.len() as u32 + 1;
        self.history.push(GoalCompletionRecord {
            sequence,
            kind: kind.into(),
            outcome,
            summary: summary.into(),
            ending_kind: self.ending_kind,
            stamped_at: stamp_now(),
        });
        if self.history.len() > 32 {
            let skip = self.history.len() - 32;
            self.history.drain(0..skip);
        }
        self.last_outcome = Some(outcome);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionRequestKind {
    Assess,
    AttemptComplete,
    WaiveCriteria,
    WaiveVerification,
    UserEnd,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionAssessment {
    pub kind: CompletionRequestKind,
    pub matched_rules: Vec<&'static str>,
    pub waive_reason: Option<String>,
}

/// Soft-path report (Planner-owned; never mutates files).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalCompletionReport {
    pub status: GoalCompletionStatus,
    pub outcome: GoalCompletionOutcome,
    pub goal_id: Option<String>,
    pub ending_kind: Option<GoalEndingKind>,
    pub criteria: Vec<CriterionEvaluation>,
    pub summary: String,
    pub evidence_used: Vec<String>,
    pub verification_state: Option<String>,
    pub verification_waived: bool,
    pub tasks_completed: usize,
    pub tasks_total: usize,
    pub blocked: bool,
    pub mutated: bool,
    pub invented_success: bool,
    pub model_authoritative: bool,
    pub planner_owned: bool,
    pub unknown_treated_as_satisfied: bool,
    pub notes: Vec<String>,
}

impl Default for GoalCompletionReport {
    fn default() -> Self {
        Self {
            status: GoalCompletionStatus::Rejected,
            outcome: GoalCompletionOutcome::Rejected,
            goal_id: None,
            ending_kind: None,
            criteria: Vec::new(),
            summary: String::new(),
            evidence_used: Vec::new(),
            verification_state: None,
            verification_waived: false,
            tasks_completed: 0,
            tasks_total: 0,
            blocked: false,
            mutated: false,
            invented_success: false,
            model_authoritative: false,
            planner_owned: true,
            unknown_treated_as_satisfied: false,
            notes: Vec::new(),
        }
    }
}

impl GoalCompletionReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Goal Completion\n\n");
        out.push_str(&format!("**{}**\n\n", self.outcome.label()));
        out.push_str(&format!("- Status: `{}`\n", self.status.as_str()));
        if let Some(id) = &self.goal_id {
            out.push_str(&format!("- Goal: `{id}`\n"));
        }
        if let Some(kind) = self.ending_kind {
            out.push_str(&format!("- Ending: **{}**\n", kind.label()));
        }
        if let Some(vs) = &self.verification_state {
            out.push_str(&format!("- Verification: `{vs}`"));
            if self.verification_waived {
                out.push_str(" (waived by user)");
            }
            out.push('\n');
        }
        out.push_str(&format!(
            "- Tasks completed: {}/{}\n",
            self.tasks_completed, self.tasks_total
        ));
        if self.blocked {
            out.push_str("- Blocked: yes — cannot become Completed\n");
        }
        if !self.summary.is_empty() {
            out.push_str("\n### Summary (evidence-based)\n");
            out.push_str(&self.summary);
            out.push('\n');
        }
        if !self.criteria.is_empty() {
            out.push_str("\n### Acceptance criteria\n");
            for c in &self.criteria {
                let tag = c.source.as_str();
                out.push_str(&format!(
                    "- [{}] ({tag}) {}\n",
                    c.satisfaction.label(),
                    c.text
                ));
                if !c.note.is_empty() {
                    out.push_str(&format!("  - {}\n", c.note));
                }
                if let Some(reason) = &c.waived_reason {
                    out.push_str(&format!("  - Waiver: {reason}\n"));
                }
                if !c.evidence_refs.is_empty() {
                    out.push_str(&format!("  - Evidence: {}\n", c.evidence_refs.join(", ")));
                }
            }
        }
        if !self.evidence_used.is_empty() {
            out.push_str("\n### Evidence used\n");
            for e in &self.evidence_used {
                out.push_str(&format!("- {e}\n"));
            }
        }
        out.push_str("\n### Guards\n");
        out.push_str(&format!(
            "- Silent mutation: {}\n",
            if self.mutated { "YES (bug)" } else { "no" }
        ));
        out.push_str(&format!(
            "- Invented success: {}\n",
            if self.invented_success {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        out.push_str(&format!(
            "- Model authoritative completion: {}\n",
            if self.model_authoritative {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        out.push_str(&format!(
            "- Unknown treated as satisfied: {}\n",
            if self.unknown_treated_as_satisfied {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str(&format!("- {n}\n"));
            }
        }
        out.push_str(
            "\n_Planner owns completion — model proposals are not authoritative. User-ended ≠ successfully completed._\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "goal_completion status={} outcome={} ending={} criteria={} verified={} waived_v={} blocked={} mutated={} invented={} model_auth={} unknown_as_sat={} planner_owned={}",
            self.status.as_str(),
            self.outcome.as_str(),
            self.ending_kind
                .map(|k| k.as_str())
                .unwrap_or("none"),
            self.criteria.len(),
            self.verification_state.as_deref().unwrap_or("—"),
            self.verification_waived,
            self.blocked,
            self.mutated,
            self.invented_success,
            self.model_authoritative,
            self.unknown_treated_as_satisfied,
            self.planner_owned,
        )
    }
}

fn stamp_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn goal_is_blocked(goal: &CodingGoal) -> bool {
    goal.status == CodingGoalStatus::Blocked
        || goal.agent_lifecycle.state
            == crate::agent_state_machine::AgentLifecycleState::Blocked
        || goal.tasks.iter().any(|t| t.status == GoalTaskStatus::Blocked)
        || (!goal.blockers.is_empty()
            && goal.status == CodingGoalStatus::Blocked)
}

fn tasks_summary(goal: &CodingGoal) -> (usize, usize, bool, bool) {
    let total = goal.tasks.len();
    let completed = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Completed)
        .count();
    let any_failed = goal
        .tasks
        .iter()
        .any(|t| t.status == GoalTaskStatus::Failed);
    let any_open = goal.tasks.iter().any(|t| {
        matches!(
            t.status,
            GoalTaskStatus::Pending | GoalTaskStatus::InProgress | GoalTaskStatus::Blocked
        )
    });
    (completed, total, any_failed, any_open)
}

fn prior_waiver(goal: &CodingGoal, index: usize) -> Option<&CriterionEvaluation> {
    goal.completion
        .criteria
        .iter()
        .find(|c| c.index == index && c.satisfaction == CriterionSatisfaction::Waived)
}

/// Evaluate every acceptance criterion from actual goal evidence.
pub fn evaluate_criteria(goal: &CodingGoal) -> Vec<CriterionEvaluation> {
    let (completed, total, any_failed, any_open) = tasks_summary(goal);
    let verification = goal.verification_state;
    let verification_waived = goal.completion.verification_waived;

    let mut criteria_src = goal.acceptance_criteria.clone();
    if criteria_src.is_empty() {
        // Represent the objective as a proposed criterion so evaluation is honest.
        criteria_src.push(AcceptanceCriterion {
            text: goal.user_objective.clone(),
            source: AcceptanceCriterionSource::Proposed,
        });
    }

    let mut out = Vec::with_capacity(criteria_src.len());
    for (index, criterion) in criteria_src.iter().enumerate() {
        if let Some(prior) = prior_waiver(goal, index) {
            out.push(CriterionEvaluation {
                index,
                text: criterion.text.clone(),
                source: criterion.source,
                satisfaction: CriterionSatisfaction::Waived,
                evidence_refs: prior.evidence_refs.clone(),
                note: prior.note.clone(),
                waived_reason: prior.waived_reason.clone(),
            });
            continue;
        }

        let mut evidence_refs = Vec::new();
        let mut note: String;
        let satisfaction = if verification == GoalVerificationState::Failed && !verification_waived
        {
            evidence_refs.push("verification:failed".into());
            note = "Failed verification prevents treating this criterion as satisfied.".into();
            CriterionSatisfaction::Unsatisfied
        } else if any_failed {
            evidence_refs.push("tasks:failed".into());
            note = "At least one task failed — criterion remains unsatisfied.".into();
            CriterionSatisfaction::Unsatisfied
        } else if verification == GoalVerificationState::Passed
            && !any_open
            && (total == 0 || completed == total)
        {
            evidence_refs.push("verification:passed".into());
            if total > 0 {
                evidence_refs.push(format!("tasks:completed:{completed}/{total}"));
            }
            if let Some(ev) = &goal.agent_lifecycle.completion_evidence {
                for r in &ev.references {
                    if !evidence_refs.iter().any(|e| e == r) {
                        evidence_refs.push(r.clone());
                    }
                }
            }
            for item in goal.completed_work.iter().take(4) {
                evidence_refs.push(format!("completed_work:{item}"));
            }
            note = "Supported by verification pass and task/completion evidence.".into();
            CriterionSatisfaction::Satisfied
        } else if any_open {
            evidence_refs.push("tasks:open".into());
            note = "Open tasks remain — not satisfied.".into();
            CriterionSatisfaction::Unsatisfied
        } else {
            evidence_refs.push(format!("verification:{}", verification.as_str()));
            if !goal.remaining_work.is_empty() {
                evidence_refs.push("remaining_work:present".into());
            }
            note =
                "Insufficient evidence to mark satisfied — recorded as unknown (not satisfied)."
                    .into();
            CriterionSatisfaction::Unknown
        };

        debug_assert!(
            !(satisfaction == CriterionSatisfaction::Satisfied
                && evidence_refs.is_empty()),
            "Satisfied requires evidence refs"
        );

        out.push(CriterionEvaluation {
            index,
            text: criterion.text.clone(),
            source: criterion.source,
            satisfaction,
            evidence_refs,
            note,
            waived_reason: None,
        });
    }
    out
}

/// Derive overall outcome from evaluated criteria + goal state (no mutation).
pub fn assess_completion(goal: &CodingGoal) -> (GoalCompletionOutcome, Vec<CriterionEvaluation>, Vec<String>) {
    let criteria = evaluate_criteria(goal);
    let mut evidence_used = Vec::new();
    for c in &criteria {
        for r in &c.evidence_refs {
            if !evidence_used.iter().any(|e| e == r) {
                evidence_used.push(r.clone());
            }
        }
    }

    if goal.status.is_terminal()
        || matches!(
            goal.agent_lifecycle.state,
            crate::agent_state_machine::AgentLifecycleState::Completed
                | crate::agent_state_machine::AgentLifecycleState::Cancelled
        )
    {
        let outcome = if goal.completion.ending_kind == Some(GoalEndingKind::UserEnded)
            || goal.status == CodingGoalStatus::Cancelled
        {
            GoalCompletionOutcome::AlreadyTerminal
        } else if goal.status == CodingGoalStatus::Completed {
            GoalCompletionOutcome::AlreadyTerminal
        } else {
            GoalCompletionOutcome::AlreadyTerminal
        };
        return (outcome, criteria, evidence_used);
    }

    if goal_is_blocked(goal) {
        return (GoalCompletionOutcome::Blocked, criteria, evidence_used);
    }

    if goal.verification_state == GoalVerificationState::Failed
        && !goal.completion.verification_waived
    {
        return (
            GoalCompletionOutcome::FailedVerification,
            criteria,
            evidence_used,
        );
    }

    let any_unknown = criteria
        .iter()
        .any(|c| c.satisfaction == CriterionSatisfaction::Unknown);
    let any_unsatisfied = criteria
        .iter()
        .any(|c| c.satisfaction == CriterionSatisfaction::Unsatisfied);
    let all_met = criteria.iter().all(|c| c.satisfaction.counts_as_met());

    // Constitutional: unknown never counts as satisfied.
    debug_assert!(
        !(any_unknown && all_met),
        "Unknown criterion must not yield all_met"
    );

    if all_met && !any_unknown && !any_unsatisfied {
        (
            GoalCompletionOutcome::Ready,
            criteria,
            evidence_used,
        )
    } else {
        (
            GoalCompletionOutcome::Incomplete,
            criteria,
            evidence_used,
        )
    }
}

fn report_from_assessment(
    goal: &CodingGoal,
    status: GoalCompletionStatus,
    outcome: GoalCompletionOutcome,
    criteria: Vec<CriterionEvaluation>,
    evidence_used: Vec<String>,
    summary: String,
    notes: Vec<String>,
) -> GoalCompletionReport {
    let (tasks_completed, tasks_total, _, _) = tasks_summary(goal);
    let unknown_as_sat = criteria.iter().any(|c| {
        c.satisfaction == CriterionSatisfaction::Unknown && c.note.contains("treated as satisfied")
    });
    GoalCompletionReport {
        status,
        outcome,
        goal_id: Some(goal.id.clone()),
        ending_kind: goal.completion.ending_kind,
        criteria,
        summary,
        evidence_used,
        verification_state: Some(goal.verification_state.as_str().to_string()),
        verification_waived: goal.completion.verification_waived,
        tasks_completed,
        tasks_total,
        blocked: goal_is_blocked(goal),
        mutated: false,
        invented_success: false,
        model_authoritative: false,
        planner_owned: true,
        unknown_treated_as_satisfied: unknown_as_sat,
        notes,
    }
}

fn build_completion_evidence(
    goal: &CodingGoal,
    criteria: &[CriterionEvaluation],
) -> CompletionEvidence {
    let mut references = Vec::new();
    for c in criteria {
        for r in &c.evidence_refs {
            if !references.iter().any(|x| x == r) {
                references.push(r.clone());
            }
        }
    }
    if references.is_empty() {
        references.push(format!("goal:{}", goal.id));
    }
    let waived_any = criteria
        .iter()
        .any(|c| c.satisfaction == CriterionSatisfaction::Waived)
        || goal.completion.verification_waived;
    let verification_passed = goal.verification_state == GoalVerificationState::Passed
        || goal.completion.verification_waived;
    let source = if waived_any && !matches!(goal.verification_state, GoalVerificationState::Passed)
    {
        CompletionEvidenceSource::UserAffirmedWithEvidence
    } else if goal.verification_state == GoalVerificationState::Passed {
        CompletionEvidenceSource::VerificationPassed
    } else {
        CompletionEvidenceSource::UserAffirmedWithEvidence
    };
    let summary = format!(
        "C5.10 evidence-based completion for goal {} (verification={}, waived_v={}, criteria={})",
        goal.id,
        goal.verification_state.as_str(),
        goal.completion.verification_waived,
        criteria.len()
    );
    CompletionEvidence {
        summary,
        source,
        verification_passed,
        references,
    }
}

/// Apply waiver to all non-Satisfied criteria (user-explicit).
pub fn waive_remaining_criteria(mut goal: CodingGoal, reason: &str) -> (CodingGoal, GoalCompletionReport) {
    let (outcome_before, mut criteria, evidence_used) = assess_completion(&goal);
    let reason = if reason.trim().is_empty() {
        "User waived remaining acceptance criteria.".to_string()
    } else {
        reason.trim().to_string()
    };
    for c in &mut criteria {
        if c.satisfaction != CriterionSatisfaction::Satisfied {
            c.satisfaction = CriterionSatisfaction::Waived;
            c.waived_reason = Some(reason.clone());
            c.note = format!("Explicitly waived by user: {reason}");
            c.evidence_refs.push("user:waiver".into());
        }
    }
    goal.completion.criteria = criteria.clone();
    goal.completion.notes.push(format!("Criteria waived: {reason}"));
    goal.updated_at = stamp_now();
    let summary = format!(
        "User waived remaining criteria (prior outcome: {}). Reason: {reason}",
        outcome_before.as_str()
    );
    goal.completion
        .push_history("waive_criteria", GoalCompletionOutcome::Incomplete, &summary);
    let report = report_from_assessment(
        &goal,
        GoalCompletionStatus::Assessed,
        GoalCompletionOutcome::Incomplete,
        criteria,
        evidence_used,
        summary,
        vec![
            "Waiver recorded — does not silently mark the goal Completed.".into(),
            "Ask to mark the goal complete after waivers if evidence otherwise allows.".into(),
        ],
    );
    // Re-assess after waiver for accurate outcome on report.
    let (outcome, criteria2, evidence2) = assess_completion(&goal);
    goal.completion.criteria = criteria2.clone();
    let mut report = report_from_assessment(
        &goal,
        GoalCompletionStatus::Assessed,
        outcome,
        criteria2,
        evidence2,
        format!("After waiver: {}.", outcome.label()),
        report.notes,
    );
    report.notes.push(format!("Waiver reason: {reason}"));
    (goal, report)
}

/// Explicitly waive failed / unresolved verification (user-only).
pub fn waive_verification(mut goal: CodingGoal, reason: &str) -> (CodingGoal, GoalCompletionReport) {
    let reason = if reason.trim().is_empty() {
        "User waived failed/unresolved verification.".to_string()
    } else {
        reason.trim().to_string()
    };
    goal.completion.verification_waived = true;
    goal.completion.verification_waiver_reason = Some(reason.clone());
    goal.completion
        .notes
        .push(format!("Verification waived: {reason}"));
    goal.updated_at = stamp_now();
    let summary = format!("User waived verification. Reason: {reason}");
    goal.completion
        .push_history("waive_verification", GoalCompletionOutcome::Incomplete, &summary);
    let (outcome, criteria, evidence) = assess_completion(&goal);
    goal.completion.criteria = criteria.clone();
    let report = report_from_assessment(
        &goal,
        GoalCompletionStatus::Assessed,
        outcome,
        criteria,
        evidence,
        summary,
        vec![
            "Verification waiver is user-explicit — success is not invented.".into(),
        ],
    );
    (goal, report)
}

/// Attempt evidence-based successful completion (Planner-owned).
pub fn attempt_complete(mut goal: CodingGoal) -> (CodingGoal, GoalCompletionReport) {
    let (outcome, criteria, evidence_used) = assess_completion(&goal);
    goal.completion.criteria = criteria.clone();

    if matches!(
        outcome,
        GoalCompletionOutcome::AlreadyTerminal
    ) {
        let report = report_from_assessment(
            &goal,
            GoalCompletionStatus::Rejected,
            outcome,
            criteria,
            evidence_used,
            "Goal is already terminal — completion not re-applied.".into(),
            vec![],
        );
        return (goal, report);
    }

    if outcome == GoalCompletionOutcome::Blocked {
        let summary = "Blocked goals cannot become Completed.".to_string();
        goal.completion
            .push_history("attempt_complete", outcome, &summary);
        let report = report_from_assessment(
            &goal,
            GoalCompletionStatus::Rejected,
            outcome,
            criteria,
            evidence_used,
            summary,
            vec!["Unblock the goal before completing.".into()],
        );
        return (goal, report);
    }

    if outcome == GoalCompletionOutcome::FailedVerification {
        let summary =
            "Failed verification prevents successful completion unless explicitly waived."
                .to_string();
        goal.completion
            .push_history("attempt_complete", outcome, &summary);
        let report = report_from_assessment(
            &goal,
            GoalCompletionStatus::Incomplete,
            outcome,
            criteria,
            evidence_used,
            summary,
            vec![
                "Say **waive failed verification** with a reason, or recover/fix first.".into(),
            ],
        );
        return (goal, report);
    }

    if outcome != GoalCompletionOutcome::Ready {
        let unknown = criteria
            .iter()
            .filter(|c| c.satisfaction == CriterionSatisfaction::Unknown)
            .count();
        let unsat = criteria
            .iter()
            .filter(|c| c.satisfaction == CriterionSatisfaction::Unsatisfied)
            .count();
        let summary = format!(
            "Goal is not ready to complete (unknown={unknown}, unsatisfied={unsat}). Unknown is never treated as satisfied."
        );
        goal.completion
            .push_history("attempt_complete", GoalCompletionOutcome::Incomplete, &summary);
        let report = report_from_assessment(
            &goal,
            GoalCompletionStatus::Incomplete,
            GoalCompletionOutcome::Incomplete,
            criteria,
            evidence_used,
            summary,
            vec![
                "Satisfy criteria with evidence, or explicitly waive them.".into(),
                "Or say **end this goal incomplete** to user-end without successful completion."
                    .into(),
            ],
        );
        return (goal, report);
    }

    let evidence = build_completion_evidence(&goal, &criteria);
    match complete_goal(goal.clone(), Some(evidence)) {
        Ok(mut completed) => {
            completed.completion.criteria = criteria.clone();
            completed.completion.ending_kind = Some(GoalEndingKind::SuccessfullyCompleted);
            let summary = "Goal successfully completed based on acceptance criteria, tasks, and verification evidence.".to_string();
            completed.completion.push_history(
                "complete",
                GoalCompletionOutcome::SuccessfullyCompleted,
                &summary,
            );
            completed.notes.push(
                "C5.10: successfully completed (evidence-based) — distinct from user-ended."
                    .into(),
            );
            let mut report = report_from_assessment(
                &completed,
                GoalCompletionStatus::Completed,
                GoalCompletionOutcome::SuccessfullyCompleted,
                criteria,
                evidence_used,
                summary,
                vec![
                    "Planner declared completion — model did not.".into(),
                    "History preserved on the Coding Goal.".into(),
                ],
            );
            report.ending_kind = Some(GoalEndingKind::SuccessfullyCompleted);
            (completed, report)
        }
        Err(msg) => {
            let summary = format!("Could not complete: {msg}");
            goal.completion
                .push_history("attempt_complete", GoalCompletionOutcome::Rejected, &summary);
            let report = report_from_assessment(
                &goal,
                GoalCompletionStatus::Rejected,
                GoalCompletionOutcome::Rejected,
                criteria,
                evidence_used,
                summary,
                vec![],
            );
            (goal, report)
        }
    }
}

/// User explicitly ends an incomplete goal (≠ SuccessfullyCompleted).
pub fn user_end_incomplete(goal: CodingGoal, reason: &str) -> (CodingGoal, GoalCompletionReport) {
    let reason = if reason.trim().is_empty() {
        "User ended the goal without successful completion.".to_string()
    } else {
        reason.trim().to_string()
    };
    let (outcome_before, criteria, evidence_used) = assess_completion(&goal);
    let mut ended = cancel_goal(goal, &format!("User-ended incomplete: {reason}"));
    ended.completion.criteria = criteria.clone();
    ended.completion.ending_kind = Some(GoalEndingKind::UserEnded);
    let summary = format!(
        "User ended incomplete goal (assessment was {}). Reason: {reason}",
        outcome_before.label()
    );
    ended.completion.push_history(
        "user_end",
        GoalCompletionOutcome::UserEnded,
        &summary,
    );
    ended.notes.push(
        "C5.10: user-ended (incomplete) — distinct from successfully completed.".into(),
    );
    let mut report = report_from_assessment(
        &ended,
        GoalCompletionStatus::UserEnded,
        GoalCompletionOutcome::UserEnded,
        criteria,
        evidence_used,
        summary,
        vec![
            "Preserved completed work and failed task state.".into(),
            "Ending kind is user_ended — not successfully_completed.".into(),
        ],
    );
    report.ending_kind = Some(GoalEndingKind::UserEnded);
    (ended, report)
}

/// Detect completion phrases (Conversation First; phrase-only).
pub fn detect_completion_request(request: &UserRequest) -> Option<CompletionAssessment> {
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

    // C5.8 / C5.9 win over completion for cancel/pause/recover phrasing.
    if trimmed.starts_with("pause ")
        || trimmed.starts_with("resume ")
        || trimmed.starts_with("cancel ")
        || trimmed.starts_with("stop this goal")
        || trimmed.starts_with("stop working on this goal")
        || trimmed.starts_with("recover ")
        || trimmed.starts_with("propose a corrective")
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "goal completion status"
            | "completion status"
            | "is this goal complete"
            | "is the goal complete"
            | "assess goal completion"
            | "assess completion"
            | "check goal completion"
    ) || trimmed.starts_with("is this goal complete")
        || trimmed.starts_with("assess goal completion")
    {
        return Some(CompletionAssessment {
            kind: CompletionRequestKind::Assess,
            matched_rules: vec!["c510_assess"],
            waive_reason: None,
        });
    }

    if matches!(
        trimmed.as_str(),
        "show goal completion" | "goal completion" | "completion evidence"
    ) {
        return Some(CompletionAssessment {
            kind: CompletionRequestKind::Status,
            matched_rules: vec!["c510_status"],
            waive_reason: None,
        });
    }

    if looks_like_complete_attempt(&trimmed) {
        return Some(CompletionAssessment {
            kind: CompletionRequestKind::AttemptComplete,
            matched_rules: vec!["c510_complete"],
            waive_reason: None,
        });
    }

    if looks_like_user_end(&trimmed) {
        let reason = extract_reason(content, &[" because ", ": ", " — ", " - "]);
        return Some(CompletionAssessment {
            kind: CompletionRequestKind::UserEnd,
            matched_rules: vec!["c510_user_end"],
            waive_reason: reason,
        });
    }

    if looks_like_waive_verification(&trimmed) {
        let reason = extract_reason(content, &[" because ", ": ", " — ", " - "]);
        return Some(CompletionAssessment {
            kind: CompletionRequestKind::WaiveVerification,
            matched_rules: vec!["c510_waive_verification"],
            waive_reason: reason,
        });
    }

    if looks_like_waive_criteria(&trimmed) {
        let reason = extract_reason(content, &[" because ", ": ", " — ", " - "]);
        return Some(CompletionAssessment {
            kind: CompletionRequestKind::WaiveCriteria,
            matched_rules: vec!["c510_waive_criteria"],
            waive_reason: reason,
        });
    }

    None
}

fn looks_like_complete_attempt(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "mark this goal complete"
            | "mark the goal complete"
            | "mark goal complete"
            | "complete this goal"
            | "complete the goal"
            | "goal is done"
            | "goal is complete"
            | "we've finished this goal"
            | "we finished this goal"
            | "finish this goal"
    ) || trimmed.starts_with("mark this goal as complete")
        || trimmed.starts_with("mark the goal as complete")
}

fn looks_like_user_end(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "end this goal incomplete"
            | "end the goal incomplete"
            | "close this goal incomplete"
            | "accept this goal as incomplete"
            | "end this goal without completing"
            | "close this goal without completing"
            | "user end this goal"
            | "mark this goal ended"
    ) || trimmed.starts_with("end this goal incomplete")
        || trimmed.starts_with("accept this goal as incomplete")
        || trimmed.starts_with("close this goal incomplete")
        || trimmed.starts_with("end this goal without completing")
}

fn looks_like_waive_verification(trimmed: &str) -> bool {
    trimmed.starts_with("waive failed verification")
        || trimmed.starts_with("waive the failed verification")
        || trimmed.starts_with("waive verification")
        || trimmed == "waive failed verification"
}

fn looks_like_waive_criteria(trimmed: &str) -> bool {
    trimmed.starts_with("waive remaining criteria")
        || trimmed.starts_with("waive remaining acceptance criteria")
        || trimmed.starts_with("waive acceptance criteria")
        || trimmed.starts_with("waive the remaining criteria")
        || trimmed == "waive remaining criteria"
}

fn extract_reason(content: &str, markers: &[&str]) -> Option<String> {
    let lower = content.to_ascii_lowercase();
    for marker in markers {
        if let Some(idx) = lower.find(marker) {
            let rest = content[idx + marker.len()..].trim();
            if !rest.is_empty() {
                return Some(
                    rest.trim_end_matches(|c: char| c == '.' || c == '!')
                        .to_string(),
                );
            }
        }
    }
    None
}

/// Apply a detected completion assessment onto the active goal (if any).
pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &CompletionAssessment,
) -> (GoalCompletionReport, Option<CodingGoal>) {
    let Some(goal) = goal else {
        return (
            GoalCompletionReport {
                status: GoalCompletionStatus::NoActiveGoal,
                outcome: GoalCompletionOutcome::NoActiveGoal,
                summary: "No active Coding Goal.".into(),
                notes: vec!["Create or activate a Coding Goal before completion.".into()],
                ..Default::default()
            },
            None,
        );
    };

    match assessment.kind {
        CompletionRequestKind::Assess | CompletionRequestKind::Status => {
            let (outcome, criteria, evidence) = assess_completion(goal);
            let mut updated = goal.clone();
            updated.completion.criteria = criteria.clone();
            let summary = format!(
                "Completion assessment: {}. Planner-owned; model cannot declare completion.",
                outcome.label()
            );
            updated
                .completion
                .push_history("assess", outcome, &summary);
            let status = if assessment.kind == CompletionRequestKind::Status {
                GoalCompletionStatus::Status
            } else {
                GoalCompletionStatus::Assessed
            };
            let report = report_from_assessment(
                &updated,
                status,
                outcome,
                criteria,
                evidence,
                summary,
                vec![
                    "Unknown criteria are never treated as satisfied.".into(),
                    "Successful completion requires evidence or explicit waiver.".into(),
                ],
            );
            (report, Some(updated))
        }
        CompletionRequestKind::WaiveCriteria => {
            let reason = assessment
                .waive_reason
                .clone()
                .unwrap_or_else(|| "User waived remaining criteria.".into());
            let (updated, report) = waive_remaining_criteria(goal.clone(), &reason);
            (report, Some(updated))
        }
        CompletionRequestKind::WaiveVerification => {
            let reason = assessment
                .waive_reason
                .clone()
                .unwrap_or_else(|| "User waived verification.".into());
            let (updated, report) = waive_verification(goal.clone(), &reason);
            (report, Some(updated))
        }
        CompletionRequestKind::AttemptComplete => {
            let (updated, report) = attempt_complete(goal.clone());
            (report, Some(updated))
        }
        CompletionRequestKind::UserEnd => {
            let reason = assessment
                .waive_reason
                .clone()
                .unwrap_or_else(|| "User ended incomplete.".into());
            let (updated, report) = user_end_incomplete(goal.clone(), &reason);
            (report, Some(updated))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_state_machine::{
        transition_goal, AgentLifecycleEvent, AgentLifecycleState,
    };
    use crate::coding_goal::create_goal;
    use crate::goal_decomposition::GoalTask;
    use jaymi_context::ContextBundle;

    fn base_goal() -> CodingGoal {
        let assessment = crate::coding_goal::detect_coding_goal_request(&UserRequest::new(
            "Build a login system. Acceptance criteria: tests pass.",
        ))
        .expect("create goal phrase");
        create_goal(&assessment, &ContextBundle::default(), None)
    }

    fn with_passed_verification(mut g: CodingGoal) -> CodingGoal {
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginUnderstanding {
                reason: "c510".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginInvestigation {
                reason: "c510".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginPlanning {
                reason: "c510".into(),
            },
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
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginVerification {
                reason: "v".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::VerificationPassed {
                evidence_ref: "verify:ok".into(),
            },
        )
        .unwrap();
        g.verification_state = GoalVerificationState::Passed;
        g.completed_work.push("Applied login changes.".into());
        g.remaining_work.clear();
        g
    }

    fn all_tasks_completed(mut g: CodingGoal) -> CodingGoal {
        if g.tasks.is_empty() {
            g.tasks.push(GoalTask {
                id: "t1".into(),
                description: "Implement login".into(),
                status: GoalTaskStatus::Completed,
                ..GoalTask::default()
            });
        } else {
            for t in &mut g.tasks {
                t.status = GoalTaskStatus::Completed;
            }
        }
        g
    }

    #[test]
    fn all_criteria_satisfied_ready() {
        let g = all_tasks_completed(with_passed_verification(base_goal()));
        let (outcome, criteria, _) = assess_completion(&g);
        assert_eq!(outcome, GoalCompletionOutcome::Ready);
        assert!(criteria.iter().all(|c| c.satisfaction == CriterionSatisfaction::Satisfied));
        assert!(criteria.iter().all(|c| !c.evidence_refs.is_empty()));
    }

    #[test]
    fn failed_verification_blocks_success() {
        let mut g = with_passed_verification(base_goal());
        g.verification_state = GoalVerificationState::Failed;
        g = transition_goal(
            g.clone(),
            AgentLifecycleEvent::VerificationFailed {
                evidence_ref: "fail".into(),
            },
        )
        .unwrap_or(g);
        let (outcome, criteria, _) = assess_completion(&g);
        assert_eq!(outcome, GoalCompletionOutcome::FailedVerification);
        assert!(criteria
            .iter()
            .any(|c| c.satisfaction == CriterionSatisfaction::Unsatisfied));
        let (_, report) = attempt_complete(g);
        assert_ne!(
            report.outcome,
            GoalCompletionOutcome::SuccessfullyCompleted
        );
        assert!(!report.invented_success);
    }

    #[test]
    fn unknown_criterion_not_satisfied() {
        let g = base_goal(); // no verification yet
        let (outcome, criteria, _) = assess_completion(&g);
        assert_eq!(outcome, GoalCompletionOutcome::Incomplete);
        assert!(criteria
            .iter()
            .any(|c| c.satisfaction == CriterionSatisfaction::Unknown));
        assert!(!criteria
            .iter()
            .any(|c| c.satisfaction == CriterionSatisfaction::Satisfied));
    }

    #[test]
    fn blocked_task_cannot_complete() {
        let mut g = all_tasks_completed(with_passed_verification(base_goal()));
        g.tasks.push(GoalTask {
            id: "blocked".into(),
            description: "Needs decision".into(),
            status: GoalTaskStatus::Blocked,
            ..GoalTask::default()
        });
        g.status = CodingGoalStatus::Blocked;
        g.agent_lifecycle.state = AgentLifecycleState::Blocked;
        g.blockers.push("Need product decision.".into());
        let (outcome, _, _) = assess_completion(&g);
        assert_eq!(outcome, GoalCompletionOutcome::Blocked);
        let (_, report) = attempt_complete(g);
        assert_eq!(report.outcome, GoalCompletionOutcome::Blocked);
        assert_ne!(report.status, GoalCompletionStatus::Completed);
    }

    #[test]
    fn user_cancellation_distinct_from_success() {
        // C5.8-style cancel via user_end path distinction + cancel_goal.
        let g = base_goal();
        let cancelled = cancel_goal(g, "User cancelled the coding goal.");
        assert_eq!(cancelled.status, CodingGoalStatus::Cancelled);
        assert_ne!(cancelled.status, CodingGoalStatus::Completed);
        let (outcome, _, _) = assess_completion(&cancelled);
        assert_eq!(outcome, GoalCompletionOutcome::AlreadyTerminal);
    }

    #[test]
    fn user_acceptance_waiver_then_complete() {
        let mut g = all_tasks_completed(with_passed_verification(base_goal()));
        // Force one criterion unknown by clearing verification temporarily then restoring
        // after waive path: instead waive while incomplete.
        g.verification_state = GoalVerificationState::Unknown;
        let (g, waive_report) =
            waive_remaining_criteria(g, "Demo scope — accept remaining criteria");
        assert!(waive_report
            .criteria
            .iter()
            .any(|c| c.satisfaction == CriterionSatisfaction::Waived));
        let (g, v_report) = waive_verification(g, "Accept without fresh verification");
        assert!(v_report.verification_waived);
        let (g, report) = attempt_complete(g);
        assert_eq!(
            report.outcome,
            GoalCompletionOutcome::SuccessfullyCompleted
        );
        assert_eq!(
            g.completion.ending_kind,
            Some(GoalEndingKind::SuccessfullyCompleted)
        );
        assert_eq!(g.status, CodingGoalStatus::Completed);
        assert!(!report.invented_success);
        assert!(!report.model_authoritative);
        assert!(!g.completion.history.is_empty());
    }

    #[test]
    fn incomplete_goal_rejects_complete() {
        let g = base_goal();
        let (g2, report) = attempt_complete(g);
        assert_eq!(report.outcome, GoalCompletionOutcome::Incomplete);
        assert_eq!(g2.status, CodingGoalStatus::Active);
        assert!(!report.unknown_treated_as_satisfied);
    }

    #[test]
    fn successful_completion() {
        let g = all_tasks_completed(with_passed_verification(base_goal()));
        let (g, report) = attempt_complete(g);
        assert_eq!(
            report.outcome,
            GoalCompletionOutcome::SuccessfullyCompleted
        );
        assert_eq!(report.status, GoalCompletionStatus::Completed);
        assert_eq!(g.status, CodingGoalStatus::Completed);
        assert_eq!(
            g.completion.ending_kind,
            Some(GoalEndingKind::SuccessfullyCompleted)
        );
        assert!(!report.mutated);
        assert!(!report.invented_success);
        assert!(report.planner_owned);
        assert!(!report.evidence_used.is_empty());
    }

    #[test]
    fn user_end_incomplete_distinct() {
        let g = base_goal();
        let (g, report) = user_end_incomplete(g, "Stopping early");
        assert_eq!(report.outcome, GoalCompletionOutcome::UserEnded);
        assert_eq!(
            g.completion.ending_kind,
            Some(GoalEndingKind::UserEnded)
        );
        assert_eq!(g.status, CodingGoalStatus::Cancelled);
        assert_ne!(
            g.completion.ending_kind,
            Some(GoalEndingKind::SuccessfullyCompleted)
        );
    }

    #[test]
    fn no_hidden_mutation_on_assess() {
        let g = base_goal();
        let assessment = CompletionAssessment {
            kind: CompletionRequestKind::Assess,
            matched_rules: vec!["c510_assess"],
            waive_reason: None,
        };
        let (report, updated) = apply_assessment(Some(&g), &assessment);
        assert!(!report.mutated);
        assert!(!report.invented_success);
        assert!(!report.model_authoritative);
        assert!(!report.unknown_treated_as_satisfied);
        assert!(updated.unwrap().mutated == false);
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_completion_request(&UserRequest::new(
            "Assess goal completion."
        ))
        .is_some());
        assert!(detect_completion_request(&UserRequest::new(
            "Mark this goal complete."
        ))
        .is_some());
        assert!(detect_completion_request(&UserRequest::new(
            "Waive remaining criteria because demo."
        ))
        .is_some());
        assert!(detect_completion_request(&UserRequest::new(
            "End this goal incomplete because pausing."
        ))
        .is_some());
        assert!(detect_completion_request(&UserRequest::new("Cancel this goal.")).is_none());
        assert!(detect_completion_request(&UserRequest::new("Recover this goal.")).is_none());
        assert!(detect_completion_request(&UserRequest::new("Build a login system.")).is_none());
    }

    #[test]
    fn history_preserved_across_ops() {
        let g = base_goal();
        let (g, _) = waive_remaining_criteria(g, "scope");
        let (g, _) = user_end_incomplete(g, "done enough");
        assert!(g.completion.history.len() >= 2);
        assert_eq!(
            g.completion.ending_kind,
            Some(GoalEndingKind::UserEnded)
        );
    }
}
