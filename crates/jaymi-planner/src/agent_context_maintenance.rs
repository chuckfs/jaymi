//! Agent Context Maintenance (Sprint C5.5).
//!
//! Keep a multi-step Coding Goal coherent for model requests **without** stuffing
//! the entire execution history into every turn.
//!
//! ```text
//! CodingGoal (Planner-owned)
//!   → MaintainedAgentContext (compact view)
//!   → ContextEngine::assemble_with (sole ContextBundle factory)
//!   → LlmContext::from_bundle + extension (no AgentContextEngine)
//! ```
//!
//! Constitutional constraints:
//! - Goal state remains Planner-owned
//! - ContextEngine remains the sole ContextBundle factory
//! - Workspace Intelligence / session snapshots remain the observation source
//! - Do not duplicate WorkspaceSnapshot / ProjectSnapshot / GitSnapshot
//! - Prefer ContextPolicy-selected evidence from the assembled bundle
//! - Never inject the full workflow / transcript history
//! - Compaction must retain user constraints + important decisions
//! - Failed verification evidence stays available for Correcting
//! - No project file mutation; no autonomous Memory Engine writes
//! - Ordinary C2/C3/C4 conversations unchanged when no active Coding Goal

use crate::agent_state_machine::AgentLifecycleState;
use crate::coding_goal::{CodingGoal, CodingGoalStatus, GoalVerificationState};
use crate::goal_decomposition::GoalTaskStatus;
use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

/// Soft budget for the goal-maintenance extension (fits inside ContextEngine’s
/// larger character envelope).
pub const MAX_GOAL_CONTEXT_CHARS: usize = 4_000;
/// Completed tasks kept as short summaries (oldest dropped first when compacting).
pub const MAX_COMPLETED_TASK_SUMMARIES: usize = 8;
/// Workflow history entries summarized into the compact view.
pub const MAX_WORKFLOW_SUMMARY_ENTRIES: usize = 12;
/// Architectural / user decisions retained across compaction.
pub const MAX_DECISION_ENTRIES: usize = 8;
/// Failure / verification-failure records retained for correction.
pub const MAX_FAILURE_ENTRIES: usize = 6;
pub const MAX_RELEVANT_FILES: usize = 16;
pub const MAX_RELEVANT_SYMBOLS: usize = 16;
pub const MAX_CONSTRAINT_CHARS: usize = 240;
pub const LINE_COMPACT_CHARS: usize = 160;

pub const LLM_EXTENSION_KEY: &str = "agent_goal_context";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentContextRequestKind {
    Maintain,
    Show,
    Compact,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContextAssessment {
    pub kind: AgentContextRequestKind,
    pub matched_rules: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextStatus {
    Maintained,
    Compacted,
    Status,
    NoActiveGoal,
    Rejected,
}

impl AgentContextStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Maintained => "maintained",
            Self::Compacted => "compacted",
            Self::Status => "status",
            Self::NoActiveGoal => "no_active_goal",
            Self::Rejected => "rejected",
        }
    }
}

/// One recoverable decision (constraint-adjacent; never silently dropped).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    pub summary: String,
    pub source: String,
}

/// Compact completed-task line (not full workflow dump).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompletedTaskSummary {
    pub task_id: String,
    pub summary: String,
}

/// Failure / verification failure retained for correction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FailureRecord {
    pub kind: String,
    pub detail: String,
    pub task_id: Option<String>,
}

/// Current-task slice — highest priority in the maintained view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CurrentTaskSlice {
    pub task_id: String,
    pub description: String,
    pub status: String,
    pub mutation_expected: bool,
    pub relevant_files: Vec<String>,
    pub reuse_hints: Vec<String>,
}

/// Planner-owned compact goal context for model requests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MaintainedAgentContext {
    pub goal_id: String,
    pub objective: String,
    pub goal_status: String,
    pub agent_lifecycle: String,
    /// User constraints — **never** dropped by compaction.
    pub constraints: Vec<String>,
    /// Important decisions — recoverable across compaction.
    pub decisions: Vec<DecisionRecord>,
    pub current_task: Option<CurrentTaskSlice>,
    pub completed_summaries: Vec<CompletedTaskSummary>,
    pub pending_task_ids: Vec<String>,
    pub failures: Vec<FailureRecord>,
    pub coding_plan_ref: Option<String>,
    pub approved_ops_refs: Vec<String>,
    pub execution_summary_refs: Vec<String>,
    pub verification_refs: Vec<String>,
    pub relevant_files: Vec<String>,
    pub relevant_symbols: Vec<String>,
    /// Lightweight workspace labels from the assembled bundle (not full snapshots).
    pub workspace_refresh: Option<String>,
    pub chars_estimate: usize,
    pub compacted: bool,
    pub notes: Vec<String>,
}

impl Default for MaintainedAgentContext {
    fn default() -> Self {
        Self {
            goal_id: String::new(),
            objective: String::new(),
            goal_status: CodingGoalStatus::Active.as_str().into(),
            agent_lifecycle: AgentLifecycleState::Idle.as_str().into(),
            constraints: Vec::new(),
            decisions: Vec::new(),
            current_task: None,
            completed_summaries: Vec::new(),
            pending_task_ids: Vec::new(),
            failures: Vec::new(),
            coding_plan_ref: None,
            approved_ops_refs: Vec::new(),
            execution_summary_refs: Vec::new(),
            verification_refs: Vec::new(),
            relevant_files: Vec::new(),
            relevant_symbols: Vec::new(),
            workspace_refresh: None,
            chars_estimate: 0,
            compacted: false,
            notes: Vec::new(),
        }
    }
}

impl MaintainedAgentContext {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Maintained Agent Context\n");
        out.push_str(
            "\n_Compact goal state for coherent multi-step work — not a full transcript, not a second ContextBundle._\n",
        );
        out.push_str(&format!(
            "\n### Goal\n- ID: `{}`\n- Status: **{}** · lifecycle: **{}**\n- Objective: {}\n",
            self.goal_id, self.goal_status, self.agent_lifecycle, self.objective
        ));
        out.push_str(&format!(
            "\n### Budget\n- chars≈{} / {} · compacted={}\n",
            self.chars_estimate, MAX_GOAL_CONTEXT_CHARS, self.compacted
        ));
        if !self.constraints.is_empty() {
            out.push_str("\n### Constraints (retained)\n");
            for c in &self.constraints {
                out.push_str("- ");
                out.push_str(c);
                out.push('\n');
            }
        }
        if let Some(task) = &self.current_task {
            out.push_str("\n### Current task (priority)\n");
            out.push_str(&format!(
                "- `{}` — {} ({})\n",
                task.task_id, task.description, task.status
            ));
        }
        if !self.completed_summaries.is_empty() {
            out.push_str("\n### Completed (summarized)\n");
            for s in &self.completed_summaries {
                out.push_str(&format!("- `{}`: {}\n", s.task_id, s.summary));
            }
        }
        if !self.pending_task_ids.is_empty() {
            out.push_str(&format!(
                "\n### Pending\n- {}\n",
                self.pending_task_ids.join(", ")
            ));
        }
        if !self.failures.is_empty() {
            out.push_str("\n### Failures (retained for correction)\n");
            for f in &self.failures {
                out.push_str(&format!("- [{}] {}\n", f.kind, f.detail));
            }
        }
        if !self.decisions.is_empty() {
            out.push_str("\n### Decisions (recoverable)\n");
            for d in &self.decisions {
                out.push_str(&format!("- {} ({})\n", d.summary, d.source));
            }
        }
        if let Some(ws) = &self.workspace_refresh {
            out.push_str(&format!("\n### Workspace (refreshed labels)\n- {ws}\n"));
        }
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
            "agent_context goal={} lifecycle={} current={:?} completed={} failures={} chars={} compacted={} constraints={}",
            self.goal_id,
            self.agent_lifecycle,
            self.current_task.as_ref().map(|t| t.task_id.as_str()),
            self.completed_summaries.len(),
            self.failures.len(),
            self.chars_estimate,
            self.compacted,
            self.constraints.len(),
        )
    }

    pub fn to_extension_value(&self) -> serde_json::Value {
        serde_json::json!({
            "goal_id": self.goal_id,
            "objective": self.objective,
            "goal_status": self.goal_status,
            "agent_lifecycle": self.agent_lifecycle,
            "constraints": self.constraints,
            "decisions": self.decisions.iter().map(|d| {
                serde_json::json!({"summary": d.summary, "source": d.source})
            }).collect::<Vec<_>>(),
            "current_task": self.current_task.as_ref().map(|t| serde_json::json!({
                "task_id": t.task_id,
                "description": t.description,
                "status": t.status,
                "mutation_expected": t.mutation_expected,
                "relevant_files": t.relevant_files,
            })),
            "completed_summaries": self.completed_summaries.iter().map(|s| {
                serde_json::json!({"task_id": s.task_id, "summary": s.summary})
            }).collect::<Vec<_>>(),
            "pending_task_ids": self.pending_task_ids,
            "failures": self.failures.iter().map(|f| {
                serde_json::json!({"kind": f.kind, "detail": f.detail, "task_id": f.task_id})
            }).collect::<Vec<_>>(),
            "coding_plan_ref": self.coding_plan_ref,
            "approved_ops_refs": self.approved_ops_refs,
            "execution_summary_refs": self.execution_summary_refs,
            "verification_refs": self.verification_refs,
            "relevant_files": self.relevant_files,
            "relevant_symbols": self.relevant_symbols,
            "workspace_refresh": self.workspace_refresh,
            "chars_estimate": self.chars_estimate,
            "compacted": self.compacted,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentContextReport {
    pub status: AgentContextStatus,
    pub context: Option<MaintainedAgentContext>,
    pub notes: Vec<String>,
    pub mutated: bool,
    pub tools_executed: bool,
    pub memory_written: bool,
    pub duplicate_assembler: bool,
}

impl Default for AgentContextReport {
    fn default() -> Self {
        Self {
            status: AgentContextStatus::Rejected,
            context: None,
            notes: Vec::new(),
            mutated: false,
            tools_executed: false,
            memory_written: false,
            duplicate_assembler: false,
        }
    }
}

impl AgentContextReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Agent Context Maintenance\n");
        out.push_str(&format!("\n### Status\n- **{}**\n", self.status.as_str()));
        out.push_str(&format!(
            "\n### Safety\n- mutated={} · tools_executed={} · memory_written={} · duplicate_assembler={}\n",
            self.mutated, self.tools_executed, self.memory_written, self.duplicate_assembler
        ));
        if let Some(ctx) = &self.context {
            out.push_str("\n");
            out.push_str(&ctx.to_markdown());
        }
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
            "agent_context_maintenance status={} mutated={} memory_written={} duplicate_assembler={} {}",
            self.status.as_str(),
            self.mutated,
            self.memory_written,
            self.duplicate_assembler,
            self.context
                .as_ref()
                .map(|c| c.to_diagnostics_summary())
                .unwrap_or_else(|| "context=—".into()),
        )
    }
}

fn compact_line(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= LINE_COMPACT_CHARS {
        return trimmed.to_string();
    }
    trimmed.chars().take(LINE_COMPACT_CHARS).collect::<String>() + "…"
}

fn estimate_chars(ctx: &MaintainedAgentContext) -> usize {
    let mut n = ctx.objective.len() + ctx.goal_id.len() + 64;
    for c in &ctx.constraints {
        n += c.len() + 2;
    }
    for d in &ctx.decisions {
        n += d.summary.len() + d.source.len() + 4;
    }
    if let Some(t) = &ctx.current_task {
        n += t.description.len() + t.task_id.len() + 32;
        for f in &t.relevant_files {
            n += f.len() + 1;
        }
    }
    for s in &ctx.completed_summaries {
        n += s.summary.len() + s.task_id.len() + 4;
    }
    for f in &ctx.failures {
        n += f.detail.len() + f.kind.len() + 4;
    }
    for f in &ctx.relevant_files {
        n += f.len() + 1;
    }
    for s in &ctx.relevant_symbols {
        n += s.len() + 1;
    }
    n += ctx.workspace_refresh.as_ref().map(|s| s.len()).unwrap_or(0);
    for r in &ctx.approved_ops_refs {
        n += r.len() + 1;
    }
    for r in &ctx.execution_summary_refs {
        n += r.len() + 1;
    }
    for r in &ctx.verification_refs {
        n += r.len() + 1;
    }
    n
}

fn prefer_policy_paths(goal_paths: &[String], bundle: Option<&ContextBundle>) -> Vec<String> {
    let Some(bundle) = bundle else {
        return goal_paths
            .iter()
            .take(MAX_RELEVANT_FILES)
            .cloned()
            .collect();
    };
    let mut selected = Vec::new();
    if let Some(path) = bundle.current_file().path.clone() {
        if !path.is_empty() {
            selected.push(path);
        }
    }
    for open in bundle.open_files().files.iter().take(8) {
        if !open.path.is_empty() && !selected.iter().any(|p| p == &open.path) {
            selected.push(open.path.clone());
        }
    }
    for summary in bundle.file_summaries().entries.iter().take(8) {
        if !summary.path.is_empty() && !selected.iter().any(|p| p == &summary.path) {
            selected.push(summary.path.clone());
        }
    }
    for path in goal_paths {
        if !selected.iter().any(|p| p == path) {
            selected.push(path.clone());
        }
        if selected.len() >= MAX_RELEVANT_FILES {
            break;
        }
    }
    selected.truncate(MAX_RELEVANT_FILES);
    selected
}

fn workspace_refresh_label(bundle: Option<&ContextBundle>) -> Option<String> {
    let bundle = bundle?;
    let mut parts = Vec::new();
    if let Some(kind) = bundle.workspace_kind() {
        parts.push(format!("workspace_kind={kind}"));
    }
    let project = bundle.active_project();
    if let Some(name) = project.name.clone().filter(|s| !s.is_empty()) {
        parts.push(format!("project={name}"));
    } else if let Some(root) = project.root_directory.clone().filter(|s| !s.is_empty()) {
        parts.push(format!("project_root={root}"));
    }
    let git = bundle.git_status();
    if git.branch.is_some() || git.modified_count > 0 || !git.summary.is_empty() {
        parts.push(format!(
            "git={}{}",
            git.branch.as_deref().unwrap_or("—"),
            if git.modified_count > 0 || !git.dirty_paths.is_empty() {
                " (dirty)"
            } else {
                ""
            }
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn extract_decisions(goal: &CodingGoal) -> Vec<DecisionRecord> {
    let mut out = Vec::new();
    for c in &goal.constraints {
        out.push(DecisionRecord {
            summary: compact_line(c),
            source: "constraint".into(),
        });
    }
    for ac in &goal.acceptance_criteria {
        out.push(DecisionRecord {
            summary: compact_line(&ac.text),
            source: format!("acceptance:{}", ac.source.as_str()),
        });
    }
    for note in &goal.notes {
        let lower = note.to_ascii_lowercase();
        if lower.contains("decision")
            || lower.contains("architecture")
            || lower.contains("chose")
            || lower.contains("agreed")
        {
            out.push(DecisionRecord {
                summary: compact_line(note),
                source: "goal_note".into(),
            });
        }
    }
    for rec in goal.workflow_history.iter().rev() {
        if rec.kind.contains("decision") || rec.detail.to_ascii_lowercase().contains("decid") {
            out.push(DecisionRecord {
                summary: compact_line(&rec.detail),
                source: format!("workflow:{}", rec.kind),
            });
        }
        if out.len() >= MAX_DECISION_ENTRIES * 2 {
            break;
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(d.summary.clone()));
    out.truncate(MAX_DECISION_ENTRIES);
    out
}

fn extract_failures(goal: &CodingGoal) -> Vec<FailureRecord> {
    let mut out = Vec::new();
    for rec in goal.workflow_history.iter().rev() {
        let failish = rec.kind.contains("fail")
            || rec.kind.contains("verify_fail")
            || rec.kind == "apply_failure";
        if failish {
            out.push(FailureRecord {
                kind: rec.kind.clone(),
                detail: compact_line(&rec.detail),
                task_id: rec.task_id.clone(),
            });
        }
        if out.len() >= MAX_FAILURE_ENTRIES {
            break;
        }
    }
    if matches!(goal.verification_state, GoalVerificationState::Failed)
        || goal.agent_lifecycle.state == AgentLifecycleState::Correcting
        || goal.agent_lifecycle.state == AgentLifecycleState::Failed
    {
        if out.is_empty() {
            out.push(FailureRecord {
                kind: "verification_state".into(),
                detail: format!(
                    "verification={} lifecycle={}",
                    goal.verification_state.as_str(),
                    goal.agent_lifecycle.state.as_str()
                ),
                task_id: goal.active_task_id.clone(),
            });
        }
    }
    for b in &goal.blockers {
        out.push(FailureRecord {
            kind: "blocker".into(),
            detail: compact_line(b),
            task_id: None,
        });
    }
    out.truncate(MAX_FAILURE_ENTRIES);
    out
}

fn refs_from_history(
    goal: &CodingGoal,
) -> (Option<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut plan = goal.agent_lifecycle.approved_plan_id.clone();
    let mut approved = Vec::new();
    let mut exec = Vec::new();
    let mut verify = Vec::new();
    for rec in &goal.workflow_history {
        let detail = &rec.detail;
        if rec.kind.contains("apply") {
            if let Some(id) = detail
                .split('`')
                .nth(1)
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
            {
                if plan.is_none() {
                    plan = Some(id.clone());
                }
                if !approved.contains(&id) {
                    approved.push(id.clone());
                }
                if !exec.contains(&id) {
                    exec.push(id);
                }
            }
        }
        if rec.kind.contains("verify") || rec.kind.contains("task_completed") {
            if let Some(id) = detail
                .split('`')
                .nth(1)
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
            {
                if !verify.contains(&id) {
                    verify.push(id);
                }
            }
        }
    }
    approved.truncate(6);
    exec.truncate(6);
    verify.truncate(6);
    (plan, approved, exec, verify)
}

/// Build a maintained view from the active Coding Goal + optional assembled bundle.
pub fn maintain_from_goal(
    goal: &CodingGoal,
    bundle: Option<&ContextBundle>,
) -> MaintainedAgentContext {
    let active_id = goal.active_task_id.clone().or_else(|| {
        goal.tasks
            .iter()
            .find(|t| t.status == GoalTaskStatus::InProgress)
            .map(|t| t.id.clone())
    });

    let current_task = active_id.as_ref().and_then(|tid| {
        goal.tasks.iter().find(|t| &t.id == tid).map(|t| CurrentTaskSlice {
            task_id: t.id.clone(),
            description: compact_line(&t.description),
            status: t.status.as_str().into(),
            mutation_expected: t.mutation_expected,
            relevant_files: t
                .relevant_files
                .iter()
                .take(MAX_RELEVANT_FILES / 2)
                .cloned()
                .collect(),
            reuse_hints: t.reuse_hints.iter().take(4).cloned().collect(),
        })
    });

    let mut completed_summaries = Vec::new();
    for t in &goal.tasks {
        if t.status == GoalTaskStatus::Completed {
            let summary = t
                .notes
                .last()
                .cloned()
                .unwrap_or_else(|| t.description.clone());
            completed_summaries.push(CompletedTaskSummary {
                task_id: t.id.clone(),
                summary: compact_line(&summary),
            });
        }
    }
    if completed_summaries.is_empty() {
        for (i, w) in goal.completed_work.iter().enumerate() {
            completed_summaries.push(CompletedTaskSummary {
                task_id: format!("work-{i}"),
                summary: compact_line(w),
            });
        }
    }
    if completed_summaries.len() > MAX_COMPLETED_TASK_SUMMARIES {
        let skip = completed_summaries.len() - MAX_COMPLETED_TASK_SUMMARIES;
        completed_summaries = completed_summaries.into_iter().skip(skip).collect();
    }

    let pending_task_ids: Vec<String> = goal
        .tasks
        .iter()
        .filter(|t| t.status == GoalTaskStatus::Pending)
        .map(|t| t.id.clone())
        .collect();

    let mut file_sources = goal.relevant_files.clone();
    if let Some(task) = &current_task {
        for f in &task.relevant_files {
            if !file_sources.iter().any(|p| p == f) {
                file_sources.insert(0, f.clone());
            }
        }
    }
    let relevant_files = prefer_policy_paths(&file_sources, bundle);
    let relevant_symbols: Vec<String> = goal
        .relevant_symbols
        .iter()
        .take(MAX_RELEVANT_SYMBOLS)
        .cloned()
        .collect();

    let (coding_plan_ref, approved_ops_refs, execution_summary_refs, verification_refs) =
        refs_from_history(goal);

    let mut constraints: Vec<String> = goal
        .constraints
        .iter()
        .map(|c| {
            if c.chars().count() > MAX_CONSTRAINT_CHARS {
                c.chars().take(MAX_CONSTRAINT_CHARS).collect::<String>() + "…"
            } else {
                c.clone()
            }
        })
        .collect();
    constraints.truncate(12);

    let mut notes = vec![
        "Built from CodingGoal + ContextBundle labels — ContextEngine remains sole factory."
            .into(),
        "Current task prioritized; completed history summarized.".into(),
    ];
    if goal.workflow_history.len() > MAX_WORKFLOW_SUMMARY_ENTRIES {
        notes.push(format!(
            "Workflow history has {} entries — not injected in full (cap summary {}).",
            goal.workflow_history.len(),
            MAX_WORKFLOW_SUMMARY_ENTRIES
        ));
    }

    let mut ctx = MaintainedAgentContext {
        goal_id: goal.id.clone(),
        objective: compact_line(&goal.user_objective),
        goal_status: goal.status.as_str().into(),
        agent_lifecycle: goal.agent_lifecycle.state.as_str().into(),
        constraints,
        decisions: extract_decisions(goal),
        current_task,
        completed_summaries,
        pending_task_ids,
        failures: extract_failures(goal),
        coding_plan_ref,
        approved_ops_refs,
        execution_summary_refs,
        verification_refs,
        relevant_files,
        relevant_symbols,
        workspace_refresh: workspace_refresh_label(bundle),
        chars_estimate: 0,
        compacted: false,
        notes,
    };
    ctx.chars_estimate = estimate_chars(&ctx);
    if ctx.chars_estimate > MAX_GOAL_CONTEXT_CHARS {
        ctx = compact_context(ctx);
    }
    ctx
}

/// Force compaction (completed history shrinks; constraints/decisions retained).
pub fn compact_context(mut ctx: MaintainedAgentContext) -> MaintainedAgentContext {
    ctx.compacted = true;
    while ctx.completed_summaries.len() > 3 && estimate_chars(&ctx) > MAX_GOAL_CONTEXT_CHARS {
        ctx.completed_summaries.remove(0);
    }
    while ctx.completed_summaries.len() > 1 && estimate_chars(&ctx) > MAX_GOAL_CONTEXT_CHARS {
        ctx.completed_summaries.remove(0);
    }
    ctx.relevant_files.truncate(MAX_RELEVANT_FILES / 2);
    ctx.relevant_symbols.truncate(MAX_RELEVANT_SYMBOLS / 2);
    if let Some(task) = &mut ctx.current_task {
        task.relevant_files.truncate(4);
        task.reuse_hints.truncate(2);
    }
    ctx.approved_ops_refs.truncate(3);
    ctx.execution_summary_refs.truncate(3);
    ctx.verification_refs.truncate(3);
    ctx.failures.truncate(MAX_FAILURE_ENTRIES.min(4));
    for c in &mut ctx.constraints {
        *c = compact_line(c);
    }
    for d in &mut ctx.decisions {
        d.summary = compact_line(&d.summary);
    }
    ctx.notes.push(
        "Compacted completed history; constraints and decisions retained.".into(),
    );
    ctx.chars_estimate = estimate_chars(&ctx);
    if ctx.chars_estimate > MAX_GOAL_CONTEXT_CHARS {
        ctx.pending_task_ids.truncate(4);
        ctx.chars_estimate = estimate_chars(&ctx);
    }
    ctx
}

pub fn detect_agent_context_request(request: &UserRequest) -> Option<AgentContextAssessment> {
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
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::fix_from_verification::steals_from_other_detect(request)
    {
        return None;
    }

    if matches!(
        trimmed.as_str(),
        "maintain agent context"
            | "maintain goal context"
            | "refresh agent context"
            | "refresh goal context"
            | "update agent context"
    ) {
        return Some(AgentContextAssessment {
            kind: AgentContextRequestKind::Maintain,
            matched_rules: vec!["maintain_phrase"],
        });
    }
    if matches!(
        trimmed.as_str(),
        "show agent context"
            | "show goal context"
            | "agent context"
            | "what is the agent context"
    ) {
        return Some(AgentContextAssessment {
            kind: AgentContextRequestKind::Show,
            matched_rules: vec!["show_phrase"],
        });
    }
    if matches!(
        trimmed.as_str(),
        "compact goal context" | "compact agent context" | "compact the goal context"
    ) {
        return Some(AgentContextAssessment {
            kind: AgentContextRequestKind::Compact,
            matched_rules: vec!["compact_phrase"],
        });
    }
    if matches!(
        trimmed.as_str(),
        "agent context status" | "goal context status"
    ) {
        return Some(AgentContextAssessment {
            kind: AgentContextRequestKind::Status,
            matched_rules: vec!["status_phrase"],
        });
    }
    None
}

/// Apply soft-path assessment — updates goal’s stored maintenance snapshot when maintaining/compacting.
pub fn apply_assessment(
    goal: Option<&CodingGoal>,
    assessment: &AgentContextAssessment,
    bundle: Option<&ContextBundle>,
) -> (AgentContextReport, Option<CodingGoal>) {
    let Some(goal) = goal else {
        return (
            AgentContextReport {
                status: AgentContextStatus::NoActiveGoal,
                notes: vec![
                    "No active Coding Goal — create one before maintaining agent context.".into(),
                ],
                ..Default::default()
            },
            None,
        );
    };

    match assessment.kind {
        AgentContextRequestKind::Maintain | AgentContextRequestKind::Show => {
            let ctx = maintain_from_goal(goal, bundle);
            let mut updated = goal.clone();
            updated.maintained_context = Some(ctx.clone());
            updated.updated_at = crate::coding_goal::stamp_now();
            (
                AgentContextReport {
                    status: AgentContextStatus::Maintained,
                    context: Some(ctx),
                    notes: vec![
                        "ContextEngine remains the sole ContextBundle factory.".into(),
                        "No Memory Engine write; no project files modified.".into(),
                    ],
                    ..Default::default()
                },
                Some(updated),
            )
        }
        AgentContextRequestKind::Compact => {
            let ctx = compact_context(maintain_from_goal(goal, bundle));
            let mut updated = goal.clone();
            updated.maintained_context = Some(ctx.clone());
            updated.updated_at = crate::coding_goal::stamp_now();
            (
                AgentContextReport {
                    status: AgentContextStatus::Compacted,
                    context: Some(ctx),
                    notes: vec![
                        "Completed task history compacted; constraints retained.".into(),
                    ],
                    ..Default::default()
                },
                Some(updated),
            )
        }
        AgentContextRequestKind::Status => {
            let ctx = goal
                .maintained_context
                .clone()
                .unwrap_or_else(|| maintain_from_goal(goal, bundle));
            (
                AgentContextReport {
                    status: AgentContextStatus::Status,
                    context: Some(ctx),
                    notes: vec!["Status only — no store write required.".into()],
                    ..Default::default()
                },
                None,
            )
        }
    }
}

/// Attach compacted agent context onto an engine-minted LlmContext.
///
/// No-op when there is no active goal — preserves ordinary C2/C3/C4 conversations.
pub fn attach_to_llm(
    llm: jaymi_context::LlmContext,
    goal: Option<&CodingGoal>,
    bundle: &ContextBundle,
) -> jaymi_context::LlmContext {
    let Some(goal) = goal else {
        return llm;
    };
    if goal.status.is_terminal() {
        return llm;
    }
    let ctx = maintain_from_goal(goal, Some(bundle));
    llm.with_extension(LLM_EXTENSION_KEY, ctx.to_extension_value())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use crate::goal_decomposition::{apply_decomposition, propose_tasks, GoalTask};
    use crate::multi_step_execution::{start_goal_execution, WorkflowRecord};
    use jaymi_context::ContextBundle;

    fn goal_with_tasks() -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(
            "Build a login system without OAuth.",
        ))
        .unwrap();
        let g = create_goal(&a, &ContextBundle::default(), None);
        let (tasks, order) = propose_tasks(&g).unwrap();
        apply_decomposition(g, tasks, order)
    }

    #[test]
    fn multi_task_context_prioritizes_current() {
        let (g, _) = start_goal_execution(goal_with_tasks()).unwrap();
        let ctx = maintain_from_goal(&g, None);
        assert!(ctx.current_task.is_some());
        assert_eq!(
            ctx.current_task.as_ref().unwrap().task_id,
            g.active_task_id.clone().unwrap()
        );
    }

    #[test]
    fn user_constraints_retained_after_compact() {
        let mut g = goal_with_tasks();
        assert!(g
            .constraints
            .iter()
            .any(|c| c.to_ascii_lowercase().contains("oauth")));
        for i in 0..40 {
            g.workflow_history.push(WorkflowRecord {
                sequence: i,
                kind: "noise".into(),
                task_id: None,
                detail: format!("long detail {i} {}", "x".repeat(80)),
            });
            g.completed_work
                .push(format!("done step {i} {}", "y".repeat(60)));
        }
        let ctx = compact_context(maintain_from_goal(&g, None));
        assert!(!ctx.constraints.is_empty());
        assert!(ctx
            .constraints
            .iter()
            .any(|c| c.to_ascii_lowercase().contains("oauth")));
        assert!(ctx.compacted);
    }

    #[test]
    fn failed_task_retained_when_correcting() {
        let mut g = goal_with_tasks();
        g.verification_state = GoalVerificationState::Failed;
        g.agent_lifecycle.state = AgentLifecycleState::Correcting;
        g.workflow_history.push(WorkflowRecord {
            sequence: 1,
            kind: "verify_fail".into(),
            task_id: Some("t1".into()),
            detail: "Verification failed `cargo-test`".into(),
        });
        let ctx = maintain_from_goal(&g, None);
        assert!(!ctx.failures.is_empty());
        assert!(ctx.failures.iter().any(|f| f.kind.contains("verify")));
    }

    #[test]
    fn long_history_not_fully_injected() {
        let mut g = goal_with_tasks();
        for i in 0..80 {
            g.workflow_history.push(WorkflowRecord {
                sequence: i,
                kind: "step".into(),
                task_id: None,
                detail: format!("history entry {i}"),
            });
        }
        let ctx = maintain_from_goal(&g, None);
        assert!(ctx
            .notes
            .iter()
            .any(|n| n.contains("not injected") || n.contains("Workflow history")));
        assert!(ctx.chars_estimate <= MAX_GOAL_CONTEXT_CHARS + 400);
    }

    #[test]
    fn decisions_recoverable() {
        let mut g = goal_with_tasks();
        g.notes
            .push("Decision: use session cookies for auth architecture.".into());
        let ctx = maintain_from_goal(&g, None);
        assert!(ctx
            .decisions
            .iter()
            .any(|d| d.summary.contains("session cookies")));
        let compacted = compact_context(ctx);
        assert!(compacted
            .decisions
            .iter()
            .any(|d| d.summary.contains("session cookies")));
    }

    #[test]
    fn detect_phrases() {
        assert!(detect_agent_context_request(&UserRequest::new(
            "Maintain agent context."
        ))
        .is_some());
        assert!(detect_agent_context_request(&UserRequest::new(
            "Compact goal context."
        ))
        .is_some());
        assert!(detect_agent_context_request(&UserRequest::new(
            "Build a login system."
        ))
        .is_none());
    }

    #[test]
    fn no_duplicate_snapshot_types() {
        let g = goal_with_tasks();
        let ctx = maintain_from_goal(&g, Some(&ContextBundle::default()));
        let s = serde_json::to_string(&ctx).unwrap();
        assert!(!s.contains("WorkspaceSnapshot"));
        assert!(!s.contains("ProjectSnapshot"));
        assert!(!s.contains("GitSnapshot"));
    }

    #[test]
    fn independent_completed_summaries() {
        let mut g = goal_with_tasks();
        g.tasks = vec![
            GoalTask {
                id: "a".into(),
                description: "A".into(),
                status: GoalTaskStatus::Completed,
                notes: vec!["finished A".into()],
                ..GoalTask::default()
            },
            GoalTask {
                id: "b".into(),
                description: "B".into(),
                status: GoalTaskStatus::InProgress,
                ..GoalTask::default()
            },
            GoalTask {
                id: "c".into(),
                description: "C".into(),
                status: GoalTaskStatus::Pending,
                ..GoalTask::default()
            },
        ];
        g.active_task_id = Some("b".into());
        let ctx = maintain_from_goal(&g, None);
        assert_eq!(ctx.current_task.as_ref().unwrap().task_id, "b");
        assert!(ctx.completed_summaries.iter().any(|s| s.task_id == "a"));
        assert!(ctx.pending_task_ids.iter().any(|id| id == "c"));
    }
}
