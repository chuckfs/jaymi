//! Agent State Machine (Sprint C5.3).
//!
//! Planner-owned lifecycle for an active [`CodingGoal`]. This is the first
//! explicit agent lifecycle — **bounded and non-autonomous**.
//!
//! Distinctions:
//! - [`AgentLifecycleState`] — authoritative goal-level agent lifecycle
//! - [`crate::coding_goal::CodingGoalStatus`] — coarse goal bookkeeping (C5.1)
//! - [`crate::goal_decomposition::GoalTaskStatus`] — per-task status (C5.2)
//!
//! Constitutional constraints:
//! - Planner owns all transitions; LLM / UI cannot invent or force them
//! - Every transition has an explicit [`AgentLifecycleEvent`] cause
//! - Invalid transitions are rejected
//! - Completed requires completion evidence
//! - AwaitingApproval / Verifying / Correcting do not silently mutate
//! - Executing requires an already-approved operation reference
//! - Correcting prepares a new reviewed C3 plan path — no auto-mutation
//! - No automatic failure → mutation; no unbounded transition loops
//! - C3/C4 keep ownership of mutation / verification execution
//! - Not multi-task autonomous orchestration

use crate::coding_goal::{CodingGoal, CodingGoalStatus};

/// Hard cap on recorded transitions for one goal lifecycle (anti-runaway).
pub const MAX_AGENT_TRANSITIONS: usize = 64;

/// Authoritative agent lifecycle for one Coding Goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycleState {
    Idle,
    Understanding,
    Investigating,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Correcting,
    Blocked,
    Paused,
    Cancelled,
    Completed,
    Failed,
}

impl AgentLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Understanding => "understanding",
            Self::Investigating => "investigating",
            Self::Planning => "planning",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
            Self::Correcting => "correcting",
            Self::Blocked => "blocked",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Understanding => "Understanding",
            Self::Investigating => "Investigating",
            Self::Planning => "Planning",
            Self::AwaitingApproval => "Awaiting approval",
            Self::Executing => "Executing",
            Self::Verifying => "Verifying",
            Self::Correcting => "Correcting",
            Self::Blocked => "Blocked",
            Self::Paused => "Paused",
            Self::Cancelled => "Cancelled",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }

    /// Mutation tools must not run while in these states.
    pub fn allows_mutation_execution(self) -> bool {
        matches!(self, Self::Executing)
    }

    /// Verification must not mutate the project.
    pub fn allows_silent_mutation(self) -> bool {
        false
    }
}

impl Default for AgentLifecycleState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Source of completion evidence (never invented).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionEvidenceSource {
    /// C4 verification reported pass.
    VerificationPassed,
    /// C3 apply ExecutionSummary with successful apply.
    ApprovedExecutionSummary,
    /// Explicit user affirmation **and** attached verification/apply refs.
    UserAffirmedWithEvidence,
}

impl CompletionEvidenceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerificationPassed => "verification_passed",
            Self::ApprovedExecutionSummary => "approved_execution_summary",
            Self::UserAffirmedWithEvidence => "user_affirmed_with_evidence",
        }
    }
}

/// Evidence required before [`AgentLifecycleState::Completed`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompletionEvidence {
    pub summary: String,
    pub source: CompletionEvidenceSource,
    /// True when verification (C4) actually passed.
    pub verification_passed: bool,
    /// Plan / verification / summary ids — must be non-empty.
    pub references: Vec<String>,
}

impl CompletionEvidence {
    pub fn is_sufficient(&self) -> bool {
        if self.references.is_empty() || self.summary.trim().is_empty() {
            return false;
        }
        match self.source {
            CompletionEvidenceSource::VerificationPassed => self.verification_passed,
            CompletionEvidenceSource::ApprovedExecutionSummary => true,
            CompletionEvidenceSource::UserAffirmedWithEvidence => {
                self.verification_passed || !self.references.is_empty()
            }
        }
    }
}

/// Explicit cause for a Planner-owned transition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycleEvent {
    /// Idle → Understanding (goal activated / begin work).
    BeginUnderstanding { reason: String },
    /// → Investigating (C2 path opening — state only here).
    BeginInvestigation { reason: String },
    /// → Planning (C3 plan authoring — state only).
    BeginPlanning { reason: String },
    /// → AwaitingApproval (Review Before Action).
    SubmitForApproval { plan_id: String },
    /// Approval granted; does **not** auto-execute.
    RecordApproval { plan_id: String },
    /// → Executing only with an already-approved plan/ops reference.
    BeginExecution { approved_plan_id: String },
    /// → Verifying (C4) — observational / tool-gated separately.
    BeginVerification { reason: String },
    /// Verification succeeded (evidence for later Complete).
    VerificationPassed { evidence_ref: String },
    /// Verification failed → Correcting (not mutation).
    VerificationFailed { evidence_ref: String },
    /// → Correcting: prepare a **new** reviewed C3 plan (proposal path).
    BeginCorrection { reason: String },
    /// Execution tool/apply failure → Failed (not auto-correct mutate).
    ExecutionFailed { reason: String },
    /// → Blocked (distinguishable from Failed).
    Block { reason: String },
    /// Clear block and return toward prior non-block work state.
    Unblock { reason: String },
    /// → Paused (distinguishable from Cancelled).
    Pause { reason: String },
    /// Paused → prior state.
    Resume { reason: String },
    /// → Cancelled (explicit).
    Cancel { reason: String },
    /// → Completed (requires sufficient evidence).
    Complete { evidence: CompletionEvidence },
    /// → Failed (distinguishable from Blocked).
    Fail { reason: String },
}

impl AgentLifecycleEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::BeginUnderstanding { .. } => "begin_understanding",
            Self::BeginInvestigation { .. } => "begin_investigation",
            Self::BeginPlanning { .. } => "begin_planning",
            Self::SubmitForApproval { .. } => "submit_for_approval",
            Self::RecordApproval { .. } => "record_approval",
            Self::BeginExecution { .. } => "begin_execution",
            Self::BeginVerification { .. } => "begin_verification",
            Self::VerificationPassed { .. } => "verification_passed",
            Self::VerificationFailed { .. } => "verification_failed",
            Self::BeginCorrection { .. } => "begin_correction",
            Self::ExecutionFailed { .. } => "execution_failed",
            Self::Block { .. } => "block",
            Self::Unblock { .. } => "unblock",
            Self::Pause { .. } => "pause",
            Self::Resume { .. } => "resume",
            Self::Cancel { .. } => "cancel",
            Self::Complete { .. } => "complete",
            Self::Fail { .. } => "fail",
        }
    }
}

/// One recorded transition (diagnostics / anti-loop).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentTransitionRecord {
    pub from: AgentLifecycleState,
    pub to: AgentLifecycleState,
    pub event: String,
    pub reason: String,
    pub sequence: u32,
}

/// Planner-owned agent lifecycle surface attached to a Coding Goal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentLifecycle {
    pub state: AgentLifecycleState,
    /// State restored by Resume (not used for Cancel).
    pub paused_from: Option<AgentLifecycleState>,
    /// State before Block (Unblock returns here when safe).
    pub blocked_from: Option<AgentLifecycleState>,
    /// Last approved plan id eligible for BeginExecution.
    pub approved_plan_id: Option<String>,
    /// True after RecordApproval for that plan (Executing gate).
    pub approval_recorded: bool,
    pub completion_evidence: Option<CompletionEvidence>,
    pub history: Vec<AgentTransitionRecord>,
    pub transition_count: u32,
    /// Correcting must lead to a new reviewed plan — track intent only.
    pub correction_requires_new_c3_plan: bool,
    pub last_error: Option<String>,
    pub notes: Vec<String>,
}

impl Default for AgentLifecycle {
    fn default() -> Self {
        Self {
            state: AgentLifecycleState::Idle,
            paused_from: None,
            blocked_from: None,
            approved_plan_id: None,
            approval_recorded: false,
            completion_evidence: None,
            history: Vec::new(),
            transition_count: 0,
            correction_requires_new_c3_plan: false,
            last_error: None,
            notes: vec![
                "Agent lifecycle is Planner-owned — LLM/UI cannot invent transitions.".into(),
            ],
        }
    }
}

impl AgentLifecycle {
    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "agent_lifecycle state={} paused_from={:?} blocked_from={:?} transitions={}/{} approval={} correcting_c3={} allows_mutation={}",
            self.state.as_str(),
            self.paused_from.map(|s| s.as_str()),
            self.blocked_from.map(|s| s.as_str()),
            self.transition_count,
            MAX_AGENT_TRANSITIONS,
            self.approval_recorded,
            self.correction_requires_new_c3_plan,
            self.state.allows_mutation_execution(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    Invalid {
        from: AgentLifecycleState,
        event: String,
        detail: String,
    },
    MissingCompletionEvidence,
    InsufficientCompletionEvidence,
    MissingApprovedPlan,
    TransitionLimitReached,
    Terminal {
        state: AgentLifecycleState,
    },
}

impl TransitionError {
    pub fn to_string_message(&self) -> String {
        match self {
            Self::Invalid { from, event, detail } => {
                format!("Invalid transition from {} via {event}: {detail}", from.label())
            }
            Self::MissingCompletionEvidence => {
                "Completed requires completion evidence.".into()
            }
            Self::InsufficientCompletionEvidence => {
                "Completion evidence is insufficient (need verification/apply refs)."
                    .into()
            }
            Self::MissingApprovedPlan => {
                "Executing requires an already-approved plan id.".into()
            }
            Self::TransitionLimitReached => {
                format!(
                    "Agent transition limit ({MAX_AGENT_TRANSITIONS}) reached — refusing further transitions."
                )
            }
            Self::Terminal { state } => {
                format!("Agent lifecycle is terminal ({}).", state.label())
            }
        }
    }
}

fn event_reason(event: &AgentLifecycleEvent) -> String {
    match event {
        AgentLifecycleEvent::BeginUnderstanding { reason }
        | AgentLifecycleEvent::BeginInvestigation { reason }
        | AgentLifecycleEvent::BeginPlanning { reason }
        | AgentLifecycleEvent::BeginVerification { reason }
        | AgentLifecycleEvent::BeginCorrection { reason }
        | AgentLifecycleEvent::ExecutionFailed { reason }
        | AgentLifecycleEvent::Block { reason }
        | AgentLifecycleEvent::Unblock { reason }
        | AgentLifecycleEvent::Pause { reason }
        | AgentLifecycleEvent::Resume { reason }
        | AgentLifecycleEvent::Cancel { reason }
        | AgentLifecycleEvent::Fail { reason } => reason.clone(),
        AgentLifecycleEvent::SubmitForApproval { plan_id }
        | AgentLifecycleEvent::RecordApproval { plan_id } => {
            format!("plan_id={plan_id}")
        }
        AgentLifecycleEvent::BeginExecution { approved_plan_id } => {
            format!("approved_plan_id={approved_plan_id}")
        }
        AgentLifecycleEvent::VerificationPassed { evidence_ref }
        | AgentLifecycleEvent::VerificationFailed { evidence_ref } => {
            format!("evidence_ref={evidence_ref}")
        }
        AgentLifecycleEvent::Complete { evidence } => evidence.summary.clone(),
    }
}

/// Compute next state + side effects without mutating (pure validation).
pub fn next_state(
    life: &AgentLifecycle,
    event: &AgentLifecycleEvent,
) -> Result<AgentLifecycleState, TransitionError> {
    if life.transition_count as usize >= MAX_AGENT_TRANSITIONS {
        return Err(TransitionError::TransitionLimitReached);
    }

    let from = life.state;
    // Cancelled / Completed are hard terminals. Failed allows BeginCorrection only
    // (new reviewed C3 plan — never auto-mutation).
    if matches!(
        from,
        AgentLifecycleState::Cancelled | AgentLifecycleState::Completed
    ) {
        return Err(TransitionError::Terminal { state: from });
    }
    if from == AgentLifecycleState::Failed
        && !matches!(event, AgentLifecycleEvent::BeginCorrection { .. })
        && !matches!(event, AgentLifecycleEvent::Cancel { .. })
    {
        return Err(TransitionError::Terminal { state: from });
    }

    // Cancel is always explicit and allowed from any non-hard-terminal state.
    if matches!(event, AgentLifecycleEvent::Cancel { .. }) {
        return Ok(AgentLifecycleState::Cancelled);
    }

    // Pause from any non-terminal except Paused itself.
    if matches!(event, AgentLifecycleEvent::Pause { .. }) {
        if from == AgentLifecycleState::Paused {
            return Err(TransitionError::Invalid {
                from,
                event: event.name().into(),
                detail: "Already paused.".into(),
            });
        }
        return Ok(AgentLifecycleState::Paused);
    }

    // Block from any non-terminal except Blocked/Paused handling.
    if matches!(event, AgentLifecycleEvent::Block { .. }) {
        if from == AgentLifecycleState::Blocked {
            return Err(TransitionError::Invalid {
                from,
                event: event.name().into(),
                detail: "Already blocked.".into(),
            });
        }
        return Ok(AgentLifecycleState::Blocked);
    }

    match (from, event) {
        (AgentLifecycleState::Idle, AgentLifecycleEvent::BeginUnderstanding { .. }) => {
            Ok(AgentLifecycleState::Understanding)
        }
        (AgentLifecycleState::Idle, AgentLifecycleEvent::BeginPlanning { .. }) => {
            Ok(AgentLifecycleState::Planning)
        }
        (AgentLifecycleState::Understanding, AgentLifecycleEvent::BeginInvestigation { .. }) => {
            Ok(AgentLifecycleState::Investigating)
        }
        // Allow skipping straight to investigation from Idle for C2-first flows.
        (AgentLifecycleState::Idle, AgentLifecycleEvent::BeginInvestigation { .. }) => {
            Ok(AgentLifecycleState::Investigating)
        }
        (AgentLifecycleState::Investigating, AgentLifecycleEvent::BeginPlanning { .. }) => {
            Ok(AgentLifecycleState::Planning)
        }
        (AgentLifecycleState::Understanding, AgentLifecycleEvent::BeginPlanning { .. }) => {
            Ok(AgentLifecycleState::Planning)
        }
        (AgentLifecycleState::Planning, AgentLifecycleEvent::SubmitForApproval { .. }) => {
            Ok(AgentLifecycleState::AwaitingApproval)
        }
        (AgentLifecycleState::Correcting, AgentLifecycleEvent::BeginPlanning { .. }) => {
            Ok(AgentLifecycleState::Planning)
        }
        (AgentLifecycleState::Correcting, AgentLifecycleEvent::SubmitForApproval { .. }) => {
            Ok(AgentLifecycleState::AwaitingApproval)
        }
        (
            AgentLifecycleState::AwaitingApproval,
            AgentLifecycleEvent::RecordApproval { .. },
        ) => Ok(AgentLifecycleState::AwaitingApproval), // stay until BeginExecution
        (
            AgentLifecycleState::AwaitingApproval,
            AgentLifecycleEvent::BeginExecution { approved_plan_id },
        ) => {
            if approved_plan_id.trim().is_empty() {
                return Err(TransitionError::MissingApprovedPlan);
            }
            if !life.approval_recorded
                || life.approved_plan_id.as_deref() != Some(approved_plan_id.as_str())
            {
                return Err(TransitionError::MissingApprovedPlan);
            }
            Ok(AgentLifecycleState::Executing)
        }
        // Reject mutation while awaiting approval via BeginExecution without approval.
        (AgentLifecycleState::AwaitingApproval, AgentLifecycleEvent::BeginVerification { .. }) => {
            Err(TransitionError::Invalid {
                from,
                event: event.name().into(),
                detail: "Cannot verify before approved execution.".into(),
            })
        }
        (AgentLifecycleState::Executing, AgentLifecycleEvent::BeginVerification { .. }) => {
            Ok(AgentLifecycleState::Verifying)
        }
        (AgentLifecycleState::Executing, AgentLifecycleEvent::ExecutionFailed { .. }) => {
            Ok(AgentLifecycleState::Failed)
        }
        (AgentLifecycleState::Verifying, AgentLifecycleEvent::VerificationPassed { .. }) => {
            Ok(AgentLifecycleState::Verifying) // stay; Complete is separate
        }
        (AgentLifecycleState::Verifying, AgentLifecycleEvent::VerificationFailed { .. }) => {
            Ok(AgentLifecycleState::Correcting)
        }
        (AgentLifecycleState::Verifying, AgentLifecycleEvent::BeginCorrection { .. }) => {
            Ok(AgentLifecycleState::Correcting)
        }
        (AgentLifecycleState::Failed, AgentLifecycleEvent::BeginCorrection { .. }) => {
            // Allowed: correction prepares a new reviewed plan — not auto-mutation.
            Ok(AgentLifecycleState::Correcting)
        }
        // Explicit: no direct Failed/Verifying → Executing (would be auto-mutation).
        (AgentLifecycleState::Failed, AgentLifecycleEvent::BeginExecution { .. })
        | (AgentLifecycleState::Verifying, AgentLifecycleEvent::BeginExecution { .. })
        | (AgentLifecycleState::Correcting, AgentLifecycleEvent::BeginExecution { .. }) => {
            Err(TransitionError::Invalid {
                from,
                event: event.name().into(),
                detail: "No automatic failure→mutation; require Planning→AwaitingApproval→approved BeginExecution."
                    .into(),
            })
        }
        (AgentLifecycleState::Paused, AgentLifecycleEvent::Resume { .. }) => {
            Ok(life.paused_from.unwrap_or(AgentLifecycleState::Idle))
        }
        (AgentLifecycleState::Blocked, AgentLifecycleEvent::Unblock { .. }) => {
            Ok(life.blocked_from.unwrap_or(AgentLifecycleState::Idle))
        }
        (_, AgentLifecycleEvent::Complete { evidence }) => {
            if !matches!(
                from,
                AgentLifecycleState::Verifying
                    | AgentLifecycleState::Executing
                    | AgentLifecycleState::Planning
                    | AgentLifecycleState::AwaitingApproval
                    | AgentLifecycleState::Idle
                    | AgentLifecycleState::Understanding
                    | AgentLifecycleState::Investigating
            ) {
                // Allow complete from work states after evidence; not from Paused without resume.
                if from == AgentLifecycleState::Paused || from == AgentLifecycleState::Blocked {
                    return Err(TransitionError::Invalid {
                        from,
                        event: event.name().into(),
                        detail: "Resume/Unblock before Complete.".into(),
                    });
                }
            }
            if evidence.summary.trim().is_empty() && evidence.references.is_empty() {
                return Err(TransitionError::MissingCompletionEvidence);
            }
            if !evidence.is_sufficient() {
                return Err(TransitionError::InsufficientCompletionEvidence);
            }
            // Prefer completing after verification when possible.
            if matches!(
                from,
                AgentLifecycleState::Verifying
                    | AgentLifecycleState::Executing
                    | AgentLifecycleState::Planning
                    | AgentLifecycleState::AwaitingApproval
                    | AgentLifecycleState::Investigating
                    | AgentLifecycleState::Understanding
                    | AgentLifecycleState::Idle
                    | AgentLifecycleState::Correcting
            ) {
                Ok(AgentLifecycleState::Completed)
            } else {
                Err(TransitionError::Invalid {
                    from,
                    event: event.name().into(),
                    detail: "Cannot complete from this state.".into(),
                })
            }
        }
        (_, AgentLifecycleEvent::Fail { .. }) => Ok(AgentLifecycleState::Failed),
        _ => Err(TransitionError::Invalid {
            from,
            event: event.name().into(),
            detail: "Transition not permitted.".into(),
        }),
    }
}

/// Apply a Planner-owned transition to the agent lifecycle.
pub fn apply_event(
    mut life: AgentLifecycle,
    event: AgentLifecycleEvent,
) -> Result<AgentLifecycle, TransitionError> {
    let from = life.state;
    let to = next_state(&life, &event)?;
    let reason = event_reason(&event);

    // Side effects
    match &event {
        AgentLifecycleEvent::SubmitForApproval { plan_id } => {
            life.approved_plan_id = Some(plan_id.clone());
            life.approval_recorded = false;
            life.correction_requires_new_c3_plan = false;
        }
        AgentLifecycleEvent::RecordApproval { plan_id } => {
            life.approved_plan_id = Some(plan_id.clone());
            life.approval_recorded = true;
        }
        AgentLifecycleEvent::BeginExecution { approved_plan_id } => {
            life.approved_plan_id = Some(approved_plan_id.clone());
        }
        AgentLifecycleEvent::BeginCorrection { .. }
        | AgentLifecycleEvent::VerificationFailed { .. } => {
            life.correction_requires_new_c3_plan = true;
            life.approval_recorded = false;
            life.notes.push(
                "Correcting: a new C3 Coding Plan must go through Review Before Action — no auto-mutation."
                    .into(),
            );
        }
        AgentLifecycleEvent::Pause { .. } => {
            life.paused_from = Some(from);
        }
        AgentLifecycleEvent::Resume { .. } => {
            life.paused_from = None;
        }
        AgentLifecycleEvent::Block { reason } => {
            life.blocked_from = Some(from);
            life.last_error = Some(reason.clone());
        }
        AgentLifecycleEvent::Unblock { .. } => {
            life.blocked_from = None;
            life.last_error = None;
        }
        AgentLifecycleEvent::Complete { evidence } => {
            life.completion_evidence = Some(evidence.clone());
            life.correction_requires_new_c3_plan = false;
        }
        AgentLifecycleEvent::ExecutionFailed { reason }
        | AgentLifecycleEvent::Fail { reason } => {
            life.last_error = Some(reason.clone());
        }
        AgentLifecycleEvent::Cancel { .. } => {
            life.paused_from = None;
            life.approval_recorded = false;
        }
        _ => {}
    }

    life.transition_count = life.transition_count.saturating_add(1);
    life.history.push(AgentTransitionRecord {
        from,
        to,
        event: event.name().to_string(),
        reason,
        sequence: life.transition_count,
    });
    life.state = to;

    // Guard: AwaitingApproval never allows mutation execution.
    debug_assert!(
        !(life.state == AgentLifecycleState::AwaitingApproval && life.state.allows_mutation_execution())
    );
    // Guard: Verifying never allows silent mutation.
    debug_assert!(!life.state.allows_silent_mutation());

    Ok(life)
}

/// Apply event onto a Coding Goal (Planner-owned). Syncs coarse C5.1 status when terminal.
pub fn transition_goal(
    mut goal: CodingGoal,
    event: AgentLifecycleEvent,
) -> Result<CodingGoal, TransitionError> {
    let life = goal.agent_lifecycle.clone();
    let event_for_policy = event.clone();
    let next = apply_event(life, event)?;
    match next.state {
        AgentLifecycleState::Cancelled => {
            goal.status = CodingGoalStatus::Cancelled;
        }
        AgentLifecycleState::Completed => {
            goal.status = CodingGoalStatus::Completed;
            goal.remaining_work.clear();
        }
        AgentLifecycleState::Blocked => {
            goal.status = CodingGoalStatus::Blocked;
            if let Some(err) = &next.last_error {
                if !goal.blockers.iter().any(|b| b == err) {
                    goal.blockers.push(err.clone());
                }
            }
        }
        AgentLifecycleState::Failed => {
            // Failed ≠ Blocked: keep goal Active at C5.1 layer unless already terminal.
            if !matches!(
                goal.status,
                CodingGoalStatus::Cancelled | CodingGoalStatus::Completed
            ) {
                goal.status = CodingGoalStatus::Active;
            }
        }
        AgentLifecycleState::Paused
        | AgentLifecycleState::Idle
        | AgentLifecycleState::Understanding
        | AgentLifecycleState::Investigating
        | AgentLifecycleState::Planning
        | AgentLifecycleState::AwaitingApproval
        | AgentLifecycleState::Executing
        | AgentLifecycleState::Verifying
        | AgentLifecycleState::Correcting => {
            if goal.status == CodingGoalStatus::Blocked
                && next.state != AgentLifecycleState::Blocked
            {
                goal.status = CodingGoalStatus::Active;
            }
        }
    }
    goal.agent_lifecycle = next;
    // C5.6 — bind / invalidate approvals (reuse Review; no AgentApproval system).
    goal = crate::approval_autonomy_policy::sync_after_lifecycle_event(goal, &event_for_policy);
    goal.mutated = false;
    goal.tools_executed = false;
    goal.updated_at = now_stamp();
    Ok(goal)
}

fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Soft-path / report for conversational lifecycle control (pause/resume/etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentLifecycleRequestKind {
    Pause,
    Resume,
    /// Report current agent state.
    Status,
    /// Begin understanding / activate work.
    BeginUnderstanding,
    BeginInvestigation,
    BeginPlanning,
    BeginCorrection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLifecycleAssessment {
    pub kind: AgentLifecycleRequestKind,
    pub matched_rules: Vec<&'static str>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentLifecycleReport {
    pub ok: bool,
    pub from: Option<AgentLifecycleState>,
    pub to: Option<AgentLifecycleState>,
    pub event: Option<String>,
    pub error: Option<String>,
    pub lifecycle: Option<AgentLifecycle>,
    pub notes: Vec<String>,
    pub mutated: bool,
    pub tools_executed: bool,
    pub llm_directed: bool,
    pub ui_arbitrary: bool,
}

impl Default for AgentLifecycleReport {
    fn default() -> Self {
        Self {
            ok: false,
            from: None,
            to: None,
            event: None,
            error: None,
            lifecycle: None,
            notes: Vec::new(),
            mutated: false,
            tools_executed: false,
            llm_directed: false,
            ui_arbitrary: false,
        }
    }
}

impl AgentLifecycleReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Agent Lifecycle\n");
        out.push_str(
            "\n_Planner-owned state machine — not driven by the LLM or arbitrary UI mutations._\n",
        );
        if let Some(err) = &self.error {
            out.push_str(&format!("\n### Rejected\n- {err}\n"));
        } else if self.ok {
            out.push_str("\n### Transition\n");
            if let (Some(from), Some(to)) = (self.from, self.to) {
                out.push_str(&format!("- {} → **{}**\n", from.label(), to.label()));
            }
            if let Some(ev) = &self.event {
                out.push_str(&format!("- Event: `{ev}`\n"));
            }
        }
        if let Some(life) = &self.lifecycle {
            out.push_str(&format!("\n### Current state\n- **{}**\n", life.state.label()));
            out.push_str(&format!(
                "- Allows mutation execution: {}\n",
                life.state.allows_mutation_execution()
            ));
            out.push_str(&format!(
                "- Transitions: {}/{}\n",
                life.transition_count, MAX_AGENT_TRANSITIONS
            ));
            if life.correction_requires_new_c3_plan {
                out.push_str(
                    "- Correcting requires a **new reviewed C3 Coding Plan** (no auto-mutation).\n",
                );
            }
            if let Some(p) = life.paused_from {
                out.push_str(&format!("- Paused from: {}\n", p.label()));
            }
            if let Some(b) = life.blocked_from {
                out.push_str(&format!("- Blocked from: {}\n", b.label()));
            }
        }
        out.push_str(&format!(
            "\n### Safety\n- mutated={} · tools_executed={} · llm_directed={} · ui_arbitrary={}\n",
            self.mutated, self.tools_executed, self.llm_directed, self.ui_arbitrary
        ));
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
            "agent_lifecycle_report ok={} from={:?} to={:?} err={:?} mutated={} llm_directed={} ui_arbitrary={}",
            self.ok,
            self.from.map(|s| s.as_str()),
            self.to.map(|s| s.as_str()),
            self.error.as_deref(),
            self.mutated,
            self.llm_directed,
            self.ui_arbitrary,
        )
    }
}

/// Detect conversational lifecycle controls (not LLM tool payloads).
pub fn detect_agent_lifecycle_request(
    request: &jaymi_core::UserRequest,
) -> Option<AgentLifecycleAssessment> {
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

    // Yield pause/resume to C5.8 Agent Pause/Resume/Cancel.
    if crate::agent_pause_resume_cancel::detect_control_request(request).is_some() {
        return None;
    }

    // Yield to C5.1 goal cancel/complete/block/status and C5.2 decompose.
    if crate::coding_goal::detect_coding_goal_request(request).is_some()
        || crate::goal_decomposition::detect_decomposition_request(request).is_some()
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "pause this goal"
            | "pause the goal"
            | "pause goal"
            | "pause the agent"
            | "pause agent"
    ) || trimmed.starts_with("pause this goal")
        || trimmed.starts_with("pause the goal")
    {
        // Unreachable when C5.8 detect runs first; kept as defensive fallback.
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::Pause,
            matched_rules: vec!["agent_pause"],
            reason: "User paused the agent lifecycle.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "resume this goal"
            | "resume the goal"
            | "resume goal"
            | "resume the agent"
            | "resume agent"
            | "unpause this goal"
            | "unpause the goal"
    ) || trimmed.starts_with("resume this goal")
        || trimmed.starts_with("resume the goal")
    {
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::Resume,
            matched_rules: vec!["agent_resume"],
            reason: "User resumed the agent lifecycle.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "agent state"
            | "agent status"
            | "lifecycle state"
            | "what's the agent state"
            | "what is the agent state"
            | "show agent state"
    ) || trimmed.starts_with("what's the agent")
        || trimmed.starts_with("what is the agent")
    {
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::Status,
            matched_rules: vec!["agent_status"],
            reason: "Status query.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "begin understanding"
            | "start understanding"
            | "start working on this goal"
            | "begin working on this goal"
    ) {
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::BeginUnderstanding,
            matched_rules: vec!["agent_begin_understanding"],
            reason: "User began understanding.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "begin investigation"
            | "start investigation"
            | "begin investigating"
            | "start investigating"
    ) {
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::BeginInvestigation,
            matched_rules: vec!["agent_begin_investigation"],
            reason: "User began investigation.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "begin planning" | "start planning" | "begin the plan"
    ) {
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::BeginPlanning,
            matched_rules: vec!["agent_begin_planning"],
            reason: "User began planning.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "begin correction"
            | "start correction"
            | "begin correcting"
            | "prepare a correction plan"
    ) {
        return Some(AgentLifecycleAssessment {
            kind: AgentLifecycleRequestKind::BeginCorrection,
            matched_rules: vec!["agent_begin_correction"],
            reason: "User requested correction planning (new reviewed C3 plan).".into(),
        });
    }

    None
}

pub fn event_for_assessment(assessment: &AgentLifecycleAssessment) -> Option<AgentLifecycleEvent> {
    match assessment.kind {
        AgentLifecycleRequestKind::Pause => Some(AgentLifecycleEvent::Pause {
            reason: assessment.reason.clone(),
        }),
        AgentLifecycleRequestKind::Resume => Some(AgentLifecycleEvent::Resume {
            reason: assessment.reason.clone(),
        }),
        AgentLifecycleRequestKind::BeginUnderstanding => {
            Some(AgentLifecycleEvent::BeginUnderstanding {
                reason: assessment.reason.clone(),
            })
        }
        AgentLifecycleRequestKind::BeginInvestigation => {
            Some(AgentLifecycleEvent::BeginInvestigation {
                reason: assessment.reason.clone(),
            })
        }
        AgentLifecycleRequestKind::BeginPlanning => Some(AgentLifecycleEvent::BeginPlanning {
            reason: assessment.reason.clone(),
        }),
        AgentLifecycleRequestKind::BeginCorrection => Some(AgentLifecycleEvent::BeginCorrection {
            reason: assessment.reason.clone(),
        }),
        AgentLifecycleRequestKind::Status => None,
    }
}

/// Apply a conversational lifecycle assessment to an optional active goal.
pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &AgentLifecycleAssessment,
) -> (AgentLifecycleReport, Option<CodingGoal>) {
    let mut report = AgentLifecycleReport {
        notes: vec![
            "Transitions are Planner-owned with explicit events.".into(),
            "LLM cannot directly change agent state; UI cannot arbitrarily mutate it.".into(),
        ],
        ..Default::default()
    };

    let Some(goal) = goal else {
        report.error = Some("No active Coding Goal.".into());
        return (report, None);
    };

    if assessment.kind == AgentLifecycleRequestKind::Status {
        report.ok = true;
        report.from = Some(goal.agent_lifecycle.state);
        report.to = Some(goal.agent_lifecycle.state);
        report.lifecycle = Some(goal.agent_lifecycle.clone());
        return (report, Some(goal.clone()));
    }

    let Some(event) = event_for_assessment(assessment) else {
        report.error = Some("No lifecycle event for this request.".into());
        return (report, None);
    };

    report.from = Some(goal.agent_lifecycle.state);
    report.event = Some(event.name().into());
    match transition_goal(goal.clone(), event) {
        Ok(updated) => {
            report.ok = true;
            report.to = Some(updated.agent_lifecycle.state);
            report.lifecycle = Some(updated.agent_lifecycle.clone());
            (report, Some(updated))
        }
        Err(err) => {
            report.error = Some(err.to_string_message());
            report.lifecycle = Some(goal.agent_lifecycle.clone());
            (report, None)
        }
    }
}

/// All valid (from, event_kind) pairs exercised by unit tests.
pub fn valid_happy_path_events(plan_id: &str) -> Vec<AgentLifecycleEvent> {
    vec![
        AgentLifecycleEvent::BeginUnderstanding {
            reason: "start".into(),
        },
        AgentLifecycleEvent::BeginInvestigation {
            reason: "investigate".into(),
        },
        AgentLifecycleEvent::BeginPlanning {
            reason: "plan".into(),
        },
        AgentLifecycleEvent::SubmitForApproval {
            plan_id: plan_id.into(),
        },
        AgentLifecycleEvent::RecordApproval {
            plan_id: plan_id.into(),
        },
        AgentLifecycleEvent::BeginExecution {
            approved_plan_id: plan_id.into(),
        },
        AgentLifecycleEvent::BeginVerification {
            reason: "verify".into(),
        },
        AgentLifecycleEvent::VerificationPassed {
            evidence_ref: "ver-1".into(),
        },
        AgentLifecycleEvent::Complete {
            evidence: CompletionEvidence {
                summary: "Verification passed for the goal.".into(),
                source: CompletionEvidenceSource::VerificationPassed,
                verification_passed: true,
                references: vec!["ver-1".into()],
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use jaymi_context::ContextBundle;
    use jaymi_core::UserRequest;

    fn fresh_goal() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new("Build a login system.")).unwrap();
        create_goal(&a, &ContextBundle::default(), None)
    }

    fn apply(goal: CodingGoal, event: AgentLifecycleEvent) -> CodingGoal {
        transition_goal(goal, event).expect("transition")
    }

    #[test]
    fn every_happy_path_transition() {
        let mut goal = fresh_goal();
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Idle);
        for event in valid_happy_path_events("plan-1") {
            let from = goal.agent_lifecycle.state;
            goal = apply(goal, event);
            assert_ne!(
                goal.agent_lifecycle.history.last().map(|h| h.from),
                None
            );
            assert_eq!(
                goal.agent_lifecycle.history.last().unwrap().from,
                from
            );
        }
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Completed);
        assert_eq!(goal.status, CodingGoalStatus::Completed);
        assert!(goal.agent_lifecycle.completion_evidence.is_some());
        assert!(!goal.mutated);
    }

    #[test]
    fn invalid_transitions_rejected() {
        let goal = fresh_goal();
        let err = transition_goal(
            goal.clone(),
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "x".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, TransitionError::Invalid { .. }));

        let err = transition_goal(
            goal,
            AgentLifecycleEvent::Complete {
                evidence: CompletionEvidence {
                    summary: String::new(),
                    source: CompletionEvidenceSource::VerificationPassed,
                    verification_passed: false,
                    references: vec![],
                },
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TransitionError::MissingCompletionEvidence
                | TransitionError::InsufficientCompletionEvidence
        ));
    }

    #[test]
    fn cancel_during_investigation() {
        let mut goal = fresh_goal();
        goal = apply(
            goal,
            AgentLifecycleEvent::BeginInvestigation {
                reason: "inv".into(),
            },
        );
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Investigating);
        goal = apply(
            goal,
            AgentLifecycleEvent::Cancel {
                reason: "stop".into(),
            },
        );
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Cancelled);
        assert_eq!(goal.status, CodingGoalStatus::Cancelled);
        assert!(goal.agent_lifecycle.paused_from.is_none());
    }

    #[test]
    fn cancel_during_approval() {
        let mut goal = fresh_goal();
        for e in [
            AgentLifecycleEvent::BeginPlanning {
                reason: "p".into(),
            },
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "p1".into(),
            },
        ] {
            goal = apply(goal, e);
        }
        assert_eq!(
            goal.agent_lifecycle.state,
            AgentLifecycleState::AwaitingApproval
        );
        assert!(!goal.agent_lifecycle.state.allows_mutation_execution());
        goal = apply(
            goal,
            AgentLifecycleEvent::Cancel {
                reason: "cancel approval".into(),
            },
        );
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Cancelled);
    }

    #[test]
    fn pause_and_resume() {
        let mut goal = fresh_goal();
        goal = apply(
            goal,
            AgentLifecycleEvent::BeginUnderstanding {
                reason: "u".into(),
            },
        );
        goal = apply(
            goal,
            AgentLifecycleEvent::Pause {
                reason: "pause".into(),
            },
        );
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Paused);
        assert_eq!(
            goal.agent_lifecycle.paused_from,
            Some(AgentLifecycleState::Understanding)
        );
        goal = apply(
            goal,
            AgentLifecycleEvent::Resume {
                reason: "resume".into(),
            },
        );
        assert_eq!(
            goal.agent_lifecycle.state,
            AgentLifecycleState::Understanding
        );
        assert!(goal.agent_lifecycle.paused_from.is_none());
    }

    #[test]
    fn execution_failure_and_verification_failure_correction() {
        let mut goal = fresh_goal();
        let plan = "plan-x";
        for e in [
            AgentLifecycleEvent::BeginPlanning {
                reason: "p".into(),
            },
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: plan.into(),
            },
            AgentLifecycleEvent::RecordApproval {
                plan_id: plan.into(),
            },
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: plan.into(),
            },
        ] {
            goal = apply(goal, e);
        }
        assert!(goal.agent_lifecycle.state.allows_mutation_execution());

        let failed = apply(
            goal.clone(),
            AgentLifecycleEvent::ExecutionFailed {
                reason: "apply failed".into(),
            },
        );
        assert_eq!(failed.agent_lifecycle.state, AgentLifecycleState::Failed);
        assert_ne!(failed.status, CodingGoalStatus::Blocked);

        // No direct Failed → Executing
        assert!(transition_goal(
            failed.clone(),
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: plan.into(),
            },
        )
        .is_err());

        let correcting = apply(
            failed,
            AgentLifecycleEvent::BeginCorrection {
                reason: "new plan".into(),
            },
        );
        assert_eq!(
            correcting.agent_lifecycle.state,
            AgentLifecycleState::Correcting
        );
        assert!(correcting.agent_lifecycle.correction_requires_new_c3_plan);

        // Verification failure path
        let mut goal = fresh_goal();
        for e in [
            AgentLifecycleEvent::BeginPlanning {
                reason: "p".into(),
            },
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: plan.into(),
            },
            AgentLifecycleEvent::RecordApproval {
                plan_id: plan.into(),
            },
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: plan.into(),
            },
            AgentLifecycleEvent::BeginVerification {
                reason: "v".into(),
            },
            AgentLifecycleEvent::VerificationFailed {
                evidence_ref: "e1".into(),
            },
        ] {
            goal = apply(goal, e);
        }
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Correcting);
        assert!(!goal.agent_lifecycle.state.allows_mutation_execution());
    }

    #[test]
    fn blocked_distinguishable_from_failed() {
        let mut goal = fresh_goal();
        goal = apply(
            goal,
            AgentLifecycleEvent::BeginInvestigation {
                reason: "i".into(),
            },
        );
        goal = apply(
            goal,
            AgentLifecycleEvent::Block {
                reason: "missing keys".into(),
            },
        );
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Blocked);
        assert_eq!(goal.status, CodingGoalStatus::Blocked);
        assert!(goal.blockers.iter().any(|b| b.contains("keys")));

        goal = apply(
            goal,
            AgentLifecycleEvent::Unblock {
                reason: "keys arrived".into(),
            },
        );
        assert_eq!(
            goal.agent_lifecycle.state,
            AgentLifecycleState::Investigating
        );
        assert_eq!(goal.status, CodingGoalStatus::Active);
    }

    #[test]
    fn awaiting_approval_stops_mutation_and_executing_needs_approval() {
        let mut goal = fresh_goal();
        goal = apply(
            goal,
            AgentLifecycleEvent::BeginPlanning {
                reason: "p".into(),
            },
        );
        goal = apply(
            goal,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "p1".into(),
            },
        );
        assert!(!goal.agent_lifecycle.state.allows_mutation_execution());
        assert!(transition_goal(
            goal.clone(),
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "p1".into(),
            },
        )
        .is_err());
        goal = apply(
            goal,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "p1".into(),
            },
        );
        goal = apply(
            goal,
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "p1".into(),
            },
        );
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Executing);
    }

    #[test]
    fn transition_limit_prevents_runaway() {
        let mut life = AgentLifecycle::default();
        for i in 0..MAX_AGENT_TRANSITIONS {
            let event = if life.state == AgentLifecycleState::Paused {
                AgentLifecycleEvent::Resume {
                    reason: format!("r{i}"),
                }
            } else {
                AgentLifecycleEvent::Pause {
                    reason: format!("p{i}"),
                }
            };
            life = apply_event(life, event).unwrap();
        }
        let err = apply_event(
            life,
            AgentLifecycleEvent::Pause {
                reason: "one more".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, TransitionError::TransitionLimitReached));
    }

    #[test]
    fn task_state_remains_distinguishable() {
        use crate::goal_decomposition::GoalTaskStatus;
        let goal = fresh_goal();
        assert_eq!(goal.agent_lifecycle.state, AgentLifecycleState::Idle);
        assert_eq!(goal.status, CodingGoalStatus::Active);
        // Different enums: goal agent lifecycle vs per-task status.
        assert_eq!(GoalTaskStatus::Pending.as_str(), "pending");
        assert_eq!(AgentLifecycleState::Idle.as_str(), "idle");
        // Same label string is fine — types remain distinct at the API boundary.
        let agent_completed = AgentLifecycleState::Completed;
        let task_completed = GoalTaskStatus::Completed;
        assert!(matches!(agent_completed, AgentLifecycleState::Completed));
        assert!(matches!(task_completed, GoalTaskStatus::Completed));
    }

    #[test]
    fn detect_pause_resume() {
        assert_eq!(
            detect_agent_lifecycle_request(&UserRequest::new("Pause this goal."))
                .unwrap()
                .kind,
            AgentLifecycleRequestKind::Pause
        );
        assert_eq!(
            detect_agent_lifecycle_request(&UserRequest::new("Resume this goal."))
                .unwrap()
                .kind,
            AgentLifecycleRequestKind::Resume
        );
    }
}
