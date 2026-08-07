//! Generation Planning / Coding Plans (Sprint C1.4).
//!
//! Before generating code, the Planner produces a structured **Coding Plan**
//! from already-assembled Workspace Intelligence — observation and proposal
//! only.
//!
//! Constitutional constraints:
//! - No new context systems (ContextEngine remains sole ContextBundle factory)
//! - No provider bypasses
//! - No filesystem scans
//! - No tool execution
//! - No file writes
//! - No code generation yet
//! - No [`crate::ExecutionPlan`] / tool-gated review (that is later)
//! - Planner ownership unchanged (detect + scaffold + instruct Reasoning)

use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

/// Kind of generation the user is asking to plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodingPlanKind {
    /// Greenfield / sample app ("Build Pong.").
    NewProject,
    /// New component / feature ("Create a parser.").
    Feature,
    /// Tests ("Write tests.").
    Tests,
    /// Generic generation ask.
    Generic,
}

impl CodingPlanKind {
    /// Stable id for AssembleHints / diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewProject => "new_project",
            Self::Feature => "feature",
            Self::Tests => "tests",
            Self::Generic => "generic",
        }
    }
}

/// Structured Coding Plan returned to the conversation (planning only).
///
/// Not an [`crate::ExecutionPlan`] — never executes tools or writes files.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingPlan {
    /// Ordered plan steps (what would be done, not doing it).
    pub plan_steps: Vec<String>,
    /// Candidate paths / modules to create (proposal only).
    pub files_to_create: Vec<String>,
    /// Candidate paths / modules to modify (proposal only).
    pub files_to_modify: Vec<String>,
    /// Likely dependencies / toolchain needs from WI.
    pub dependencies: Vec<String>,
    /// Estimated risk label + reasons (observational).
    pub estimated_risk: Vec<String>,
    /// Short summary of the proposed plan.
    pub summary: Vec<String>,
}

impl CodingPlan {
    /// True when every section is empty.
    pub fn is_empty(&self) -> bool {
        self.plan_steps.is_empty()
            && self.files_to_create.is_empty()
            && self.files_to_modify.is_empty()
            && self.dependencies.is_empty()
            && self.estimated_risk.is_empty()
            && self.summary.is_empty()
    }

    /// Conversation-visible markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Plan\n");
        push_section(&mut out, "Plan", &self.plan_steps);
        push_section(&mut out, "Files to Create", &self.files_to_create);
        push_section(&mut out, "Files to Modify", &self.files_to_modify);
        push_section(&mut out, "Dependencies", &self.dependencies);
        push_section(&mut out, "Estimated Risk", &self.estimated_risk);
        push_section(&mut out, "Summary", &self.summary);
        out
    }

    /// Prompt instruction for Reasoning.
    pub fn prompt_instruction(&self, kind: CodingPlanKind, goal: &str) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Respond with a Coding Plan (kind=`{}`) for goal: {goal}",
            kind.as_str()
        ));
        lines.push(
            "Use only Workspace Intelligence already in this prompt (project layout, languages, package manager, open files, git, workspace memory)."
                .into(),
        );
        lines.push(
            "Planning only — do not generate full source code, write files, execute tools, or produce an Execution Plan."
                .into(),
        );
        lines.push(
            "Fill these markdown headings exactly: Plan · Files to Create · Files to Modify · Dependencies · Estimated Risk · Summary."
                .into(),
        );
        lines.push("Starter observations from Workspace Intelligence:".into());
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
    let mut plan = CodingPlan::default();
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
    let root = active.root_directory.as_deref().unwrap_or("(no project root observed)");

    plan.summary.push(format!("Goal: {goal}"));
    plan.summary.push(format!(
        "Planning only — no code generation, tool execution, or file writes in this turn."
    ));
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

            if let Some(dir) = first_matching_dir(&project.top_level_dirs, &["apps", "src", "examples"])
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
                "Risk: **Medium–High** — greenfield sample can sprawl without agreed scope.".into(),
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
            plan.plan_steps
                .push("Identify the unit under test from active file / selection / recent edits."
                    .into());
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
                        "Test file beside the active source (confirm framework convention).".into(),
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
                "Specific create list pending clearer scope (ask what artifact is desired).".into(),
            );
            plan.estimated_risk.push(
                "Risk: **Medium** — underspecified generation ask; prefer a narrower goal.".into(),
            );
        }
    }

    // Dependencies from ProjectSnapshot / WI.
    if let Some(pm) = &project.package_manager {
        plan.dependencies
            .push(format!("package_manager: {pm}"));
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

    // Risk amplifiers from git / dirty tree.
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
            "No active project root observed — open or create a project before generating.".into(),
        );
    }

    plan.plan_steps.push(
        "Constitutional gate: this response is a Coding Plan only (no Execution Plan, no writes)."
            .into(),
    );

    plan
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
    for marker in ["parser", "module", "component", "feature", "endpoint", "api", "cli"] {
        if lower.contains(marker) {
            return marker.to_string();
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
        let pong = detect_coding_plan_request(&UserRequest::new("Build Pong.")).unwrap();
        assert_eq!(pong.kind, CodingPlanKind::NewProject);

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
}
