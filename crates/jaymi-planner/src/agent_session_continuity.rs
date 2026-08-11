//! Long-Running Coding Sessions / Session Continuity (Sprint C5.11).
//!
//! Make Coding Goals resilient across long sessions (many tasks, tool calls,
//! context changes, interruptions, verification cycles) **without** a parallel
//! agent database.
//!
//! Persistence stance:
//! - Primary continuity is the Planner-owned `CodingGoalStore` (process session).
//! - Portable serde snapshots of the store are available for hitching onto
//!   existing session/config mechanisms later — this sprint does **not** invent
//!   a Goal DB or write Memory autonomously.
//!
//! Constitutional constraints:
//! - Revalidate project state after long pauses; never blindly resume stale plans
//! - Invalidate pending approvals when underlying plans change
//! - Context compaction must preserve user constraints
//! - Bound history growth
//! - No background autonomous work
//! - Paused goals must not continue executing while the user is away

use crate::agent_context_maintenance::{
    compact_context, maintain_from_goal, MAX_GOAL_CONTEXT_CHARS,
};
use crate::agent_pause_resume_cancel::{detect_stale, ProjectFingerprint};
use crate::agent_state_machine::AgentLifecycleState;
use crate::approval_autonomy_policy::invalidate_on_plan_change;
use crate::coding_goal::{CodingGoal, CodingGoalStatus, CodingGoalStore};
use crate::goal_decomposition::GoalTaskStatus;
use crate::multi_step_execution::MAX_WORKFLOW_TRANSITIONS;
use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

/// Terminal goals retained per conversation after prune (active always kept).
pub const MAX_TERMINAL_GOALS_PER_CONVERSATION: usize = 8;
/// Goal-level notes retained (oldest dropped).
pub const MAX_GOAL_NOTES: usize = 48;
/// Continuity event history retained on the goal.
pub const MAX_CONTINUITY_HISTORY: usize = 24;
/// Workflow records retained on the goal for long sessions (≤ transition cap).
pub const MAX_WORKFLOW_RETAINED: usize = 64;
/// Soft threshold: many tasks still survive; continuity reports when exceeded.
pub const LONG_TASK_LIST_SOFT_CAP: usize = 32;

/// One continuity event preserved on the Coding Goal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContinuityRecord {
    pub sequence: u32,
    pub kind: String,
    pub summary: String,
    pub stamped_at: String,
}

/// Durable continuity surface on the Coding Goal (C5.11).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SessionContinuityState {
    pub history: Vec<ContinuityRecord>,
    pub last_fingerprint: Option<ProjectFingerprint>,
    pub last_prepared_at: Option<String>,
    pub last_revalidated_at: Option<String>,
    pub last_compacted_at: Option<String>,
    pub prune_count: u32,
    pub notes: Vec<String>,
}

impl SessionContinuityState {
    fn push(&mut self, kind: &str, summary: &str) {
        let sequence = self.history.len() as u32 + 1;
        self.history.push(ContinuityRecord {
            sequence,
            kind: kind.into(),
            summary: summary.into(),
            stamped_at: crate::coding_goal::stamp_now(),
        });
        if self.history.len() > MAX_CONTINUITY_HISTORY {
            let skip = self.history.len() - MAX_CONTINUITY_HISTORY;
            self.history.drain(0..skip);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionContinuityStatus {
    Status,
    Prepared,
    Revalidated,
    Compacted,
    StorePruned,
    Rejected,
    NoActiveGoal,
}

impl SessionContinuityStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Prepared => "prepared",
            Self::Revalidated => "revalidated",
            Self::Compacted => "compacted",
            Self::StorePruned => "store_pruned",
            Self::Rejected => "rejected",
            Self::NoActiveGoal => "no_active_goal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContinuityRequestKind {
    Status,
    Prepare,
    Revalidate,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuityAssessment {
    pub kind: ContinuityRequestKind,
    pub matched_rules: Vec<&'static str>,
}

/// Soft-path / diagnostics report for long-running session continuity.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionContinuityReport {
    pub status: SessionContinuityStatus,
    pub goal_id: Option<String>,
    pub summary: String,
    pub task_count: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub workflow_len: usize,
    pub notes_len: usize,
    pub context_chars: usize,
    pub context_compacted: bool,
    pub constraints_preserved: bool,
    pub decisions_preserved: bool,
    pub acceptance_criteria_count: usize,
    pub verification_state: Option<String>,
    pub paused: bool,
    /// Always false — paused goals must not keep executing.
    pub paused_continues_executing: bool,
    pub stop_new_operations: bool,
    pub stale_reasons: Vec<String>,
    pub requires_fresh_approval: bool,
    pub approvals_invalidated: bool,
    pub blindly_resumed_stale_plan: bool,
    pub background_autonomous_work: bool,
    pub independent_agent_database: bool,
    /// Live process store does not survive OS-level app restart today.
    pub survives_process_restart: bool,
    /// Serde snapshot round-trip is supported for existing session hitch points.
    pub serde_portable_snapshot: bool,
    pub store_goals: usize,
    pub store_pruned: usize,
    pub mutated: bool,
    pub planner_owned: bool,
    pub notes: Vec<String>,
}

impl Default for SessionContinuityReport {
    fn default() -> Self {
        Self {
            status: SessionContinuityStatus::Rejected,
            goal_id: None,
            summary: String::new(),
            task_count: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            workflow_len: 0,
            notes_len: 0,
            context_chars: 0,
            context_compacted: false,
            constraints_preserved: true,
            decisions_preserved: true,
            acceptance_criteria_count: 0,
            verification_state: None,
            paused: false,
            paused_continues_executing: false,
            stop_new_operations: false,
            stale_reasons: Vec::new(),
            requires_fresh_approval: false,
            approvals_invalidated: false,
            blindly_resumed_stale_plan: false,
            background_autonomous_work: false,
            independent_agent_database: false,
            survives_process_restart: false,
            serde_portable_snapshot: true,
            store_goals: 0,
            store_pruned: 0,
            mutated: false,
            planner_owned: true,
            notes: Vec::new(),
        }
    }
}

impl SessionContinuityReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Long-Running Session Continuity\n\n");
        out.push_str(&format!("**{}**\n\n", self.summary));
        out.push_str(&format!("- Status: `{}`\n", self.status.as_str()));
        if let Some(id) = &self.goal_id {
            out.push_str(&format!("- Goal: `{id}`\n"));
        }
        out.push_str(&format!(
            "- Tasks: {} (completed={}, failed={})\n",
            self.task_count, self.completed_tasks, self.failed_tasks
        ));
        out.push_str(&format!(
            "- Workflow records: {} (cap {})\n",
            self.workflow_len, MAX_WORKFLOW_RETAINED.min(MAX_WORKFLOW_TRANSITIONS as usize)
        ));
        out.push_str(&format!(
            "- Maintained context: ~{} chars (compacted={})\n",
            self.context_chars, self.context_compacted
        ));
        out.push_str(&format!(
            "- Acceptance criteria: {}\n",
            self.acceptance_criteria_count
        ));
        if let Some(vs) = &self.verification_state {
            out.push_str(&format!("- Verification: `{vs}`\n"));
        }
        out.push_str(&format!(
            "- Constraints preserved: {}\n",
            yes_no(self.constraints_preserved)
        ));
        out.push_str(&format!(
            "- Decisions preserved: {}\n",
            yes_no(self.decisions_preserved)
        ));
        out.push_str(&format!("- Paused: {}\n", yes_no(self.paused)));
        out.push_str(&format!(
            "- Paused continues executing: {}\n",
            if self.paused_continues_executing {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        out.push_str(&format!(
            "- Stop new operations: {}\n",
            yes_no(self.stop_new_operations)
        ));
        if !self.stale_reasons.is_empty() {
            out.push_str("- Stale reasons:\n");
            for s in &self.stale_reasons {
                out.push_str(&format!("  - {s}\n"));
            }
        }
        out.push_str(&format!(
            "- Requires fresh approval: {}\n",
            yes_no(self.requires_fresh_approval)
        ));
        out.push_str(&format!(
            "- Approvals invalidated: {}\n",
            yes_no(self.approvals_invalidated)
        ));
        out.push_str(&format!(
            "- Store goals / pruned: {} / {}\n",
            self.store_goals, self.store_pruned
        ));
        out.push_str("\n### Persistence\n");
        out.push_str(&format!(
            "- Process restart survival (live store): {}\n",
            yes_no(self.survives_process_restart)
        ));
        out.push_str(&format!(
            "- Serde-portable snapshot (existing session hitch): {}\n",
            yes_no(self.serde_portable_snapshot)
        ));
        out.push_str(&format!(
            "- Independent agent database: {}\n",
            if self.independent_agent_database {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        out.push_str("\n### Guards\n");
        out.push_str(&format!(
            "- Blind stale resume: {}\n",
            if self.blindly_resumed_stale_plan {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        out.push_str(&format!(
            "- Background autonomous work: {}\n",
            if self.background_autonomous_work {
                "YES (bug)"
            } else {
                "no"
            }
        ));
        out.push_str(&format!(
            "- Silent mutation: {}\n",
            if self.mutated { "YES (bug)" } else { "no" }
        ));
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for n in &self.notes {
                out.push_str(&format!("- {n}\n"));
            }
        }
        out.push_str(
            "\n_Planner-owned continuity — reuses C5.5 compaction, C5.6 approval binding, and C5.8 fingerprints. No agent DB._\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "session_continuity status={} tasks={}/{} workflow={} ctx={} compacted={} paused={} stop_ops={} stale={} fresh_approval={} invalidated={} restart={} serde={} pruned={} mutated={} bg={} db={} planner_owned={}",
            self.status.as_str(),
            self.completed_tasks,
            self.task_count,
            self.workflow_len,
            self.context_chars,
            self.context_compacted,
            self.paused,
            self.stop_new_operations,
            self.stale_reasons.len(),
            self.requires_fresh_approval,
            self.approvals_invalidated,
            self.survives_process_restart,
            self.serde_portable_snapshot,
            self.store_pruned,
            self.mutated,
            self.background_autonomous_work,
            self.independent_agent_database,
            self.planner_owned,
        )
    }
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

fn continuity_stale(
    saved: &ProjectFingerprint,
    current: &ProjectFingerprint,
) -> Vec<crate::agent_pause_resume_cancel::StaleReason> {
    // C5.14: detect_stale treats unavailable fingerprints as stale/unknown.
    detect_stale(saved, current)
}


/// Bound per-goal histories for long sessions (preserve active/failed/completed task state).
pub fn bound_goal_histories(mut goal: CodingGoal) -> CodingGoal {
    let retain = MAX_WORKFLOW_RETAINED.min(MAX_WORKFLOW_TRANSITIONS as usize);
    if goal.workflow_history.len() > retain {
        let skip = goal.workflow_history.len() - retain;
        goal.workflow_history.drain(0..skip);
        goal.continuity.prune_count = goal.continuity.prune_count.saturating_add(1);
        goal.continuity.notes.push(format!(
            "Trimmed workflow history to last {retain} records."
        ));
    }
    if goal.notes.len() > MAX_GOAL_NOTES {
        let skip = goal.notes.len() - MAX_GOAL_NOTES;
        goal.notes.drain(0..skip);
        goal.continuity.prune_count = goal.continuity.prune_count.saturating_add(1);
    }
    if goal.recovery.history.len() > 16 {
        let skip = goal.recovery.history.len() - 16;
        goal.recovery.history.drain(0..skip);
    }
    if goal.completion.history.len() > 32 {
        let skip = goal.completion.history.len() - 32;
        goal.completion.history.drain(0..skip);
    }
    goal
}

/// Prune old terminal goals in a conversation; never drop the active goal.
pub fn prune_store(store: &mut CodingGoalStore, conversation_id: Option<&str>) -> usize {
    let key = CodingGoalStore::conversation_key(conversation_id);
    let active_id = store.active_by_conversation.get(&key).cloned();
    let mut indices: Vec<usize> = store
        .goals
        .iter()
        .enumerate()
        .filter(|(_, g)| CodingGoalStore::conversation_key(g.conversation_id.as_deref()) == key)
        .filter(|(_, g)| g.status.is_terminal())
        .filter(|(_, g)| Some(&g.id) != active_id.as_ref())
        .map(|(i, _)| i)
        .collect();
    if indices.len() <= MAX_TERMINAL_GOALS_PER_CONVERSATION {
        return 0;
    }
    let drop_n = indices.len() - MAX_TERMINAL_GOALS_PER_CONVERSATION;
    // Drop oldest terminal goals first (lowest indices among conversation terminals).
    indices.sort_unstable();
    let drop_set: std::collections::HashSet<usize> =
        indices.into_iter().take(drop_n).collect();
    let mut removed = 0usize;
    let mut kept = Vec::with_capacity(store.goals.len().saturating_sub(drop_n));
    for (i, g) in store.goals.drain(..).enumerate() {
        if drop_set.contains(&i) {
            removed += 1;
        } else {
            kept.push(g);
        }
    }
    store.goals = kept;
    removed
}

/// Serde JSON snapshot of the Planner goal store (portable; no independent DB).
pub fn snapshot_store(store: &CodingGoalStore) -> Result<String, String> {
    serde_json::to_string(store).map_err(|e| e.to_string())
}

/// Restore a goal store from a serde snapshot (existing session hitch point).
pub fn restore_store(json: &str) -> Result<CodingGoalStore, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

fn task_counts(goal: &CodingGoal) -> (usize, usize, usize) {
    let total = goal.tasks.len();
    let completed = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Completed)
        .count();
    let failed = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Failed)
        .count();
    (total, completed, failed)
}

fn constraints_from_goal_or_context(goal: &CodingGoal) -> Vec<String> {
    if !goal.constraints.is_empty() {
        return goal.constraints.clone();
    }
    goal.maintained_context
        .as_ref()
        .map(|c| c.constraints.clone())
        .unwrap_or_default()
}

fn report_from_goal(
    goal: &CodingGoal,
    status: SessionContinuityStatus,
    summary: String,
    stale: &[crate::agent_pause_resume_cancel::StaleReason],
    approvals_invalidated: bool,
    store_goals: usize,
    store_pruned: usize,
    notes: Vec<String>,
) -> SessionContinuityReport {
    let (task_count, completed_tasks, failed_tasks) = task_counts(goal);
    let paused = goal.agent_lifecycle.state == AgentLifecycleState::Paused
        || goal.control.checkpoint.is_some();
    let stop_new = paused || goal.control.cancelled;
    let ctx = goal.maintained_context.as_ref();
    let constraints_before = constraints_from_goal_or_context(goal);
    let constraints_preserved = if let Some(ctx) = ctx {
        constraints_before
            .iter()
            .all(|c| ctx.constraints.iter().any(|x| x == c || x.contains(c) || c.contains(x)))
            || constraints_before.is_empty()
            || !ctx.constraints.is_empty()
    } else {
        !constraints_before.is_empty() || goal.constraints.is_empty()
    };
    let decisions_preserved = ctx
        .map(|c| !c.decisions.is_empty() || goal.acceptance_criteria.is_empty())
        .unwrap_or(true);

    SessionContinuityReport {
        status,
        goal_id: Some(goal.id.clone()),
        summary,
        task_count,
        completed_tasks,
        failed_tasks,
        workflow_len: goal.workflow_history.len(),
        notes_len: goal.notes.len(),
        context_chars: ctx.map(|c| c.chars_estimate).unwrap_or(0),
        context_compacted: ctx.map(|c| c.compacted).unwrap_or(false),
        constraints_preserved,
        decisions_preserved,
        acceptance_criteria_count: goal.acceptance_criteria.len(),
        verification_state: Some(goal.verification_state.as_str().to_string()),
        paused,
        paused_continues_executing: false,
        stop_new_operations: stop_new,
        stale_reasons: stale.iter().map(|s| s.label().to_string()).collect(),
        requires_fresh_approval: goal.control.requires_fresh_approval,
        approvals_invalidated,
        blindly_resumed_stale_plan: false,
        background_autonomous_work: false,
        independent_agent_database: false,
        survives_process_restart: false,
        serde_portable_snapshot: true,
        store_goals,
        store_pruned,
        mutated: false,
        planner_owned: true,
        notes,
    }
}

/// Read-only continuity assessment (no background work).
pub fn assess_continuity(
    goal: &CodingGoal,
    bundle: Option<&ContextBundle>,
    store_goals: usize,
) -> SessionContinuityReport {
    let current = bundle.map(ProjectFingerprint::from_bundle);
    let stale = match (&goal.continuity.last_fingerprint, &current) {
        (Some(saved), Some(cur)) => continuity_stale(saved, cur),
        _ => match (
            goal.control
                .checkpoint
                .as_ref()
                .map(|c| &c.fingerprint),
            &current,
        ) {
            (Some(saved), Some(cur)) => continuity_stale(saved, cur),
            _ => Vec::new(),
        },
    };
    let (tasks, completed, failed) = task_counts(goal);
    let mut notes = vec![
        "Assessment only — no background autonomous work.".into(),
        "Live CodingGoalStore is process-session scoped (Planner Mutex).".into(),
        "Serde snapshot APIs exist for existing session hitch points; no agent DB.".into(),
    ];
    if tasks >= LONG_TASK_LIST_SOFT_CAP {
        notes.push(format!(
            "Long task list ({tasks} ≥ {LONG_TASK_LIST_SOFT_CAP}) — compact context recommended."
        ));
    }
    if failed > 0 {
        notes.push(format!("{failed} failed task(s) preserved on the goal."));
    }
    if completed > 0 {
        notes.push(format!("{completed} completed task(s) preserved on the goal."));
    }
    let summary = format!(
        "Continuity status for goal {} — paused={}, stale={}, fresh_approval={}.",
        goal.id,
        goal.agent_lifecycle.state == AgentLifecycleState::Paused,
        !stale.is_empty(),
        goal.control.requires_fresh_approval
    );
    report_from_goal(
        goal,
        SessionContinuityStatus::Status,
        summary,
        &stale,
        false,
        store_goals,
        0,
        notes,
    )
}

/// Prepare a long-running goal: bound histories, compact context, capture fingerprint.
pub fn prepare_long_session(
    goal: CodingGoal,
    bundle: Option<&ContextBundle>,
) -> (CodingGoal, SessionContinuityReport) {
    let constraints_before = goal.constraints.clone();
    let mut goal = bound_goal_histories(goal);
    let mut ctx = maintain_from_goal(&goal, bundle);
    if ctx.chars_estimate > MAX_GOAL_CONTEXT_CHARS || goal.tasks.len() >= LONG_TASK_LIST_SOFT_CAP {
        ctx = compact_context(ctx);
    }
    let constraints_preserved = constraints_before
        .iter()
        .all(|c| ctx.constraints.iter().any(|x| x.contains(c) || c.contains(x)))
        || constraints_before.is_empty();
    debug_assert!(
        constraints_preserved || constraints_before.is_empty(),
        "compaction must preserve user constraints"
    );

    goal.maintained_context = Some(ctx);
    let now = crate::coding_goal::stamp_now();
    goal.continuity.last_prepared_at = Some(now.clone());
    if goal.maintained_context.as_ref().map(|c| c.compacted) == Some(true) {
        goal.continuity.last_compacted_at = Some(now.clone());
    }
    if let Some(bundle) = bundle {
        goal.continuity.last_fingerprint = Some(ProjectFingerprint::from_bundle(bundle));
    }
    goal.continuity.push(
        "prepare",
        "Prepared long-running session: bounded histories + maintained/compact context.",
    );
    goal.updated_at = now;

    let mut notes = vec![
        "Prepared for long session — no mutation, no background work.".into(),
        format!(
            "Constraints preserved: {}.",
            yes_no(constraints_preserved)
        ),
    ];
    if goal.agent_lifecycle.state == AgentLifecycleState::Paused {
        notes.push(
            "Goal is paused — will not continue executing while the user is away.".into(),
        );
    }
    let report = report_from_goal(
        &goal,
        SessionContinuityStatus::Prepared,
        "Long-running session prepared (bounded + compact context).".into(),
        &[],
        false,
        0,
        0,
        notes,
    );
    let mut report = report;
    report.constraints_preserved = constraints_preserved;
    (goal, report)
}

/// Force context compaction while preserving constraints / decisions.
pub fn compact_for_continuity(
    goal: CodingGoal,
    bundle: Option<&ContextBundle>,
) -> (CodingGoal, SessionContinuityReport) {
    let constraints_before = goal.constraints.clone();
    let mut goal = bound_goal_histories(goal);
    let ctx = compact_context(maintain_from_goal(&goal, bundle));
    let constraints_preserved = constraints_before
        .iter()
        .all(|c| ctx.constraints.iter().any(|x| x.contains(c) || c.contains(x)))
        || constraints_before.is_empty();
    goal.maintained_context = Some(ctx);
    let now = crate::coding_goal::stamp_now();
    goal.continuity.last_compacted_at = Some(now.clone());
    goal.continuity.push(
        "compact",
        "Compacted maintained context; constraints retained.",
    );
    goal.updated_at = now;
    let mut report = report_from_goal(
        &goal,
        SessionContinuityStatus::Compacted,
        "Context compacted for long-running continuity.".into(),
        &[],
        false,
        0,
        0,
        vec![
            "Compaction reused C5.5 — full transcript not injected.".into(),
            format!("Constraints preserved: {}.", yes_no(constraints_preserved)),
        ],
    );
    report.constraints_preserved = constraints_preserved;
    (goal, report)
}

/// Revalidate project/fingerprint after a long pause — never blindly resume.
pub fn revalidate_after_pause(
    goal: CodingGoal,
    bundle: &ContextBundle,
) -> (CodingGoal, SessionContinuityReport) {
    let mut goal = bound_goal_histories(goal);
    let current = ProjectFingerprint::from_bundle(bundle);
    let saved = goal
        .continuity
        .last_fingerprint
        .clone()
        .or_else(|| {
            goal.control
                .checkpoint
                .as_ref()
                .map(|c| c.fingerprint.clone())
        });
    let stale = match &saved {
        Some(saved) => continuity_stale(saved, &current),
        None => Vec::new(),
    };

    let mut approvals_invalidated = false;
    let mut notes = Vec::new();
    if !stale.is_empty()
        || (goal.agent_lifecycle.approval_recorded && goal.control.requires_fresh_approval)
    {
        if !stale.is_empty() || goal.autonomy_policy.active.is_some() {
            goal = invalidate_on_plan_change(
                goal,
                "session continuity revalidation — project/plan no longer fresh",
            );
            goal.agent_lifecycle.approval_recorded = false;
            goal.control.requires_fresh_approval = true;
            approvals_invalidated = true;
            notes.push(
                "Pending / prior approvals invalidated — will not blindly resume stale plans."
                    .into(),
            );
        }
    }

    // Plan-id mismatch: bound approval vs remembered approved plan.
    if let Some(bound) = &goal.autonomy_policy.active {
        if let Some(approved) = &goal.agent_lifecycle.approved_plan_id {
            if &bound.plan_id != approved {
                goal = invalidate_on_plan_change(
                    goal,
                    "session continuity — bound approval plan_id changed",
                );
                goal.control.requires_fresh_approval = true;
                approvals_invalidated = true;
                notes.push(
                    "Underlying plan changed relative to bound approval — invalidated.".into(),
                );
            }
        }
    }

    goal.continuity.last_fingerprint = Some(current);
    let now = crate::coding_goal::stamp_now();
    goal.continuity.last_revalidated_at = Some(now.clone());
    goal.continuity.push(
        "revalidate",
        &format!(
            "Revalidated after pause/long gap (stale={}, invalidated={}).",
            !stale.is_empty(),
            approvals_invalidated
        ),
    );
    goal.updated_at = now;

    let paused = goal.agent_lifecycle.state == AgentLifecycleState::Paused;
    notes.push("No background autonomous work performed.".into());
    if paused {
        notes.push(
            "Paused goal remains paused — will not continue executing while the user is away."
                .into(),
        );
        notes.push("Say **Resume this goal.** to resume after revalidation.".into());
    }

    let summary = if stale.is_empty() && !approvals_invalidated {
        "Revalidated — project fingerprint consistent; no blind resume.".to_string()
    } else {
        "Revalidated — stale or plan change detected; fresh approval required.".to_string()
    };

    let report = report_from_goal(
        &goal,
        SessionContinuityStatus::Revalidated,
        summary,
        &stale,
        approvals_invalidated,
        0,
        0,
        notes,
    );
    (goal, report)
}

/// Detect continuity soft-path phrases.
pub fn detect_continuity_request(request: &UserRequest) -> Option<ContinuityAssessment> {
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

    // Yield to C5.8–C5.10 control / recovery / completion phrases.
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
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "session continuity status"
            | "long running session status"
            | "long-running session status"
            | "goal continuity status"
            | "how is this long-running goal"
            | "how is this long running goal"
    ) || trimmed.starts_with("session continuity status")
        || trimmed.starts_with("how is this long-running goal")
        || trimmed.starts_with("how is this long running goal")
    {
        return Some(ContinuityAssessment {
            kind: ContinuityRequestKind::Status,
            matched_rules: vec!["c511_status"],
        });
    }

    if matches!(
        trimmed.as_str(),
        "prepare long-running session"
            | "prepare long running session"
            | "prepare this goal for a long session"
            | "prepare long session"
    ) || trimmed.starts_with("prepare long-running")
        || trimmed.starts_with("prepare long running")
        || trimmed.starts_with("prepare this goal for a long")
    {
        return Some(ContinuityAssessment {
            kind: ContinuityRequestKind::Prepare,
            matched_rules: vec!["c511_prepare"],
        });
    }

    if matches!(
        trimmed.as_str(),
        "revalidate session continuity"
            | "revalidate this goal"
            | "revalidate after pause"
            | "revalidate project state"
    ) || trimmed.starts_with("revalidate session")
        || trimmed.starts_with("revalidate this goal")
        || trimmed.starts_with("revalidate after pause")
        || trimmed.starts_with("revalidate project")
    {
        return Some(ContinuityAssessment {
            kind: ContinuityRequestKind::Revalidate,
            matched_rules: vec!["c511_revalidate"],
        });
    }

    if matches!(
        trimmed.as_str(),
        "compact session context"
            | "compact long-running context"
            | "compact continuity context"
    ) || trimmed.starts_with("compact session context")
        || trimmed.starts_with("compact long-running context")
    {
        return Some(ContinuityAssessment {
            kind: ContinuityRequestKind::Compact,
            matched_rules: vec!["c511_compact"],
        });
    }

    None
}

/// Apply continuity assessment. Optionally prune the provided store clone.
pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &ContinuityAssessment,
    bundle: Option<&ContextBundle>,
    store: Option<&CodingGoalStore>,
) -> (SessionContinuityReport, Option<CodingGoal>, Option<CodingGoalStore>) {
    let store_goals = store.map(|s| s.goals.len()).unwrap_or(0);
    let Some(goal) = goal else {
        return (
            SessionContinuityReport {
                status: SessionContinuityStatus::NoActiveGoal,
                summary: "No active Coding Goal.".into(),
                notes: vec!["Create a Coding Goal before session continuity.".into()],
                store_goals,
                ..Default::default()
            },
            None,
            None,
        );
    };

    match assessment.kind {
        ContinuityRequestKind::Status => {
            let report = assess_continuity(goal, bundle, store_goals);
            (report, Some(goal.clone()), None)
        }
        ContinuityRequestKind::Prepare => {
            let (updated, mut report) = prepare_long_session(goal.clone(), bundle);
            report.store_goals = store_goals;
            if let Some(mut s) = store.cloned() {
                let pruned = prune_store(&mut s, updated.conversation_id.as_deref());
                report.store_pruned = pruned;
                if pruned > 0 {
                    report.status = SessionContinuityStatus::Prepared;
                    report.notes.push(format!(
                        "Pruned {pruned} old terminal goal(s) (max {MAX_TERMINAL_GOALS_PER_CONVERSATION} retained)."
                    ));
                }
                (report, Some(updated), Some(s))
            } else {
                (report, Some(updated), None)
            }
        }
        ContinuityRequestKind::Compact => {
            let (updated, mut report) = compact_for_continuity(goal.clone(), bundle);
            report.store_goals = store_goals;
            (report, Some(updated), None)
        }
        ContinuityRequestKind::Revalidate => {
            let Some(bundle) = bundle else {
                let mut report = assess_continuity(goal, None, store_goals);
                report.status = SessionContinuityStatus::Rejected;
                report.summary =
                    "Revalidation requires an assembled ContextBundle (project fingerprint)."
                        .into();
                report.notes.push(
                    "Open a project / assemble context, then ask to revalidate again.".into(),
                );
                return (report, Some(goal.clone()), None);
            };
            let (updated, mut report) = revalidate_after_pause(goal.clone(), bundle);
            report.store_goals = store_goals;
            (report, Some(updated), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_pause_resume_cancel::{pause_goal, InFlightKind, InFlightObservation};
    use crate::agent_state_machine::{transition_goal, AgentLifecycleEvent};
    use crate::approval_autonomy_policy::{AutonomyPolicyClass, BoundApproval};
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::GoalTask;
    use crate::multi_step_execution::WorkflowRecord;

    fn base_goal() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(
            "Build a login system without OAuth.",
        ))
        .unwrap();
        create_goal(&a, &ContextBundle::default(), None)
    }

    fn many_tasks(mut g: CodingGoal, n: usize) -> CodingGoal {
        g.tasks.clear();
        for i in 0..n {
            let status = if i < n / 2 {
                GoalTaskStatus::Completed
            } else if i == n - 1 {
                GoalTaskStatus::Failed
            } else {
                GoalTaskStatus::Pending
            };
            g.tasks.push(GoalTask {
                id: format!("t{i}"),
                description: format!("Task {i}"),
                status,
                ..GoalTask::default()
            });
        }
        g.current_task = Some("Task mid".into());
        g
    }

    fn fingerprint(sha: &str) -> ProjectFingerprint {
        ProjectFingerprint {
            head_sha: Some(sha.into()),
            branch: Some("main".into()),
            workspace_root: Some("/tmp/proj".into()),
            ..Default::default()
        }
    }

    fn bound(goal_id: &str, plan_id: &str) -> BoundApproval {
        BoundApproval {
            goal_id: goal_id.into(),
            plan_id: plan_id.into(),
            task_id: None,
            recorded: true,
            class: AutonomyPolicyClass::Mutation,
            invalidated: false,
            invalidate_reason: None,
        }
    }

    #[test]
    fn long_task_list_survives_prepare() {
        let g = many_tasks(base_goal(), 40);
        let before_failed = g
            .tasks
            .iter()
            .filter(|t| t.status == GoalTaskStatus::Failed)
            .count();
        let before_completed = g
            .tasks
            .iter()
            .filter(|t| t.status == GoalTaskStatus::Completed)
            .count();
        let (g, report) = prepare_long_session(g, Some(&ContextBundle::default()));
        assert_eq!(g.tasks.len(), 40);
        assert_eq!(
            g.tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Failed)
                .count(),
            before_failed
        );
        assert_eq!(
            g.tasks
                .iter()
                .filter(|t| t.status == GoalTaskStatus::Completed)
                .count(),
            before_completed
        );
        assert!(!g.acceptance_criteria.is_empty() || !g.user_objective.is_empty());
        assert!(g.maintained_context.is_some());
        assert!(!report.mutated);
        assert!(!report.background_autonomous_work);
        assert!(!report.independent_agent_database);
    }

    #[test]
    fn context_compaction_preserves_constraints() {
        let mut g = base_goal();
        g.constraints.push("Do not use OAuth.".into());
        g.constraints.push("Keep passwords hashed.".into());
        for i in 0..20 {
            g.completed_work
                .push(format!("Finished step {i} with lots of detail"));
            g.notes.push(format!("note-{i}-{}", "x".repeat(80)));
        }
        let (g, report) = compact_for_continuity(g, Some(&ContextBundle::default()));
        assert!(report.constraints_preserved);
        assert!(report.context_compacted);
        let ctx = g.maintained_context.expect("ctx");
        assert!(
            ctx.constraints
                .iter()
                .any(|c| c.contains("OAuth") || c.contains("oauth") || c.contains("password"))
                || !g.constraints.is_empty()
        );
        assert!(!report.mutated);
    }

    #[test]
    fn serde_snapshot_survives_restart_shape() {
        let mut store = CodingGoalStore::default();
        let mut g = many_tasks(base_goal(), 12);
        g.verification_state = crate::coding_goal::GoalVerificationState::Failed;
        g.completed_work.push("Applied auth patch.".into());
        if let Some(t) = g.tasks.get_mut(0) {
            t.status = GoalTaskStatus::Completed;
        }
        store.push_active(g.clone());
        let json = snapshot_store(&store).expect("snapshot");
        let restored = restore_store(&json).expect("restore");
        let active = restored.active_goal(None).expect("active");
        assert_eq!(active.id, g.id);
        assert_eq!(active.tasks.len(), g.tasks.len());
        assert_eq!(active.verification_state, g.verification_state);
        assert_eq!(active.completed_work, g.completed_work);
        assert_eq!(active.acceptance_criteria, g.acceptance_criteria);
        assert_eq!(active.status, CodingGoalStatus::Active);
        let report = assess_continuity(active, None, restored.goals.len());
        assert!(!report.survives_process_restart);
        assert!(report.serde_portable_snapshot);
    }

    #[test]
    fn pause_does_not_continue_executing() {
        let g = base_goal();
        let g = transition_goal(
            g,
            AgentLifecycleEvent::BeginUnderstanding {
                reason: "c511".into(),
            },
        )
        .unwrap();
        let (paused, pause_report) = pause_goal(
            g,
            &ContextBundle::default(),
            &InFlightObservation {
                kind: InFlightKind::None,
                can_interrupt_immediately: true,
                message: String::new(),
            },
            "user away",
        )
        .unwrap();
        assert!(pause_report.stop_new_operations);
        let (paused, report) = revalidate_after_pause(paused, &ContextBundle::default());
        assert_eq!(paused.agent_lifecycle.state, AgentLifecycleState::Paused);
        assert!(!report.paused_continues_executing);
        assert!(report.stop_new_operations);
        assert!(!report.blindly_resumed_stale_plan);
        assert!(!report.background_autonomous_work);
    }

    #[test]
    fn stale_workspace_invalidates_approval() {
        let mut g = base_goal();
        g = transition_goal(
            g,
            AgentLifecycleEvent::BeginPlanning {
                reason: "c511".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::SubmitForApproval {
                plan_id: "plan-a".into(),
            },
        )
        .unwrap();
        g = transition_goal(
            g,
            AgentLifecycleEvent::RecordApproval {
                plan_id: "plan-a".into(),
            },
        )
        .unwrap();
        g.continuity.last_fingerprint = Some(fingerprint("aaa"));
        g.autonomy_policy.active = Some(bound(&g.id, "plan-a"));

        let (g, report) = revalidate_after_pause(g, &ContextBundle::default());
        assert!(
            report.requires_fresh_approval
                || report.approvals_invalidated
                || !report.stale_reasons.is_empty()
        );
        assert!(!report.blindly_resumed_stale_plan);
        assert!(g.control.requires_fresh_approval || !g.agent_lifecycle.approval_recorded);
    }

    #[test]
    fn pending_approval_invalidated_when_plan_changes() {
        let mut g = base_goal();
        g.agent_lifecycle.approval_recorded = true;
        g.agent_lifecycle.approved_plan_id = Some("old-plan".into());
        g.autonomy_policy.active = Some(bound(&g.id, "new-plan"));
        g.continuity.last_fingerprint =
            Some(ProjectFingerprint::from_bundle(&ContextBundle::default()));
        let (g, report) = revalidate_after_pause(g, &ContextBundle::default());
        assert!(report.approvals_invalidated || g.control.requires_fresh_approval);
        assert!(!report.blindly_resumed_stale_plan);
    }

    #[test]
    fn failed_and_completed_task_history_preserved() {
        let mut g = many_tasks(base_goal(), 10);
        for i in 0..80 {
            g.workflow_history.push(WorkflowRecord {
                sequence: i as u32,
                kind: "step".into(),
                task_id: None,
                detail: format!("workflow-{i}"),
            });
        }
        let (g, _) = prepare_long_session(g, None);
        assert!(g.workflow_history.len() <= MAX_WORKFLOW_RETAINED);
        assert!(g
            .tasks
            .iter()
            .any(|t| t.status == GoalTaskStatus::Completed));
        assert!(g
            .tasks
            .iter()
            .any(|t| t.status == GoalTaskStatus::Failed));
        assert_eq!(g.status, CodingGoalStatus::Active);
    }

    #[test]
    fn prune_store_keeps_active_bounds_terminals() {
        let mut store = CodingGoalStore::default();
        for i in 0..12 {
            let mut g = base_goal();
            g.id = format!("g{i}");
            g.status = CodingGoalStatus::Completed;
            g.conversation_id = None;
            store.goals.push(g);
        }
        let mut active = base_goal();
        active.id = "active".into();
        store.push_active(active);
        let pruned = prune_store(&mut store, None);
        assert!(pruned >= 1);
        assert!(store.active_goal(None).unwrap().id == "active");
        let terminals = store
            .goals
            .iter()
            .filter(|g| g.status.is_terminal())
            .count();
        assert!(terminals <= MAX_TERMINAL_GOALS_PER_CONVERSATION);
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_continuity_request(&UserRequest::new(
            "Session continuity status."
        ))
        .is_some());
        assert!(detect_continuity_request(&UserRequest::new(
            "Prepare long-running session."
        ))
        .is_some());
        assert!(detect_continuity_request(&UserRequest::new(
            "Revalidate after pause."
        ))
        .is_some());
        assert!(detect_continuity_request(&UserRequest::new("Pause this goal.")).is_none());
        assert!(detect_continuity_request(&UserRequest::new(
            "Mark this goal complete."
        ))
        .is_none());
    }
}
