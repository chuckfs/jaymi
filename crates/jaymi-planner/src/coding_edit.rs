//! Coding Edit Intent (Sprint C3.1).
//!
//! Distinguish **mutation** requests from **read** requests and structure a
//! Planner-owned [`CodingEditObjective`] — without writing files, invoking
//! mutation tools, or creating an executable Execution Plan.
//!
//! Constitutional constraints:
//! - Planner owns classification and orchestration
//! - No parallel coding-intent / context / reasoning systems
//! - Reuse Workspace Intelligence + Environmental Resolution for deixis
//! - Reuse C2 reading ambient evidence when present (no C2 redesign)
//! - Review Before Action preserved — no mutation executes in this sprint
//! - Conversation First — ordinary chat phrasing
//! - Offline First — deterministic objective / clarification

use jaymi_context::ContextBundle;
use jaymi_core::{CodingAction, UserRequest};

use crate::environmental::{
    resolve_environment, EnvironmentalResolution, ResolutionConfidence, ResolvedKind,
};

/// Kind of coding modification the user is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CodingEditKind {
    Fix,
    Change,
    Add,
    Rename,
    Refactor,
    Optimize,
    Remove,
    Create,
    Generic,
}

impl CodingEditKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fix => "fix",
            Self::Change => "change",
            Self::Add => "add",
            Self::Rename => "rename",
            Self::Refactor => "refactor",
            Self::Optimize => "optimize",
            Self::Remove => "remove",
            Self::Create => "create",
            Self::Generic => "generic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fix => "Fix",
            Self::Change => "Change",
            Self::Add => "Add",
            Self::Rename => "Rename",
            Self::Refactor => "Refactor",
            Self::Optimize => "Optimize",
            Self::Remove => "Remove",
            Self::Create => "Create",
            Self::Generic => "Edit",
        }
    }
}

/// Expected mutation scope (observational).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EditScope {
    Selection,
    Symbol,
    File,
    MultiFile,
    Project,
    Unknown,
}

impl EditScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::Symbol => "symbol",
            Self::File => "file",
            Self::MultiFile => "multi_file",
            Self::Project => "project",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether the objective is ready for later C3 slices or needs clarification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CodingEditStatus {
    /// Adequately specified — later slices may plan/apply under Review.
    Understood,
    /// Ambiguous — ask before inventing a target.
    NeedsClarification,
}

impl CodingEditStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Understood => "understood",
            Self::NeedsClarification => "needs_clarification",
        }
    }
}

impl Default for CodingEditStatus {
    fn default() -> Self {
        Self::NeedsClarification
    }
}

/// Structured coding edit objective (C3.1) — planning / intent only.
///
/// Not an [`crate::ExecutionPlan`]. Never executes tools or writes files.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingEditObjective {
    pub status: CodingEditStatus,
    pub kind: Option<CodingEditKind>,
    /// Original user objective.
    pub goal: String,
    /// Short description of the requested change.
    pub requested_change: String,
    pub target_project: Option<String>,
    pub target_file: Option<String>,
    /// Bound selection preview (truncated), when any.
    pub target_selection: Option<String>,
    pub target_symbols: Vec<String>,
    pub relevant_files: Vec<String>,
    /// Ambient WI / environmental notes (not a second context dump).
    pub relevant_context: Vec<String>,
    pub constraints: Vec<String>,
    pub ambiguities: Vec<String>,
    pub expected_scope: Option<EditScope>,
    /// How ambient C2 / WI evidence was consulted (observational).
    pub investigation_notes: Vec<String>,
    pub matched_rules: Vec<String>,
    /// Clarification question when [`CodingEditStatus::NeedsClarification`].
    pub clarification: Option<String>,
}

impl CodingEditObjective {
    pub fn needs_clarification(&self) -> bool {
        matches!(self.status, CodingEditStatus::NeedsClarification)
    }

    /// Conversation markdown — never dumps raw diagnostics.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Edit Intent\n");
        out.push_str(&format!(
            "\n### Status\n- {}\n",
            match self.status {
                CodingEditStatus::Understood => {
                    "I understand what you want to change (planning only — no files modified)."
                }
                CodingEditStatus::NeedsClarification => {
                    "I need a clearer target before planning a modification."
                }
            }
        ));

        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Kind\n- {}\n", kind.label()));
        }

        out.push_str("\n### Objective\n- ");
        out.push_str(if self.goal.is_empty() {
            "(none)"
        } else {
            &self.goal
        });
        out.push('\n');

        if !self.requested_change.is_empty() {
            out.push_str("\n### Requested change\n- ");
            out.push_str(&self.requested_change);
            out.push('\n');
        }

        out.push_str("\n### Resolved target\n");
        if let Some(project) = &self.target_project {
            out.push_str(&format!("- Project: `{project}`\n"));
        }
        if let Some(file) = &self.target_file {
            out.push_str(&format!("- File: `{file}`\n"));
        }
        if let Some(sel) = &self.target_selection {
            out.push_str(&format!("- Selection: `{sel}`\n"));
        }
        if !self.target_symbols.is_empty() {
            for sym in &self.target_symbols {
                out.push_str(&format!("- Symbol: `{sym}`\n"));
            }
        }
        if self.target_project.is_none()
            && self.target_file.is_none()
            && self.target_selection.is_none()
            && self.target_symbols.is_empty()
        {
            out.push_str("- _(unresolved — do not invent a path)_\n");
        }

        if let Some(scope) = self.expected_scope {
            out.push_str(&format!("\n### Expected scope\n- {}\n", scope.as_str()));
        }

        if !self.relevant_files.is_empty() {
            out.push_str("\n### Relevant files\n");
            for f in &self.relevant_files {
                out.push_str(&format!("- `{f}`\n"));
            }
        }

        if !self.ambiguities.is_empty() {
            out.push_str("\n### Ambiguity\n");
            for a in &self.ambiguities {
                out.push_str(&format!("- {a}\n"));
            }
        }

        if let Some(q) = &self.clarification {
            out.push_str("\n### Clarification needed\n");
            out.push_str(q);
            out.push('\n');
        }

        out.push_str("\n### Constraints\n");
        if self.constraints.is_empty() {
            out.push_str("- No file modifications in this turn.\n");
            out.push_str("- Mutations require Review Before Action in a later step.\n");
        } else {
            for c in &self.constraints {
                out.push_str(&format!("- {c}\n"));
            }
        }

        out
    }

    /// Compact diagnostics line (Coding Diagnostics / LastPlannerActivity).
    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "coding_edit status={} kind={} scope={} file={:?} symbols={} ambiguities={} investigation={} clarification={}",
            self.status.as_str(),
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.expected_scope.map(|s| s.as_str()).unwrap_or("unknown"),
            self.target_file.as_deref().unwrap_or("—"),
            self.target_symbols.len(),
            self.ambiguities.len(),
            self.investigation_notes.len(),
            self.needs_clarification(),
        )
    }
}

/// Detected edit-intent assessment before finalize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingEditAssessment {
    pub kind: CodingEditKind,
    pub goal: String,
    pub matched_rules: Vec<&'static str>,
    pub from_coding_action: bool,
}

impl CodingEditAssessment {
    pub fn hint_id(&self) -> String {
        format!("coding_edit:{}", self.kind.as_str())
    }
}

/// Detect a coding mutation request (Planner ownership). Soft-path only.
pub fn detect_coding_edit_request(request: &UserRequest) -> Option<CodingEditAssessment> {
    // Structured tool payloads are not C3.1 soft-path ownership.
    if request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.lsp.is_some()
        || request.search.is_some()
        || request.project_knowledge.is_some()
    {
        return None;
    }

    // Toolbar Edit / Refactor → mutation intent (selection resolved later).
    if let Some(action) = request.coding_action {
        return match action {
            CodingAction::EditSelection => Some(CodingEditAssessment {
                kind: CodingEditKind::Change,
                goal: request.content.trim().to_string(),
                matched_rules: vec!["coding_action_edit_selection"],
                from_coding_action: true,
            }),
            CodingAction::RefactorSelection => Some(CodingEditAssessment {
                kind: CodingEditKind::Refactor,
                goal: request.content.trim().to_string(),
                matched_rules: vec!["coding_action_refactor_selection"],
                from_coding_action: true,
            }),
            _ => None,
        };
    }

    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();

    // Yield to narrower / read-only / specialized planners.
    if steals_from_coding_edit_detect(&lower, request) {
        return None;
    }

    let (kind, rules) = classify_mutation_phrasing(&lower)?;
    Some(CodingEditAssessment {
        kind,
        goal: content.to_string(),
        matched_rules: rules,
        from_coding_action: false,
    })
}

/// True when C1.4 Coding Plan detect should yield to C3.1.
pub fn steals_from_coding_plan_detect(lower: &str) -> bool {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
    classify_mutation_phrasing(trimmed).is_some() && !looks_like_greenfield_plan(trimmed)
}

fn steals_from_coding_edit_detect(lower: &str, request: &UserRequest) -> bool {
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
        || crate::fix_from_verification::steals_from_other_detect(request)
        || crate::coding_goal::steals_from_coding_edit_detect(lower)
        || crate::goal_decomposition::detect_decomposition_request(request).is_some()
        || crate::agent_state_machine::detect_agent_lifecycle_request(request).is_some()
        || crate::multi_step_execution::detect_multi_step_execution_request(request).is_some()
        || crate::agent_context_maintenance::detect_agent_context_request(request).is_some()
        || crate::approval_autonomy_policy::detect_autonomy_policy_request(request).is_some()
        || crate::agent_progress_ux::detect_progress_request(request).is_some()
        || crate::agent_pause_resume_cancel::detect_control_request(request).is_some()
        || crate::agent_failure_recovery::detect_recovery_request(request).is_some()
        || crate::coding_goal_completion::detect_completion_request(request).is_some()
        || crate::agent_session_continuity::detect_continuity_request(request).is_some()
        || crate::agentic_coding_experience::detect_agentic_experience_request(request).is_some()
        || crate::pair_programming::looks_like_dead_code_request(lower)
    {
        return true;
    }
    if is_explicit_read_only(lower) {
        return true;
    }
    if looks_like_greenfield_plan(lower) {
        return true;
    }
    // Understanding ("this function") is broad — do not steal clear mutation verbs.
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
    if classify_mutation_phrasing(trimmed).is_none()
        && crate::coding_understanding::detect_understanding_request(request).is_some()
    {
        return true;
    }
    false
}

fn is_explicit_read_only(lower: &str) -> bool {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
    if trimmed.starts_with("explain ")
        || trimmed == "explain this"
        || trimmed.starts_with("where is ")
        || trimmed.starts_with("where are ")
        || trimmed.starts_with("where does ")
        || trimmed.starts_with("why ")
        || trimmed.starts_with("what does ")
        || trimmed.starts_with("what is ")
        || trimmed.starts_with("how does ")
        || trimmed.starts_with("how do ")
        || trimmed.contains("who calls")
        || trimmed.contains("find references")
        || trimmed.contains("show references")
    {
        return true;
    }
    if trimmed.starts_with("review ") || trimmed.contains("review this") {
        return true;
    }
    false
}

fn looks_like_greenfield_plan(lower: &str) -> bool {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
    (trimmed.starts_with("build ")
        && (trimmed.contains("pong")
            || trimmed.contains(" game")
            || trimmed.contains("app")
            || trimmed.contains("project")
            || trimmed.contains("demo")))
        || trimmed == "build pong"
        || trimmed.starts_with("scaffold ")
}

fn classify_mutation_phrasing(lower: &str) -> Option<(CodingEditKind, Vec<&'static str>)> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    if trimmed == "fix this"
        || trimmed.starts_with("fix this ")
        || trimmed.starts_with("fix the bug")
        || trimmed.starts_with("fix this bug")
        || trimmed == "fix it"
        || trimmed.starts_with("fix it ")
        || (trimmed.starts_with("fix ") && !trimmed.starts_with("fix my schedule"))
    {
        return Some((CodingEditKind::Fix, vec!["fix"]));
    }

    if trimmed.starts_with("make this faster")
        || trimmed.starts_with("make it faster")
        || trimmed.contains("make this faster")
        || trimmed.starts_with("optimize ")
        || trimmed.starts_with("speed up ")
    {
        return Some((CodingEditKind::Optimize, vec!["optimize"]));
    }

    if trimmed.starts_with("refactor ")
        || trimmed == "refactor this"
        || trimmed.starts_with("refactor this ")
        || trimmed.starts_with("clean this up")
    {
        return Some((CodingEditKind::Refactor, vec!["refactor"]));
    }

    if trimmed == "rename this"
        || trimmed.starts_with("rename this ")
        || (trimmed.starts_with("rename ")
            && !trimmed.contains("symbol")
            && !trimmed.contains("variable")
            && !trimmed.contains("method")
            && !trimmed.contains("module"))
    {
        return Some((CodingEditKind::Rename, vec!["rename"]));
    }

    if trimmed.starts_with("remove ")
        || trimmed.starts_with("delete ")
        || trimmed.contains("remove this unused")
        || trimmed.contains("delete this unused")
    {
        return Some((CodingEditKind::Remove, vec!["remove"]));
    }

    if trimmed.starts_with("create a new ")
        || trimmed.starts_with("create new ")
        || trimmed.starts_with("add a new ")
        || (trimmed.starts_with("create ")
            && (trimmed.contains(" page")
                || trimmed.contains(" file")
                || trimmed.contains(" component")
                || trimmed.contains(" screen")
                || trimmed.contains(" view")))
    {
        return Some((CodingEditKind::Create, vec!["create"]));
    }

    if trimmed.starts_with("add tests")
        || trimmed.starts_with("add test ")
        || trimmed.starts_with("add authentication")
        || trimmed.starts_with("add auth")
        || (trimmed.starts_with("add ")
            && (trimmed.contains(" for this")
                || trimmed.contains("authentication")
                || trimmed.contains(" feature")
                || trimmed.contains(" support")
                || trimmed.contains(" to ")))
    {
        return Some((CodingEditKind::Add, vec!["add"]));
    }

    if trimmed.starts_with("change ")
        || trimmed.starts_with("update ")
        || trimmed.starts_with("edit ")
        || trimmed.starts_with("modify ")
        || trimmed.contains("change the button")
        || trimmed.contains("change the color")
    {
        return Some((CodingEditKind::Change, vec!["change"]));
    }

    None
}

/// Build the objective from assessment + assembled Context + environmental resolution.
pub fn finalize_objective(
    assessment: &CodingEditAssessment,
    bundle: &ContextBundle,
    session: &jaymi_context::ContextSessionInputs,
    request: &UserRequest,
) -> CodingEditObjective {
    let env = resolve_environment(request, session);
    build_objective(assessment, bundle, &env, session)
}

fn build_objective(
    assessment: &CodingEditAssessment,
    bundle: &ContextBundle,
    env: &EnvironmentalResolution,
    session: &jaymi_context::ContextSessionInputs,
) -> CodingEditObjective {
    let mut obj = CodingEditObjective {
        status: CodingEditStatus::Understood,
        kind: Some(assessment.kind),
        goal: assessment.goal.clone(),
        requested_change: requested_change_for(assessment.kind, &assessment.goal),
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        constraints: vec![
            "No file modifications in this turn (C3.1 intent only).".into(),
            "No write_file / manage_path / terminal / git / LSP rename invocation.".into(),
            "Later mutation slices require Review Before Action.".into(),
        ],
        ..CodingEditObjective::default()
    };

    let active = bundle.active_project();
    if let Some(name) = active.name.clone() {
        obj.target_project = Some(name);
    } else if let Some(root) = active.root_directory.clone() {
        obj.target_project = Some(root);
    }

    let editor = bundle.editor_intelligence();
    let current = bundle.current_file();

    let mut bound_file: Option<String> = None;
    let mut bound_symbol: Option<String> = None;
    let mut bound_selection: Option<String> = None;
    let mut selection_bound = false;

    for reference in &env.references {
        match reference.kind {
            ResolvedKind::Selection => {
                selection_bound = true;
                if let Some(path) = &reference.path {
                    bound_file = Some(path.clone());
                }
                if let Some(sym) = &reference.symbol {
                    bound_symbol = Some(sym.clone());
                }
                obj.relevant_context.push(format!(
                    "environmental: selection ({})",
                    reference.evidence
                ));
            }
            ResolvedKind::Symbol => {
                if bound_symbol.is_none() {
                    bound_symbol = reference.symbol.clone();
                }
                if bound_file.is_none() {
                    bound_file = reference.path.clone();
                }
                obj.relevant_context.push(format!(
                    "environmental: symbol ({})",
                    reference.evidence
                ));
            }
            ResolvedKind::File | ResolvedKind::OpenTab | ResolvedKind::RecentEdit => {
                if bound_file.is_none() {
                    bound_file = reference.path.clone();
                }
                obj.relevant_context.push(format!(
                    "environmental: {} ({})",
                    reference.kind.as_str(),
                    reference.evidence
                ));
            }
            ResolvedKind::Diagnostic => {
                obj.relevant_context.push(format!(
                    "environmental: diagnostic ({})",
                    reference.evidence
                ));
                if bound_file.is_none() {
                    bound_file = reference.path.clone();
                }
            }
            ResolvedKind::Unresolved => {
                obj.ambiguities.push(format!(
                    "Could not resolve `{}` from Workspace Intelligence.",
                    reference.cue
                ));
            }
        }
    }

    if bound_file.is_none() {
        if let Some(path) = current.path.clone() {
            bound_file = Some(path);
            obj.investigation_notes
                .push("target_file from current_file (WI)".into());
        }
    }
    if bound_symbol.is_none() {
        if let Some(sym) = editor.symbol.as_ref().map(|s| s.name.clone()) {
            if !sym.trim().is_empty() {
                bound_symbol = Some(sym);
                obj.investigation_notes
                    .push("target_symbol from editor_intelligence.symbol".into());
            }
        } else if let Some(func) = editor.enclosing_function.as_ref().map(|s| s.name.clone()) {
            if !func.trim().is_empty() {
                bound_symbol = Some(func);
                obj.investigation_notes
                    .push("target_symbol from enclosing_function".into());
            }
        }
    }

    if let Some(snapshot) = &session.editor_snapshot {
        let sel = &snapshot.selection;
        if let Some(text) = sel.text.as_ref().filter(|t| !t.trim().is_empty()) {
            selection_bound = true;
            bound_selection = Some(truncate(text, 120));
            obj.investigation_notes
                .push("target_selection from editor_snapshot.selection".into());
            if bound_file.is_none() {
                if let Some(path) = sel.path.clone().or_else(|| snapshot.active_file.path.clone()) {
                    bound_file = Some(path);
                }
            }
        } else if sel.start_line != sel.end_line || sel.start_column != sel.end_column {
            selection_bound = true;
            bound_selection = Some(format!(
                "L{}:{}–L{}:{}",
                sel.start_line.saturating_add(1),
                sel.start_column.saturating_add(1),
                sel.end_line.saturating_add(1),
                sel.end_column.saturating_add(1)
            ));
            if bound_file.is_none() {
                if let Some(path) = sel.path.clone().or_else(|| snapshot.active_file.path.clone()) {
                    bound_file = Some(path);
                }
            }
        }
    }

    if let Some(named) = extract_named_target(&assessment.goal) {
        obj.relevant_context
            .push(format!("named_target_phrase: `{named}`"));
        if bound_file.is_none() && bound_symbol.is_none() {
            obj.ambiguities.push(format!(
                "Named target `{named}` is not bound to a project file/symbol yet — confirm the file or select the UI element."
            ));
            obj.investigation_notes.push(
                "named target unresolved from WI — will not invent a filesystem path".into(),
            );
        }
    }

    for path in bundle.open_files().files.iter().take(6).map(|f| &f.path) {
        if !obj.relevant_files.iter().any(|p| p == path) {
            obj.relevant_files.push(path.clone());
        }
    }
    if let Some(path) = &bound_file {
        if !obj.relevant_files.iter().any(|p| p == path) {
            obj.relevant_files.insert(0, path.clone());
        }
    }

    if editor.references.len() > 1 {
        obj.investigation_notes.push(format!(
            "ambient WI references={} (multi-file scope signal)",
            editor.references.len()
        ));
        for reference in editor.references.iter().take(8) {
            let p = reference.path.clone();
            if !p.is_empty() && !obj.relevant_files.iter().any(|x| x == &p) {
                obj.relevant_files.push(p);
            }
        }
    }

    obj.target_file = bound_file;
    obj.target_selection = bound_selection;
    if let Some(sym) = bound_symbol {
        obj.target_symbols.push(sym);
    }

    obj.expected_scope = Some(infer_scope(
        assessment.kind,
        selection_bound,
        &obj,
        env,
    ));

    let deixis = goal_has_deixis(&assessment.goal) || assessment.from_coding_action;
    let missing_target = obj.target_file.is_none()
        && obj.target_symbols.is_empty()
        && obj.target_selection.is_none();

    if env.ambiguous {
        obj.ambiguities
            .push("Multiple workspace referents competed for this request.".into());
    }

    let needs_clarify = if assessment.kind == CodingEditKind::Create {
        obj.target_project.is_none() && missing_target
    } else if deixis || assessment.from_coding_action {
        missing_target
            || (assessment.from_coding_action && !selection_bound && obj.target_symbols.is_empty())
    } else if matches!(
        assessment.kind,
        CodingEditKind::Fix
            | CodingEditKind::Change
            | CodingEditKind::Refactor
            | CodingEditKind::Optimize
    ) && missing_target
        && extract_named_target(&assessment.goal).is_some()
    {
        true
    } else {
        !obj.ambiguities.is_empty() && missing_target
    };

    if needs_clarify {
        obj.status = CodingEditStatus::NeedsClarification;
        obj.clarification = Some(clarification_question(assessment.kind, &assessment.goal));
        if obj.ambiguities.is_empty() {
            obj.ambiguities.push(
                "Insufficient Workspace Intelligence to bind a modification target.".into(),
            );
        }
    } else {
        obj.status = CodingEditStatus::Understood;
        if obj.target_file.is_none()
            && matches!(assessment.kind, CodingEditKind::Create | CodingEditKind::Add)
        {
            obj.relevant_context.push(
                "Scope is project/feature-level; concrete paths deferred to a later planning slice."
                    .into(),
            );
        }
    }

    if deixis
        && env
            .references
            .iter()
            .any(|r| matches!(r.confidence, ResolutionConfidence::Unresolved))
        && missing_target
    {
        obj.status = CodingEditStatus::NeedsClarification;
        obj.clarification = Some(clarification_question(assessment.kind, &assessment.goal));
    }

    obj
}

fn infer_scope(
    kind: CodingEditKind,
    selection_bound: bool,
    obj: &CodingEditObjective,
    env: &EnvironmentalResolution,
) -> EditScope {
    if selection_bound
        || env
            .references
            .iter()
            .any(|r| r.kind == ResolvedKind::Selection)
    {
        return EditScope::Selection;
    }
    if obj.relevant_files.len() > 1 {
        return EditScope::MultiFile;
    }
    if !obj.target_symbols.is_empty() {
        return EditScope::Symbol;
    }
    if obj.target_file.is_some() {
        return EditScope::File;
    }
    match kind {
        CodingEditKind::Create | CodingEditKind::Add => EditScope::Project,
        _ => EditScope::Unknown,
    }
}

fn requested_change_for(kind: CodingEditKind, goal: &str) -> String {
    match kind {
        CodingEditKind::Fix => format!("Fix: {goal}"),
        CodingEditKind::Change => format!("Change: {goal}"),
        CodingEditKind::Add => format!("Add: {goal}"),
        CodingEditKind::Rename => format!("Rename: {goal}"),
        CodingEditKind::Refactor => format!("Refactor: {goal}"),
        CodingEditKind::Optimize => format!("Optimize: {goal}"),
        CodingEditKind::Remove => format!("Remove: {goal}"),
        CodingEditKind::Create => format!("Create: {goal}"),
        CodingEditKind::Generic => goal.to_string(),
    }
}

fn goal_has_deixis(goal: &str) -> bool {
    let lower = goal.to_ascii_lowercase();
    lower.split_whitespace().any(|t| {
        matches!(
            t.trim_matches(|c: char| !c.is_ascii_alphanumeric()),
            "this" | "it" | "here"
        )
    }) || lower.contains("current selection")
        || lower.contains("the selection")
}

fn extract_named_target(goal: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    for marker in [
        "change the ",
        "fix the ",
        "update the ",
        "edit the ",
        "remove the ",
        "delete the ",
        "rename the ",
    ] {
        if let Some(idx) = lower.find(marker) {
            let after = goal[idx + marker.len()..]
                .trim()
                .trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
            let phrase: String = after
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            if !phrase.is_empty()
                && !phrase.eq_ignore_ascii_case("bug")
                && !phrase.eq_ignore_ascii_case("code")
            {
                return Some(phrase);
            }
        }
    }
    None
}

fn clarification_question(kind: CodingEditKind, goal: &str) -> String {
    match kind {
        CodingEditKind::Fix => {
            "Which file or selection should I fix? Open the file, select the buggy code, or name the symbol.".into()
        }
        CodingEditKind::Rename => {
            "Which symbol should be renamed, and to what name?".into()
        }
        CodingEditKind::Create => {
            "Where should the new file/page live (path or module), and what should it contain?".into()
        }
        CodingEditKind::Change | CodingEditKind::Refactor | CodingEditKind::Optimize => {
            format!(
                "I understood a {} request (“{goal}”), but I could not bind a target from the current editor/project context. Select the code, open the file, or name the symbol/path.",
                kind.label()
            )
        }
        _ => format!(
            "I understood a modification request (“{goal}”), but need a clearer target (file, selection, or symbol) before planning changes."
        ),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max_chars {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::UserRequest;

    #[test]
    fn detects_mutation_examples() {
        for (prompt, kind) in [
            ("Fix this bug.", CodingEditKind::Fix),
            ("Change the button color.", CodingEditKind::Change),
            ("Add authentication.", CodingEditKind::Add),
            ("Rename this.", CodingEditKind::Rename),
            ("Refactor this function.", CodingEditKind::Refactor),
            ("Make this faster.", CodingEditKind::Optimize),
            ("Add tests for this.", CodingEditKind::Add),
            ("Add image generation to Settings.", CodingEditKind::Add),
            ("Create a new settings page.", CodingEditKind::Create),
            ("Remove this unused function.", CodingEditKind::Remove),
        ] {
            let a = detect_coding_edit_request(&UserRequest::new(prompt))
                .unwrap_or_else(|| panic!("expected detect for {prompt}"));
            assert_eq!(a.kind, kind, "prompt={prompt}");
        }
    }

    #[test]
    fn does_not_steal_read_only() {
        for prompt in [
            "Explain this.",
            "Where is this used?",
            "Why does this happen?",
            "What does this file do?",
            "Review this code.",
            "Who calls this?",
            "Where is authentication handled?",
        ] {
            assert!(
                detect_coding_edit_request(&UserRequest::new(prompt)).is_none(),
                "must not steal read-only: {prompt}"
            );
        }
    }

    #[test]
    fn does_not_steal_greenfield_or_multi_file_rename_symbol() {
        assert!(detect_coding_edit_request(&UserRequest::new("Build Pong.")).is_none());
        assert!(
            detect_coding_edit_request(&UserRequest::new("Rename this variable.")).is_none(),
            "C1.7 owns rename symbol/variable"
        );
    }

    #[test]
    fn coding_action_edit_is_mutation() {
        let req = UserRequest::coding_action(CodingAction::EditSelection);
        let a = detect_coding_edit_request(&req).expect("edit selection");
        assert_eq!(a.kind, CodingEditKind::Change);
        assert!(a.from_coding_action);
    }
}
