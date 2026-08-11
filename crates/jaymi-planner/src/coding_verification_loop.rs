//! Bounded Coding Verification Loop (Sprint C4.6).
//!
//! Connect C3 mutation and C4 verification into one bounded conversational
//! workflow owned by the Planner — without a parallel agent or C5 loop.
//!
//! Canonical flow:
//! Approved Coding Change → Apply → Verify → Result
//! PASS → Verification Summary → Done
//! FAIL → Diagnostics → C2 Investigation → C3 Coding Plan → Review
//!       → optional approved correction → Apply → Verify again
//!
//! Constitutional constraints:
//! - Reuse C2 / C3 / C4 — no second coding-agent architecture
//! - Every corrective mutation requires explicit Review/Approval
//! - Never auto-approve; never silently retry mutation
//! - Hard [`MAX_CYCLES`] bound; stop and explain when reached
//! - User can cancel/stop; failed verification never triggers uncontrolled mutation
//! - Not C5's general-purpose autonomous Agent Loop

use crate::execution_plan::ExecutionSummary;
use crate::verification_diagnostics::{VerificationEvidence, VerificationResultKind};
use crate::verification_summary::{VerificationStatus, VerificationSummary};

/// Hard maximum verification/recovery cycles for one user-requested workflow.
pub const MAX_CYCLES: u32 = 3;

/// Phase of the bounded Apply → Verify → (optional Fix) workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopPhase {
    /// No active workflow.
    Idle,
    /// Coding apply finished; verification offered but not run.
    AppliedAwaitingVerify,
    /// Verification ExecutionPlan is awaiting Review Before Action.
    AwaitingVerifyReview,
    /// Verification completed successfully — terminal.
    Passed,
    /// Verification failed; corrective plan may be offered (if under max).
    FailedAwaitingFix,
    /// Corrective Coding Plan proposed (C4.4); awaiting user-driven patch review.
    FixProposed,
    /// Hard cycle limit reached — stop.
    Exhausted,
    /// User cancelled / stopped the workflow.
    Cancelled,
}

impl LoopPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::AppliedAwaitingVerify => "applied_awaiting_verify",
            Self::AwaitingVerifyReview => "awaiting_verify_review",
            Self::Passed => "passed",
            Self::FailedAwaitingFix => "failed_awaiting_fix",
            Self::FixProposed => "fix_proposed",
            Self::Exhausted => "exhausted",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::AppliedAwaitingVerify => "Applied — awaiting verification",
            Self::AwaitingVerifyReview => "Verification awaiting review",
            Self::Passed => "Passed",
            Self::FailedAwaitingFix => "Failed — awaiting fix",
            Self::FixProposed => "Corrective plan proposed",
            Self::Exhausted => "Cycle limit reached",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Passed | Self::Exhausted | Self::Cancelled
        )
    }
}

impl Default for LoopPhase {
    fn default() -> Self {
        Self::Idle
    }
}

/// Outcome of one verify attempt inside the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleOutcome {
    Pass,
    Fail,
    Cancelled,
    Timeout,
    ToolFailure,
    Blocked,
    Unknown,
}

impl CycleOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::ToolFailure => "tool_failure",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

/// One recorded Apply→Verify (and optional Fix) cycle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CycleRecord {
    pub index: u32,
    pub files_edited: Vec<String>,
    pub verification_command: Option<String>,
    pub verification_kind: Option<VerificationResultKind>,
    pub outcome: CycleOutcome,
    pub notes: Vec<String>,
}

/// Planner-owned bounded coding verification workflow state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodingVerificationLoop {
    pub phase: LoopPhase,
    /// Completed verification attempts in this workflow (1..=max_cycles when verifying).
    pub cycles_completed: u32,
    pub max_cycles: u32,
    pub history: Vec<CycleRecord>,
    pub active: bool,
    pub goal: String,
    pub notes: Vec<String>,
    /// Always false — loop never auto-approves.
    pub auto_approved: bool,
    /// Always false — loop never silently retries mutation.
    pub silent_retry: bool,
}

impl Default for CodingVerificationLoop {
    fn default() -> Self {
        Self {
            phase: LoopPhase::Idle,
            cycles_completed: 0,
            max_cycles: MAX_CYCLES,
            history: Vec::new(),
            active: false,
            goal: String::new(),
            notes: Vec::new(),
            auto_approved: false,
            silent_retry: false,
        }
    }
}

impl CodingVerificationLoop {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            active: true,
            phase: LoopPhase::AppliedAwaitingVerify,
            ..Self::default()
        }
    }

    pub fn allows_fix_offer(&self) -> bool {
        self.active
            && matches!(self.phase, LoopPhase::FailedAwaitingFix)
            && self.cycles_completed < self.max_cycles
    }

    pub fn is_exhausted(&self) -> bool {
        matches!(self.phase, LoopPhase::Exhausted)
            || (self.cycles_completed >= self.max_cycles
                && matches!(self.phase, LoopPhase::FailedAwaitingFix))
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Verification Loop\n");
        out.push_str(&format!("\n### Phase\n- {}\n", self.phase.label()));
        out.push_str(&format!(
            "\n### Cycles\n- {} of {} completed\n",
            self.cycles_completed, self.max_cycles
        ));
        if !self.goal.is_empty() {
            out.push_str(&format!("\n### Goal\n- {}\n", self.goal));
        }
        if !self.history.is_empty() {
            out.push_str("\n### History\n");
            for cycle in &self.history {
                out.push_str(&format!(
                    "- Cycle {}: {}{}\n",
                    cycle.index,
                    cycle.outcome.as_str(),
                    cycle
                        .verification_command
                        .as_ref()
                        .map(|c| format!(" (`{c}`)"))
                        .unwrap_or_default()
                ));
            }
        }
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for note in &self.notes {
                out.push_str(&format!("- {note}\n"));
            }
        }
        out.push_str(
            "\n### Constraints\n\
             - Every corrective mutation requires Review Before Action.\n\
             - No automatic approval; no silent mutation retries.\n\
             - Not a general-purpose autonomous agent loop (C5).\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "coding_verification_loop phase={} cycles={}/{} history={} active={} auto_approved={} silent_retry={}",
            self.phase.as_str(),
            self.cycles_completed,
            self.max_cycles,
            self.history.len(),
            self.active,
            self.auto_approved,
            self.silent_retry,
        )
    }

    /// Conversation footnote for cycle progress (non-diagnostic).
    pub fn progress_note(&self) -> Option<String> {
        if !self.active {
            return None;
        }
        match self.phase {
            LoopPhase::AppliedAwaitingVerify => Some(format!(
                "Verification loop: apply complete — run verification when ready (cycle limit {})."
                , self.max_cycles
            )),
            LoopPhase::FailedAwaitingFix => Some(format!(
                "Verification loop: failure after cycle {}/{} — review a corrective plan or stop.",
                self.cycles_completed, self.max_cycles
            )),
            LoopPhase::FixProposed => Some(format!(
                "Verification loop: corrective plan proposed after cycle {}/{} — Approve patches to continue.",
                self.cycles_completed, self.max_cycles
            )),
            LoopPhase::Exhausted => Some(format!(
                "Verification loop stopped: reached the maximum of {} verification/recovery cycles.",
                self.max_cycles
            )),
            LoopPhase::Passed => Some("Verification loop: passed — done.".into()),
            LoopPhase::Cancelled => Some("Verification loop cancelled.".into()),
            LoopPhase::AwaitingVerifyReview => Some(format!(
                "Verification loop: awaiting approval to run checks (cycle {}/{} next).",
                self.cycles_completed.saturating_add(1),
                self.max_cycles
            )),
            LoopPhase::Idle => None,
        }
    }
}

/// Detect an explicit user request to stop the bounded loop.
pub fn detect_loop_cancel(request_text: &str) -> bool {
    let lower = request_text.trim().to_ascii_lowercase();
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '!' || c == '?');
    matches!(
        trimmed,
        "stop"
            | "stop here"
            | "cancel"
            | "cancel this"
            | "never mind"
            | "nevermind"
            | "abort"
            | "give up"
            | "enough"
            | "leave it"
            | "don't fix"
            | "do not fix"
            | "stop verifying"
            | "stop the loop"
            | "end verification"
            | "stop verification"
    ) || trimmed.starts_with("stop the ")
        || trimmed.starts_with("cancel the ")
        || trimmed.starts_with("don't bother")
        || trimmed.starts_with("do not bother")
}

/// Start or refresh a loop after a successful coding apply.
pub fn on_apply_completed(
    existing: Option<CodingVerificationLoop>,
    summary: &ExecutionSummary,
) -> CodingVerificationLoop {
    let goal = if summary.goal.is_empty() {
        "Coding change".into()
    } else {
        summary.goal.clone()
    };
    let mut loop_state = existing.unwrap_or_else(|| CodingVerificationLoop::new(goal.clone()));
    if !loop_state.active || loop_state.phase.is_terminal() {
        loop_state = CodingVerificationLoop::new(goal);
    } else {
        // Continuing after an approved correction apply.
        loop_state.phase = LoopPhase::AppliedAwaitingVerify;
        loop_state.notes.push(format!(
            "Correction applied — verify again (cycles completed so far: {}).",
            loop_state.cycles_completed
        ));
    }
    loop_state.auto_approved = false;
    loop_state.silent_retry = false;
    if loop_state.goal.is_empty() {
        loop_state.goal = summary.goal.clone();
    }
    loop_state
}

/// Mark that verification is paused for Review Before Action.
pub fn on_verify_awaiting_review(mut loop_state: CodingVerificationLoop) -> CodingVerificationLoop {
    if loop_state.active && !loop_state.phase.is_terminal() {
        loop_state.phase = LoopPhase::AwaitingVerifyReview;
    }
    loop_state
}

/// Advance the loop after a verification result (C4.2/C4.5).
pub fn on_verification_result(
    mut loop_state: CodingVerificationLoop,
    summary: &VerificationSummary,
    evidence: Option<&VerificationEvidence>,
) -> CodingVerificationLoop {
    if !loop_state.active {
        return loop_state;
    }
    if matches!(
        summary.status,
        Some(VerificationStatus::NotRun) | Some(VerificationStatus::Blocked)
    ) {
        if matches!(summary.status, Some(VerificationStatus::Blocked)) {
            loop_state.notes.push(
                "Verification was blocked — no cycle counted; adjust permissions and retry."
                    .into(),
            );
        }
        return loop_state;
    }
    if matches!(summary.status, Some(VerificationStatus::Cancelled)) {
        return cancel_loop(loop_state, "Verification cancelled by user.");
    }

    let outcome = outcome_from_summary(summary, evidence);
    let next_index = loop_state.cycles_completed.saturating_add(1);
    let record = CycleRecord {
        index: next_index,
        files_edited: summary.changed_files.clone(),
        verification_command: summary
            .verified_commands
            .first()
            .map(|c| c.command.clone())
            .or_else(|| evidence.and_then(|e| e.command.clone())),
        verification_kind: evidence.and_then(|e| e.kind).or(summary.evidence_kind),
        outcome,
        notes: summary.remains.iter().take(4).cloned().collect(),
    };
    loop_state.history.push(record);
    loop_state.cycles_completed = next_index;

    match outcome {
        CycleOutcome::Pass => {
            loop_state.phase = LoopPhase::Passed;
            loop_state.active = false;
            loop_state
                .notes
                .push("Verification passed — workflow complete.".into());
        }
        CycleOutcome::Fail
        | CycleOutcome::Timeout
        | CycleOutcome::ToolFailure
        | CycleOutcome::Unknown => {
            if loop_state.cycles_completed >= loop_state.max_cycles {
                loop_state.phase = LoopPhase::Exhausted;
                loop_state.active = false;
                loop_state.notes.push(format!(
                    "Reached the maximum of {} verification/recovery cycles — stopping. Review the history, then start a fresh request if you want another attempt.",
                    loop_state.max_cycles
                ));
            } else {
                loop_state.phase = LoopPhase::FailedAwaitingFix;
                loop_state.notes.push(format!(
                    "Verification failed on cycle {}/{} — a corrective plan may be reviewed next (never auto-applied).",
                    loop_state.cycles_completed, loop_state.max_cycles
                ));
            }
        }
        CycleOutcome::Cancelled | CycleOutcome::Blocked => {
            // Handled above for cancel; blocked shouldn't increment often.
        }
    }
    loop_state.auto_approved = false;
    loop_state.silent_retry = false;
    loop_state
}

fn outcome_from_summary(
    summary: &VerificationSummary,
    evidence: Option<&VerificationEvidence>,
) -> CycleOutcome {
    match summary.status {
        Some(VerificationStatus::Passed) => CycleOutcome::Pass,
        Some(VerificationStatus::Cancelled) => CycleOutcome::Cancelled,
        Some(VerificationStatus::Blocked) => CycleOutcome::Blocked,
        Some(VerificationStatus::Failed) => match evidence.and_then(|e| e.kind) {
            Some(VerificationResultKind::Timeout) => CycleOutcome::Timeout,
            Some(VerificationResultKind::ToolFailure) => CycleOutcome::ToolFailure,
            Some(VerificationResultKind::EnvironmentFailure) => CycleOutcome::ToolFailure,
            _ => CycleOutcome::Fail,
        },
        Some(VerificationStatus::Unknown) | Some(VerificationStatus::NotRun) | None => {
            CycleOutcome::Unknown
        }
    }
}

/// Mark that C4.4 proposed a corrective plan (still no mutation).
pub fn on_fix_proposed(mut loop_state: CodingVerificationLoop) -> CodingVerificationLoop {
    if !loop_state.active {
        return loop_state;
    }
    if loop_state.is_exhausted() || matches!(loop_state.phase, LoopPhase::Exhausted) {
        loop_state.notes.push(
            "Cycle limit already reached — corrective plan is informational only; no further auto-loop."
                .into(),
        );
        return loop_state;
    }
    if matches!(
        loop_state.phase,
        LoopPhase::FailedAwaitingFix | LoopPhase::AppliedAwaitingVerify
    ) {
        loop_state.phase = LoopPhase::FixProposed;
        loop_state.notes.push(
            "Corrective Coding Plan proposed — mutations still require Review Before Action."
                .into(),
        );
    }
    loop_state.auto_approved = false;
    loop_state.silent_retry = false;
    loop_state
}

/// User cancelled / stopped the workflow.
pub fn cancel_loop(
    mut loop_state: CodingVerificationLoop,
    reason: impl Into<String>,
) -> CodingVerificationLoop {
    let reason = reason.into();
    loop_state.phase = LoopPhase::Cancelled;
    loop_state.active = false;
    loop_state.notes.push(reason);
    loop_state.auto_approved = false;
    loop_state.silent_retry = false;
    loop_state
}

/// Whether Review Fix chips should be suppressed (exhausted / cancelled / passed).
pub fn suppress_fix_offer(loop_state: Option<&CodingVerificationLoop>) -> bool {
    match loop_state {
        None => false,
        Some(state) => {
            !state.active
                || matches!(
                    state.phase,
                    LoopPhase::Exhausted
                        | LoopPhase::Cancelled
                        | LoopPhase::Passed
                        | LoopPhase::Idle
                )
                || state.cycles_completed >= state.max_cycles
        }
    }
}

/// Append a short loop progress note onto conversation content when useful.
pub fn append_progress_note(content: &str, loop_state: &CodingVerificationLoop) -> String {
    let Some(note) = loop_state.progress_note() else {
        return content.to_string();
    };
    if content.contains(&note) {
        return content.to_string();
    }
    format!("{content}\n\n_{note}_\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{ExecutionPlanId, ExecutionStatus};
    use crate::verification_summary::{VerificationStatus, VerificationSummary};

    fn apply_summary() -> ExecutionSummary {
        ExecutionSummary {
            plan_id: ExecutionPlanId::from_existing("p1"),
            status: ExecutionStatus::Completed,
            goal: "Fix this bug.".into(),
            actions_performed: vec!["write_file".into()],
            resources_changed: Vec::new(),
            files_edited: vec!["src/lib.rs".into()],
            duration_ms: 1,
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
        }
    }

    fn failed_summary() -> VerificationSummary {
        VerificationSummary {
            status: Some(VerificationStatus::Failed),
            changed_files: vec!["src/lib.rs".into()],
            evidence_kind: Some(VerificationResultKind::BuildFailure),
            recommended_fix_prompt: Some("Fix the build failure.".into()),
            recommended_fix_label: Some("Review Fix".into()),
            ..Default::default()
        }
    }

    fn passed_summary() -> VerificationSummary {
        VerificationSummary {
            status: Some(VerificationStatus::Passed),
            ..Default::default()
        }
    }

    #[test]
    fn successful_first_verification_ends_loop() {
        let mut state = on_apply_completed(None, &apply_summary());
        assert_eq!(state.phase, LoopPhase::AppliedAwaitingVerify);
        state = on_verification_result(state, &passed_summary(), None);
        assert_eq!(state.phase, LoopPhase::Passed);
        assert!(!state.active);
        assert_eq!(state.cycles_completed, 1);
        assert!(!state.auto_approved);
        assert!(!state.silent_retry);
    }

    #[test]
    fn failure_then_fix_then_pass() {
        let mut state = on_apply_completed(None, &apply_summary());
        state = on_verification_result(state, &failed_summary(), None);
        assert_eq!(state.phase, LoopPhase::FailedAwaitingFix);
        assert!(state.allows_fix_offer());
        state = on_fix_proposed(state);
        assert_eq!(state.phase, LoopPhase::FixProposed);
        // Approved correction apply
        state = on_apply_completed(Some(state), &apply_summary());
        assert_eq!(state.phase, LoopPhase::AppliedAwaitingVerify);
        state = on_verification_result(state, &passed_summary(), None);
        assert_eq!(state.phase, LoopPhase::Passed);
        assert_eq!(state.history.len(), 2);
    }

    #[test]
    fn maximum_cycles_exhausted() {
        let mut state = on_apply_completed(None, &apply_summary());
        for i in 1..=MAX_CYCLES {
            state = on_verification_result(state, &failed_summary(), None);
            if i < MAX_CYCLES {
                assert_eq!(state.phase, LoopPhase::FailedAwaitingFix);
                state = on_fix_proposed(state);
                state = on_apply_completed(Some(state), &apply_summary());
            }
        }
        assert_eq!(state.phase, LoopPhase::Exhausted);
        assert!(suppress_fix_offer(Some(&state)));
        assert!(!state.active);
        assert_eq!(state.cycles_completed, MAX_CYCLES);
    }

    #[test]
    fn cancel_phrase_and_cancel_loop() {
        assert!(detect_loop_cancel("Stop."));
        assert!(detect_loop_cancel("never mind"));
        assert!(!detect_loop_cancel("Fix the build failure."));
        let state = on_apply_completed(None, &apply_summary());
        let cancelled = cancel_loop(state, "User stopped the workflow.");
        assert_eq!(cancelled.phase, LoopPhase::Cancelled);
        assert!(!cancelled.active);
    }

    #[test]
    fn no_infinite_loop_flags() {
        let state = CodingVerificationLoop::new("x");
        assert!(!state.auto_approved);
        assert!(!state.silent_retry);
        assert_eq!(state.max_cycles, MAX_CYCLES);
    }

    #[test]
    fn timeout_counts_as_cycle_without_auto_fix_for_exhausted() {
        let mut state = on_apply_completed(None, &apply_summary());
        let timeout = VerificationSummary {
            status: Some(VerificationStatus::Failed),
            evidence_kind: Some(VerificationResultKind::Timeout),
            ..Default::default()
        };
        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::Timeout),
            ..Default::default()
        };
        state = on_verification_result(state, &timeout, Some(&evidence));
        assert_eq!(state.cycles_completed, 1);
        assert_eq!(state.history[0].outcome, CycleOutcome::Timeout);
    }
}
