//! Goal Decomposition (Sprint C5.2).
//!
//! Turn a [`CodingGoal`] into a structured set of **proposed** tasks.
//! Planner owns the resulting goal state. Reasoning may later propose
//! structure; this sprint uses Planner-owned deterministic templates so
//! tests do not require a model — and never executes tasks.
//!
//! Constitutional constraints:
//! - Planner-owned; no second orchestration / AgentContextEngine
//! - Do not execute tasks, mutate files, or auto-approve plans
//! - Reuse C2 investigation + C3 Coding Plans when work runs later (notes only here)
//! - Preserve goal constraints; do not invent unnecessary work
//! - Detect circular dependencies; refuse impossible orderings
//! - C0–C4 / C5.1 remain closed; no autonomous task execution / agent state machine

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use jaymi_core::UserRequest;

use crate::coding_goal::{CodingGoal, CodingGoalStatus};

/// Lifecycle of one proposed goal task (not an ExecutionPlan step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskStatus {
    Pending,
    InProgress,
    AwaitingApproval,
    Completed,
    Failed,
    Blocked,
    Cancelled,
}

impl GoalTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "In progress",
            Self::AwaitingApproval => "Awaiting approval",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Blocked => "Blocked",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

impl Default for GoalTaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Relative risk of a proposed task (observational — not ToolRisk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskRisk {
    Low,
    Medium,
    High,
}

impl GoalTaskRisk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

impl Default for GoalTaskRisk {
    fn default() -> Self {
        Self::Medium
    }
}

/// One proposed task inside a Coding Goal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalTask {
    pub id: String,
    pub description: String,
    pub objective: String,
    /// Task IDs that must reach Completed before this task may start.
    pub dependencies: Vec<String>,
    pub relevant_files: Vec<String>,
    pub relevant_context: Vec<String>,
    /// Expected verification when this task is later executed (C4) — proposal only.
    pub expected_verification: Option<String>,
    pub status: GoalTaskStatus,
    pub risk: GoalTaskRisk,
    pub mutation_expected: bool,
    /// Hints that later work should reuse C2 / C3 (not invoked here).
    pub reuse_hints: Vec<String>,
    pub notes: Vec<String>,
}

impl Default for GoalTask {
    fn default() -> Self {
        Self {
            id: String::new(),
            description: String::new(),
            objective: String::new(),
            dependencies: Vec::new(),
            relevant_files: Vec::new(),
            relevant_context: Vec::new(),
            expected_verification: None,
            status: GoalTaskStatus::Pending,
            risk: GoalTaskRisk::Medium,
            mutation_expected: false,
            reuse_hints: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// Outcome of a decomposition soft-path turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecompositionStatus {
    /// Tasks proposed and attached to the active goal.
    Proposed,
    /// Task status transition applied.
    TaskUpdated,
    /// No active Coding Goal to decompose.
    NoActiveGoal,
    /// Request too vague / no goal context.
    Ambiguous,
    /// Cycle detected in proposed or existing dependencies.
    CircularDependency,
    /// Dependency references a missing task id.
    InvalidDependencies,
}

impl DecompositionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::TaskUpdated => "task_updated",
            Self::NoActiveGoal => "no_active_goal",
            Self::Ambiguous => "ambiguous",
            Self::CircularDependency => "circular_dependency",
            Self::InvalidDependencies => "invalid_dependencies",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecompositionRequestKind {
    Decompose,
    CancelTask,
    CompleteTask,
    /// Explicit status transition (e.g. start / await approval) for tests + UX.
    SetTaskStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompositionAssessment {
    pub kind: DecompositionRequestKind,
    pub matched_rules: Vec<&'static str>,
    pub task_id: Option<String>,
    pub target_status: Option<GoalTaskStatus>,
}

/// Planner-owned decomposition report (conversation + diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalDecompositionReport {
    pub status: DecompositionStatus,
    pub goal_id: Option<String>,
    pub goal_objective: Option<String>,
    pub tasks: Vec<GoalTask>,
    /// Topological order of task IDs when acyclic.
    pub order: Vec<String>,
    pub preserved_constraints: Vec<String>,
    pub circular_dependency: Option<Vec<String>>,
    pub notes: Vec<String>,
    pub matched_rules: Vec<String>,
    /// Always false — decomposition never mutates the workspace.
    pub mutated: bool,
    /// Always false — decomposition never runs tools.
    pub tools_executed: bool,
    /// Always false — never auto-approves Coding / Execution plans.
    pub auto_approved: bool,
}

impl Default for GoalDecompositionReport {
    fn default() -> Self {
        Self {
            status: DecompositionStatus::Ambiguous,
            goal_id: None,
            goal_objective: None,
            tasks: Vec::new(),
            order: Vec::new(),
            preserved_constraints: Vec::new(),
            circular_dependency: None,
            notes: Vec::new(),
            matched_rules: Vec::new(),
            mutated: false,
            tools_executed: false,
            auto_approved: false,
        }
    }
}

impl GoalDecompositionReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Goal Decomposition\n");
        out.push_str(
            "\n_Proposed tasks only — nothing was executed, no files changed, and no plan was auto-approved._\n",
        );

        match self.status {
            DecompositionStatus::NoActiveGoal => {
                out.push_str(
                    "\nNo active Coding Goal. Create one first (for example, “Build a login system.”), then ask to break it into tasks.\n",
                );
                return out;
            }
            DecompositionStatus::Ambiguous => {
                out.push_str(
                    "\nI need a clearer Coding Goal before proposing tasks. Say what you want accomplished, then ask to decompose it.\n",
                );
                return out;
            }
            DecompositionStatus::CircularDependency => {
                out.push_str("\n### Status\n- **Circular dependency detected** — tasks were not applied.\n");
                if let Some(cycle) = &self.circular_dependency {
                    out.push_str(&format!("- Cycle: `{}`\n", cycle.join(" → ")));
                }
            }
            DecompositionStatus::InvalidDependencies => {
                out.push_str(
                    "\n### Status\n- **Invalid dependencies** — a task referenced a missing dependency id.\n",
                );
            }
            DecompositionStatus::Proposed => {
                out.push_str("\n### Status\n- **Proposed** — attached to the active Coding Goal.\n");
            }
            DecompositionStatus::TaskUpdated => {
                out.push_str("\n### Status\n- **Task updated** — status transition only.\n");
            }
        }

        if let Some(gid) = &self.goal_id {
            out.push_str(&format!("\n### Goal\n- ID: `{gid}`\n"));
        }
        if let Some(obj) = &self.goal_objective {
            out.push_str(&format!("- Objective: {obj}\n"));
        }

        if !self.preserved_constraints.is_empty() {
            out.push_str("\n### Preserved constraints\n");
            for c in &self.preserved_constraints {
                out.push_str("- ");
                out.push_str(c);
                out.push('\n');
            }
        }

        if !self.tasks.is_empty() {
            out.push_str("\n### Proposed tasks\n");
            for (i, task) in self.tasks.iter().enumerate() {
                out.push_str(&format!(
                    "{}. **{}** (`{}`) — {}\n",
                    i + 1,
                    task.description,
                    task.id,
                    task.status.label()
                ));
                out.push_str(&format!("   - Objective: {}\n", task.objective));
                if !task.dependencies.is_empty() {
                    out.push_str(&format!(
                        "   - Depends on: `{}`\n",
                        task.dependencies.join("`, `")
                    ));
                }
                out.push_str(&format!(
                    "   - Risk: {} · Mutation expected: {}\n",
                    task.risk.label(),
                    task.mutation_expected
                ));
                if let Some(v) = &task.expected_verification {
                    out.push_str(&format!("   - Expected verification: {v}\n"));
                }
                if !task.reuse_hints.is_empty() {
                    out.push_str(&format!("   - Later reuse: {}\n", task.reuse_hints.join("; ")));
                }
            }
        }

        if !self.order.is_empty() {
            out.push_str(&format!(
                "\n### Suggested order\n- `{}`\n",
                self.order.join("` → `")
            ));
        }

        out.push_str(&format!(
            "\n### Safety\n- mutated={} · tools_executed={} · auto_approved={}\n",
            self.mutated, self.tools_executed, self.auto_approved
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
            "\nTasks are proposals. Later work uses existing C2 investigation and C3 Coding Plans under Review Before Action — not a parallel task runner.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "goal_decomposition status={} goal={:?} tasks={} order={} circular={} mutated={} tools_executed={} auto_approved={}",
            self.status.as_str(),
            self.goal_id.as_deref().unwrap_or("—"),
            self.tasks.len(),
            self.order.len(),
            self.circular_dependency.is_some(),
            self.mutated,
            self.tools_executed,
            self.auto_approved,
        )
    }
}

fn next_task_id(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{seq}")
}

/// Detect decompose / task status intents.
pub fn detect_decomposition_request(request: &UserRequest) -> Option<DecompositionAssessment> {
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

    // Never steal goal create/cancel/complete/block/status or C4 paths.
    if crate::coding_goal::detect_coding_goal_request(request).is_some()
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::fix_from_verification::steals_from_other_detect(request)
    {
        return None;
    }

    if let Some((task_id, status, rule)) = looks_like_task_status(&trimmed) {
        let kind = match status {
            GoalTaskStatus::Cancelled => DecompositionRequestKind::CancelTask,
            GoalTaskStatus::Completed => DecompositionRequestKind::CompleteTask,
            _ => DecompositionRequestKind::SetTaskStatus,
        };
        return Some(DecompositionAssessment {
            kind,
            matched_rules: vec![rule],
            task_id: Some(task_id),
            target_status: Some(status),
        });
    }

    if looks_like_decompose(&trimmed) {
        return Some(DecompositionAssessment {
            kind: DecompositionRequestKind::Decompose,
            matched_rules: vec!["decompose_goal"],
            task_id: None,
            target_status: None,
        });
    }

    // Vague “break things down” without goal language → ambiguous soft path
    // only when it clearly asks for steps but not a coding goal create.
    if looks_like_ambiguous_decompose(&trimmed) {
        return Some(DecompositionAssessment {
            kind: DecompositionRequestKind::Decompose,
            matched_rules: vec!["decompose_ambiguous"],
            task_id: None,
            target_status: None,
        });
    }

    None
}

fn looks_like_decompose(trimmed: &str) -> bool {
    trimmed == "decompose the goal"
        || trimmed == "decompose this goal"
        || trimmed == "break down the goal"
        || trimmed == "break down this goal"
        || trimmed == "break this goal into tasks"
        || trimmed == "break the goal into tasks"
        || trimmed == "propose tasks"
        || trimmed == "propose tasks for this goal"
        || trimmed == "propose tasks for the goal"
        || trimmed == "what are the steps"
        || trimmed == "what are the steps for this goal"
        || trimmed == "list the tasks"
        || trimmed == "list tasks for this goal"
        || trimmed.starts_with("decompose ")
        || trimmed.starts_with("break this goal")
        || trimmed.starts_with("break the goal into")
        || trimmed.starts_with("break down this goal")
        || trimmed.starts_with("propose tasks for")
        || trimmed.starts_with("split this goal")
        || trimmed.starts_with("split the goal")
}

fn looks_like_ambiguous_decompose(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "break it down"
            | "break things down"
            | "what are the steps for that"
            | "make a task list"
            | "task list please"
    )
}

fn looks_like_task_status(trimmed: &str) -> Option<(String, GoalTaskStatus, &'static str)> {
    let patterns: &[(&str, GoalTaskStatus, &str)] = &[
        ("cancel task ", GoalTaskStatus::Cancelled, "cancel_task"),
        ("cancelled task ", GoalTaskStatus::Cancelled, "cancel_task"),
        ("complete task ", GoalTaskStatus::Completed, "complete_task"),
        ("mark task ", GoalTaskStatus::Completed, "complete_task_mark"), // refined below
        ("start task ", GoalTaskStatus::InProgress, "start_task"),
        ("begin task ", GoalTaskStatus::InProgress, "start_task"),
        ("block task ", GoalTaskStatus::Blocked, "block_task"),
        ("fail task ", GoalTaskStatus::Failed, "fail_task"),
    ];

    for (prefix, status, rule) in patterns {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let id = rest
                .trim()
                .trim_start_matches("as ")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| c == '`' || c == '"' || c == '\'')
                .to_string();
            if id.is_empty() {
                continue;
            }
            // "mark task X complete" / "mark task X as complete"
            if *rule == "complete_task_mark" {
                if trimmed.contains("complete") {
                    return Some((id, GoalTaskStatus::Completed, "complete_task"));
                }
                if trimmed.contains("awaiting approval") || trimmed.contains("awaiting_approval") {
                    return Some((id, GoalTaskStatus::AwaitingApproval, "await_task"));
                }
                if trimmed.contains("in progress") || trimmed.contains("in_progress") {
                    return Some((id, GoalTaskStatus::InProgress, "start_task"));
                }
                if trimmed.contains("blocked") {
                    return Some((id, GoalTaskStatus::Blocked, "block_task"));
                }
                if trimmed.contains("failed") || trimmed.contains("fail") {
                    return Some((id, GoalTaskStatus::Failed, "fail_task"));
                }
                if trimmed.contains("cancel") {
                    return Some((id, GoalTaskStatus::Cancelled, "cancel_task"));
                }
                continue;
            }
            return Some((id, *status, rule));
        }
    }

    // "cancel task-1" style without space after "task" already handled.
    // "mark task-1 complete"
    if trimmed.starts_with("mark task") && trimmed.contains("complete") {
        let id = extract_task_id_token(trimmed)?;
        return Some((id, GoalTaskStatus::Completed, "complete_task"));
    }
    if trimmed.starts_with("cancel task") {
        let id = extract_task_id_token(trimmed)?;
        return Some((id, GoalTaskStatus::Cancelled, "cancel_task"));
    }

    None
}

fn extract_task_id_token(trimmed: &str) -> Option<String> {
    for token in trimmed.split_whitespace() {
        let t = token.trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == ',');
        if t.starts_with("task-") || (t.starts_with('t') && t.contains('-') && t.len() > 2) {
            return Some(t.to_string());
        }
    }
    None
}

/// Validate dependencies: all ids exist, no cycles. Returns topo order or cycle path.
pub fn validate_dependencies(
    tasks: &[GoalTask],
) -> Result<Vec<String>, DependencyError> {
    let ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    for task in tasks {
        for dep in &task.dependencies {
            if !ids.contains(dep.as_str()) {
                return Err(DependencyError::Missing {
                    task_id: task.id.clone(),
                    missing: dep.clone(),
                });
            }
            if dep == &task.id {
                return Err(DependencyError::Circular {
                    cycle: vec![task.id.clone(), task.id.clone()],
                });
            }
        }
    }

    // Kahn topological sort + cycle detection (declaration order for zero-indegree ties).
    let mut indegree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for task in tasks {
        indegree.entry(task.id.as_str()).or_insert(0);
        for dep in &task.dependencies {
            adj.entry(dep.as_str()).or_default().push(task.id.as_str());
            *indegree.entry(task.id.as_str()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = tasks
        .iter()
        .map(|t| t.id.as_str())
        .filter(|id| indegree.get(id).copied().unwrap_or(0) == 0)
        .collect();

    let mut order = Vec::new();
    let mut indegree_mut = indegree;
    while let Some(id) = queue.pop_front() {
        order.push(id.to_string());
        if let Some(children) = adj.get(id) {
            for child in children {
                if let Some(d) = indegree_mut.get_mut(child) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
    }

    if order.len() != tasks.len() {
        let cycle = find_cycle(tasks).unwrap_or_else(|| {
            tasks
                .iter()
                .filter(|t| indegree_mut.get(t.id.as_str()).copied().unwrap_or(0) > 0)
                .map(|t| t.id.clone())
                .collect()
        });
        return Err(DependencyError::Circular { cycle });
    }

    Ok(order)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyError {
    Missing { task_id: String, missing: String },
    Circular { cycle: Vec<String> },
}

fn find_cycle(tasks: &[GoalTask]) -> Option<Vec<String>> {
    let map: HashMap<&str, &GoalTask> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    fn dfs<'a>(
        id: &'a str,
        map: &HashMap<&str, &'a GoalTask>,
        visiting: &mut HashSet<&'a str>,
        visited: &mut HashSet<&'a str>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        if visited.contains(id) {
            return None;
        }
        if visiting.contains(id) {
            let mut cycle = vec![id.to_string()];
            for s in stack.iter().rev() {
                cycle.push((*s).to_string());
                if *s == id {
                    break;
                }
            }
            cycle.reverse();
            return Some(cycle);
        }
        visiting.insert(id);
        stack.push(id);
        if let Some(task) = map.get(id) {
            for dep in &task.dependencies {
                // Edge dep → task means we walk from task to deps for "waiting on";
                // for cycle, walk dependency edges as task → dep (depends-on graph).
                if let Some(c) = dfs(dep.as_str(), map, visiting, visited, stack) {
                    return Some(c);
                }
            }
        }
        stack.pop();
        visiting.remove(id);
        visited.insert(id);
        None
    }

    for task in tasks {
        if let Some(c) = dfs(
            task.id.as_str(),
            &map,
            &mut visiting,
            &mut visited,
            &mut stack,
        ) {
            return Some(c);
        }
    }
    None
}

fn wants_single_task(goal: &CodingGoal) -> bool {
    goal.constraints.iter().any(|c| {
        let l = c.to_ascii_lowercase();
        l.contains("single task")
            || l.contains("one task")
            || l.contains("do not break")
            || l.contains("don't break")
            || l.contains("as one task")
            || l.contains("keep this as one")
    })
}

/// Propose tasks for a goal (Planner-owned templates). Does not mutate the goal.
pub fn propose_tasks(goal: &CodingGoal) -> Result<(Vec<GoalTask>, Vec<String>), DependencyError> {
    let files = goal.relevant_files.clone();
    let constraints = goal.constraints.clone();
    let lower = goal.user_objective.to_ascii_lowercase();

    let mut tasks = if wants_single_task(goal) {
        vec![single_task(goal, &files, &constraints)]
    } else if lower.contains("login") || (lower.contains("auth") && lower.contains("system")) {
        login_system_tasks(goal, &files, &constraints)
    } else if lower.contains("refactor") && lower.contains("architecture") {
        refactor_architecture_tasks(goal, &files, &constraints)
    } else if lower.contains("fix all")
        && (lower.contains("error") || lower.contains("compiler") || lower.contains("test"))
    {
        fix_all_errors_tasks(goal, &files, &constraints)
    } else if lower.starts_with("add ")
        && (lower.contains(" to the application")
            || lower.contains(" to the app")
            || lower.contains(" to the project"))
    {
        add_feature_tasks(goal, &files, &constraints)
    } else {
        // Generic multi-step without inventing UI/session specifics.
        generic_multi_tasks(goal, &files, &constraints)
    };

    // Stamp preserved constraints onto every task note once.
    if !constraints.is_empty() {
        for task in &mut tasks {
            task.notes.push(format!(
                "Preserved goal constraints: {}",
                constraints.join("; ")
            ));
        }
    }

    let order = validate_dependencies(&tasks)?;
    Ok((tasks, order))
}

fn base_task(
    description: &str,
    objective: &str,
    deps: Vec<String>,
    files: &[String],
    risk: GoalTaskRisk,
    mutation_expected: bool,
    verification: Option<&str>,
    reuse: &[&str],
) -> GoalTask {
    GoalTask {
        id: next_task_id("task"),
        description: description.into(),
        objective: objective.into(),
        dependencies: deps,
        relevant_files: files.to_vec(),
        relevant_context: Vec::new(),
        expected_verification: verification.map(|s| s.to_string()),
        status: GoalTaskStatus::Pending,
        risk,
        mutation_expected,
        reuse_hints: reuse.iter().map(|s| (*s).to_string()).collect(),
        notes: vec![
            "Proposed only — not executed.".into(),
            "Details may be refined after investigation.".into(),
        ],
    }
}

fn single_task(goal: &CodingGoal, files: &[String], _constraints: &[String]) -> GoalTask {
    let mut t = base_task(
        "Accomplish the stated objective",
        &goal.user_objective,
        Vec::new(),
        files,
        GoalTaskRisk::Medium,
        true,
        Some("Verify the change against the goal’s acceptance criteria (C4 when run)."),
        &["C3 Coding Plan when implementing", "C4 verification when checking"],
    );
    t.notes.push("Single-task decomposition per goal constraint.".into());
    t
}

fn login_system_tasks(goal: &CodingGoal, files: &[String], _constraints: &[String]) -> Vec<GoalTask> {
    let t1 = base_task(
        "Investigate existing authentication architecture",
        "Map how auth works today (or confirm greenfield) before designing changes.",
        Vec::new(),
        files,
        GoalTaskRisk::Low,
        false,
        Some("No mutation — investigation evidence only (C2)."),
        &["C2 investigation / codebase Q&A"],
    );
    let t1_id = t1.id.clone();

    let t2 = base_task(
        "Design authentication flow",
        "Propose login / session flow consistent with the goal and constraints.",
        vec![t1_id.clone()],
        files,
        GoalTaskRisk::Low,
        false,
        Some("Design review in conversation — no execution."),
        &["C3 Coding Plan (design section) later"],
    );
    let t2_id = t2.id.clone();

    let t3 = base_task(
        "Implement authentication module",
        "Add or update auth module code per the agreed design.",
        vec![t2_id.clone()],
        files,
        GoalTaskRisk::High,
        true,
        Some("Compile / typecheck after implementation (C4)."),
        &["C2 investigation if needed", "C3 Coding Plan + Review Before Action"],
    );
    let t3_id = t3.id.clone();

    let t4 = base_task(
        "Implement login UI",
        "Add or update login user-facing surface if the project has a UI layer.",
        vec![t3_id.clone()],
        files,
        GoalTaskRisk::Medium,
        true,
        Some("Manual/UI smoke check when applicable."),
        &["C3 Coding Plan + Review Before Action"],
    );
    let t4_id = t4.id.clone();

    let t5 = base_task(
        "Add session handling",
        "Persist and validate sessions according to constraints.",
        vec![t3_id.clone()],
        files,
        GoalTaskRisk::High,
        true,
        Some("Session lifecycle checks / targeted tests (C4)."),
        &["C3 Coding Plan + Review Before Action"],
    );
    let t5_id = t5.id.clone();

    let t6 = base_task(
        "Add tests",
        "Cover auth/login/session behavior with project-appropriate tests.",
        vec![t3_id.clone(), t5_id.clone()],
        files,
        GoalTaskRisk::Medium,
        true,
        Some("Run the project’s test command (C4.2)."),
        &["C3 Coding Plan", "C4 test execution"],
    );
    let t6_id = t6.id.clone();

    let t7 = base_task(
        "Verify implementation",
        &format!("Verify the goal is met: {}", goal.user_objective),
        vec![t4_id, t6_id],
        files,
        GoalTaskRisk::Low,
        false,
        Some("Build/test verification summary (C4.5) — no silent pass claims."),
        &["C4 verification"],
    );

    vec![t1, t2, t3, t4, t5, t6, t7]
}

fn refactor_architecture_tasks(
    goal: &CodingGoal,
    files: &[String],
    _constraints: &[String],
) -> Vec<GoalTask> {
    let t1 = base_task(
        "Investigate current architecture",
        "Map modules, boundaries, and call sites before refactoring.",
        Vec::new(),
        files,
        GoalTaskRisk::Low,
        false,
        Some("Investigation evidence only (C2)."),
        &["C2 investigation / relationship tracing"],
    );
    let t1_id = t1.id.clone();
    let t2 = base_task(
        "Design target architecture",
        "Propose the refactored structure without implementing it yet.",
        vec![t1_id.clone()],
        files,
        GoalTaskRisk::Low,
        false,
        Some("Design review in conversation."),
        &["C3 Coding Plan (proposal)"],
    );
    let t2_id = t2.id.clone();
    let t3 = base_task(
        "Implement refactor in reviewed steps",
        &format!("Apply the refactor for: {}", goal.user_objective),
        vec![t2_id.clone()],
        files,
        GoalTaskRisk::High,
        true,
        Some("Compile check after each approved step (C4)."),
        &["C3 Coding Plan + Review Before Action"],
    );
    let t3_id = t3.id.clone();
    let t4 = base_task(
        "Add or update tests",
        "Protect refactored behavior with tests.",
        vec![t3_id.clone()],
        files,
        GoalTaskRisk::Medium,
        true,
        Some("Run tests (C4.2)."),
        &["C3 Coding Plan", "C4 test execution"],
    );
    let t4_id = t4.id.clone();
    let t5 = base_task(
        "Verify refactor",
        "Confirm architecture goal and constraints still hold.",
        vec![t4_id],
        files,
        GoalTaskRisk::Low,
        false,
        Some("Verification summary (C4.5)."),
        &["C4 verification"],
    );
    vec![t1, t2, t3, t4, t5]
}

fn fix_all_errors_tasks(
    goal: &CodingGoal,
    files: &[String],
    _constraints: &[String],
) -> Vec<GoalTask> {
    let t1 = base_task(
        "Investigate compiler / test failures",
        "Collect and classify current failures before editing.",
        Vec::new(),
        files,
        GoalTaskRisk::Low,
        false,
        Some("Build/test diagnostics (C4.1–C4.3) — observational."),
        &["C4 verification diagnostics", "C2 investigation"],
    );
    let t1_id = t1.id.clone();
    let t2 = base_task(
        "Fix failures via reviewed coding plans",
        &format!("Address failures for: {}", goal.user_objective),
        vec![t1_id.clone()],
        files,
        GoalTaskRisk::High,
        true,
        Some("Re-run failing checks after approved fixes (C4)."),
        &["C4.4 fix-from-verification → C3 plan", "Review Before Action"],
    );
    let t2_id = t2.id.clone();
    let t3 = base_task(
        "Verify clean build/tests",
        "Confirm the failure set is cleared without inventing passes.",
        vec![t2_id],
        files,
        GoalTaskRisk::Low,
        false,
        Some("C4.5 verification summary."),
        &["C4 verification"],
    );
    vec![t1, t2, t3]
}

fn add_feature_tasks(
    goal: &CodingGoal,
    files: &[String],
    _constraints: &[String],
) -> Vec<GoalTask> {
    let t1 = base_task(
        "Investigate integration points",
        "Find where the feature should plug into the existing application.",
        Vec::new(),
        files,
        GoalTaskRisk::Low,
        false,
        Some("Investigation only (C2)."),
        &["C2 investigation"],
    );
    let t1_id = t1.id.clone();
    let t2 = base_task(
        "Implement the feature",
        &goal.user_objective,
        vec![t1_id.clone()],
        files,
        GoalTaskRisk::High,
        true,
        Some("Compile / typecheck (C4)."),
        &["C3 Coding Plan + Review Before Action"],
    );
    let t2_id = t2.id.clone();
    let t3 = base_task(
        "Add tests",
        "Cover the new feature with appropriate tests.",
        vec![t2_id.clone()],
        files,
        GoalTaskRisk::Medium,
        true,
        Some("Run tests (C4.2)."),
        &["C3 Coding Plan", "C4 test execution"],
    );
    let t3_id = t3.id.clone();
    let t4 = base_task(
        "Verify the feature",
        "Confirm the feature meets the goal.",
        vec![t3_id],
        files,
        GoalTaskRisk::Low,
        false,
        Some("Verification summary (C4.5)."),
        &["C4 verification"],
    );
    vec![t1, t2, t3, t4]
}

fn generic_multi_tasks(
    goal: &CodingGoal,
    files: &[String],
    _constraints: &[String],
) -> Vec<GoalTask> {
    let t1 = base_task(
        "Investigate the current codebase",
        "Gather enough context before proposing changes.",
        Vec::new(),
        files,
        GoalTaskRisk::Low,
        false,
        Some("Investigation only (C2)."),
        &["C2 investigation"],
    );
    let t1_id = t1.id.clone();
    let t2 = base_task(
        "Implement required changes",
        &goal.user_objective,
        vec![t1_id.clone()],
        files,
        GoalTaskRisk::High,
        true,
        Some("Compile / checks (C4)."),
        &["C3 Coding Plan + Review Before Action"],
    );
    let t2_id = t2.id.clone();
    let t3 = base_task(
        "Verify the outcome",
        "Check the goal against acceptance criteria.",
        vec![t2_id],
        files,
        GoalTaskRisk::Low,
        false,
        Some("C4 verification when run."),
        &["C4 verification"],
    );
    vec![t1, t2, t3]
}

/// Attach proposed tasks onto the goal (Planner-owned state update).
pub fn apply_decomposition(
    mut goal: CodingGoal,
    tasks: Vec<GoalTask>,
    order: Vec<String>,
) -> CodingGoal {
    goal.tasks = tasks;
    goal.remaining_work = goal
        .tasks
        .iter()
        .filter(|t| !t.status.is_terminal())
        .map(|t| t.description.clone())
        .collect();
    goal.current_task = order.first().and_then(|id| {
        goal.tasks
            .iter()
            .find(|t| &t.id == id)
            .map(|t| t.description.clone())
    });
    goal.updated_at = now_stamp();
    goal.mutated = false;
    goal.tools_executed = false;
    goal.notes.push(format!(
        "Decomposed into {} proposed task(s) — none executed.",
        goal.tasks.len()
    ));
    goal
}

fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Transition a task status with basic legality checks (no execution).
pub fn transition_task_status(
    mut goal: CodingGoal,
    task_id: &str,
    to: GoalTaskStatus,
) -> Result<(CodingGoal, GoalTask), String> {
    let idx = goal
        .tasks
        .iter()
        .position(|t| t.id == task_id)
        .ok_or_else(|| format!("Unknown task id `{task_id}`"))?;

    let from = goal.tasks[idx].status;
    if from.is_terminal() && to != from {
        return Err(format!(
            "Task `{task_id}` is already {} — cannot transition to {}.",
            from.label(),
            to.label()
        ));
    }

    // Completing requires dependencies completed (prevents impossible ordering).
    if to == GoalTaskStatus::Completed {
        let deps = goal.tasks[idx].dependencies.clone();
        for dep in deps {
            let dep_status = goal
                .tasks
                .iter()
                .find(|t| t.id == dep)
                .map(|t| t.status)
                .ok_or_else(|| format!("Missing dependency `{dep}`"))?;
            if dep_status != GoalTaskStatus::Completed {
                return Err(format!(
                    "Cannot complete `{task_id}` while dependency `{dep}` is {}.",
                    dep_status.label()
                ));
            }
        }
    }

    // Starting in progress also requires deps completed.
    if to == GoalTaskStatus::InProgress || to == GoalTaskStatus::AwaitingApproval {
        let deps = goal.tasks[idx].dependencies.clone();
        for dep in deps {
            let dep_status = goal
                .tasks
                .iter()
                .find(|t| t.id == dep)
                .map(|t| t.status)
                .ok_or_else(|| format!("Missing dependency `{dep}`"))?;
            if dep_status != GoalTaskStatus::Completed {
                return Err(format!(
                    "Cannot set `{task_id}` to {} while dependency `{dep}` is {}.",
                    to.label(),
                    dep_status.label()
                ));
            }
        }
    }

    goal.tasks[idx].status = to;
    goal.tasks[idx]
        .notes
        .push(format!("Status: {} → {}.", from.label(), to.label()));
    goal.updated_at = now_stamp();
    goal.mutated = false;
    goal.tools_executed = false;

    goal.remaining_work = goal
        .tasks
        .iter()
        .filter(|t| !t.status.is_terminal())
        .map(|t| t.description.clone())
        .collect();
    goal.current_task = goal
        .tasks
        .iter()
        .find(|t| t.status == GoalTaskStatus::InProgress)
        .or_else(|| {
            goal.tasks
                .iter()
                .find(|t| t.status == GoalTaskStatus::Pending)
        })
        .map(|t| t.description.clone());

    if to == GoalTaskStatus::Completed {
        let desc = goal.tasks[idx].description.clone();
        if !goal.completed_work.iter().any(|c| c == &desc) {
            goal.completed_work.push(desc);
        }
    }

    let task = goal.tasks[idx].clone();
    Ok((goal, task))
}

/// Run decompose against an optional active goal.
pub fn decompose_active_goal(
    goal: Option<&CodingGoal>,
    assessment: &DecompositionAssessment,
) -> (GoalDecompositionReport, Option<CodingGoal>) {
    let mut report = GoalDecompositionReport {
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        mutated: false,
        tools_executed: false,
        auto_approved: false,
        ..Default::default()
    };

    if assessment.matched_rules.iter().any(|r| *r == "decompose_ambiguous") && goal.is_none() {
        report.status = DecompositionStatus::Ambiguous;
        report.notes.push(
            "Ambiguous decomposition request with no active Coding Goal.".into(),
        );
        return (report, None);
    }

    let Some(goal) = goal else {
        report.status = DecompositionStatus::NoActiveGoal;
        return (report, None);
    };

    if goal.status != CodingGoalStatus::Active && goal.status != CodingGoalStatus::Blocked {
        report.status = DecompositionStatus::Ambiguous;
        report.goal_id = Some(goal.id.clone());
        report.notes.push(format!(
            "Goal is {} — decompose/active task updates expect an active (or blocked) goal.",
            goal.status.label()
        ));
        return (report, None);
    }

    report.goal_id = Some(goal.id.clone());
    report.goal_objective = Some(goal.user_objective.clone());
    report.preserved_constraints = goal.constraints.clone();

    match assessment.kind {
        DecompositionRequestKind::Decompose => match propose_tasks(goal) {
            Ok((tasks, order)) => {
                report.status = DecompositionStatus::Proposed;
                report.tasks = tasks.clone();
                report.order = order.clone();
                report.notes.push(
                    "Planner-owned proposal. Reasoning was not required for this template decomposition."
                        .into(),
                );
                report.notes.push(
                    "No C2/C3/C4 tools ran — reuse hints describe later Review Before Action paths."
                        .into(),
                );
                let updated = apply_decomposition(goal.clone(), tasks, order);
                (report, Some(updated))
            }
            Err(DependencyError::Circular { cycle }) => {
                report.status = DecompositionStatus::CircularDependency;
                report.circular_dependency = Some(cycle);
                report.notes.push("Refused to apply a cyclic task graph.".into());
                (report, None)
            }
            Err(DependencyError::Missing { task_id, missing }) => {
                report.status = DecompositionStatus::InvalidDependencies;
                report.notes.push(format!(
                    "Task `{task_id}` depends on missing `{missing}`."
                ));
                (report, None)
            }
        },
        DecompositionRequestKind::CancelTask
        | DecompositionRequestKind::CompleteTask
        | DecompositionRequestKind::SetTaskStatus => {
            let Some(task_id) = &assessment.task_id else {
                report.status = DecompositionStatus::Ambiguous;
                report.notes.push("Missing task id.".into());
                return (report, None);
            };
            let Some(to) = assessment.target_status else {
                report.status = DecompositionStatus::Ambiguous;
                return (report, None);
            };
            match transition_task_status(goal.clone(), task_id, to) {
                Ok((updated, _)) => {
                    report.status = DecompositionStatus::TaskUpdated;
                    report.tasks = updated.tasks.clone();
                    if let Ok(order) = validate_dependencies(&updated.tasks) {
                        report.order = order;
                    }
                    report.notes.push(format!(
                        "Task `{task_id}` → {} (no execution).",
                        to.label()
                    ));
                    (report, Some(updated))
                }
                Err(msg) => {
                    report.status = DecompositionStatus::InvalidDependencies;
                    report.tasks = goal.tasks.clone();
                    report.notes.push(msg);
                    (report, None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_goal::{create_goal, detect_coding_goal_request};
    use jaymi_context::ContextBundle;

    fn goal(objective: &str) -> CodingGoal {
        let a = detect_coding_goal_request(&UserRequest::new(objective)).expect("goal");
        create_goal(&a, &ContextBundle::default(), None)
    }

    #[test]
    fn multi_task_login_system() {
        let g = goal("Build a login system.");
        let (tasks, order) = propose_tasks(&g).unwrap();
        assert!(tasks.len() >= 5);
        assert_eq!(order.len(), tasks.len());
        assert!(tasks.iter().any(|t| t.description.contains("Investigate")));
        assert!(tasks.iter().any(|t| t.description.contains("Verify")));
        assert!(tasks.iter().any(|t| t.mutation_expected));
        assert!(tasks.iter().any(|t| !t.mutation_expected));
    }

    #[test]
    fn single_task_when_constrained() {
        let mut g = goal("Build a login system.");
        g.constraints
            .push("Keep this as one task only.".into());
        let (tasks, order) = propose_tasks(&g).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(order.len(), 1);
        assert!(tasks[0]
            .notes
            .iter()
            .any(|n| n.contains("Preserved goal constraints")));
    }

    #[test]
    fn detects_circular_dependencies() {
        let mut a = GoalTask {
            id: "task-a".into(),
            description: "A".into(),
            dependencies: vec!["task-b".into()],
            ..Default::default()
        };
        let b = GoalTask {
            id: "task-b".into(),
            description: "B".into(),
            dependencies: vec!["task-a".into()],
            ..Default::default()
        };
        a.objective = "A".into();
        let err = validate_dependencies(&[a, b]).unwrap_err();
        match err {
            DependencyError::Circular { cycle } => {
                assert!(cycle.len() >= 2);
            }
            _ => panic!("expected circular"),
        }
    }

    #[test]
    fn dependencies_enforce_order_on_complete() {
        let g = goal("Add image generation to the application.");
        let (tasks, order) = propose_tasks(&g).unwrap();
        let updated = apply_decomposition(g, tasks, order);
        let second = updated.tasks[1].id.clone();
        let err = transition_task_status(updated.clone(), &second, GoalTaskStatus::Completed)
            .unwrap_err();
        assert!(err.contains("dependency"));

        let first = updated.tasks[0].id.clone();
        let (updated, _) =
            transition_task_status(updated, &first, GoalTaskStatus::Completed).unwrap();
        let (updated, task) =
            transition_task_status(updated, &second, GoalTaskStatus::Completed).unwrap();
        assert_eq!(task.status, GoalTaskStatus::Completed);
        assert!(!updated.mutated);
        assert!(!updated.tools_executed);
    }

    #[test]
    fn cancel_task_transition() {
        let g = goal("Refactor the authentication architecture.");
        let (tasks, order) = propose_tasks(&g).unwrap();
        let updated = apply_decomposition(g, tasks, order);
        let id = updated.tasks[0].id.clone();
        let (updated, task) =
            transition_task_status(updated, &id, GoalTaskStatus::Cancelled).unwrap();
        assert_eq!(task.status, GoalTaskStatus::Cancelled);
        assert!(updated
            .remaining_work
            .iter()
            .all(|d| d != &task.description));
    }

    #[test]
    fn preserve_constraints_on_decompose() {
        let mut g = goal("Build a login system without OAuth.");
        // ensure constraint extracted or injected
        if g.constraints.is_empty() {
            g.constraints.push("without OAuth".into());
        }
        let (tasks, _) = propose_tasks(&g).unwrap();
        assert!(tasks.iter().all(|t| {
            t.notes
                .iter()
                .any(|n| n.to_ascii_lowercase().contains("oauth") || n.contains("constraints"))
        }));
    }

    #[test]
    fn detect_decompose_phrases() {
        assert!(detect_decomposition_request(&UserRequest::new(
            "Break this goal into tasks."
        ))
        .is_some());
        assert_eq!(
            detect_decomposition_request(&UserRequest::new("Cancel task task-1"))
                .unwrap()
                .kind,
            DecompositionRequestKind::CancelTask
        );
    }

    #[test]
    fn no_mutation_flags() {
        let g = goal("Build a login system.");
        let a = detect_decomposition_request(&UserRequest::new("Decompose the goal.")).unwrap();
        let (report, updated) = decompose_active_goal(Some(&g), &a);
        assert_eq!(report.status, DecompositionStatus::Proposed);
        assert!(!report.mutated);
        assert!(!report.tools_executed);
        assert!(!report.auto_approved);
        let u = updated.unwrap();
        assert!(!u.mutated);
        assert!(!u.tools_executed);
        assert!(!u.tasks.is_empty());
    }
}
