//! Agent Pause, Resume, and Cancellation (Sprint C5.8 + C5.14 integrity).
//!
//! Give users reliable control over active Coding Goals — **Planner-owned**
//! orchestration on top of the C5.3 lifecycle. Does **not** create a parallel
//! cancellation engine or silently undo successful mutations.
//!
//! ```text
//! Pause  → stop new ops; preserve goal/task/plan/verification; resumable
//! Resume → revalidate project fingerprint; refuse stale approved mutation
//! Cancel → stop future work; invalidate approvals; keep history; no rollback
//! ```
//!
//! **C5.14 — Resume Integrity:** pause/resume must capture real ContextBundle
//! fingerprints (via ContextEngine). Unavailable fingerprints are **unknown/stale**,
//! never treated as “unchanged.”
//!
//! Tool / stream interruption reuses existing Tool and StreamingLifecycle
//! cancellation semantics — this module surfaces honesty when an in-flight
//! operation cannot be interrupted immediately.

use crate::agent_state_machine::{
    transition_goal, AgentLifecycleEvent, AgentLifecycleState,
};
use crate::approval_autonomy_policy::{
    assert_execution_authorized, invalidate_on_cancel, invalidate_on_plan_change,
};
use crate::coding_goal::{cancel_goal, CodingGoal, CodingGoalStatus, GoalVerificationState};
use crate::conversation_state::ConversationState;
use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;
use jaymi_reasoning::StreamingLifecycle;

/// Snapshot of ambient project state captured at pause (or cancel) time.
///
/// Built only from a ContextEngine-minted [`ContextBundle`] — never a parallel
/// workspace walker.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ProjectFingerprint {
    pub head_sha: Option<String>,
    pub branch: Option<String>,
    pub dirty_count: usize,
    pub staged_count: usize,
    pub conflict_count: usize,
    pub workspace_root: Option<String>,
    /// Active editor path when known (C5.14 — file-level drift).
    pub current_file: Option<String>,
    /// Whether the active editor buffer was dirty at capture.
    pub current_file_dirty: bool,
    /// Stable signature of sample dirty paths (empty when unavailable).
    pub dirty_paths_sig: String,
}

impl ProjectFingerprint {
    pub fn from_bundle(bundle: &ContextBundle) -> Self {
        let gs = bundle.git_status();
        let cf = bundle.current_file();
        let mut paths = gs.dirty_paths.clone();
        paths.sort();
        let dirty_paths_sig = if paths.is_empty() {
            String::new()
        } else {
            paths.into_iter().take(12).collect::<Vec<_>>().join("\n")
        };
        Self {
            head_sha: gs.head_sha.clone(),
            branch: gs.branch.clone(),
            dirty_count: gs.modified_count,
            staged_count: gs.staged_count,
            conflict_count: gs.conflict_count,
            workspace_root: bundle.active_project().root_directory.clone(),
            current_file: cf.path.clone(),
            current_file_dirty: cf.dirty,
            dirty_paths_sig,
        }
    }

    /// True when the fingerprint carries enough ambient signal to compare.
    ///
    /// An empty / default fingerprint (typical of `ContextEngine::empty_bundle`)
    /// is **unavailable** — never treated as an unchanged workspace.
    pub fn is_available(&self) -> bool {
        self.workspace_root.is_some()
            || self.head_sha.is_some()
            || self.branch.is_some()
            || self.current_file.is_some()
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "fingerprint available={} head={} branch={} dirty={} staged={} conflicts={} root={} file={} file_dirty={}",
            self.is_available(),
            self.head_sha.as_deref().unwrap_or("—"),
            self.branch.as_deref().unwrap_or("—"),
            self.dirty_count,
            self.staged_count,
            self.conflict_count,
            self.workspace_root.as_deref().unwrap_or("—"),
            self.current_file.as_deref().unwrap_or("—"),
            self.current_file_dirty,
        )
    }
}

/// Why resume considers ambient project state stale relative to the pause checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleReason {
    HeadChanged,
    BranchChanged,
    ConflictsAppeared,
    DirtyGrew,
    WorkspaceRootChanged,
    /// Active editor path or dirty flag changed while paused (C5.14).
    CurrentFileChanged,
    /// Real fingerprint missing on pause and/or resume — unknown, not unchanged (C5.14).
    FingerprintUnavailable,
}

impl StaleReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HeadChanged => "head_changed",
            Self::BranchChanged => "branch_changed",
            Self::ConflictsAppeared => "conflicts_appeared",
            Self::DirtyGrew => "dirty_grew",
            Self::WorkspaceRootChanged => "workspace_root_changed",
            Self::CurrentFileChanged => "current_file_changed",
            Self::FingerprintUnavailable => "fingerprint_unavailable",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::HeadChanged => "Git HEAD changed while paused",
            Self::BranchChanged => "Git branch changed while paused",
            Self::ConflictsAppeared => "Merge conflicts appeared while paused",
            Self::DirtyGrew => "Working tree dirty state changed while paused",
            Self::WorkspaceRootChanged => "Workspace root changed while paused",
            Self::CurrentFileChanged => "Active file / dirty buffer changed while paused",
            Self::FingerprintUnavailable => {
                "Project fingerprint unavailable — treated as unknown/stale (not unchanged)"
            }
        }
    }
}

/// Compare pause fingerprint to current bundle.
///
/// Unavailable fingerprints (empty bundle / missing ambient signal) yield
/// [`StaleReason::FingerprintUnavailable`] — **never** “unchanged.”
pub fn detect_stale(
    saved: &ProjectFingerprint,
    current: &ProjectFingerprint,
) -> Vec<StaleReason> {
    let mut out = Vec::new();
    if !saved.is_available() || !current.is_available() {
        out.push(StaleReason::FingerprintUnavailable);
        return out;
    }

    if saved.workspace_root != current.workspace_root {
        out.push(StaleReason::WorkspaceRootChanged);
    }
    if saved.head_sha != current.head_sha {
        out.push(StaleReason::HeadChanged);
    }
    if saved.branch != current.branch {
        out.push(StaleReason::BranchChanged);
    }
    if current.conflict_count != saved.conflict_count {
        out.push(StaleReason::ConflictsAppeared);
    }
    if current.dirty_count != saved.dirty_count
        || saved.dirty_paths_sig != current.dirty_paths_sig
        || current.staged_count != saved.staged_count
    {
        out.push(StaleReason::DirtyGrew);
    }
    if saved.current_file != current.current_file
        || saved.current_file_dirty != current.current_file_dirty
    {
        out.push(StaleReason::CurrentFileChanged);
    }
    out
}

/// Planner-owned pause checkpoint — not a second agent authority.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PauseCheckpoint {
    pub paused_from: AgentLifecycleState,
    pub fingerprint: ProjectFingerprint,
    pub preserved_plan_id: Option<String>,
    pub preserved_approval_recorded: bool,
    pub preserved_task_id: Option<String>,
    pub preserved_verification: GoalVerificationState,
    pub stop_new_operations: bool,
    pub paused_at: String,
    pub notes: Vec<String>,
}

impl Default for PauseCheckpoint {
    fn default() -> Self {
        Self {
            paused_from: AgentLifecycleState::Idle,
            fingerprint: ProjectFingerprint::default(),
            preserved_plan_id: None,
            preserved_approval_recorded: false,
            preserved_task_id: None,
            preserved_verification: GoalVerificationState::NotStarted,
            stop_new_operations: false,
            paused_at: String::new(),
            notes: Vec::new(),
        }
    }
}

/// Durable control surface on the Coding Goal (C5.8).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GoalControlState {
    pub checkpoint: Option<Box<PauseCheckpoint>>,
    /// True after explicit cancel — never continue work.
    pub cancelled: bool,
    pub cancel_count: u32,
    pub last_cancel_reason: Option<String>,
    /// Last in-flight honesty line (observational).
    pub last_in_flight_note: Option<String>,
    /// True when resume invalidated approvals due to stale project / plan.
    pub requires_fresh_approval: bool,
    pub notes: Vec<String>,
}

/// Observed in-flight work (composed from existing stream / conversation state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InFlightKind {
    None,
    GenerationStreaming,
    ConversationBusy,
    ReviewPausedPlan,
    VerificationLikely,
}

impl InFlightKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::GenerationStreaming => "generation_streaming",
            Self::ConversationBusy => "conversation_busy",
            Self::ReviewPausedPlan => "review_paused_plan",
            Self::VerificationLikely => "verification_likely",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InFlightObservation {
    pub kind: InFlightKind,
    /// When false, Pause/Cancel still stop **new** work but must say so honestly.
    pub can_interrupt_immediately: bool,
    pub message: String,
}

impl Default for InFlightObservation {
    fn default() -> Self {
        Self {
            kind: InFlightKind::None,
            can_interrupt_immediately: true,
            message: String::new(),
        }
    }
}

/// Observe in-flight work using **existing** ConversationState / StreamingLifecycle
/// (no parallel cancellation engine).
pub fn observe_in_flight(
    conversation: ConversationState,
    stream: Option<StreamingLifecycle>,
    has_review_paused_plan: bool,
    lifecycle: AgentLifecycleState,
) -> InFlightObservation {
    if has_review_paused_plan {
        return InFlightObservation {
            kind: InFlightKind::ReviewPausedPlan,
            can_interrupt_immediately: true,
            message: "A Review-paused Execution Plan remains until invalidated; agent cancel does not silently roll back completed mutations.".into(),
        };
    }
    if matches!(
        stream,
        Some(StreamingLifecycle::Thinking) | Some(StreamingLifecycle::Streaming)
    ) {
        return InFlightObservation {
            kind: InFlightKind::GenerationStreaming,
            can_interrupt_immediately: false,
            message: "Generation may still finish — use existing Stop/cancel generation. No new agent operations will begin; successful writes will not be silently undone.".into(),
        };
    }
    if matches!(
        conversation,
        ConversationState::Reasoning | ConversationState::Executing
    ) {
        return InFlightObservation {
            kind: InFlightKind::ConversationBusy,
            can_interrupt_immediately: false,
            message: "Planner conversation is still busy. Pause/Cancel stop new goal operations; in-flight work uses existing cancellation semantics.".into(),
        };
    }
    if matches!(
        lifecycle,
        AgentLifecycleState::Executing | AgentLifecycleState::Verifying
    ) {
        return InFlightObservation {
            kind: InFlightKind::VerificationLikely,
            can_interrupt_immediately: false,
            message: "An execute/verify phase may have an in-flight tool run. New operations are stopped; interrupt uses existing Tool / stream cancel — completed mutations are not rolled back.".into(),
        };
    }
    InFlightObservation::default()
}

/// True when the goal must not begin new operations (pause or cancel).
pub fn allows_new_operations(goal: &CodingGoal) -> bool {
    if goal.control.cancelled || goal.status == CodingGoalStatus::Cancelled {
        return false;
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::Cancelled {
        return false;
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::Paused {
        return false;
    }
    true
}

fn stamp_now() -> String {
    crate::coding_goal::stamp_now()
}

fn capture_checkpoint(goal: &CodingGoal, bundle: &ContextBundle) -> PauseCheckpoint {
    let fingerprint = ProjectFingerprint::from_bundle(bundle);
    let mut notes = vec![
        "Pause preserves goal, tasks, pending plan, and verification state.".into(),
        "No new operations will begin until Resume.".into(),
    ];
    if !fingerprint.is_available() {
        notes.push(
            "C5.14: fingerprint unavailable at pause — resume will treat state as unknown/stale (not unchanged)."
                .into(),
        );
    } else {
        notes.push("C5.14: real workspace fingerprint captured via ContextBundle.".into());
    }
    PauseCheckpoint {
        paused_from: goal
            .agent_lifecycle
            .paused_from
            .unwrap_or(goal.agent_lifecycle.state),
        fingerprint,
        preserved_plan_id: goal.agent_lifecycle.approved_plan_id.clone().or_else(|| {
            goal.autonomy_policy
                .active
                .as_ref()
                .map(|b| b.plan_id.clone())
        }),
        preserved_approval_recorded: goal.agent_lifecycle.approval_recorded,
        preserved_task_id: goal.active_task_id.clone(),
        preserved_verification: goal.verification_state,
        stop_new_operations: true,
        paused_at: stamp_now(),
        notes,
    }
}

/// Pause: stop new ops; preserve state; remain resumable.
pub fn pause_goal(
    goal: CodingGoal,
    bundle: &ContextBundle,
    in_flight: &InFlightObservation,
    reason: &str,
) -> Result<(CodingGoal, AgentControlReport), String> {
    if goal.control.cancelled || goal.status == CodingGoalStatus::Cancelled {
        return Err("Goal is cancelled — cannot pause.".into());
    }
    if goal.agent_lifecycle.state == AgentLifecycleState::Paused {
        let mut report = AgentControlReport::paused_idempotent(&goal, in_flight);
        report.notes.push("Already paused.".into());
        return Ok((goal, report));
    }

    let from = goal.agent_lifecycle.state;
    let mut updated = transition_goal(
        goal,
        AgentLifecycleEvent::Pause {
            reason: reason.into(),
        },
    )
    .map_err(|e| e.to_string_message())?;

    let mut checkpoint = capture_checkpoint(&updated, bundle);
    checkpoint.paused_from = from;
    updated.control.checkpoint = Some(Box::new(checkpoint.clone()));
    updated.control.last_in_flight_note = if in_flight.kind != InFlightKind::None {
        Some(in_flight.message.clone())
    } else {
        None
    };
    updated.control.requires_fresh_approval = false;
    updated.control.notes.push(format!("Paused from {}.", from.label()));
    updated.notes.push(format!("C5.8 pause: {reason}"));

    let report = AgentControlReport {
        status: AgentControlStatus::Paused,
        goal_id: Some(updated.id.clone()),
        from: Some(from),
        to: Some(AgentLifecycleState::Paused),
        preserved_plan: checkpoint.preserved_plan_id.clone(),
        preserved_task: checkpoint.preserved_task_id.clone(),
        preserved_verification: Some(checkpoint.preserved_verification.as_str().into()),
        stop_new_operations: true,
        resumable: true,
        stale_reasons: Vec::new(),
        requires_fresh_approval: false,
        approvals_invalidated: false,
        rolled_back_mutations: false,
        continued_after_cancel: false,
        in_flight: in_flight.clone(),
        parallel_cancel_engine: false,
        planner_owned: true,
        notes: {
            let mut n = checkpoint.notes.clone();
            if in_flight.kind != InFlightKind::None {
                n.push(in_flight.message.clone());
                if !in_flight.can_interrupt_immediately {
                    n.push(
                        "In-flight operation cannot be interrupted immediately — surfaced honestly."
                            .into(),
                    );
                }
            }
            n.push(checkpoint.fingerprint.to_diagnostics_summary());
            n
        },
    };
    Ok((updated, report))
}

fn approval_still_usable(goal: &CodingGoal) -> bool {
    let Some(plan) = goal
        .agent_lifecycle
        .approved_plan_id
        .as_deref()
        .or_else(|| {
            goal.autonomy_policy
                .active
                .as_ref()
                .filter(|b| !b.invalidated)
                .map(|b| b.plan_id.as_str())
        })
    else {
        // No pending approved mutation — nothing stale to refuse.
        return true;
    };
    assert_execution_authorized(goal, plan).is_ok()
}

/// Resume: revalidate project; never blindly execute stale approved ops.
pub fn resume_goal(
    goal: CodingGoal,
    bundle: &ContextBundle,
    in_flight: &InFlightObservation,
    reason: &str,
) -> Result<(CodingGoal, AgentControlReport), String> {
    if goal.control.cancelled || goal.status == CodingGoalStatus::Cancelled {
        return Err("Goal is cancelled — cannot resume.".into());
    }
    if goal.agent_lifecycle.state != AgentLifecycleState::Paused {
        return Err("Goal is not paused.".into());
    }

    let checkpoint = goal
        .control
        .checkpoint
        .as_deref()
        .cloned()
        .unwrap_or_else(|| {
            let mut c = capture_checkpoint(&goal, bundle);
            c.paused_from = goal
                .agent_lifecycle
                .paused_from
                .unwrap_or(AgentLifecycleState::Idle);
            c
        });
    let current = ProjectFingerprint::from_bundle(bundle);
    let stale = detect_stale(&checkpoint.fingerprint, &current);
    let mut requires_fresh = !stale.is_empty() || !approval_still_usable(&goal);

    let mut prepared = goal;
    if requires_fresh {
        prepared = invalidate_on_plan_change(
            prepared,
            if stale.is_empty() {
                "resume — prior approval no longer usable"
            } else {
                "resume — project state changed while paused"
            },
        );
        prepared.agent_lifecycle.approval_recorded = false;
        if matches!(
            prepared.agent_lifecycle.paused_from,
            Some(AgentLifecycleState::Executing)
                | Some(AgentLifecycleState::AwaitingApproval)
                | Some(AgentLifecycleState::Verifying)
        ) {
            prepared.agent_lifecycle.paused_from = Some(AgentLifecycleState::Planning);
            prepared.notes.push(
                "Stale or invalid approval — resuming to Planning; fresh Review Before Action required before mutation.".into(),
            );
        }
        prepared.control.requires_fresh_approval = true;
    }

    let from = AgentLifecycleState::Paused;
    let mut updated = transition_goal(
        prepared,
        AgentLifecycleEvent::Resume {
            reason: reason.into(),
        },
    )
    .map_err(|e| e.to_string_message())?;

    updated.control.checkpoint = None;
    updated.control.requires_fresh_approval = requires_fresh;
    updated.control.last_in_flight_note = if in_flight.kind != InFlightKind::None {
        Some(in_flight.message.clone())
    } else {
        None
    };
    if requires_fresh
        && updated.agent_lifecycle.state == AgentLifecycleState::Executing
        && !approval_still_usable(&updated)
    {
        updated.agent_lifecycle.state = AgentLifecycleState::Planning;
        updated.agent_lifecycle.approval_recorded = false;
        requires_fresh = true;
        updated.control.requires_fresh_approval = true;
    }

    let to = updated.agent_lifecycle.state;
    let report = AgentControlReport {
        status: if requires_fresh {
            AgentControlStatus::ResumedNeedsApproval
        } else {
            AgentControlStatus::Resumed
        },
        goal_id: Some(updated.id.clone()),
        from: Some(from),
        to: Some(to),
        preserved_plan: checkpoint.preserved_plan_id.clone(),
        preserved_task: checkpoint
            .preserved_task_id
            .clone()
            .or_else(|| updated.active_task_id.clone()),
        preserved_verification: Some(checkpoint.preserved_verification.as_str().into()),
        stop_new_operations: false,
        resumable: true,
        stale_reasons: stale.clone(),
        requires_fresh_approval: requires_fresh,
        approvals_invalidated: requires_fresh,
        rolled_back_mutations: false,
        continued_after_cancel: false,
        in_flight: in_flight.clone(),
        parallel_cancel_engine: false,
        planner_owned: true,
        notes: {
            let mut n = vec![
                format!("Resumed to {}.", to.label()),
                "Task / verification state preserved.".into(),
            ];
            for s in &stale {
                n.push(format!("Stale: {}", s.label()));
            }
            if requires_fresh {
                n.push(
                    "Fresh approval required — will not blindly execute prior approved operations."
                        .into(),
                );
            } else {
                n.push("Project fingerprint matched; prior approval still attributable.".into());
            }
            if in_flight.kind != InFlightKind::None {
                n.push(in_flight.message.clone());
            }
            n.push(current.to_diagnostics_summary());
            n
        },
    };
    Ok((updated, report))
}

/// Cancel: stop future work; invalidate approvals; preserve history; no silent rollback.
pub fn cancel_active_goal(
    goal: CodingGoal,
    in_flight: &InFlightObservation,
    reason: &str,
) -> (CodingGoal, AgentControlReport) {
    let from = goal.agent_lifecycle.state;
    let history_len_before = goal.agent_lifecycle.history.len();
    let workflow_len_before = goal.workflow_history.len();
    let already = goal.control.cancelled
        || goal.status == CodingGoalStatus::Cancelled
        || from == AgentLifecycleState::Cancelled;

    let mut updated = if already {
        let mut g = invalidate_on_cancel(goal, reason);
        g.control.cancelled = true;
        g.control.cancel_count = g.control.cancel_count.saturating_add(1);
        g.control.last_cancel_reason = Some(reason.into());
        g.status = CodingGoalStatus::Cancelled;
        g.agent_lifecycle.state = AgentLifecycleState::Cancelled;
        g
    } else {
        let mut g = cancel_goal(goal, reason);
        g = invalidate_on_cancel(g, reason);
        g.control.cancelled = true;
        g.control.cancel_count = g.control.cancel_count.saturating_add(1);
        g.control.last_cancel_reason = Some(reason.into());
        g.control.checkpoint = None;
        g
    };

    updated.control.last_in_flight_note = if in_flight.kind != InFlightKind::None {
        Some(in_flight.message.clone())
    } else {
        None
    };
    updated.notes.push(format!("C5.8 cancel: {reason}"));

    let report = AgentControlReport {
        status: if already {
            AgentControlStatus::CancelRepeated
        } else {
            AgentControlStatus::Cancelled
        },
        goal_id: Some(updated.id.clone()),
        from: Some(from),
        to: Some(AgentLifecycleState::Cancelled),
        preserved_plan: None,
        preserved_task: updated.active_task_id.clone(),
        preserved_verification: Some(updated.verification_state.as_str().into()),
        stop_new_operations: true,
        resumable: false,
        stale_reasons: Vec::new(),
        requires_fresh_approval: false,
        approvals_invalidated: true,
        rolled_back_mutations: false,
        continued_after_cancel: false,
        in_flight: in_flight.clone(),
        parallel_cancel_engine: false,
        planner_owned: true,
        notes: {
            let mut n = vec![
                "Future work stopped.".into(),
                "Pending approvals invalidated.".into(),
                "Execution / lifecycle history preserved.".into(),
                "Successful mutations are not silently rolled back.".into(),
                format!(
                    "history_len {}→{}; workflow_len {}→{}",
                    history_len_before,
                    updated.agent_lifecycle.history.len(),
                    workflow_len_before,
                    updated.workflow_history.len()
                ),
            ];
            if already {
                n.push("Repeated cancel — idempotent.".into());
            }
            if in_flight.kind != InFlightKind::None {
                n.push(in_flight.message.clone());
                if !in_flight.can_interrupt_immediately {
                    n.push(
                        "In-flight operation may still finish; cancel stops new work only.".into(),
                    );
                }
            }
            n
        },
    };
    (updated, report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentControlStatus {
    Paused,
    Resumed,
    ResumedNeedsApproval,
    Cancelled,
    CancelRepeated,
    Status,
    Rejected,
    NoActiveGoal,
}

impl AgentControlStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Resumed => "resumed",
            Self::ResumedNeedsApproval => "resumed_needs_approval",
            Self::Cancelled => "cancelled",
            Self::CancelRepeated => "cancel_repeated",
            Self::Status => "status",
            Self::Rejected => "rejected",
            Self::NoActiveGoal => "no_active_goal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentControlReport {
    pub status: AgentControlStatus,
    pub goal_id: Option<String>,
    pub from: Option<AgentLifecycleState>,
    pub to: Option<AgentLifecycleState>,
    pub preserved_plan: Option<String>,
    pub preserved_task: Option<String>,
    pub preserved_verification: Option<String>,
    pub stop_new_operations: bool,
    pub resumable: bool,
    pub stale_reasons: Vec<StaleReason>,
    pub requires_fresh_approval: bool,
    pub approvals_invalidated: bool,
    pub rolled_back_mutations: bool,
    pub continued_after_cancel: bool,
    pub in_flight: InFlightObservation,
    pub parallel_cancel_engine: bool,
    pub planner_owned: bool,
    pub notes: Vec<String>,
}

impl Default for AgentControlReport {
    fn default() -> Self {
        Self {
            status: AgentControlStatus::Rejected,
            goal_id: None,
            from: None,
            to: None,
            preserved_plan: None,
            preserved_task: None,
            preserved_verification: None,
            stop_new_operations: false,
            resumable: false,
            stale_reasons: Vec::new(),
            requires_fresh_approval: false,
            approvals_invalidated: false,
            rolled_back_mutations: false,
            continued_after_cancel: false,
            in_flight: InFlightObservation::default(),
            parallel_cancel_engine: false,
            planner_owned: true,
            notes: Vec::new(),
        }
    }
}

impl AgentControlReport {
    fn paused_idempotent(goal: &CodingGoal, in_flight: &InFlightObservation) -> Self {
        Self {
            status: AgentControlStatus::Paused,
            goal_id: Some(goal.id.clone()),
            from: Some(AgentLifecycleState::Paused),
            to: Some(AgentLifecycleState::Paused),
            stop_new_operations: true,
            resumable: true,
            in_flight: in_flight.clone(),
            planner_owned: true,
            parallel_cancel_engine: false,
            rolled_back_mutations: false,
            ..Default::default()
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Goal Control\n\n");
        out.push_str(&format!(
            "**{}**\n\n",
            self.status.as_str().replace('_', " ")
        ));
        if let Some(id) = &self.goal_id {
            out.push_str(&format!("- Goal: `{id}`\n"));
        }
        if let (Some(from), Some(to)) = (self.from, self.to) {
            out.push_str(&format!("- Lifecycle: {} → {}\n", from.label(), to.label()));
        }
        if self.stop_new_operations {
            out.push_str("- New operations: **stopped**\n");
        }
        if self.resumable {
            out.push_str("- Resumable: yes\n");
        }
        if let Some(plan) = &self.preserved_plan {
            out.push_str(&format!("- Preserved plan: `{plan}`\n"));
        }
        if let Some(task) = &self.preserved_task {
            out.push_str(&format!("- Preserved task: `{task}`\n"));
        }
        if let Some(v) = &self.preserved_verification {
            out.push_str(&format!("- Verification state: `{v}`\n"));
        }
        if !self.stale_reasons.is_empty() {
            out.push_str("\n### Stale project state\n");
            for s in &self.stale_reasons {
                out.push_str(&format!("- {}\n", s.label()));
            }
        }
        if self.requires_fresh_approval {
            out.push_str("\n**Fresh approval required** before mutation.\n");
        }
        if self.approvals_invalidated {
            out.push_str("- Approvals invalidated.\n");
        }
        out.push_str(&format!(
            "- Silent rollback: {}\n",
            if self.rolled_back_mutations {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        if self.in_flight.kind != InFlightKind::None {
            out.push_str("\n### In-flight\n");
            out.push_str(&format!("- Kind: `{}`\n", self.in_flight.kind.as_str()));
            out.push_str(&format!(
                "- Immediate interrupt: {}\n",
                if self.in_flight.can_interrupt_immediately {
                    "yes"
                } else {
                    "no — surfaced honestly"
                }
            ));
            out.push_str(&format!("- {}\n", self.in_flight.message));
        }
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str(&format!("- {n}\n"));
            }
        }
        out.push_str(
            "\n_Planner-owned control — no parallel cancellation engine; Tool cancel reuses existing semantics._\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "agent_control status={} from={} to={} stop_new={} stale={} fresh_approval={} invalidated={} rollback={} in_flight={} parallel_engine={} planner_owned={}",
            self.status.as_str(),
            self.from.map(|s| s.as_str()).unwrap_or("—"),
            self.to.map(|s| s.as_str()).unwrap_or("—"),
            self.stop_new_operations,
            self.stale_reasons.len(),
            self.requires_fresh_approval,
            self.approvals_invalidated,
            self.rolled_back_mutations,
            self.in_flight.kind.as_str(),
            self.parallel_cancel_engine,
            self.planner_owned,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentControlRequestKind {
    Pause,
    Resume,
    Cancel,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentControlAssessment {
    pub kind: AgentControlRequestKind,
    pub matched_rules: Vec<&'static str>,
    pub reason: String,
}

/// Detect pause / resume / cancel control phrases (Conversation First).
pub fn detect_control_request(request: &UserRequest) -> Option<AgentControlAssessment> {
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

    // Phrase-only detect — do not call sibling soft-path detectors (avoids cycles
    // with agent_state_machine / agent_progress_ux / coding_edit steals chains).

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
        return Some(AgentControlAssessment {
            kind: AgentControlRequestKind::Pause,
            matched_rules: vec!["c58_pause"],
            reason: "User paused the coding goal.".into(),
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
        return Some(AgentControlAssessment {
            kind: AgentControlRequestKind::Resume,
            matched_rules: vec!["c58_resume"],
            reason: "User resumed the coding goal.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "cancel this goal"
            | "cancel the goal"
            | "cancel goal"
            | "cancel my goal"
            | "abandon this goal"
            | "abandon the goal"
            | "stop this goal"
            | "stop working on this goal"
            | "drop this goal"
    ) || trimmed.starts_with("cancel this goal")
        || trimmed.starts_with("stop this goal")
    {
        return Some(AgentControlAssessment {
            kind: AgentControlRequestKind::Cancel,
            matched_rules: vec!["c58_cancel"],
            reason: "User cancelled the coding goal.".into(),
        });
    }

    if matches!(
        trimmed.as_str(),
        "goal control status" | "pause resume status" | "control status"
    ) {
        return Some(AgentControlAssessment {
            kind: AgentControlRequestKind::Status,
            matched_rules: vec!["c58_status"],
            reason: "Status query.".into(),
        });
    }

    None
}

pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &AgentControlAssessment,
    bundle: &ContextBundle,
    in_flight: &InFlightObservation,
) -> (AgentControlReport, Option<CodingGoal>) {
    let Some(goal) = goal else {
        return (
            AgentControlReport {
                status: AgentControlStatus::NoActiveGoal,
                notes: vec!["No active Coding Goal.".into()],
                planner_owned: true,
                ..Default::default()
            },
            None,
        );
    };

    if assessment.kind == AgentControlRequestKind::Status {
        let report = AgentControlReport {
            status: AgentControlStatus::Status,
            goal_id: Some(goal.id.clone()),
            from: Some(goal.agent_lifecycle.state),
            to: Some(goal.agent_lifecycle.state),
            stop_new_operations: !allows_new_operations(goal),
            resumable: goal.agent_lifecycle.state == AgentLifecycleState::Paused,
            requires_fresh_approval: goal.control.requires_fresh_approval,
            in_flight: in_flight.clone(),
            planner_owned: true,
            notes: vec![
                format!("cancelled={}", goal.control.cancelled),
                format!("cancel_count={}", goal.control.cancel_count),
                format!("allows_new_ops={}", allows_new_operations(goal)),
            ],
            ..Default::default()
        };
        return (report, Some(goal.clone()));
    }

    match assessment.kind {
        AgentControlRequestKind::Pause => match pause_goal(
            goal.clone(),
            bundle,
            in_flight,
            &assessment.reason,
        ) {
            Ok((g, r)) => (r, Some(g)),
            Err(e) => (
                AgentControlReport {
                    status: AgentControlStatus::Rejected,
                    goal_id: Some(goal.id.clone()),
                    notes: vec![e],
                    planner_owned: true,
                    ..Default::default()
                },
                None,
            ),
        },
        AgentControlRequestKind::Resume => match resume_goal(
            goal.clone(),
            bundle,
            in_flight,
            &assessment.reason,
        ) {
            Ok((g, r)) => (r, Some(g)),
            Err(e) => (
                AgentControlReport {
                    status: AgentControlStatus::Rejected,
                    goal_id: Some(goal.id.clone()),
                    notes: vec![e],
                    planner_owned: true,
                    ..Default::default()
                },
                None,
            ),
        },
        AgentControlRequestKind::Cancel => {
            let (g, r) = cancel_active_goal(goal.clone(), in_flight, &assessment.reason);
            (r, Some(g))
        }
        AgentControlRequestKind::Status => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_state_machine::{transition_goal, AgentLifecycleEvent};
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::{apply_decomposition, propose_tasks};
    use crate::multi_step_execution::start_goal_execution;
    use jaymi_context::{
        ActiveProjectSection, ContextBundle, CurrentFileSection, GitStatusSection,
    };

    fn base_goal() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(
            "Build a login system without OAuth.",
        ))
        .unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let (tasks, order) = propose_tasks(&g).unwrap();
        apply_decomposition(g, tasks, order)
    }

    fn started() -> CodingGoal {
        start_goal_execution(base_goal()).unwrap().0
    }

    fn bundle_with_git(head: &str, dirty: usize) -> ContextBundle {
        ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("demo".into()),
                name: Some("demo".into()),
                root_directory: Some("/proj".into()),
                detail: None,
            })
            .git_status(GitStatusSection {
                is_repository: true,
                head_sha: Some(head.into()),
                branch: Some("main".into()),
                modified_count: dirty,
                staged_count: 0,
                conflict_count: 0,
                dirty_paths: if dirty > 0 {
                    vec!["/proj/src/lib.rs".into()]
                } else {
                    Vec::new()
                },
                ..GitStatusSection::default()
            })
            .build()
    }

    fn bundle_with_file(path: &str, dirty: bool, head: &str) -> ContextBundle {
        ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("demo".into()),
                name: Some("demo".into()),
                root_directory: Some("/proj".into()),
                detail: None,
            })
            .current_file(CurrentFileSection {
                path: Some(path.into()),
                dirty,
                language: Some("rust".into()),
            })
            .git_status(GitStatusSection {
                is_repository: true,
                head_sha: Some(head.into()),
                branch: Some("main".into()),
                ..GitStatusSection::default()
            })
            .build()
    }

    fn approved_executing(mut g: CodingGoal) -> CodingGoal {
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginPlanning { reason: "p".into() },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-1".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "plan-1".into(),
            },
        );
        to_phase(
            g,
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "plan-1".into(),
            },
        )
    }

    fn to_phase(g: CodingGoal, event: AgentLifecycleEvent) -> CodingGoal {
        transition_goal(g, event).unwrap()
    }

    #[test]
    fn pause_preserves_plan_task_verification_and_stops_new_ops() {
        let mut g = started();
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginPlanning {
                reason: "p".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-1".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "plan-1".into(),
            },
        );
        g.verification_state = GoalVerificationState::InProgress;
        let task = g.active_task_id.clone();
        let bundle = bundle_with_git("abc", 1);
        let (paused, report) = pause_goal(
            g,
            &bundle,
            &InFlightObservation::default(),
            "user pause",
        )
        .unwrap();
        assert_eq!(paused.agent_lifecycle.state, AgentLifecycleState::Paused);
        assert!(!allows_new_operations(&paused));
        assert!(report.resumable);
        assert!(report.stop_new_operations);
        let cp = paused.control.checkpoint.as_ref().unwrap();
        assert_eq!(cp.preserved_plan_id.as_deref(), Some("plan-1"));
        assert_eq!(cp.preserved_task_id, task);
        assert_eq!(cp.preserved_verification, GoalVerificationState::InProgress);
        assert!(paused.agent_lifecycle.approval_recorded);
    }

    #[test]
    fn resume_fresh_keeps_approval_when_fingerprint_matches() {
        let mut g = started();
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginPlanning { reason: "p".into() },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-1".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "plan-1".into(),
            },
        );
        let bundle = bundle_with_git("abc", 0);
        let (paused, _) = pause_goal(g, &bundle, &InFlightObservation::default(), "p").unwrap();
        let (resumed, report) =
            resume_goal(paused, &bundle, &InFlightObservation::default(), "r").unwrap();
        assert_ne!(resumed.agent_lifecycle.state, AgentLifecycleState::Paused);
        assert!(!report.requires_fresh_approval);
        assert!(allows_new_operations(&resumed));
    }

    #[test]
    fn stale_resume_invalidates_approval_and_avoids_executing() {
        let mut g = started();
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginPlanning { reason: "p".into() },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-1".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "plan-1".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "plan-1".into(),
            },
        );
        let pause_bundle = bundle_with_git("abc", 0);
        let (paused, _) =
            pause_goal(g, &pause_bundle, &InFlightObservation::default(), "p").unwrap();
        assert_eq!(
            paused.agent_lifecycle.paused_from,
            Some(AgentLifecycleState::Executing)
        );
        let resume_bundle = bundle_with_git("def", 3);
        let (resumed, report) =
            resume_goal(paused, &resume_bundle, &InFlightObservation::default(), "r").unwrap();
        assert!(!report.stale_reasons.is_empty());
        assert!(report.requires_fresh_approval);
        assert!(report.approvals_invalidated);
        assert_ne!(resumed.agent_lifecycle.state, AgentLifecycleState::Executing);
        assert!(!resumed.agent_lifecycle.approval_recorded);
        assert!(assert_execution_authorized(&resumed, "plan-1").is_err());
    }

    #[test]
    fn cancel_invalidates_preserves_history_no_rollback() {
        let mut g = started();
        g.workflow_history
            .push(crate::multi_step_execution::WorkflowRecord {
                sequence: 1,
                kind: "step".into(),
                task_id: None,
                detail: "did work".into(),
            });
        let hist_before = g.agent_lifecycle.history.len();
        let (cancelled, report) =
            cancel_active_goal(g, &InFlightObservation::default(), "stop");
        assert_eq!(cancelled.status, CodingGoalStatus::Cancelled);
        assert!(cancelled.control.cancelled);
        assert!(!allows_new_operations(&cancelled));
        assert!(report.approvals_invalidated);
        assert!(!report.rolled_back_mutations);
        assert!(!report.continued_after_cancel);
        assert!(cancelled.agent_lifecycle.history.len() >= hist_before);
        assert!(!cancelled.workflow_history.is_empty());

        let (again, r2) =
            cancel_active_goal(cancelled, &InFlightObservation::default(), "again");
        assert_eq!(r2.status, AgentControlStatus::CancelRepeated);
        assert_eq!(again.control.cancel_count, 2);
    }

    #[test]
    fn cancel_during_every_phase() {
        let cases: Vec<(&str, Box<dyn Fn(CodingGoal) -> CodingGoal>)> = vec![
            ("investigating", Box::new(|g| g)),
            (
                "planning",
                Box::new(|g| {
                    to_phase(
                        g,
                        AgentLifecycleEvent::BeginPlanning {
                            reason: "p".into(),
                        },
                    )
                }),
            ),
            (
                "approval",
                Box::new(|g| {
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginPlanning {
                            reason: "p".into(),
                        },
                    );
                    to_phase(
                        g,
                        AgentLifecycleEvent::SubmitForApproval {
                            plan_id: "p".into(),
                        },
                    )
                }),
            ),
            (
                "executing",
                Box::new(|g| {
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginPlanning {
                            reason: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::SubmitForApproval {
                            plan_id: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::RecordApproval {
                            plan_id: "p".into(),
                        },
                    );
                    to_phase(
                        g,
                        AgentLifecycleEvent::BeginExecution {
                            approved_plan_id: "p".into(),
                        },
                    )
                }),
            ),
            (
                "verifying",
                Box::new(|g| {
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginPlanning {
                            reason: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::SubmitForApproval {
                            plan_id: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::RecordApproval {
                            plan_id: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginExecution {
                            approved_plan_id: "p".into(),
                        },
                    );
                    to_phase(
                        g,
                        AgentLifecycleEvent::BeginVerification {
                            reason: "v".into(),
                        },
                    )
                }),
            ),
            (
                "correcting",
                Box::new(|g| {
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginPlanning {
                            reason: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::SubmitForApproval {
                            plan_id: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::RecordApproval {
                            plan_id: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginExecution {
                            approved_plan_id: "p".into(),
                        },
                    );
                    let g = to_phase(
                        g,
                        AgentLifecycleEvent::BeginVerification {
                            reason: "v".into(),
                        },
                    );
                    to_phase(
                        g,
                        AgentLifecycleEvent::VerificationFailed {
                            evidence_ref: "fail".into(),
                        },
                    )
                }),
            ),
        ];
        for (label, build) in cases {
            let g = build(started());
            let from = g.agent_lifecycle.state;
            let (c, report) = cancel_active_goal(g, &InFlightObservation::default(), label);
            assert_eq!(
                c.agent_lifecycle.state,
                AgentLifecycleState::Cancelled,
                "{label} from {from:?}"
            );
            assert_eq!(report.status, AgentControlStatus::Cancelled);
            assert!(!allows_new_operations(&c));
        }
    }

    #[test]
    fn in_flight_honesty_when_streaming() {
        let obs = observe_in_flight(
            ConversationState::Reasoning,
            Some(StreamingLifecycle::Streaming),
            false,
            AgentLifecycleState::Executing,
        );
        assert_eq!(obs.kind, InFlightKind::GenerationStreaming);
        assert!(!obs.can_interrupt_immediately);
        assert!(!obs.message.is_empty());

        let g = started();
        let (paused, report) = pause_goal(
            g,
            &ContextBundle::default(),
            &obs,
            "pause during exec",
        )
        .unwrap();
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("honestly") || n.contains("Generation")));
        assert!(!allows_new_operations(&paused));
    }

    #[test]
    fn resume_after_verification_failure() {
        let mut g = started();
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginPlanning { reason: "p".into() },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "p".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "p".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginExecution {
                approved_plan_id: "p".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::BeginVerification {
                reason: "v".into(),
            },
        );
        g = to_phase(
            g,
            AgentLifecycleEvent::VerificationFailed {
                evidence_ref: "fail".into(),
            },
        );
        assert_eq!(g.agent_lifecycle.state, AgentLifecycleState::Correcting);
        let bundle = bundle_with_git("abc", 0);
        let (paused, _) = pause_goal(g, &bundle, &InFlightObservation::default(), "p").unwrap();
        let (resumed, report) =
            resume_goal(paused, &bundle, &InFlightObservation::default(), "r").unwrap();
        assert_eq!(resumed.agent_lifecycle.state, AgentLifecycleState::Correcting);
        assert!(matches!(
            report.status,
            AgentControlStatus::Resumed | AgentControlStatus::ResumedNeedsApproval
        ));
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_control_request(&UserRequest::new("Pause this goal.")).is_some());
        assert!(detect_control_request(&UserRequest::new("Resume this goal.")).is_some());
        assert!(detect_control_request(&UserRequest::new("Cancel this goal.")).is_some());
        assert!(detect_control_request(&UserRequest::new("Build a login system.")).is_none());
    }

    #[test]
    fn c514_unchanged_workspace_resume_keeps_approval() {
        let g = approved_executing(started());
        let bundle = bundle_with_git("abc123", 0);
        assert!(ProjectFingerprint::from_bundle(&bundle).is_available());
        let (paused, pause_report) =
            pause_goal(g, &bundle, &InFlightObservation::default(), "pause").unwrap();
        assert!(!pause_report.stale_reasons.contains(&StaleReason::FingerprintUnavailable));
        let cp = paused.control.checkpoint.as_ref().unwrap();
        assert!(cp.fingerprint.is_available());
        assert_eq!(cp.fingerprint.head_sha.as_deref(), Some("abc123"));

        let (resumed, report) =
            resume_goal(paused, &bundle, &InFlightObservation::default(), "resume").unwrap();
        assert!(report.stale_reasons.is_empty());
        assert!(!report.requires_fresh_approval);
        assert!(!report.approvals_invalidated);
        assert!(resumed.agent_lifecycle.approval_recorded);
        assert!(!resumed.mutated);
        assert_ne!(resumed.agent_lifecycle.state, AgentLifecycleState::Paused);
    }

    #[test]
    fn c514_changed_file_is_stale() {
        let g = approved_executing(started());
        let pause_bundle = bundle_with_file("/proj/src/a.rs", false, "abc");
        let (paused, _) =
            pause_goal(g, &pause_bundle, &InFlightObservation::default(), "p").unwrap();
        let resume_bundle = bundle_with_file("/proj/src/a.rs", true, "abc");
        let stale = detect_stale(
            &paused.control.checkpoint.as_ref().unwrap().fingerprint,
            &ProjectFingerprint::from_bundle(&resume_bundle),
        );
        assert!(stale.contains(&StaleReason::CurrentFileChanged));
        let (resumed, report) =
            resume_goal(paused, &resume_bundle, &InFlightObservation::default(), "r").unwrap();
        assert!(report.requires_fresh_approval);
        assert!(report.approvals_invalidated);
        assert!(report.stale_reasons.contains(&StaleReason::CurrentFileChanged));
        assert!(!resumed.agent_lifecycle.approval_recorded);
        assert!(!resumed.mutated);
    }

    #[test]
    fn c514_changed_project_state_is_stale() {
        let g = approved_executing(started());
        let pause_bundle = bundle_with_git("aaa", 0);
        let (paused, _) =
            pause_goal(g, &pause_bundle, &InFlightObservation::default(), "p").unwrap();
        let resume_bundle = bundle_with_git("bbb", 2);
        let (resumed, report) =
            resume_goal(paused, &resume_bundle, &InFlightObservation::default(), "r").unwrap();
        assert!(report.stale_reasons.contains(&StaleReason::HeadChanged));
        assert!(report.stale_reasons.contains(&StaleReason::DirtyGrew));
        assert!(report.requires_fresh_approval);
        assert!(report.approvals_invalidated);
        assert!(assert_execution_authorized(&resumed, "plan-1").is_err());
        assert!(!resumed.mutated);
    }

    #[test]
    fn c514_unavailable_fingerprint_is_unknown_stale_not_unchanged() {
        let empty = ContextBundle::default();
        let fp = ProjectFingerprint::from_bundle(&empty);
        assert!(!fp.is_available());
        let stale = detect_stale(&fp, &fp);
        assert_eq!(stale, vec![StaleReason::FingerprintUnavailable]);

        let g = approved_executing(started());
        let (paused, _) =
            pause_goal(g, &empty, &InFlightObservation::default(), "p").unwrap();
        assert!(!paused
            .control
            .checkpoint
            .as_ref()
            .unwrap()
            .fingerprint
            .is_available());
        let (resumed, report) =
            resume_goal(paused, &empty, &InFlightObservation::default(), "r").unwrap();
        assert!(report
            .stale_reasons
            .contains(&StaleReason::FingerprintUnavailable));
        assert!(report.requires_fresh_approval);
        assert!(report.approvals_invalidated);
        assert!(!resumed.mutated);
        assert!(!report.rolled_back_mutations);
    }

    #[test]
    fn c514_stale_approval_invalidated_on_resume() {
        let g = approved_executing(started());
        let pause_bundle = bundle_with_git("same", 0);
        let (paused, _) =
            pause_goal(g, &pause_bundle, &InFlightObservation::default(), "p").unwrap();
        // Project unchanged, but approval binding already unusable → still invalidate.
        let mut paused = paused;
        paused.autonomy_policy.active = Some(crate::approval_autonomy_policy::BoundApproval {
            goal_id: paused.id.clone(),
            plan_id: "other-plan".into(),
            task_id: None,
            recorded: true,
            class: crate::approval_autonomy_policy::AutonomyPolicyClass::Mutation,
            invalidated: false,
            invalidate_reason: None,
        });
        // Keep approved_plan_id as plan-1 so assert_execution_authorized fails vs active binding.
        let (resumed, report) =
            resume_goal(paused, &pause_bundle, &InFlightObservation::default(), "r").unwrap();
        assert!(report.requires_fresh_approval || report.approvals_invalidated);
        assert!(assert_execution_authorized(&resumed, "plan-1").is_err());
    }

    #[test]
    fn c514_pause_resume_no_accidental_mutation() {
        let g = approved_executing(started());
        let bundle = bundle_with_git("abc", 0);
        let (paused, pause_report) =
            pause_goal(g, &bundle, &InFlightObservation::default(), "p").unwrap();
        assert!(!paused.mutated);
        assert!(!paused.tools_executed);
        assert!(!pause_report.rolled_back_mutations);
        let (resumed, resume_report) =
            resume_goal(paused, &bundle, &InFlightObservation::default(), "r").unwrap();
        assert!(!resumed.mutated);
        assert!(!resumed.tools_executed);
        assert!(!resume_report.rolled_back_mutations);
        assert!(!resume_report.continued_after_cancel);
        assert!(!resume_report.parallel_cancel_engine);
    }
}
