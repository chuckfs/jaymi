//! Generation Planning / Coding Plans (Sprint C1.4 + C3.3).
//!
//! **C1.4** — Before generating code, the Planner produces a structured
//! **Coding Plan** from already-assembled Workspace Intelligence —
//! observation and proposal only.
//!
//! **C3.3** — After Change Investigation (C3.2), the same [`CodingPlan`] type
//! is filled from investigation evidence for mutation requests — still
//! planning only (no file contents, writes, tools, or executable Execution
//! Plans).
//!
//! Constitutional constraints:
//! - No new context systems (ContextEngine remains sole ContextBundle factory)
//! - No provider bypasses
//! - No filesystem scans (C1.4 scaffold) / no additional tool calls (C3.3)
//! - No file writes
//! - No code generation yet
//! - No executable mutation [`crate::ExecutionPlan`] in this sprint
//! - Planner ownership unchanged
//! - Constitutional architecture conflicts are flagged, never silently designed around

use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

use crate::change_investigation::{
    ChangeInvestigationMode, ChangeInvestigationReport,
};
use crate::coding_edit::{
    CodingEditKind, CodingEditObjective, CodingEditStatus,
};

/// Cap rows copied from investigation into the plan.
const PLAN_ROW_CAP: usize = 12;

/// Where a Coding Plan was authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum CodingPlanOrigin {
    /// C1.4 greenfield / generation planning from WI scaffold.
    #[default]
    GenerationPlanning,
    /// C3.3 mutation plan grounded in Change Investigation.
    ChangeInvestigation,
}

impl CodingPlanOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GenerationPlanning => "generation_planning",
            Self::ChangeInvestigation => "change_investigation",
        }
    }
}

/// Kind of generation the user is asking to plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CodingPlanKind {
    /// Greenfield / sample app ("Build Pong.").
    NewProject,
    /// New component / feature ("Create a parser.").
    Feature,
    /// Tests ("Write tests.").
    Tests,
    /// Generic generation ask.
    Generic,
    /// Mutation plan derived from C3.1/C3.2 (fix / add / refactor / …).
    Mutation,
}

impl CodingPlanKind {
    /// Stable id for AssembleHints / diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewProject => "new_project",
            Self::Feature => "feature",
            Self::Tests => "tests",
            Self::Generic => "generic",
            Self::Mutation => "mutation",
        }
    }
}

/// Structured Coding Plan returned to the conversation (planning only).
///
/// Not an [`crate::ExecutionPlan`] — never executes tools or writes files.
/// C1.5 / later slices may translate this into reviewed generation ops.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingPlan {
    /// Plan origin (C1.4 scaffold vs C3.3 investigation-grounded).
    pub origin: CodingPlanOrigin,
    /// Kind when known (generation kinds or mutation).
    pub kind: Option<CodingPlanKind>,
    /// User-facing objective.
    pub objective: Option<String>,
    /// Why these steps / files — grounded in evidence when possible.
    pub rationale: Vec<String>,
    /// Ordered plan steps (what would be done, not doing it).
    pub plan_steps: Vec<String>,
    /// Explicit ordering / dependency notes between steps.
    pub step_dependencies: Vec<String>,
    /// Candidate paths / modules to create (proposal only).
    pub files_to_create: Vec<String>,
    /// Candidate paths / modules to modify (proposal only).
    pub files_to_modify: Vec<String>,
    /// Candidate paths to remove when evidence supports it (proposal only).
    pub files_to_remove: Vec<String>,
    /// Symbols expected to change or be introduced.
    pub symbols_affected: Vec<String>,
    /// Likely dependencies / toolchain needs from WI / investigation.
    pub dependencies: Vec<String>,
    /// Tests to add or update (proposal only).
    pub tests: Vec<String>,
    /// Documentation to update (proposal only).
    pub documentation: Vec<String>,
    /// Estimated risk label + reasons (observational).
    pub estimated_risk: Vec<String>,
    /// Ambiguities / unknowns that remain visible.
    pub unresolved_questions: Vec<String>,
    /// Constitutional / architectural conflicts (flagged, not designed around).
    pub constitutional_conflicts: Vec<String>,
    /// Short summary of the proposed plan.
    pub summary: Vec<String>,
}

impl CodingPlan {
    /// True when every section is empty.
    pub fn is_empty(&self) -> bool {
        self.objective.is_none()
            && self.rationale.is_empty()
            && self.plan_steps.is_empty()
            && self.step_dependencies.is_empty()
            && self.files_to_create.is_empty()
            && self.files_to_modify.is_empty()
            && self.files_to_remove.is_empty()
            && self.symbols_affected.is_empty()
            && self.dependencies.is_empty()
            && self.tests.is_empty()
            && self.documentation.is_empty()
            && self.estimated_risk.is_empty()
            && self.unresolved_questions.is_empty()
            && self.constitutional_conflicts.is_empty()
            && self.summary.is_empty()
    }

    /// Conversation-visible markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Plan\n");
        if self.origin != CodingPlanOrigin::GenerationPlanning || self.kind.is_some() {
            out.push_str(&format!(
                "\n### Origin\n- {}{}\n",
                self.origin.as_str(),
                self.kind
                    .map(|k| format!(" ({})", k.as_str()))
                    .unwrap_or_default()
            ));
        }
        if let Some(objective) = &self.objective {
            out.push_str("\n### Objective\n- ");
            out.push_str(objective);
            out.push('\n');
        }
        push_section(&mut out, "Rationale", &self.rationale);
        push_section(&mut out, "Plan", &self.plan_steps);
        push_section(&mut out, "Step ordering", &self.step_dependencies);
        push_section(&mut out, "Files to Create", &self.files_to_create);
        push_section(&mut out, "Files to Modify", &self.files_to_modify);
        push_section(&mut out, "Files to Remove", &self.files_to_remove);
        push_section(&mut out, "Symbols affected", &self.symbols_affected);
        push_section(&mut out, "Dependencies", &self.dependencies);
        push_section(&mut out, "Tests", &self.tests);
        push_section(&mut out, "Documentation", &self.documentation);
        push_section(&mut out, "Estimated Risk", &self.estimated_risk);
        push_section(&mut out, "Unresolved questions", &self.unresolved_questions);
        push_section(
            &mut out,
            "Constitutional conflicts",
            &self.constitutional_conflicts,
        );
        push_section(&mut out, "Summary", &self.summary);
        out.push_str("\n### Notes\n");
        out.push_str("- Planning only — no file contents, writes, mutation tools, or executable Execution Plan in this turn.\n");
        out
    }

    /// Compact diagnostics line.
    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "coding_plan origin={} kind={} steps={} create={} modify={} remove={} symbols={} tests={} docs={} unresolved={} conflicts={} risk={}",
            self.origin.as_str(),
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.plan_steps.len(),
            self.files_to_create.len(),
            self.files_to_modify.len(),
            self.files_to_remove.len(),
            self.symbols_affected.len(),
            self.tests.len(),
            self.documentation.len(),
            self.unresolved_questions.len(),
            self.constitutional_conflicts.len(),
            self.estimated_risk.len(),
        )
    }

    /// Prompt instruction for Reasoning.
    pub fn prompt_instruction(&self, kind: CodingPlanKind, goal: &str) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Respond with a Coding Plan (kind=`{}`) for goal: {goal}",
            kind.as_str()
        ));
        lines.push(
            "Use only Workspace Intelligence / Change Investigation already in this prompt."
                .into(),
        );
        lines.push(
            "Planning only — do not generate full source code, write files, execute tools, or produce an Execution Plan."
                .into(),
        );
        lines.push(
            "Fill these markdown headings exactly: Objective · Rationale · Plan · Step ordering · Files to Create · Files to Modify · Files to Remove · Symbols affected · Dependencies · Tests · Documentation · Estimated Risk · Unresolved questions · Constitutional conflicts · Summary."
                .into(),
        );
        lines.push("Starter observations:".into());
        lines.push(self.to_markdown());
        lines.join("\n")
    }
}

fn push_section(out: &mut String, title: &str, items: &[String]) {
    out.push_str("\n### ");
    out.push_str(title);
    out.push('\n');
    if items.is_empty() {
        out.push_str("- _(not observed / not proposed yet)_\n");
        return;
    }
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
}

/// Planner assessment: generation-planning mode for this conversational turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingPlanAssessment {
    /// Kind of generation plan.
    pub kind: CodingPlanKind,
    /// Normalized goal text (from the user request).
    pub goal: String,
    /// Rule ids that matched (diagnostics).
    pub matched_rules: Vec<&'static str>,
    /// WI scaffold (filled after Context assemble).
    pub scaffold: CodingPlan,
}

impl CodingPlanAssessment {
    /// Stable AssembleHints label.
    pub fn hint_id(&self) -> String {
        format!("coding_plan:{}", self.kind.as_str())
    }
}

/// Extension key on [`jaymi_context::LlmContext`] for the prompt section.
pub const LLM_EXTENSION_KEY: &str = "coding_plan";

/// Detect whether this request asks for a Coding Plan (no codegen / tools / writes).
pub fn detect_coding_plan_request(request: &UserRequest) -> Option<CodingPlanAssessment> {
    if request.coding_action.is_some() {
        return None;
    }
    // Structured write / manage / terminal payloads are tool intents — not C1.4.
    if request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
    {
        return None;
    }

    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    if !looks_like_generation_plan(&lower) {
        return None;
    }

    let (kind, rule) = if looks_like_tests_plan(&lower) {
        (CodingPlanKind::Tests, "plan_tests")
    } else if looks_like_new_project_plan(&lower) {
        (CodingPlanKind::NewProject, "plan_new_project")
    } else if looks_like_feature_plan(&lower) {
        (CodingPlanKind::Feature, "plan_feature")
    } else {
        (CodingPlanKind::Generic, "plan_generic")
    };

    Some(CodingPlanAssessment {
        kind,
        goal: content.to_string(),
        matched_rules: vec![rule],
        scaffold: CodingPlan::default(),
    })
}

fn looks_like_generation_plan(lower: &str) -> bool {
    // Do not steal explain / review / architecture questions.
    // Do not steal C1.5 "generate / apply the plan" (execution of a Coding Plan).
    if lower.contains("review")
        || lower.contains("explain")
        || lower.starts_with("what ")
        || lower.starts_with("why ")
        || lower.starts_with("how does")
        || lower.starts_with("how do ")
        || lower.contains("architecture does")
        || lower.contains("responsible for")
        || crate::code_generation::steals_from_coding_plan_detect(lower)
        || crate::multi_file::steals_from_coding_plan_detect(lower)
        || crate::coding_edit::steals_from_coding_plan_detect(lower)
        || crate::coding_goal::steals_from_coding_plan_detect(lower)
    {
        return false;
    }

    looks_like_tests_plan(lower)
        || looks_like_new_project_plan(lower)
        || looks_like_feature_plan(lower)
        || ((lower.starts_with("build ")
            || lower.starts_with("create ")
            || lower.starts_with("make ")
            || lower.starts_with("implement ")
            || lower.starts_with("generate ")
            || lower.starts_with("scaffold ")
            || lower.starts_with("write ")
            || lower.starts_with("add "))
            && lower.len() < 160)
}

fn looks_like_tests_plan(lower: &str) -> bool {
    lower == "write tests"
        || lower == "write tests."
        || lower.starts_with("write tests ")
        || lower.starts_with("write test ")
        || lower.starts_with("add tests")
        || lower.starts_with("create tests")
        || lower.starts_with("generate tests")
        || lower.contains("write unit tests")
        || lower.contains("add unit tests")
        || (lower.contains("write") && lower.contains("test") && !lower.contains("testify"))
}

fn looks_like_new_project_plan(lower: &str) -> bool {
    let verbs = lower.starts_with("build ")
        || lower.starts_with("create ")
        || lower.starts_with("make ")
        || lower.starts_with("scaffold ");
    let product = lower.contains("pong")
        || lower.contains(" game")
        || lower.ends_with(" game")
        || lower.ends_with(" game.")
        || lower.contains("app")
        || lower.contains("application")
        || lower.contains("project")
        || lower.contains("demo");
    (verbs && product)
        || lower.starts_with("build pong")
        || lower == "build pong."
        || lower == "build pong"
}

fn looks_like_feature_plan(lower: &str) -> bool {
    let verbs = lower.starts_with("create ")
        || lower.starts_with("build ")
        || lower.starts_with("implement ")
        || lower.starts_with("add ")
        || lower.starts_with("make ");
    let feature = lower.contains("parser")
        || lower.contains("module")
        || lower.contains("component")
        || lower.contains("feature")
        || lower.contains("endpoint")
        || lower.contains("api")
        || lower.contains("cli")
        || lower.contains("library")
        || lower.contains("crate");
    verbs && feature
}

/// Attach Coding Plan scaffold after Context assemble.
pub fn finalize_assessment(
    mut assessment: CodingPlanAssessment,
    bundle: &ContextBundle,
) -> CodingPlanAssessment {
    assessment.scaffold = scaffold_from_bundle(assessment.kind, &assessment.goal, bundle);
    assessment
}

/// Fill a WI-backed Coding Plan scaffold (proposal only).
pub fn scaffold_from_bundle(
    kind: CodingPlanKind,
    goal: &str,
    bundle: &ContextBundle,
) -> CodingPlan {
    let mut plan = CodingPlan {
        origin: CodingPlanOrigin::GenerationPlanning,
        kind: Some(kind),
        objective: Some(goal.to_string()),
        ..CodingPlan::default()
    };
    let active = bundle.active_project();
    let project = bundle.project_intelligence();
    let file = bundle.current_file();
    let git = bundle.git_status();
    let memory = bundle.workspace_memory();
    let open = bundle.open_files();

    let project_name = active
        .name
        .clone()
        .unwrap_or_else(|| "the open project".into());
    let root = active
        .root_directory
        .as_deref()
        .unwrap_or("(no project root observed)");

    plan.summary.push(format!("Goal: {goal}"));
    plan.summary.push(
        "Planning only — no code generation, tool execution, or file writes in this turn.".into(),
    );
    plan.summary
        .push(format!("Observed project context: `{project_name}` @ `{root}`."));

    match kind {
        CodingPlanKind::NewProject => {
            plan.plan_steps.push(
                "Orient on observed languages / package manager (do not invent a stack)."
                    .into(),
            );
            plan.plan_steps.push(
                "Propose a minimal module layout under existing top-level dirs when present."
                    .into(),
            );
            plan.plan_steps
                .push("List files to create vs reuse before any generation turn.".into());
            plan.plan_steps.push(
                "Stop after this Coding Plan — await explicit approval to generate code later."
                    .into(),
            );

            if let Some(dir) =
                first_matching_dir(&project.top_level_dirs, &["apps", "src", "examples"])
            {
                plan.files_to_create.push(format!(
                    "`{dir}/` entrypoint module for the new sample (proposal only)."
                ));
            } else if project.top_level_dirs.is_empty() {
                plan.files_to_create.push(
                    "Top-level source entry (layout not observed yet — confirm root first)."
                        .into(),
                );
            } else {
                plan.files_to_create.push(format!(
                    "New module under observed top-level `{}` (confirm placement).",
                    project.top_level_dirs[0]
                ));
            }
            plan.files_to_create
                .push("README or docs note describing the sample (optional).".into());

            if let Some(path) = &file.path {
                plan.files_to_modify
                    .push(format!("Possibly `{path}` if wiring into the active file."));
            }
            for member in project.dependency_summary.workspace_members.iter().take(3) {
                plan.files_to_modify.push(format!(
                    "Workspace member `{member}` manifest if a new package is added (proposal)."
                ));
            }

            plan.estimated_risk.push(
                "Risk: **Medium–High** — greenfield sample can sprawl without agreed scope."
                    .into(),
            );
        }
        CodingPlanKind::Feature => {
            plan.plan_steps
                .push("Map the feature onto observed top-level dirs / workspace members.".into());
            plan.plan_steps
                .push("Identify create vs modify candidates from open files and layout.".into());
            plan.plan_steps
                .push("List dependency needs from ProjectSnapshot package manager.".into());
            plan.plan_steps
                .push("Deliver Coding Plan only — no generation in this turn.".into());

            let feature_slug = feature_slug_from_goal(goal);
            if let Some(dir) =
                first_matching_dir(&project.top_level_dirs, &["crates", "src", "lib", "apps"])
            {
                plan.files_to_create.push(format!(
                    "`{dir}/…/{feature_slug}` module (confirm exact path before creation)."
                ));
            } else {
                plan.files_to_create.push(format!(
                    "New `{feature_slug}` module path (layout thin — ask where it should live)."
                ));
            }

            for path in memory.recent_edits.iter().take(3) {
                plan.files_to_modify
                    .push(format!("Recent edit candidate: `{path}`"));
            }
            for entry in open.files.iter().take(4) {
                plan.files_to_modify
                    .push(format!("Open file may need wiring: `{}`", entry.path));
            }
            if let Some(path) = &file.path {
                plan.files_to_modify
                    .push(format!("Active file `{path}` may import / expose the feature."));
            }

            plan.estimated_risk.push(
                "Risk: **Medium** — feature placement wrong → churn; confirm module home first."
                    .into(),
            );
        }
        CodingPlanKind::Tests => {
            plan.plan_steps.push(
                "Identify the unit under test from active file / selection / recent edits."
                    .into(),
            );
            plan.plan_steps
                .push("Propose test file locations matching observed project conventions.".into());
            plan.plan_steps
                .push("List fixtures / deps needed — do not run the test tool yet.".into());
            plan.plan_steps
                .push("Stop after the Coding Plan; generation and `cargo test` come later.".into());

            if let Some(path) = &file.path {
                plan.files_to_modify
                    .push(format!("Source under test (likely): `{path}`"));
                if path.ends_with(".rs") {
                    plan.files_to_create.push(format!(
                        "Co-located or `tests/` companion for `{path}` (confirm convention)."
                    ));
                } else {
                    plan.files_to_create.push(
                        "Test file beside the active source (confirm framework convention)."
                            .into(),
                    );
                }
            } else {
                plan.files_to_create.push(
                    "Test module path TBD — open a source file or name the target.".into(),
                );
                for path in memory.recent_edits.iter().take(3) {
                    plan.files_to_modify
                        .push(format!("Recent edit may be under test: `{path}`"));
                }
            }
            if project.top_level_dirs.iter().any(|d| d == "tests") {
                plan.files_to_create
                    .push("Candidate under observed `tests/` directory.".into());
            }

            plan.estimated_risk.push(
                "Risk: **Low–Medium** — tests are usually additive; still confirm target module."
                    .into(),
            );
        }
        CodingPlanKind::Generic => {
            plan.plan_steps
                .push("Clarify deliverable boundaries using observed project layout.".into());
            plan.plan_steps
                .push("Separate files to create vs modify before any generation.".into());
            plan.plan_steps
                .push("Record dependencies / risk — planning turn only.".into());
            plan.files_to_create.push(
                "Specific create list pending clearer scope (ask what artifact is desired)."
                    .into(),
            );
            plan.estimated_risk.push(
                "Risk: **Medium** — underspecified generation ask; prefer a narrower goal."
                    .into(),
            );
        }
        CodingPlanKind::Mutation => {
            plan.plan_steps.push(
                "Use Change Investigation evidence before proposing mutation steps.".into(),
            );
            plan.estimated_risk.push(
                "Risk: **Unknown** — mutation scaffold without investigation is incomplete."
                    .into(),
            );
        }
    }

    if let Some(pm) = &project.package_manager {
        plan.dependencies.push(format!("package_manager: {pm}"));
    }
    if let Some(build) = &project.build_system {
        plan.dependencies.push(format!("build_system: {build}"));
    }
    for lang in project.languages.iter().take(6) {
        plan.dependencies.push(format!("language: {lang}"));
    }
    for dep in project.dependency_summary.top_level.iter().take(8) {
        plan.dependencies
            .push(format!("existing_dependency: {dep}"));
    }
    if plan.dependencies.is_empty() {
        plan.dependencies.push(
            "No package manager / languages observed yet — confirm toolchain before generating."
                .into(),
        );
    }

    let dirty = git.modified_count + git.staged_count + git.untracked_count + git.conflict_count;
    if dirty > 0 {
        plan.estimated_risk.push(format!(
            "Working tree already has {dirty} dirty path(s) — plan around existing changes."
        ));
    }
    if git.conflict_count > 0 {
        plan.estimated_risk.push(
            "Merge conflicts observed — resolve before any generation turn.".into(),
        );
    }
    if active.root_directory.is_none() {
        plan.estimated_risk.push(
            "No active project root observed — open or create a project before generating."
                .into(),
        );
    }

    plan.plan_steps.push(
        "Constitutional gate: this response is a Coding Plan only (no Execution Plan, no writes)."
            .into(),
    );

    plan
}

/// C3.3 — mutation Coding Plan from C3.1 objective + C3.2 investigation.
///
/// Pure Planner authorship: no tools, no FS writes, no file contents.
pub fn plan_from_change_investigation(
    objective: &CodingEditObjective,
    investigation: &ChangeInvestigationReport,
    _bundle: &ContextBundle,
) -> CodingPlan {
    let mut plan = CodingPlan {
        origin: CodingPlanOrigin::ChangeInvestigation,
        kind: Some(CodingPlanKind::Mutation),
        objective: Some(objective.goal.clone()),
        ..CodingPlan::default()
    };

    plan.rationale.push(format!(
        "Grounded in Change Investigation (mode={}).",
        investigation.mode.as_str()
    ));
    if let Some(kind) = objective.kind {
        plan.rationale
            .push(format!("Edit kind from Coding Edit Intent: {}.", kind.label()));
    }
    if let Some(file) = investigation
        .primary_file
        .as_ref()
        .or(objective.target_file.as_ref())
    {
        plan.rationale
            .push(format!("Primary target file observed: `{file}`."));
    }
    for pattern in investigation.implementation_patterns.iter().take(4) {
        push_unique(&mut plan.rationale, format!("Pattern: {pattern}"));
    }
    for note in investigation.related_functionality.iter().take(4) {
        push_unique(&mut plan.rationale, format!("Related: {note}"));
    }

    for q in &objective.ambiguities {
        push_unique(&mut plan.unresolved_questions, q.clone());
    }
    if let Some(c) = &objective.clarification {
        push_unique(
            &mut plan.unresolved_questions,
            format!("Clarification pending: {c}"),
        );
    }
    for u in &investigation.unknowns {
        push_unique(&mut plan.unresolved_questions, u.clone());
    }

    flag_constitutional_conflicts(&mut plan, objective, investigation);

    if matches!(objective.status, CodingEditStatus::NeedsClarification) {
        plan.plan_steps.push(
            "Resolve clarification / bind a concrete target before proposing mutation steps."
                .into(),
        );
        plan.plan_steps.push(
            "Re-run Change Investigation once the target is clear.".into(),
        );
        plan.estimated_risk.push(
            "Risk: **High** — planning under NeedsClarification; do not invent file paths."
                .into(),
        );
        plan.summary.push(
            "Coding Plan deferred pending clarification — ambiguities listed below.".into(),
        );
        plan.summary.push(
            "Planning only — no file contents, writes, tools, or Execution Plan.".into(),
        );
        return plan;
    }

    fill_mutation_steps(&mut plan, objective, investigation);
    fill_grounded_files(&mut plan, objective, investigation);
    fill_symbols_deps_tests_docs(&mut plan, investigation);
    fill_risk_and_summary(&mut plan, objective, investigation);
    plan
}

fn fill_mutation_steps(
    plan: &mut CodingPlan,
    objective: &CodingEditObjective,
    investigation: &ChangeInvestigationReport,
) {
    let kind = objective.kind.unwrap_or(CodingEditKind::Generic);
    match kind {
        CodingEditKind::Add | CodingEditKind::Create => {
            plan.plan_steps.extend([
                "1. Add / extend the provider or capability abstraction where investigation found registration patterns.".into(),
                "2. Register the new surface with the observed registry / capability system.".into(),
                "3. Add configuration hooks for any config paths identified.".into(),
                "4. Wire Settings / UI entry points observed in affected files.".into(),
                "5. Add or update tests covering the new path.".into(),
                "6. Update documentation candidates identified by investigation.".into(),
            ]);
            plan.step_dependencies.extend([
                "Step 2 depends on step 1 (registration needs the abstraction).".into(),
                "Step 3–4 depend on step 2 (UI/config should bind a registered surface).".into(),
                "Step 5 depends on steps 1–4 (tests exercise the wired path).".into(),
                "Step 6 can follow step 4 once the public surface is stable.".into(),
            ]);
        }
        CodingEditKind::Fix | CodingEditKind::Change => {
            plan.plan_steps.extend([
                "1. Confirm the primary failure / change site from investigation evidence.".into(),
                "2. Modify the primary target file / symbol.".into(),
                "3. Update dependent call sites among affected files.".into(),
                "4. Adjust tests that may be affected.".into(),
                "5. Note documentation only if public behavior changes.".into(),
            ]);
            plan.step_dependencies.extend([
                "Step 2 depends on step 1 (do not edit before the site is confirmed).".into(),
                "Step 3 depends on step 2.".into(),
                "Step 4 depends on steps 2–3.".into(),
            ]);
        }
        CodingEditKind::Rename => {
            plan.plan_steps.extend([
                "1. Rename the primary symbol at its definition site.".into(),
                "2. Update references across affected files.".into(),
                "3. Update tests and docs that name the symbol.".into(),
            ]);
            plan.step_dependencies.push(
                "Steps 2–3 depend on step 1 (definition rename first).".into(),
            );
        }
        CodingEditKind::Remove => {
            plan.plan_steps.extend([
                "1. Confirm no remaining required call sites beyond the removal set.".into(),
                "2. Remove the target symbol / file.".into(),
                "3. Clean imports and dependents in affected files.".into(),
                "4. Update or delete obsolete tests.".into(),
            ]);
            plan.step_dependencies.push(
                "Steps 2–4 depend on step 1 (safety check before deletion).".into(),
            );
        }
        CodingEditKind::Refactor | CodingEditKind::Optimize => {
            plan.plan_steps.extend([
                "1. Freeze the behavioral contract from investigation notes.".into(),
                "2. Refactor / optimize the primary module.".into(),
                "3. Update cross-subsystem call sites when side effects were flagged.".into(),
                "4. Strengthen tests around the refactored path.".into(),
            ]);
            plan.step_dependencies.extend([
                "Step 2 depends on step 1.".into(),
                "Step 3 depends on step 2.".into(),
                "Step 4 depends on steps 2–3.".into(),
            ]);
        }
        CodingEditKind::Generic => {
            plan.plan_steps.extend([
                "1. Restate the objective against investigation evidence.".into(),
                "2. Modify grounded affected files only.".into(),
                "3. Cover with tests; update docs if public.".into(),
            ]);
            plan.step_dependencies
                .push("Steps 2–3 depend on an Understood objective.".into());
        }
    }

    if investigation.truncated {
        plan.plan_steps.push(
            "Investigation was truncated under C2 budget — re-check unknowns before applying."
                .into(),
        );
    }
    plan.plan_steps.push(
        "Stop after this Coding Plan — no file contents, writes, or Execution Plan in this turn."
            .into(),
    );
}

fn fill_grounded_files(
    plan: &mut CodingPlan,
    objective: &CodingEditObjective,
    investigation: &ChangeInvestigationReport,
) {
    for path in investigation.affected_files.iter().take(PLAN_ROW_CAP) {
        if looks_like_test_path(path) || looks_like_doc_path(path) || looks_like_config_path(path)
        {
            continue;
        }
        push_unique(
            &mut plan.files_to_modify,
            format!("`{path}` (from investigation)"),
        );
    }
    if let Some(primary) = investigation
        .primary_file
        .as_ref()
        .or(objective.target_file.as_ref())
    {
        push_unique(
            &mut plan.files_to_modify,
            format!("`{primary}` (primary target)"),
        );
    }
    for path in objective.relevant_files.iter().take(PLAN_ROW_CAP) {
        if looks_like_test_path(path) {
            continue;
        }
        push_unique(
            &mut plan.files_to_modify,
            format!("`{path}` (edit-intent relevant)"),
        );
    }
    for path in investigation
        .configuration_possibly_affected
        .iter()
        .take(6)
    {
        push_unique(
            &mut plan.files_to_modify,
            format!("`{path}` (configuration — proposal)"),
        );
    }

    if matches!(
        objective.kind,
        Some(CodingEditKind::Add) | Some(CodingEditKind::Create)
    ) {
        let slug = feature_slug_from_goal(&objective.goal);
        if investigation.affected_files.iter().any(|p| {
            let l = p.to_ascii_lowercase();
            l.contains("provider") || l.contains("registry") || l.contains("capability")
        }) {
            plan.files_to_create.push(format!(
                "New `{slug}` provider / capability module beside observed provider registration (confirm path)."
            ));
        } else if let Some(primary) = &investigation.primary_file {
            plan.files_to_create.push(format!(
                "New module near `{primary}` for `{slug}` (confirm placement before create)."
            ));
        } else {
            plan.files_to_create.push(format!(
                "New `{slug}` module (placement unresolved — see Unresolved questions)."
            ));
            plan.unresolved_questions.push(
                "Exact create path not grounded — confirm module home before generation."
                    .into(),
            );
        }
        if !investigation.tests_possibly_affected.is_empty() {
            plan.files_to_create.push(format!(
                "Test companion for `{slug}` (match observed test layout)."
            ));
        }
    }

    if matches!(objective.kind, Some(CodingEditKind::Remove)) {
        if let Some(primary) = investigation
            .primary_file
            .as_ref()
            .or(objective.target_file.as_ref())
        {
            plan.files_to_remove.push(format!(
                "`{primary}` (only if whole-file removal is intended)"
            ));
        } else {
            plan.unresolved_questions.push(
                "Removal target file unknown — do not delete paths without evidence.".into(),
            );
        }
    }

    if plan.files_to_modify.is_empty()
        && plan.files_to_create.is_empty()
        && plan.files_to_remove.is_empty()
    {
        plan.unresolved_questions.push(
            "No grounded file list yet — investigation did not bind create/modify/remove paths."
                .into(),
        );
    }
}

fn fill_symbols_deps_tests_docs(
    plan: &mut CodingPlan,
    investigation: &ChangeInvestigationReport,
) {
    for sym in investigation
        .primary_symbols
        .iter()
        .chain(investigation.affected_symbols.iter())
        .take(PLAN_ROW_CAP)
    {
        push_unique(&mut plan.symbols_affected, sym.clone());
    }
    for dep in investigation.related_dependencies.iter().take(PLAN_ROW_CAP) {
        push_unique(&mut plan.dependencies, dep.clone());
    }
    for path in investigation.tests_possibly_affected.iter().take(PLAN_ROW_CAP) {
        push_unique(&mut plan.tests, format!("Update / extend `{path}`"));
    }
    if plan.tests.is_empty() && !investigation.affected_files.is_empty() {
        plan.tests.push(
            "Add coverage for the changed path (no existing test file observed).".into(),
        );
    }
    for item in &investigation.related_functionality {
        if let Some(path) = item.strip_prefix("docs_candidate=") {
            push_unique(&mut plan.documentation, format!("Update `{path}`"));
        }
    }
    if plan.documentation.is_empty()
        && matches!(
            investigation.edit_kind,
            Some(CodingEditKind::Add) | Some(CodingEditKind::Create)
        )
    {
        plan.documentation.push(
            "Update feature docs if a public Settings / capability surface changes.".into(),
        );
    }
}

fn fill_risk_and_summary(
    plan: &mut CodingPlan,
    objective: &CodingEditObjective,
    investigation: &ChangeInvestigationReport,
) {
    let risk_level = if !plan.constitutional_conflicts.is_empty() {
        "High"
    } else if investigation.truncated || !plan.unresolved_questions.is_empty() {
        "Medium–High"
    } else if investigation.affected_files.len() > 3
        || matches!(
            objective.kind,
            Some(CodingEditKind::Refactor) | Some(CodingEditKind::Remove)
        )
    {
        "Medium"
    } else {
        "Low–Medium"
    };
    plan.estimated_risk.push(format!(
        "Risk: **{risk_level}** — from investigation scope and unknowns."
    ));
    for effect in investigation.likely_side_effects.iter().take(6) {
        push_unique(&mut plan.estimated_risk, format!("Side effect: {effect}"));
    }
    if investigation.mode == ChangeInvestigationMode::Light {
        plan.estimated_risk.push(
            "Investigation was light — treat file lists as provisional.".into(),
        );
    }

    plan.summary
        .push(format!("Mutation Coding Plan for: {}", objective.goal));
    plan.summary.push(format!(
        "Grounded files: {} modify / {} create / {} remove; {} unresolved.",
        plan.files_to_modify.len(),
        plan.files_to_create.len(),
        plan.files_to_remove.len(),
        plan.unresolved_questions.len(),
    ));
    plan.summary.push(
        "Planning only — translate to ExecutionPlan only in a later reviewed slice.".into(),
    );
}

fn flag_constitutional_conflicts(
    plan: &mut CodingPlan,
    objective: &CodingEditObjective,
    investigation: &ChangeInvestigationReport,
) {
    let goal_l = objective.goal.to_ascii_lowercase();
    if goal_l.contains("without review")
        || goal_l.contains("skip review")
        || goal_l.contains("auto-apply")
        || goal_l.contains("auto apply")
        || goal_l.contains("just write the files")
        || goal_l.contains("don't ask")
        || goal_l.contains("do not ask")
    {
        plan.constitutional_conflicts.push(
            "Review Before Action — refusing to plan silent/auto-applied mutation; user must review before writes."
                .into(),
        );
    }
    if goal_l.contains("bypass planner")
        || goal_l.contains("call the model directly")
        || goal_l.contains("skip context")
    {
        plan.constitutional_conflicts.push(
            "Planner Over Models / Intelligence Through Context — cannot design around Planner or ContextEngine."
                .into(),
        );
    }
    if goal_l.contains("cloud-only")
        || (goal_l.contains("require cloud") && goal_l.contains("only"))
    {
        plan.constitutional_conflicts.push(
            "Offline First — flagging cloud-only mutation design; prefer local-capable approach."
                .into(),
        );
    }

    for constraint in &investigation.architectural_constraints {
        let lower = constraint.to_ascii_lowercase();
        if lower.contains("coordinate shared contracts")
            || (lower.contains("cross") && lower.contains("subsystem"))
        {
            push_unique(
                &mut plan.constitutional_conflicts,
                format!(
                    "Architecture: {constraint} — plan must not silently ignore subsystem boundaries."
                ),
            );
        }
    }
}

fn looks_like_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/tests/")
        || lower.contains("test")
        || lower.ends_with("_test.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
}

fn looks_like_doc_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/docs/") || lower.ends_with(".md")
}

fn looks_like_config_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
        || lower.contains("/config/")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if value.trim().is_empty() {
        return;
    }
    if !list.iter().any(|x| x == &value) {
        list.push(value);
    }
}

fn first_matching_dir<'a>(dirs: &'a [String], candidates: &[&str]) -> Option<&'a str> {
    for candidate in candidates {
        if let Some(found) = dirs.iter().find(|d| d.eq_ignore_ascii_case(candidate)) {
            return Some(found.as_str());
        }
    }
    None
}

fn feature_slug_from_goal(goal: &str) -> String {
    let lower = goal.to_ascii_lowercase();
    for marker in [
        "image generation",
        "parser",
        "module",
        "component",
        "feature",
        "endpoint",
        "api",
        "cli",
        "settings",
    ] {
        if lower.contains(marker) {
            return marker.replace(' ', "_");
        }
    }
    let cleaned: String = goal
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let slug = cleaned.trim_matches('_');
    if slug.is_empty() {
        "feature".into()
    } else {
        slug.chars().take(32).collect()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ActiveProjectSection, ContextBundle, CurrentFileSection, ProjectIntelligenceSection,
    };

    #[test]
    fn detects_generation_plan_examples() {
        let pong = detect_coding_plan_request(&UserRequest::new("Build Pong."));
        assert!(
            pong.is_none(),
            "greenfield pong yields to Coding Goal / agentic experience"
        );

        let parser = detect_coding_plan_request(&UserRequest::new("Create a parser.")).unwrap();
        assert_eq!(parser.kind, CodingPlanKind::Feature);

        let tests = detect_coding_plan_request(&UserRequest::new("Write tests.")).unwrap();
        assert_eq!(tests.kind, CodingPlanKind::Tests);

        assert!(detect_coding_plan_request(&UserRequest::new("Explain this file.")).is_none());
        assert!(detect_coding_plan_request(&UserRequest::new("Review my changes.")).is_none());
        assert!(detect_coding_plan_request(&UserRequest::new("hello")).is_none());
    }

    #[test]
    fn scaffold_plan_uses_bundle_only() {
        let bundle = ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("p1".into()),
                name: Some("Demo".into()),
                root_directory: Some("/proj".into()),
                detail: None,
            })
            .current_file(CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            })
            .project_intelligence(ProjectIntelligenceSection {
                languages: vec!["rust".into()],
                package_manager: Some("cargo".into()),
                build_system: Some("cargo".into()),
                top_level_dirs: vec!["crates".into(), "apps".into(), "tests".into()],
                dependency_summary: jaymi_context::DependencyGraphSummary {
                    top_level: vec!["serde".into()],
                    direct_count: 1,
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();

        let plan = scaffold_from_bundle(CodingPlanKind::Feature, "Create a parser.", &bundle);
        assert!(!plan.plan_steps.is_empty());
        assert_eq!(plan.origin, CodingPlanOrigin::GenerationPlanning);
        assert_eq!(plan.kind, Some(CodingPlanKind::Feature));
        assert!(plan.files_to_create.iter().any(|f| f.contains("parser")));
        assert!(plan.dependencies.iter().any(|d| d.contains("cargo")));
        assert!(plan.estimated_risk.iter().any(|r| r.contains("Risk")));
        let md = plan.to_markdown();
        assert!(md.contains("## Coding Plan"));
        assert!(md.contains("### Files to Create"));
        assert!(md.contains("### Estimated Risk"));
        assert!(md.contains("### Summary"));
        assert!(md.contains("planning only") || md.contains("Planning only"));
    }

    #[test]
    fn plan_from_investigation_is_grounded_and_ordered() {
        use crate::change_investigation::{
            ChangeEvidence, ChangeInvestigationMode, ChangeInvestigationReport,
        };
        use crate::coding_edit::{
            CodingEditKind, CodingEditObjective, CodingEditStatus,
        };

        let objective = CodingEditObjective {
            status: CodingEditStatus::Understood,
            kind: Some(CodingEditKind::Add),
            goal: "Add image generation to Settings.".into(),
            target_file: Some("/proj/apps/jaymi/src/settings_workspace.rs".into()),
            target_symbols: vec!["Settings".into()],
            ..CodingEditObjective::default()
        };
        let investigation = ChangeInvestigationReport {
            mode: ChangeInvestigationMode::Full,
            goal: objective.goal.clone(),
            edit_kind: Some(CodingEditKind::Add),
            primary_file: Some("/proj/apps/jaymi/src/settings_workspace.rs".into()),
            primary_symbols: vec!["Settings".into()],
            affected_files: vec![
                "/proj/apps/jaymi/src/settings_workspace.rs".into(),
                "/proj/crates/jaymi-providers/src/lib.rs".into(),
                "/proj/crates/jaymi-capabilities/src/lib.rs".into(),
            ],
            affected_symbols: vec!["Settings".into(), "ModelRegistry".into()],
            related_dependencies: vec!["package_manager=cargo".into()],
            tests_possibly_affected: vec!["/proj/apps/jaymi/tests/settings.rs".into()],
            configuration_possibly_affected: vec!["/proj/Cargo.toml".into()],
            related_functionality: vec![
                "docs_candidate=/proj/docs/settings.md".into(),
            ],
            evidence: vec![ChangeEvidence {
                path: "/proj/apps/jaymi/src/settings_workspace.rs".into(),
                title: "settings_workspace.rs".into(),
                matched: "Settings".into(),
                relevance: 90,
                line: Some(10),
                preview: "struct Settings".into(),
                source: "investigation".into(),
            }],
            architectural_constraints: vec![
                "Coordinate shared contracts when editing across observed subsystems.".into(),
            ],
            ..ChangeInvestigationReport::default()
        };
        let bundle = ContextBundle::builder().build();
        let plan = plan_from_change_investigation(&objective, &investigation, &bundle);
        assert_eq!(plan.origin, CodingPlanOrigin::ChangeInvestigation);
        assert_eq!(plan.kind, Some(CodingPlanKind::Mutation));
        assert!(plan.objective.as_deref().unwrap().contains("image generation"));
        assert!(!plan.rationale.is_empty());
        assert!(plan.plan_steps.len() >= 6);
        assert!(!plan.step_dependencies.is_empty());
        assert!(plan
            .files_to_modify
            .iter()
            .any(|f| f.contains("settings_workspace.rs")));
        assert!(!plan.files_to_create.is_empty());
        assert!(plan.symbols_affected.iter().any(|s| s == "Settings"));
        assert!(plan.tests.iter().any(|t| t.contains("settings.rs")));
        assert!(plan.documentation.iter().any(|d| d.contains("settings.md")));
        assert!(!plan.constitutional_conflicts.is_empty());
        assert!(plan.to_markdown().contains("### Step ordering"));
        assert!(plan.to_diagnostics_summary().contains("change_investigation"));
    }

    #[test]
    fn plan_keeps_ambiguity_visible_without_inventing_paths() {
        use crate::change_investigation::{ChangeInvestigationMode, ChangeInvestigationReport};
        use crate::coding_edit::{
            CodingEditKind, CodingEditObjective, CodingEditStatus,
        };

        let objective = CodingEditObjective {
            status: CodingEditStatus::NeedsClarification,
            kind: Some(CodingEditKind::Fix),
            goal: "Fix this.".into(),
            ambiguities: vec!["No bound target".into()],
            clarification: Some("Which file?".into()),
            ..CodingEditObjective::default()
        };
        let investigation = ChangeInvestigationReport {
            mode: ChangeInvestigationMode::Light,
            unknowns: vec!["Affected files unknown".into()],
            ..ChangeInvestigationReport::default()
        };
        let plan = plan_from_change_investigation(
            &objective,
            &investigation,
            &ContextBundle::builder().build(),
        );
        assert!(plan.files_to_modify.is_empty());
        assert!(plan.files_to_create.is_empty());
        assert!(plan
            .unresolved_questions
            .iter()
            .any(|q| q.contains("No bound target") || q.contains("Clarification")));
        assert!(plan.estimated_risk.iter().any(|r| r.contains("High")));
    }

    #[test]
    fn plan_flags_review_bypass_conflict() {
        use crate::change_investigation::{ChangeInvestigationMode, ChangeInvestigationReport};
        use crate::coding_edit::{
            CodingEditKind, CodingEditObjective, CodingEditStatus,
        };

        let objective = CodingEditObjective {
            status: CodingEditStatus::Understood,
            kind: Some(CodingEditKind::Change),
            goal: "Change the button without review and auto-apply.".into(),
            target_file: Some("/proj/src/button.rs".into()),
            ..CodingEditObjective::default()
        };
        let investigation = ChangeInvestigationReport {
            mode: ChangeInvestigationMode::Full,
            primary_file: Some("/proj/src/button.rs".into()),
            affected_files: vec!["/proj/src/button.rs".into()],
            ..ChangeInvestigationReport::default()
        };
        let plan = plan_from_change_investigation(
            &objective,
            &investigation,
            &ContextBundle::builder().build(),
        );
        assert!(plan
            .constitutional_conflicts
            .iter()
            .any(|c| c.contains("Review Before Action")));
    }
}
