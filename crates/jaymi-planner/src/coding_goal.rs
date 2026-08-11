//! Coding Goal Model (Sprint C5.1).
//!
//! Planner-owned representation of a **multi-step coding objective** —
//! "what Jaymi is trying to accomplish" — distinct from CodingPlan,
//! ExecutionPlan, and VerificationResult.
//!
//! Constitutional constraints:
//! - Planner owns the goal; no AgentContextEngine / second Planner / second orchestrator
//! - Creating a goal never executes tools or mutates files
//! - Ordinary questions and one-step edits stay Conversation First (no forced goal)
//! - Ambiguous requests stay conversational until sufficiently specified
//! - Do not invent unsupported acceptance criteria (label proposed when used)
//! - C0–C4 remain closed; C5.2 owns goal decomposition (proposed tasks only)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

/// Lifecycle status of a coding goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingGoalStatus {
    Active,
    Completed,
    Cancelled,
    Blocked,
}

impl CodingGoalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
            Self::Blocked => "Blocked",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

impl Default for CodingGoalStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Whether an acceptance criterion came from the user or was proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceCriterionSource {
    Explicit,
    Proposed,
}

impl AcceptanceCriterionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Proposed => "proposed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AcceptanceCriterion {
    pub text: String,
    pub source: AcceptanceCriterionSource,
}

/// Observational link to C4 verification — not a VerificationResult itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalVerificationState {
    NotStarted,
    InProgress,
    Passed,
    Failed,
    Blocked,
    Unknown,
}

impl GoalVerificationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "Not started",
            Self::InProgress => "In progress",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Blocked => "Blocked",
            Self::Unknown => "Unknown",
        }
    }
}

impl Default for GoalVerificationState {
    fn default() -> Self {
        Self::NotStarted
    }
}

/// Planner-owned multi-step coding objective (not a plan, not an execution).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodingGoal {
    pub id: String,
    pub user_objective: String,
    pub project_id: Option<String>,
    pub workspace_root: Option<String>,
    pub conversation_id: Option<String>,
    pub constraints: Vec<String>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub status: CodingGoalStatus,
    /// Current task/step — set when C5.2 decomposes or a task is in progress.
    pub current_task: Option<String>,
    pub completed_work: Vec<String>,
    pub remaining_work: Vec<String>,
    pub blockers: Vec<String>,
    pub relevant_files: Vec<String>,
    pub relevant_symbols: Vec<String>,
    pub verification_state: GoalVerificationState,
    pub created_at: String,
    pub updated_at: String,
    /// Always false on create — goals do not mutate.
    pub mutated: bool,
    /// Always false on create — goals do not run tools.
    pub tools_executed: bool,
    pub notes: Vec<String>,
    pub matched_rules: Vec<String>,
    /// Proposed tasks from C5.2 Goal Decomposition (never auto-executed).
    pub tasks: Vec<crate::goal_decomposition::GoalTask>,
    /// Authoritative agent lifecycle (C5.3) — distinct from status and task status.
    pub agent_lifecycle: crate::agent_state_machine::AgentLifecycle,
    /// Active task id for C5.4 multi-step orchestration (if any).
    pub active_task_id: Option<String>,
    /// Orchestration history (C5.4) — not an ExecutionPlan.
    pub workflow_history: Vec<crate::multi_step_execution::WorkflowRecord>,
    /// Count of workflow transitions (bounded).
    pub workflow_transition_count: u32,
    /// Last maintained compact agent context (C5.5) — not a ContextBundle.
    pub maintained_context: Option<crate::agent_context_maintenance::MaintainedAgentContext>,
    /// Approval / autonomy policy binding (C5.6) — not an AgentApproval system.
    pub autonomy_policy: crate::approval_autonomy_policy::GoalAutonomyState,
    /// Pause / resume / cancel control surface (C5.8) — not a second agent state.
    pub control: crate::agent_pause_resume_cancel::GoalControlState,
    /// Failure / recovery history (C5.9) — bounded; never silent mutation.
    pub recovery: crate::agent_failure_recovery::GoalRecoveryState,
    /// Evidence-based completion surface (C5.10) — Planner-owned; not model-declared.
    pub completion: crate::coding_goal_completion::GoalCompletionState,
    /// Long-running session continuity (C5.11) — bounds + revalidation; no agent DB.
    pub continuity: crate::agent_session_continuity::SessionContinuityState,
}

impl Default for CodingGoal {
    fn default() -> Self {
        let now = now_stamp();
        Self {
            id: String::new(),
            user_objective: String::new(),
            project_id: None,
            workspace_root: None,
            conversation_id: None,
            constraints: Vec::new(),
            acceptance_criteria: Vec::new(),
            status: CodingGoalStatus::Active,
            current_task: None,
            completed_work: Vec::new(),
            remaining_work: Vec::new(),
            blockers: Vec::new(),
            relevant_files: Vec::new(),
            relevant_symbols: Vec::new(),
            verification_state: GoalVerificationState::NotStarted,
            created_at: now.clone(),
            updated_at: now,
            mutated: false,
            tools_executed: false,
            notes: Vec::new(),
            matched_rules: Vec::new(),
            tasks: Vec::new(),
            agent_lifecycle: crate::agent_state_machine::AgentLifecycle::default(),
            active_task_id: None,
            workflow_history: Vec::new(),
            workflow_transition_count: 0,
            maintained_context: None,
            autonomy_policy: crate::approval_autonomy_policy::GoalAutonomyState::default(),
            control: crate::agent_pause_resume_cancel::GoalControlState::default(),
            recovery: crate::agent_failure_recovery::GoalRecoveryState::default(),
            completion: crate::coding_goal_completion::GoalCompletionState::default(),
            continuity: crate::agent_session_continuity::SessionContinuityState::default(),
        }
    }
}

impl CodingGoal {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Goal\n");
        out.push_str(
            "\n_This records what you want accomplished — not a Coding Plan, Execution Plan, or verification run._\n",
        );
        out.push_str(&format!("\n### Status\n- **{}**\n", self.status.label()));
        out.push_str(&format!("\n### Goal ID\n- `{}`\n", self.id));
        out.push_str("\n### Objective\n- ");
        out.push_str(&self.user_objective);
        out.push('\n');

        if self.project_id.is_some() || self.workspace_root.is_some() {
            out.push_str("\n### Project / workspace\n");
            if let Some(pid) = &self.project_id {
                out.push_str(&format!("- Project: `{pid}`\n"));
            }
            if let Some(root) = &self.workspace_root {
                out.push_str(&format!("- Workspace: `{root}`\n"));
            }
        }

        if let Some(cid) = &self.conversation_id {
            out.push_str(&format!("\n### Conversation\n- `{cid}`\n"));
        }

        if !self.constraints.is_empty() {
            out.push_str("\n### Constraints\n");
            for c in &self.constraints {
                out.push_str("- ");
                out.push_str(c);
                out.push('\n');
            }
        }

        if !self.acceptance_criteria.is_empty() {
            out.push_str("\n### Acceptance criteria\n");
            for (i, c) in self.acceptance_criteria.iter().enumerate() {
                let tag = match c.source {
                    AcceptanceCriterionSource::Explicit => "explicit",
                    AcceptanceCriterionSource::Proposed => "proposed",
                };
                let sat = self
                    .completion
                    .criteria
                    .iter()
                    .find(|e| e.index == i)
                    .map(|e| e.satisfaction.label())
                    .unwrap_or("Unevaluated");
                out.push_str(&format!("- [{sat}] ({tag}) {}\n", c.text));
            }
        } else {
            out.push_str(
                "\n### Acceptance criteria\n- None stated yet — say what “done” means when you know.\n",
            );
        }

        if let Some(ending) = self.completion.ending_kind {
            out.push_str(&format!(
                "\n### Completion ending\n- **{}**\n",
                ending.label()
            ));
        }

        out.push_str(&format!(
            "\n### Current task\n- {}\n",
            self.current_task
                .as_deref()
                .unwrap_or("_Not decomposed yet — ask to break this goal into tasks._")
        ));

        if !self.tasks.is_empty() {
            out.push_str("\n### Tasks\n");
            for (i, task) in self.tasks.iter().enumerate() {
                out.push_str(&format!(
                    "{}. {} (`{}`) — {}\n",
                    i + 1,
                    task.description,
                    task.id,
                    task.status.label()
                ));
            }
        }

        if !self.completed_work.is_empty() {
            out.push_str("\n### Completed work\n");
            for item in &self.completed_work {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
        }

        if !self.remaining_work.is_empty() {
            out.push_str("\n### Remaining work\n");
            for item in &self.remaining_work {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
        } else if self.status == CodingGoalStatus::Active {
            out.push_str(
                "\n### Remaining work\n- _Tracked at goal level only — no step breakdown yet._\n",
            );
        }

        if !self.blockers.is_empty() {
            out.push_str("\n### Blockers\n");
            for b in &self.blockers {
                out.push_str("- ");
                out.push_str(b);
                out.push('\n');
            }
        }

        if !self.relevant_files.is_empty() || !self.relevant_symbols.is_empty() {
            out.push_str("\n### Relevant context\n");
            for f in &self.relevant_files {
                out.push_str(&format!("- File: `{f}`\n"));
            }
            for s in &self.relevant_symbols {
                out.push_str(&format!("- Symbol: `{s}`\n"));
            }
        }

        out.push_str(&format!(
            "\n### Agent lifecycle\n- **{}**\n",
            self.agent_lifecycle.state.label()
        ));
        out.push_str(&format!(
            "- Allows mutation execution: {}\n",
            self.agent_lifecycle.state.allows_mutation_execution()
        ));

        out.push_str(&format!(
            "\n### Verification\n- {}\n",
            self.verification_state.label()
        ));
        out.push_str(&format!(
            "\n### Timestamps\n- Created: `{}`\n- Updated: `{}`\n",
            self.created_at, self.updated_at
        ));
        out.push_str(&format!(
            "\n### Safety\n- mutated={} · tools_executed={}\n",
            self.mutated, self.tools_executed
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
            "\nSay **cancel this goal**, **mark this goal complete**, or **this goal is blocked** when status should change. Ordinary coding questions stay conversational — they do not auto-create goals.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "coding_goal id={} status={} objective={:?} mutated={} tools_executed={} verification={} blockers={} criteria={}",
            self.id,
            self.status.as_str(),
            truncate(&self.user_objective, 80),
            self.mutated,
            self.tools_executed,
            self.verification_state.as_str(),
            self.blockers.len(),
            self.acceptance_criteria.len(),
        )
    }
}

/// Soft-path assessment before a goal is minted or updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingGoalAssessment {
    pub kind: CodingGoalRequestKind,
    pub objective: String,
    pub matched_rules: Vec<&'static str>,
    pub block_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodingGoalRequestKind {
    Create,
    Cancel,
    Complete,
    Block,
    Status,
}

/// Planner-owned store: goals persist across turns; keyed by conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodingGoalStore {
    /// All goals (history + active), newest last within each conversation.
    pub goals: Vec<CodingGoal>,
    /// Active goal id per conversation key (`_default` when none).
    pub active_by_conversation: HashMap<String, String>,
}

impl CodingGoalStore {
    pub fn conversation_key(conversation_id: Option<&str>) -> String {
        conversation_id
            .filter(|s| !s.is_empty())
            .unwrap_or("_default")
            .to_string()
    }

    pub fn active_goal(&self, conversation_id: Option<&str>) -> Option<&CodingGoal> {
        let key = Self::conversation_key(conversation_id);
        let id = self.active_by_conversation.get(&key)?;
        self.goals.iter().find(|g| &g.id == id)
    }

    pub fn active_goal_mut(&mut self, conversation_id: Option<&str>) -> Option<&mut CodingGoal> {
        let key = Self::conversation_key(conversation_id);
        let id = self.active_by_conversation.get(&key)?.clone();
        self.goals.iter_mut().find(|g| g.id == id)
    }

    pub fn push_active(&mut self, goal: CodingGoal) {
        let key = Self::conversation_key(goal.conversation_id.as_deref());
        self.active_by_conversation
            .insert(key, goal.id.clone());
        self.goals.push(goal);
    }

    pub fn goals_for_conversation(&self, conversation_id: Option<&str>) -> Vec<&CodingGoal> {
        let key = Self::conversation_key(conversation_id);
        self.goals
            .iter()
            .filter(|g| Self::conversation_key(g.conversation_id.as_deref()) == key)
            .collect()
    }
}

fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Public stamp helper for sibling C5 modules (store updates only).
pub fn stamp_now() -> String {
    now_stamp()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn new_goal_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("goal-{}-{}", now_stamp(), seq)
}

/// Detect create / cancel / complete / block / status intents.
pub fn detect_coding_goal_request(request: &UserRequest) -> Option<CodingGoalAssessment> {
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

    if looks_like_status_query(&trimmed) {
        return Some(CodingGoalAssessment {
            kind: CodingGoalRequestKind::Status,
            objective: content.to_string(),
            matched_rules: vec!["goal_status"],
            block_reason: None,
        });
    }

    if looks_like_cancel_goal(&trimmed) {
        return Some(CodingGoalAssessment {
            kind: CodingGoalRequestKind::Cancel,
            objective: content.to_string(),
            matched_rules: vec!["goal_cancel"],
            block_reason: None,
        });
    }

    if looks_like_complete_goal(&trimmed) {
        return Some(CodingGoalAssessment {
            kind: CodingGoalRequestKind::Complete,
            objective: content.to_string(),
            matched_rules: vec!["goal_complete"],
            block_reason: None,
        });
    }

    if let Some(reason) = looks_like_block_goal(&trimmed, content) {
        return Some(CodingGoalAssessment {
            kind: CodingGoalRequestKind::Block,
            objective: content.to_string(),
            matched_rules: vec!["goal_block"],
            block_reason: reason,
        });
    }

    // Read-only / Q&A / specialized paths — never force a persistent goal.
    if steals_from_other_detect(request, &lower) {
        return None;
    }

    if looks_like_ambiguous(&trimmed) {
        return None;
    }

    if looks_like_one_step_coding(&trimmed) {
        return None;
    }

    if let Some(rules) = looks_like_multi_step_coding_goal(&trimmed) {
        return Some(CodingGoalAssessment {
            kind: CodingGoalRequestKind::Create,
            objective: content.to_string(),
            matched_rules: rules,
            block_reason: None,
        });
    }

    None
}

/// Yield C1.4 Coding Plan when the request is a persistent multi-step goal.
pub fn steals_from_coding_plan_detect(lower: &str) -> bool {
    let trimmed = lower
        .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?');
    looks_like_multi_step_coding_goal(trimmed).is_some()
}

/// Yield C3.1 coding edit when the request is a persistent multi-step goal.
pub fn steals_from_coding_edit_detect(lower: &str) -> bool {
    steals_from_coding_plan_detect(lower)
}

fn steals_from_other_detect(request: &UserRequest, lower: &str) -> bool {
    // Do not call fix_from_verification here — C4.4 yields multi-step “fix all…”
    // goals to this module; a reverse steal would drop those creates.
    if crate::coding_review::detect_review_request(request).is_some()
        || crate::relationship_tracing::detect_relationship_trace_request(request).is_some()
        || crate::deep_investigation::detect_deep_investigation_request(request).is_some()
        || crate::codebase_qa::detect_codebase_qa_request(request).is_some()
        || crate::symbol_navigation::detect_symbol_navigation_request(request).is_some()
        || crate::code_search::detect_code_search_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
    {
        return true;
    }
    // Explicit read-only / understanding questions.
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
    trimmed.starts_with("explain ")
        || trimmed.starts_with("what does ")
        || trimmed.starts_with("what is ")
        || trimmed.starts_with("where is ")
        || trimmed.starts_with("where are ")
        || trimmed.starts_with("why ")
        || trimmed.starts_with("how does ")
        || trimmed.starts_with("how do ")
        || trimmed.starts_with("who calls")
        || trimmed.contains("find references")
        || trimmed.starts_with("review ")
}

fn looks_like_status_query(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "what's my goal"
            | "whats my goal"
            | "what is my goal"
            | "what's the goal"
            | "whats the goal"
            | "what is the goal"
            | "what's my coding goal"
            | "what is my coding goal"
            | "show the goal"
            | "show my goal"
            | "show the coding goal"
            | "current goal"
            | "current coding goal"
    ) || trimmed.starts_with("what's my current goal")
        || trimmed.starts_with("what is my current goal")
}

fn looks_like_cancel_goal(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "cancel this goal"
            | "cancel the goal"
            | "cancel my goal"
            | "cancel coding goal"
            | "abandon this goal"
            | "abandon the goal"
            | "drop this goal"
            | "drop the goal"
    ) || trimmed.starts_with("cancel this goal")
        || trimmed.starts_with("cancel the coding goal")
        || trimmed.starts_with("stop working on this goal")
        || trimmed.starts_with("stop this goal")
}

fn looks_like_complete_goal(trimmed: &str) -> bool {
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

fn looks_like_block_goal(trimmed: &str, original: &str) -> Option<Option<String>> {
    let blocked = trimmed == "this goal is blocked"
        || trimmed == "the goal is blocked"
        || trimmed == "block this goal"
        || trimmed == "block the goal"
        || trimmed.starts_with("this goal is blocked")
        || trimmed.starts_with("we're blocked on")
        || trimmed.starts_with("we are blocked on")
        || trimmed.starts_with("blocked on ")
        || trimmed.starts_with("block this goal because")
        || trimmed.starts_with("block the goal because");
    if !blocked {
        return None;
    }
    let reason = extract_block_reason(original);
    Some(reason)
}

fn extract_block_reason(content: &str) -> Option<String> {
    let lower = content.to_ascii_lowercase();
    for marker in [" because ", " on ", ": "] {
        if let Some(idx) = lower.find(marker) {
            if lower.contains("block") || lower.contains("blocked") {
                let rest = content[idx + marker.len()..].trim();
                if !rest.is_empty() {
                    return Some(rest.trim_end_matches(|c: char| c == '.' || c == '!').to_string());
                }
            }
        }
    }
    None
}

fn looks_like_ambiguous(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "improve the code"
            | "make it better"
            | "help with authentication"
            | "help with auth"
            | "fix things"
            | "clean this up"
            | "do something with login"
            | "maybe improve auth"
            | "thoughts on auth"
            | "what about login"
            | "authentication"
            | "login"
    ) || trimmed.starts_with("maybe ")
        || trimmed.starts_with("perhaps ")
        || trimmed.starts_with("thoughts on ")
        || trimmed.starts_with("what about ")
        || (trimmed.starts_with("help with ") && trimmed.split_whitespace().count() <= 4)
        || (trimmed.starts_with("improve ") && !trimmed.contains("architecture") && trimmed.len() < 40)
}

fn looks_like_one_step_coding(trimmed: &str) -> bool {
    // Targeted single edits — stay on C3.1 / conversational, not persistent goals.
    trimmed.starts_with("rename ")
        || trimmed.starts_with("fix this typo")
        || trimmed.starts_with("fix the typo")
        || trimmed.starts_with("add a comment")
        || trimmed.starts_with("add comment")
        || trimmed.starts_with("delete this line")
        || trimmed.starts_with("remove this line")
        || trimmed.starts_with("change this function to")
        || trimmed.starts_with("update this variable")
        || trimmed == "fix this"
        || trimmed == "fix it"
        || trimmed == "write tests"
        || trimmed == "write tests."
        || trimmed.starts_with("write unit tests")
        || trimmed.starts_with("add a unit test")
        || trimmed == "create a parser"
        || trimmed == "create a parser."
}

/// Meaningful multi-step coding objectives (examples from C5.1 / C5.12).
fn looks_like_multi_step_coding_goal(trimmed: &str) -> Option<Vec<&'static str>> {
    let mut rules = Vec::new();

    if (trimmed.contains("fix all") || trimmed.contains("fix every"))
        && (trimmed.contains("error")
            || trimmed.contains("warning")
            || trimmed.contains("compiler")
            || trimmed.contains("test")
            || trimmed.contains("failure")
            || trimmed.contains("failures"))
    {
        rules.push("goal_fix_all_errors");
    }

    if trimmed.contains("refactor")
        && (trimmed.contains("architecture")
            || trimmed.contains("system")
            || (trimmed.contains("module") && trimmed.contains("across")))
    {
        rules.push("goal_refactor_architecture");
    }

    if (trimmed.starts_with("build ")
        || trimmed.starts_with("create ")
        || trimmed.starts_with("implement ")
        || trimmed.starts_with("design "))
        && trimmed.contains("system")
    {
        rules.push("goal_build_system");
    }

    if (trimmed.starts_with("add ") || trimmed.starts_with("implement "))
        && (trimmed.contains(" to the application")
            || trimmed.contains(" to the app")
            || trimmed.contains(" to the project")
            || trimmed.contains(" to this project")
            || trimmed.contains(" to this application")
            || trimmed.contains(" into the application")
            || trimmed.contains(" into the app")
            || trimmed.contains(" into the project"))
    {
        rules.push("goal_add_to_application");
    }

    // Broader multi-step product work (still not every "build X").
    if (trimmed.starts_with("build ") || trimmed.starts_with("implement "))
        && (trimmed.contains("end to end")
            || trimmed.contains("end-to-end")
            || trimmed.contains("from scratch")
            || trimmed.contains("full stack")
            || trimmed.contains("full-stack"))
    {
        rules.push("goal_end_to_end");
    }

    // Greenfield product / game (C5.12 agentic experience) — not a one-step plan.
    if (trimmed.starts_with("build ")
        || trimmed.starts_with("create ")
        || trimmed.starts_with("make ")
        || trimmed.starts_with("implement "))
        && (trimmed.contains("pong")
            || trimmed.contains(" game")
            || trimmed.ends_with("game")
            || trimmed.contains("sample app")
            || trimmed.contains("demo app")
            || trimmed.contains("toy app"))
    {
        rules.push("goal_greenfield_product");
    }

    if trimmed.contains("migrate ")
        && (trimmed.contains(" to ") || trimmed.contains("architecture") || trimmed.contains("across"))
    {
        rules.push("goal_migrate");
    }

    if rules.is_empty() {
        None
    } else {
        Some(rules)
    }
}

/// Mint a new goal from assessment + ambient Context (no tools).
pub fn create_goal(
    assessment: &CodingGoalAssessment,
    context: &ContextBundle,
    conversation_id: Option<String>,
) -> CodingGoal {
    let now = now_stamp();
    let (project_id, workspace_root) = project_from_context(context);
    let (files, symbols) = light_relevant_from_context(context, &assessment.objective);
    let constraints = extract_constraints(&assessment.objective);
    let acceptance_criteria = extract_acceptance_criteria(&assessment.objective);

    let mut notes = vec![
        "Goal created — representation only; no tools ran and no files changed.".into(),
        "A Coding Goal is not a Coding Plan, Execution Plan, or VerificationResult.".into(),
    ];
    if acceptance_criteria
        .iter()
        .any(|c| c.source == AcceptanceCriterionSource::Proposed)
    {
        notes.push(
            "Some acceptance criteria are proposed paraphrases of your request — not invented requirements."
                .into(),
        );
    }

    CodingGoal {
        id: new_goal_id(),
        user_objective: assessment.objective.clone(),
        project_id,
        workspace_root,
        conversation_id,
        constraints,
        acceptance_criteria,
        status: CodingGoalStatus::Active,
        current_task: None,
        completed_work: Vec::new(),
        remaining_work: vec![
            "Accomplish the stated objective (steps not decomposed yet).".into(),
        ],
        blockers: Vec::new(),
        relevant_files: files,
        relevant_symbols: symbols,
        verification_state: GoalVerificationState::NotStarted,
        created_at: now.clone(),
        updated_at: now,
        mutated: false,
        tools_executed: false,
        notes,
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        tasks: Vec::new(),
        agent_lifecycle: crate::agent_state_machine::AgentLifecycle::default(),
        active_task_id: None,
        workflow_history: Vec::new(),
        workflow_transition_count: 0,
        maintained_context: None,
        autonomy_policy: crate::approval_autonomy_policy::GoalAutonomyState::default(),
        control: crate::agent_pause_resume_cancel::GoalControlState::default(),
        recovery: crate::agent_failure_recovery::GoalRecoveryState::default(),
        completion: crate::coding_goal_completion::GoalCompletionState::default(),
        continuity: crate::agent_session_continuity::SessionContinuityState::default(),
    }
}

pub fn cancel_goal(goal: CodingGoal, reason: &str) -> CodingGoal {
    match crate::agent_state_machine::transition_goal(
        goal.clone(),
        crate::agent_state_machine::AgentLifecycleEvent::Cancel {
            reason: reason.to_string(),
        },
    ) {
        Ok(updated) => updated,
        Err(_) => {
            // Already terminal or limit — still force coarse cancel bookkeeping
            // and invalidate approvals (C5.6 / C5.8).
            let mut goal =
                crate::approval_autonomy_policy::invalidate_on_cancel(goal, reason);
            goal.status = CodingGoalStatus::Cancelled;
            goal.agent_lifecycle.state =
                crate::agent_state_machine::AgentLifecycleState::Cancelled;
            goal.control.cancelled = true;
            goal.control.cancel_count = goal.control.cancel_count.saturating_add(1);
            goal.control.last_cancel_reason = Some(reason.into());
            goal.updated_at = now_stamp();
            goal.mutated = false;
            goal.tools_executed = false;
            goal.current_task = None;
            goal.notes.push(format!("Cancelled: {reason}"));
            goal
        }
    }
}

pub fn complete_goal(
    goal: CodingGoal,
    evidence: Option<crate::agent_state_machine::CompletionEvidence>,
) -> Result<CodingGoal, String> {
    let evidence = evidence.ok_or_else(|| {
        "Cannot complete the agent lifecycle without completion evidence (verification/apply refs)."
            .to_string()
    })?;
    crate::agent_state_machine::transition_goal(
        goal,
        crate::agent_state_machine::AgentLifecycleEvent::Complete { evidence },
    )
    .map_err(|e| e.to_string_message())
}

pub fn block_goal(mut goal: CodingGoal, reason: Option<String>) -> CodingGoal {
    let reason = reason.unwrap_or_else(|| "Unspecified blocker.".into());
    match crate::agent_state_machine::transition_goal(
        goal.clone(),
        crate::agent_state_machine::AgentLifecycleEvent::Block {
            reason: reason.clone(),
        },
    ) {
        Ok(updated) => updated,
        Err(_) => {
            goal.status = CodingGoalStatus::Blocked;
            goal.agent_lifecycle.state =
                crate::agent_state_machine::AgentLifecycleState::Blocked;
            goal.updated_at = now_stamp();
            goal.mutated = false;
            goal.tools_executed = false;
            goal.verification_state = GoalVerificationState::Blocked;
            if !goal.blockers.iter().any(|b| b == &reason) {
                goal.blockers.push(reason.clone());
            }
            goal.notes.push(format!("Blocked: {reason}"));
            goal
        }
    }
}

fn project_from_context(context: &ContextBundle) -> (Option<String>, Option<String>) {
    let project = context.active_project();
    let project_id = project
        .project_id
        .clone()
        .or_else(|| project.name.clone());
    let workspace_root = project.root_directory.clone();
    (project_id, workspace_root)
}

fn light_relevant_from_context(
    context: &ContextBundle,
    objective: &str,
) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut symbols = Vec::new();

    // Ambient open file only — no filesystem scan.
    if let Some(path) = context.current_file().path.clone() {
        files.push(path);
    }

    // Path-like tokens mentioned in the objective.
    for token in objective.split_whitespace() {
        let t = token.trim_matches(|c: char| {
            c == ',' || c == '.' || c == ';' || c == ':' || c == '"' || c == '\''
        });
        if (t.contains('/') || t.ends_with(".rs") || t.ends_with(".ts") || t.ends_with(".tsx"))
            && t.len() > 2
            && !files.iter().any(|f| f == t)
        {
            files.push(t.to_string());
        }
        if t.starts_with('`') && t.ends_with('`') && t.len() > 2 {
            symbols.push(t.trim_matches('`').to_string());
        }
    }

    (files, symbols)
}

fn extract_constraints(objective: &str) -> Vec<String> {
    let lower = objective.to_ascii_lowercase();
    let mut out = Vec::new();
    for marker in ["without ", "must not ", "don't ", "do not ", "no "] {
        if let Some(idx) = lower.find(marker) {
            let rest = objective[idx..].trim();
            let end = rest
                .find([',', ';'])
                .unwrap_or_else(|| rest.len().min(120));
            let piece = rest[..end].trim().to_string();
            if piece.len() > marker.len() && !out.contains(&piece) {
                out.push(piece);
            }
        }
    }
    out
}

fn extract_acceptance_criteria(objective: &str) -> Vec<AcceptanceCriterion> {
    let lower = objective.to_ascii_lowercase();
    let mut out = Vec::new();

    // Explicit markers from the user.
    for marker in [
        "acceptance criteria:",
        "acceptance:",
        "must ",
        "so that ",
        "done when ",
        "success means ",
    ] {
        if let Some(idx) = lower.find(marker) {
            let rest = objective[idx + marker.len()..].trim();
            if !rest.is_empty() {
                let text = rest
                    .trim_end_matches(|c: char| c == '.' || c == '!')
                    .to_string();
                if text.len() > 2 {
                    out.push(AcceptanceCriterion {
                        text,
                        source: AcceptanceCriterionSource::Explicit,
                    });
                    break;
                }
            }
        }
    }

    // Proposed paraphrase only when objective itself states a clear outcome verb+object.
    // Labeled proposed — not unsupported invention.
    if out.is_empty() && looks_like_multi_step_coding_goal(
        &lower
            .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?'),
    )
    .is_some()
    {
        out.push(AcceptanceCriterion {
            text: format!(
                "The stated objective is accomplished: {}",
                objective.trim().trim_end_matches(|c: char| c == '.' || c == '!')
            ),
            source: AcceptanceCriterionSource::Proposed,
        });
    }

    out
}

/// Markdown when no active goal exists for status/cancel/complete/block.
pub fn no_active_goal_markdown(kind: CodingGoalRequestKind) -> String {
    let action = match kind {
        CodingGoalRequestKind::Cancel => "cancel",
        CodingGoalRequestKind::Complete => "complete",
        CodingGoalRequestKind::Block => "block",
        CodingGoalRequestKind::Status => "show",
        CodingGoalRequestKind::Create => "create",
    };
    format!(
        "## Coding Goal\n\nNo active coding goal to {action}.\n\nDescribe a multi-step objective (for example, “Build a login system.”) to create one. Ordinary questions and one-step edits do not create goals.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(s: &str) -> UserRequest {
        UserRequest::new(s)
    }

    #[test]
    fn creates_multi_step_examples() {
        for prompt in [
            "Build a login system.",
            "Refactor the authentication architecture.",
            "Add image generation to the application.",
            "Fix all the compiler errors in this project.",
        ] {
            let a = detect_coding_goal_request(&req(prompt)).expect(prompt);
            assert_eq!(a.kind, CodingGoalRequestKind::Create, "{prompt}");
        }
    }

    #[test]
    fn one_step_and_simple_plan_do_not_create() {
        for prompt in [
            "Rename this variable.",
            "Fix this typo.",
            "Add a comment here.",
            "Write tests.",
            "Create a parser.",
            "Build Pong.",
        ] {
            assert!(
                detect_coding_goal_request(&req(prompt)).is_none(),
                "should not create goal: {prompt}"
            );
        }
    }

    #[test]
    fn ambiguous_and_readonly_do_not_create() {
        for prompt in [
            "Improve the code.",
            "Help with authentication.",
            "What about login?",
            "Explain this function.",
            "What does AuthService do?",
            "Where is the login handler?",
        ] {
            assert!(
                detect_coding_goal_request(&req(prompt)).is_none(),
                "should not create goal: {prompt}"
            );
        }
    }

    #[test]
    fn cancel_complete_block_status_detect() {
        assert_eq!(
            detect_coding_goal_request(&req("Cancel this goal."))
                .unwrap()
                .kind,
            CodingGoalRequestKind::Cancel
        );
        assert_eq!(
            detect_coding_goal_request(&req("Mark this goal complete."))
                .unwrap()
                .kind,
            CodingGoalRequestKind::Complete
        );
        assert_eq!(
            detect_coding_goal_request(&req("This goal is blocked because OAuth keys are missing."))
                .unwrap()
                .kind,
            CodingGoalRequestKind::Block
        );
        assert_eq!(
            detect_coding_goal_request(&req("What's my current goal?"))
                .unwrap()
                .kind,
            CodingGoalRequestKind::Status
        );
    }

    #[test]
    fn create_goal_does_not_mutate_or_run_tools() {
        let assessment = detect_coding_goal_request(&req("Build a login system.")).unwrap();
        let goal = create_goal(&assessment, &ContextBundle::default(), Some("conv-1".into()));
        assert!(!goal.mutated);
        assert!(!goal.tools_executed);
        assert_eq!(goal.status, CodingGoalStatus::Active);
        assert_eq!(goal.conversation_id.as_deref(), Some("conv-1"));
        assert!(goal
            .acceptance_criteria
            .iter()
            .all(|c| matches!(
                c.source,
                AcceptanceCriterionSource::Explicit | AcceptanceCriterionSource::Proposed
            )));
        assert!(goal
            .acceptance_criteria
            .iter()
            .any(|c| c.source == AcceptanceCriterionSource::Proposed));
    }

    #[test]
    fn store_isolates_conversations() {
        let mut store = CodingGoalStore::default();
        let a = detect_coding_goal_request(&req("Build a login system.")).unwrap();
        let b = detect_coding_goal_request(&req(
            "Refactor the authentication architecture.",
        ))
        .unwrap();
        store.push_active(create_goal(&a, &ContextBundle::default(), Some("c1".into())));
        store.push_active(create_goal(&b, &ContextBundle::default(), Some("c2".into())));
        assert_eq!(
            store.active_goal(Some("c1")).unwrap().user_objective,
            "Build a login system."
        );
        assert_eq!(
            store.active_goal(Some("c2")).unwrap().user_objective,
            "Refactor the authentication architecture."
        );
        assert_eq!(store.goals_for_conversation(Some("c1")).len(), 1);
        assert_eq!(store.goals.len(), 2);
    }

    #[test]
    fn cancel_complete_block_transitions() {
        let assessment = detect_coding_goal_request(&req("Build a login system.")).unwrap();
        let goal = create_goal(&assessment, &ContextBundle::default(), None);
        let cancelled = cancel_goal(goal.clone(), "User cancelled.");
        assert_eq!(cancelled.status, CodingGoalStatus::Cancelled);
        assert_eq!(
            cancelled.agent_lifecycle.state,
            crate::agent_state_machine::AgentLifecycleState::Cancelled
        );
        assert!(!cancelled.mutated);

        assert!(complete_goal(goal.clone(), None).is_err());

        let completed = complete_goal(
            goal.clone(),
            Some(crate::agent_state_machine::CompletionEvidence {
                summary: "Verification passed.".into(),
                source: crate::agent_state_machine::CompletionEvidenceSource::VerificationPassed,
                verification_passed: true,
                references: vec!["ver-1".into()],
            }),
        )
        .unwrap();
        assert_eq!(completed.status, CodingGoalStatus::Completed);
        assert_eq!(
            completed.agent_lifecycle.state,
            crate::agent_state_machine::AgentLifecycleState::Completed
        );
        assert!(completed.remaining_work.is_empty());

        let blocked = block_goal(goal, Some("Missing API keys".into()));
        assert_eq!(blocked.status, CodingGoalStatus::Blocked);
        assert_eq!(
            blocked.agent_lifecycle.state,
            crate::agent_state_machine::AgentLifecycleState::Blocked
        );
        assert!(blocked.blockers.iter().any(|b| b.contains("API")));
    }

    #[test]
    fn steals_multi_step_from_coding_plan() {
        assert!(steals_from_coding_plan_detect(
            "build a login system."
        ));
        assert!(steals_from_coding_plan_detect("build pong."));
        assert!(steals_from_coding_plan_detect("build me a pong game."));
        assert!(!steals_from_coding_plan_detect("write tests."));
        assert!(!steals_from_coding_plan_detect("create a parser."));
    }
}
