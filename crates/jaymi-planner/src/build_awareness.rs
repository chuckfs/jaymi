//! Build Awareness (Sprint C4.1).
//!
//! Discover how the active project is built and which **verification**
//! commands are appropriate after coding changes — without executing them.
//!
//! Constitutional constraints:
//! - Planner owns verification intent and orchestration
//! - Reuse Workspace Intelligence / [`ProjectSnapshot`] (no second discovery)
//! - Prefer project-defined scripts / config / conventions over hardcoded lists
//! - Do not invent commands when evidence does not support them
//! - Read-only — no terminal / shell / mutation; Reasoning never executes
//! - Do not bypass Planner → Tool → Provider (and do not invoke tools here)

use jaymi_context::{
    BuildSystemKind, ContextBundle, ContextSessionInputs, PackageManagerKind, ProjectSnapshot,
};
use jaymi_core::UserRequest;

/// Why a candidate command would be run (observational).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationPurpose {
    CompileCheck,
    Build,
    Test,
    Lint,
    Typecheck,
    Script,
    Unknown,
}

impl VerificationPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompileCheck => "compile_check",
            Self::Build => "build",
            Self::Test => "test",
            Self::Lint => "lint",
            Self::Typecheck => "typecheck",
            Self::Script => "script",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CompileCheck => "Compile check",
            Self::Build => "Build",
            Self::Test => "Test",
            Self::Lint => "Lint",
            Self::Typecheck => "Typecheck",
            Self::Script => "Project script",
            Self::Unknown => "Unknown",
        }
    }
}

/// Confidence grounded in project evidence (not wishful defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationConfidence {
    High,
    Medium,
    Low,
    None,
}

impl VerificationConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
        }
    }
}

/// Expected shape of command output if later executed (C4.2+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutputType {
    CompilerDiagnostics,
    BuildArtifacts,
    TestResults,
    LintReport,
    ScriptOutput,
    Unknown,
}

impl ExpectedOutputType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompilerDiagnostics => "compiler_diagnostics",
            Self::BuildArtifacts => "build_artifacts",
            Self::TestResults => "test_results",
            Self::LintReport => "lint_report",
            Self::ScriptOutput => "script_output",
            Self::Unknown => "unknown",
        }
    }
}

/// Overall discovery outcome for the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildAwarenessStatus {
    /// At least one grounded candidate command.
    Ready,
    /// No session-open project / root.
    NoActiveProject,
    /// Project known but no clear build/check mechanism.
    MissingBuildConfig,
    /// Multiple ecosystems or unclear primary check.
    Ambiguous,
    /// Markers present that Tool Engine does not map to a check.
    Unsupported,
}

impl BuildAwarenessStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NoActiveProject => "no_active_project",
            Self::MissingBuildConfig => "missing_build_config",
            Self::Ambiguous => "ambiguous",
            Self::Unsupported => "unsupported",
        }
    }
}

/// One grounded verification candidate (never executed in C4.1).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct VerificationTarget {
    pub project: Option<String>,
    pub build_system: Option<String>,
    pub candidate_command: Option<String>,
    pub purpose: Option<VerificationPurpose>,
    pub working_directory: Option<String>,
    /// Config / manifest that justified the candidate.
    pub relevant_configuration: Vec<String>,
    pub expected_output_type: Option<ExpectedOutputType>,
    pub confidence: Option<VerificationConfidence>,
    pub evidence: Vec<String>,
}

/// Planner-owned build awareness report (conversation + diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct BuildAwarenessReport {
    pub status: Option<BuildAwarenessStatus>,
    pub goal: String,
    pub targets: Vec<VerificationTarget>,
    /// Preferred candidate when one stands out (index into `targets`).
    pub preferred_index: Option<usize>,
    pub notes: Vec<String>,
    pub gaps: Vec<String>,
    pub matched_rules: Vec<String>,
    /// Ambient Problems / diagnostics reused (not a second system).
    pub ambient_diagnostics: Vec<String>,
    /// Always false in C4.1 — discovery only.
    pub executed: bool,
}

impl BuildAwarenessReport {
    /// Conversation markdown — never dumps raw diagnostics internals.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Build Awareness\n");
        out.push_str(
            "\n_Discovery only — no verification command was executed in this turn._\n",
        );

        if let Some(status) = self.status {
            out.push_str(&format!(
                "\n### Status\n- {}\n",
                match status {
                    BuildAwarenessStatus::Ready => {
                        "Grounded verification candidates from project evidence."
                    }
                    BuildAwarenessStatus::NoActiveProject => {
                        "No active project is open — open a project before asking about builds."
                    }
                    BuildAwarenessStatus::MissingBuildConfig => {
                        "This project does not expose a clear build/check mechanism in Workspace Intelligence."
                    }
                    BuildAwarenessStatus::Ambiguous => {
                        "Several possible checks are present — pick one that matches what you want to verify."
                    }
                    BuildAwarenessStatus::Unsupported => {
                        "Observed project markers are not mapped to a supported verification command."
                    }
                }
            ));
        }

        if !self.goal.is_empty() {
            out.push_str("\n### Question\n- ");
            out.push_str(&self.goal);
            out.push('\n');
        }

        if let Some(idx) = self.preferred_index {
            if let Some(preferred) = self.targets.get(idx) {
                out.push_str("\n### Suggested check\n");
                append_target_markdown(&mut out, preferred);
            }
        }

        if self.targets.len() > 1
            || (self.preferred_index.is_none() && !self.targets.is_empty())
        {
            out.push_str("\n### Candidates\n");
            for (i, target) in self.targets.iter().enumerate() {
                if self.preferred_index == Some(i) && self.targets.len() > 1 {
                    continue;
                }
                append_target_markdown(&mut out, target);
                out.push('\n');
            }
        } else if self.targets.len() == 1 && self.preferred_index.is_none() {
            out.push_str("\n### Candidates\n");
            append_target_markdown(&mut out, &self.targets[0]);
        }

        if !self.ambient_diagnostics.is_empty() {
            out.push_str("\n### Ambient diagnostics (already observed)\n");
            for line in &self.ambient_diagnostics {
                out.push_str(&format!("- {line}\n"));
            }
            out.push_str(
                "- These are existing Problems/diagnostics — not the result of a fresh build.\n",
            );
        }

        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for note in &self.notes {
                out.push_str(&format!("- {note}\n"));
            }
        }

        if !self.gaps.is_empty() {
            out.push_str("\n### Gaps\n");
            for gap in &self.gaps {
                out.push_str(&format!("- {gap}\n"));
            }
        }

        out.push_str(
            "\n### Constraints\n- Read-only discovery (C4.1).\n- Commands are not run until a later verification sprint.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "build_awareness status={} targets={} preferred={} executed={} gaps={} ambient_diag={} rules={}",
            self.status.map(|s| s.as_str()).unwrap_or("unknown"),
            self.targets.len(),
            self.preferred_index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "—".into()),
            self.executed,
            self.gaps.len(),
            self.ambient_diagnostics.len(),
            self.matched_rules.len(),
        )
    }
}

fn append_target_markdown(out: &mut String, target: &VerificationTarget) {
    if let Some(cmd) = &target.candidate_command {
        out.push_str(&format!("- Command: `{cmd}`\n"));
    } else {
        out.push_str("- Command: _(none grounded)_\n");
    }
    if let Some(purpose) = target.purpose {
        out.push_str(&format!("  - Purpose: {}\n", purpose.label()));
    }
    if let Some(cwd) = &target.working_directory {
        out.push_str(&format!("  - Working directory: `{cwd}`\n"));
    }
    if let Some(build) = &target.build_system {
        out.push_str(&format!("  - Build system: `{build}`\n"));
    }
    if let Some(project) = &target.project {
        out.push_str(&format!("  - Project: `{project}`\n"));
    }
    if !target.relevant_configuration.is_empty() {
        out.push_str(&format!(
            "  - Configuration: {}\n",
            target.relevant_configuration.join(", ")
        ));
    }
    if let Some(conf) = target.confidence {
        out.push_str(&format!("  - Confidence: {}\n", conf.as_str()));
    }
    if !target.evidence.is_empty() {
        out.push_str(&format!("  - Evidence: {}\n", target.evidence.join("; ")));
    }
}

/// Detected assessment before discovery finalize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildAwarenessAssessment {
    pub goal: String,
    pub matched_rules: Vec<&'static str>,
    pub intent: BuildAwarenessIntent,
}

/// What the user is asking about builds/verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildAwarenessIntent {
    /// "Does this build?" / "Is this buildable?"
    Buildability,
    /// "Check whether this compiles."
    CompileCheck,
    /// "Verify the changes." / general verify
    Verify,
    /// "What should I run to check this?"
    RecommendCommand,
}

impl BuildAwarenessIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buildability => "buildability",
            Self::CompileCheck => "compile_check",
            Self::Verify => "verify",
            Self::RecommendCommand => "recommend_command",
        }
    }
}

/// Detect a build/verification awareness request (Planner ownership).
pub fn detect_build_awareness_request(request: &UserRequest) -> Option<BuildAwarenessAssessment> {
    // Structured tool payloads are not C4.1 soft-path ownership.
    if request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.lsp.is_some()
        || request.search.is_some()
        || request.project_knowledge.is_some()
        || request.coding_action.is_some()
    {
        return None;
    }

    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    // Yield to generation / greenfield ("Build Pong.") and specialized planners.
    // Soft-path order also prefers those routes; this keeps detect honest in isolation.
    // Do not call test_execution detect (it yields to build_awareness).
    if looks_like_creation_build(trimmed)
        || crate::code_generation::detect_generation_request(request)
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::coding_review::detect_review_request(request).is_some()
    {
        return None;
    }
    // Generation-planning ("Build Pong.") — avoid depending on full coding_plan
    // detect (which may share yield edges with understanding / Q&A).
    if trimmed.starts_with("build ")
        && !trimmed.contains("buildable")
        && !trimmed.starts_with("build?")
    {
        // "build this" / "build the project" as creation remain out of scope;
        // verification uses "does this build" / "check the build".
        if !trimmed.contains("does this build")
            && !trimmed.contains("check the build")
            && !trimmed.contains("if this builds")
            && !trimmed.contains("if it builds")
        {
            return None;
        }
    }

    let (intent, rules) = classify_build_awareness(trimmed)?;
    Some(BuildAwarenessAssessment {
        goal: content.to_string(),
        matched_rules: rules,
        intent,
    })
}

/// True when coding-edit / other soft paths should yield to C4.1.
pub fn steals_from_other_detect(request: &UserRequest) -> bool {
    detect_build_awareness_request(request).is_some()
}

fn looks_like_creation_build(trimmed: &str) -> bool {
    trimmed.starts_with("build a ")
        || trimmed.starts_with("build an ")
        || trimmed.starts_with("build the ")
        || trimmed.starts_with("build me ")
        || trimmed.starts_with("build pong")
        || trimmed == "build pong"
}

fn classify_build_awareness(trimmed: &str) -> Option<(BuildAwarenessIntent, Vec<&'static str>)> {
    // Recommend command.
    if (trimmed.contains("what should i run")
        || trimmed.contains("what do i run")
        || trimmed.contains("what command")
        || trimmed.contains("which command")
        || trimmed.starts_with("how do i check")
        || trimmed.starts_with("how should i check")
        || trimmed.starts_with("how can i check"))
        && (trimmed.contains("check")
            || trimmed.contains("verify")
            || trimmed.contains("build")
            || trimmed.contains("compile")
            || trimmed.contains("test"))
    {
        return Some((
            BuildAwarenessIntent::RecommendCommand,
            vec!["recommend_verification_command"],
        ));
    }
    if trimmed == "what should i run to check this"
        || trimmed.starts_with("what should i run to check")
        || trimmed.starts_with("what should i run to verify")
    {
        return Some((
            BuildAwarenessIntent::RecommendCommand,
            vec!["recommend_run_to_check"],
        ));
    }

    // Compile check.
    if (trimmed.contains("compile") || trimmed.contains("compiles"))
        && (trimmed.contains("check")
            || trimmed.contains("whether")
            || trimmed.starts_with("does ")
            || trimmed.starts_with("will ")
            || trimmed.contains("does this"))
    {
        return Some((
            BuildAwarenessIntent::CompileCheck,
            vec!["compile_check_phrasing"],
        ));
    }
    if trimmed == "check whether this compiles"
        || trimmed.starts_with("check whether this compiles")
        || trimmed == "does this compile"
        || trimmed == "will this compile"
    {
        return Some((
            BuildAwarenessIntent::CompileCheck,
            vec!["compile_check_exact"],
        ));
    }

    // Buildability.
    if trimmed.contains("buildable")
        || trimmed == "does this build"
        || trimmed == "will this build"
        || trimmed.starts_with("does this build")
        || trimmed.starts_with("is this project currently buildable")
        || trimmed.starts_with("is this project buildable")
        || trimmed.starts_with("is the project buildable")
        || (trimmed.contains("currently buildable") && trimmed.contains("project"))
    {
        return Some((
            BuildAwarenessIntent::Buildability,
            vec!["buildability_phrasing"],
        ));
    }

    // Verify changes / general verify (not code review).
    if trimmed == "verify the changes"
        || trimmed == "verify my changes"
        || trimmed == "verify these changes"
        || trimmed.starts_with("verify the changes")
        || trimmed.starts_with("verify my changes")
        || ((trimmed.starts_with("verify ") || trimmed.contains("verify this"))
            && (trimmed.contains("change")
                || trimmed.contains("build")
                || trimmed.contains("project")
                || trimmed.contains("code")))
    {
        return Some((BuildAwarenessIntent::Verify, vec!["verify_phrasing"]));
    }

    // Broader "check the build" / "check if it builds".
    if (trimmed.contains("check") && (trimmed.contains("build") || trimmed.contains("compiles")))
        || trimmed == "check the build"
        || trimmed.starts_with("check if this builds")
        || trimmed.starts_with("check if it builds")
    {
        return Some((
            BuildAwarenessIntent::Buildability,
            vec!["check_build_phrasing"],
        ));
    }

    None
}

/// Discover verification targets from existing WI / project context (no tools).
pub fn discover_verification(
    assessment: &BuildAwarenessAssessment,
    bundle: &ContextBundle,
    session: &ContextSessionInputs,
) -> BuildAwarenessReport {
    let mut report = BuildAwarenessReport {
        goal: assessment.goal.clone(),
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        executed: false,
        ..Default::default()
    };
    report
        .matched_rules
        .push(format!("intent:{}", assessment.intent.as_str()));

    let active = bundle.active_project();
    // Reuse ambient ProjectSnapshot only — never invent a parallel discovery scan.
    let snap = session.project_snapshot.as_ref();

    let root = active
        .root_directory
        .clone()
        .or_else(|| snap.and_then(|s| s.metadata.root_directory.clone()));
    let project_name = active
        .name
        .clone()
        .or_else(|| snap.and_then(|s| s.metadata.name.clone()));
    let project_open = session.project_open
        || active.project_id.is_some()
        || active.root_directory.is_some()
        || snap.map(|s| s.has_project()).unwrap_or(false);

    report.ambient_diagnostics = ambient_diagnostic_lines(bundle);

    if !project_open && root.is_none() {
        report.status = Some(BuildAwarenessStatus::NoActiveProject);
        report.gaps.push(
            "No active project root in Workspace Intelligence — cannot discover build commands."
                .into(),
        );
        report.notes.push(
            "Open or select a project so ProjectSnapshot / project intelligence can observe the toolchain."
                .into(),
        );
        return report;
    }

    let Some(snapshot) = snap else {
        discover_from_intelligence_only(
            &mut report,
            bundle,
            project_name.as_deref(),
            root.as_deref(),
            assessment.intent,
        );
        return report;
    };

    discover_from_snapshot(
        &mut report,
        snapshot,
        project_name.as_deref(),
        root.as_deref(),
        assessment.intent,
    );
    report
}

fn discover_from_intelligence_only(
    report: &mut BuildAwarenessReport,
    bundle: &ContextBundle,
    project_name: Option<&str>,
    root: Option<&str>,
    intent: BuildAwarenessIntent,
) {
    let intel = bundle.project_intelligence();
    let build = intel.build_system.as_deref();
    let pm = intel.package_manager.as_deref();

    report.notes.push(
        "Full ProjectSnapshot unavailable — using ProjectIntelligenceSection only (scripts may be missing)."
            .into(),
    );

    if build.is_none() && pm.is_none() {
        report.status = Some(BuildAwarenessStatus::MissingBuildConfig);
        report.gaps.push(
            "No build_system or package_manager observed in project intelligence.".into(),
        );
        return;
    }

    // Without scripts / cargo meta we only emit ecosystem conventions when the
    // build system id is one Tool Engine already represents.
    match build {
        Some("cargo") => {
            push_cargo_targets(report, project_name, root, intent, true);
        }
        Some("npm_scripts") => {
            report.status = Some(BuildAwarenessStatus::MissingBuildConfig);
            report.gaps.push(
                "npm/package.json build system observed, but script keys are not in ProjectIntelligence — refusing to invent `npm run …`."
                    .into(),
            );
            report.notes.push(format!(
                "package_manager={}; build_system=npm_scripts",
                pm.unwrap_or("—")
            ));
        }
        Some("go") => {
            push_go_targets(report, project_name, root, intent);
        }
        Some("make") => {
            push_make_default(report, project_name, root);
        }
        Some("cmake") => {
            report.status = Some(BuildAwarenessStatus::Unsupported);
            report.gaps.push(
                "CMake is observed, but no configure/build directory evidence is available — not inventing a cmake command."
                    .into(),
            );
        }
        Some(other) => {
            report.status = Some(BuildAwarenessStatus::Unsupported);
            report
                .gaps
                .push(format!("Build system `{other}` has no mapped verification command."));
        }
        None => {
            report.status = Some(BuildAwarenessStatus::MissingBuildConfig);
            report.gaps.push(format!(
                "Package manager `{}` observed without a build_system — no command invented.",
                pm.unwrap_or("unknown")
            ));
        }
    }

    finalize_status(report);
}

fn discover_from_snapshot(
    report: &mut BuildAwarenessReport,
    snapshot: &ProjectSnapshot,
    project_name: Option<&str>,
    root: Option<&str>,
    intent: BuildAwarenessIntent,
) {
    let name = project_name
        .map(str::to_string)
        .or_else(|| snapshot.metadata.name.clone())
        .or_else(|| {
            snapshot
                .cargo
                .as_ref()
                .and_then(|c| c.package_name.clone())
        })
        .or_else(|| {
            snapshot
                .npm
                .as_ref()
                .and_then(|n| n.package_name.clone())
        });
    let cwd = root
        .map(str::to_string)
        .or_else(|| snapshot.metadata.root_directory.clone());

    let mut ecosystems = 0u32;

    if snapshot.cargo.is_some()
        || matches!(snapshot.build_system, Some(BuildSystemKind::Cargo))
        || matches!(snapshot.package_manager, Some(PackageManagerKind::Cargo))
    {
        ecosystems += 1;
        push_cargo_targets(
            report,
            name.as_deref(),
            cwd.as_deref(),
            intent,
            snapshot.cargo.is_some()
                || matches!(snapshot.build_system, Some(BuildSystemKind::Cargo)),
        );
    }

    if let Some(npm) = &snapshot.npm {
        let before = report.targets.len();
        push_npm_script_targets(
            report,
            name.as_deref(),
            cwd.as_deref(),
            snapshot.package_manager.as_ref(),
            &npm.scripts,
            intent,
        );
        if report.targets.len() > before {
            ecosystems += 1;
        } else if matches!(
            snapshot.build_system,
            Some(BuildSystemKind::NpmScripts)
        ) && snapshot.cargo.is_none()
        {
            ecosystems += 1;
            report.gaps.push(
                "package.json is present but no verification-oriented scripts were observed — not inventing npm scripts."
                    .into(),
            );
        }
    } else if matches!(snapshot.build_system, Some(BuildSystemKind::NpmScripts)) {
        ecosystems += 1;
        report.gaps.push(
            "npm_scripts build system observed without parsed package.json scripts — not inventing commands."
                .into(),
        );
    }

    if matches!(snapshot.build_system, Some(BuildSystemKind::Go))
        || matches!(snapshot.package_manager, Some(PackageManagerKind::GoMod))
    {
        ecosystems += 1;
        push_go_targets(report, name.as_deref(), cwd.as_deref(), intent);
    }

    if matches!(snapshot.build_system, Some(BuildSystemKind::Make)) {
        ecosystems += 1;
        push_make_default(report, name.as_deref(), cwd.as_deref());
    }

    if matches!(snapshot.build_system, Some(BuildSystemKind::CMake)) {
        ecosystems += 1;
        report.notes.push(
            "CMakeLists.txt observed — no single project-defined check command is available without a build directory."
                .into(),
        );
        if report.targets.is_empty() {
            report.gaps.push(
                "CMake detected; refusing to invent `cmake --build` without configuration evidence."
                    .into(),
            );
        }
    }

    if let Some(BuildSystemKind::Unknown(label)) = &snapshot.build_system {
        report.gaps.push(format!(
            "Unrecognized build system `{label}` — no command invented."
        ));
    }

    if report.targets.is_empty()
        && snapshot.build_system.is_none()
        && snapshot.package_manager.is_none()
        && snapshot.cargo.is_none()
        && snapshot.npm.is_none()
    {
        report.status = Some(BuildAwarenessStatus::MissingBuildConfig);
        report.gaps.push(
            "Project identity is present, but no build system, package manager, Cargo.toml, or package.json scripts were observed."
                .into(),
        );
        return;
    }

    if ecosystems > 1 && report.targets.len() > 1 {
        report.notes.push(
            "Multiple build ecosystems / check styles observed — listing candidates without inventing a single winner."
                .into(),
        );
    }

    // Prefer compile-oriented commands for compile/buildability intents.
    report.preferred_index = pick_preferred(&report.targets, intent);
    finalize_status(report);

    match intent {
        BuildAwarenessIntent::Buildability => {
            report.notes.push(
                "Buildability cannot be confirmed without running a check (deferred to a later sprint)."
                    .into(),
            );
            if !report.ambient_diagnostics.is_empty() {
                report.notes.push(
                    "Ambient diagnostics may indicate the project is currently not cleanly building."
                        .into(),
                );
            }
        }
        BuildAwarenessIntent::CompileCheck | BuildAwarenessIntent::Verify => {
            report.notes.push(
                "Suggested checks are grounded in project evidence; results require a later execution sprint."
                    .into(),
            );
        }
        BuildAwarenessIntent::RecommendCommand => {}
    }
}

fn finalize_status(report: &mut BuildAwarenessReport) {
    if report.targets.is_empty() {
        if report.status.is_some() {
            return;
        }
        if report
            .gaps
            .iter()
            .any(|g| g.contains("Unrecognized") || g.contains("not mapped") || g.contains("CMake"))
        {
            report.status = Some(BuildAwarenessStatus::Unsupported);
        } else {
            report.status = Some(BuildAwarenessStatus::MissingBuildConfig);
        }
        return;
    }
    if report
        .notes
        .iter()
        .any(|n| n.contains("Multiple build ecosystems"))
    {
        report.status = Some(BuildAwarenessStatus::Ambiguous);
    } else {
        report.status = Some(BuildAwarenessStatus::Ready);
    }
}

fn push_cargo_targets(
    report: &mut BuildAwarenessReport,
    project: Option<&str>,
    cwd: Option<&str>,
    intent: BuildAwarenessIntent,
    strong_evidence: bool,
) {
    let conf = if strong_evidence {
        VerificationConfidence::High
    } else {
        VerificationConfidence::Medium
    };
    let config = vec!["Cargo.toml".into()];
    let evidence_base = vec![
        "build_system=cargo".into(),
        "Cargo.toml observed (ProjectSnapshot)".into(),
    ];

    // Always offer check for compile/verify; build for buildability.
    let want_check = matches!(
        intent,
        BuildAwarenessIntent::CompileCheck
            | BuildAwarenessIntent::Verify
            | BuildAwarenessIntent::RecommendCommand
            | BuildAwarenessIntent::Buildability
    );
    let want_build = matches!(
        intent,
        BuildAwarenessIntent::Buildability
            | BuildAwarenessIntent::RecommendCommand
            | BuildAwarenessIntent::Verify
    );

    if want_check {
        report.targets.push(VerificationTarget {
            project: project.map(str::to_string),
            build_system: Some("cargo".into()),
            candidate_command: Some("cargo check".into()),
            purpose: Some(VerificationPurpose::CompileCheck),
            working_directory: cwd.map(str::to_string),
            relevant_configuration: config.clone(),
            expected_output_type: Some(ExpectedOutputType::CompilerDiagnostics),
            confidence: Some(conf),
            evidence: evidence_base.clone(),
        });
    }
    if want_build {
        report.targets.push(VerificationTarget {
            project: project.map(str::to_string),
            build_system: Some("cargo".into()),
            candidate_command: Some("cargo build".into()),
            purpose: Some(VerificationPurpose::Build),
            working_directory: cwd.map(str::to_string),
            relevant_configuration: config.clone(),
            expected_output_type: Some(ExpectedOutputType::BuildArtifacts),
            confidence: Some(conf),
            evidence: evidence_base.clone(),
        });
    }
    // cargo test is a Cargo toolchain convention already represented by Tool Engine,
    // offered as a secondary verify candidate (not invented from unrelated ecosystems).
    if matches!(
        intent,
        BuildAwarenessIntent::Verify | BuildAwarenessIntent::RecommendCommand
    ) {
        report.targets.push(VerificationTarget {
            project: project.map(str::to_string),
            build_system: Some("cargo".into()),
            candidate_command: Some("cargo test".into()),
            purpose: Some(VerificationPurpose::Test),
            working_directory: cwd.map(str::to_string),
            relevant_configuration: config,
            expected_output_type: Some(ExpectedOutputType::TestResults),
            confidence: Some(VerificationConfidence::Medium),
            evidence: {
                let mut e = evidence_base;
                e.push("Cargo test subcommand is part of the observed Cargo toolchain".into());
                e
            },
        });
    }
}

fn push_npm_script_targets(
    report: &mut BuildAwarenessReport,
    project: Option<&str>,
    cwd: Option<&str>,
    package_manager: Option<&PackageManagerKind>,
    scripts: &[String],
    intent: BuildAwarenessIntent,
) {
    if scripts.is_empty() {
        return;
    }

    let runner = match package_manager {
        Some(PackageManagerKind::Pnpm) => "pnpm run",
        Some(PackageManagerKind::Yarn) => "yarn run",
        Some(PackageManagerKind::Npm) | None => "npm run",
        Some(other) => {
            // Cargo/Go/etc. as package_manager with npm scripts → still use npm run.
            if matches!(
                other,
                PackageManagerKind::Cargo
                    | PackageManagerKind::GoMod
                    | PackageManagerKind::Pip
                    | PackageManagerKind::Poetry
            ) {
                "npm run"
            } else {
                "npm run"
            }
        }
    };

    // Prefer project-defined verification scripts; never invent missing keys.
    let ranked: &[(&str, VerificationPurpose, ExpectedOutputType)] = &[
        (
            "typecheck",
            VerificationPurpose::Typecheck,
            ExpectedOutputType::CompilerDiagnostics,
        ),
        (
            "tsc",
            VerificationPurpose::Typecheck,
            ExpectedOutputType::CompilerDiagnostics,
        ),
        (
            "check",
            VerificationPurpose::CompileCheck,
            ExpectedOutputType::CompilerDiagnostics,
        ),
        (
            "lint",
            VerificationPurpose::Lint,
            ExpectedOutputType::LintReport,
        ),
        (
            "test",
            VerificationPurpose::Test,
            ExpectedOutputType::TestResults,
        ),
        (
            "verify",
            VerificationPurpose::CompileCheck,
            ExpectedOutputType::ScriptOutput,
        ),
        (
            "ci",
            VerificationPurpose::CompileCheck,
            ExpectedOutputType::ScriptOutput,
        ),
        (
            "build",
            VerificationPurpose::Build,
            ExpectedOutputType::BuildArtifacts,
        ),
    ];

    let mut matched_any = false;
    for (key, purpose, output) in ranked {
        if !scripts.iter().any(|s| s == key) {
            continue;
        }
        // Intent filtering: compile/buildability prefer typecheck/check/build;
        // verify/recommend keep all matched verification scripts.
        let include = match intent {
            BuildAwarenessIntent::CompileCheck => matches!(
                purpose,
                VerificationPurpose::Typecheck
                    | VerificationPurpose::CompileCheck
                    | VerificationPurpose::Lint
            ),
            BuildAwarenessIntent::Buildability => matches!(
                purpose,
                VerificationPurpose::Build
                    | VerificationPurpose::CompileCheck
                    | VerificationPurpose::Typecheck
            ),
            BuildAwarenessIntent::Verify | BuildAwarenessIntent::RecommendCommand => true,
        };
        if !include {
            continue;
        }
        matched_any = true;
        report.targets.push(VerificationTarget {
            project: project.map(str::to_string),
            build_system: Some("npm_scripts".into()),
            candidate_command: Some(format!("{runner} {key}")),
            purpose: Some(*purpose),
            working_directory: cwd.map(str::to_string),
            relevant_configuration: vec![format!("package.json#scripts.{key}")],
            expected_output_type: Some(*output),
            confidence: Some(VerificationConfidence::High),
            evidence: vec![
                format!("package.json script `{key}` observed"),
                format!("runner={runner}"),
            ],
        });
    }

    if !matched_any {
        report.notes.push(format!(
            "package.json scripts observed ({}) but none matched known verification names (check/test/build/lint/…) — not inventing a command.",
            scripts.join(", ")
        ));
    }
}

fn push_go_targets(
    report: &mut BuildAwarenessReport,
    project: Option<&str>,
    cwd: Option<&str>,
    intent: BuildAwarenessIntent,
) {
    let config = vec!["go.mod".into()];
    let evidence = vec![
        "build_system=go".into(),
        "go.mod observed (ProjectSnapshot / toolchain)".into(),
    ];
    if matches!(
        intent,
        BuildAwarenessIntent::CompileCheck
            | BuildAwarenessIntent::Buildability
            | BuildAwarenessIntent::Verify
            | BuildAwarenessIntent::RecommendCommand
    ) {
        report.targets.push(VerificationTarget {
            project: project.map(str::to_string),
            build_system: Some("go".into()),
            candidate_command: Some("go build ./...".into()),
            purpose: Some(VerificationPurpose::Build),
            working_directory: cwd.map(str::to_string),
            relevant_configuration: config.clone(),
            expected_output_type: Some(ExpectedOutputType::CompilerDiagnostics),
            confidence: Some(VerificationConfidence::High),
            evidence: evidence.clone(),
        });
    }
    if matches!(
        intent,
        BuildAwarenessIntent::Verify | BuildAwarenessIntent::RecommendCommand
    ) {
        report.targets.push(VerificationTarget {
            project: project.map(str::to_string),
            build_system: Some("go".into()),
            candidate_command: Some("go test ./...".into()),
            purpose: Some(VerificationPurpose::Test),
            working_directory: cwd.map(str::to_string),
            relevant_configuration: config,
            expected_output_type: Some(ExpectedOutputType::TestResults),
            confidence: Some(VerificationConfidence::Medium),
            evidence,
        });
    }
}

fn push_make_default(report: &mut BuildAwarenessReport, project: Option<&str>, cwd: Option<&str>) {
    report.targets.push(VerificationTarget {
        project: project.map(str::to_string),
        build_system: Some("make".into()),
        candidate_command: Some("make".into()),
        purpose: Some(VerificationPurpose::Build),
        working_directory: cwd.map(str::to_string),
        relevant_configuration: vec!["Makefile".into()],
        expected_output_type: Some(ExpectedOutputType::Unknown),
        confidence: Some(VerificationConfidence::Low),
        evidence: vec![
            "Makefile observed".into(),
            "Specific Make targets are not in ProjectSnapshot — default `make` only; not inventing `make test`.".into(),
        ],
    });
    report.notes.push(
        "Make targets were not parsed from the Makefile — only the default target is suggested."
            .into(),
    );
}

fn pick_preferred(targets: &[VerificationTarget], intent: BuildAwarenessIntent) -> Option<usize> {
    if targets.is_empty() {
        return None;
    }
    let preferred_purpose = match intent {
        BuildAwarenessIntent::CompileCheck => VerificationPurpose::CompileCheck,
        BuildAwarenessIntent::Buildability => VerificationPurpose::Build,
        BuildAwarenessIntent::Verify | BuildAwarenessIntent::RecommendCommand => {
            VerificationPurpose::CompileCheck
        }
    };
    // Prefer High confidence + matching purpose, then Typecheck, then first.
    targets
        .iter()
        .enumerate()
        .filter(|(_, t)| t.purpose == Some(preferred_purpose))
        .min_by_key(|(_, t)| match t.confidence {
            Some(VerificationConfidence::High) => 0,
            Some(VerificationConfidence::Medium) => 1,
            Some(VerificationConfidence::Low) => 2,
            _ => 3,
        })
        .map(|(i, _)| i)
        .or_else(|| {
            targets
                .iter()
                .position(|t| t.purpose == Some(VerificationPurpose::Typecheck))
        })
        .or(Some(0))
}

fn ambient_diagnostic_lines(bundle: &ContextBundle) -> Vec<String> {
    let diags = bundle.diagnostics();
    diags
        .diagnostics
        .iter()
        .filter(|d| {
            let sev = d.severity.to_ascii_lowercase();
            sev.contains("error") || sev == "err"
        })
        .take(5)
        .map(|d| {
            let path = d.path.as_deref().unwrap_or("—");
            let line = d
                .line
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".into());
            format!(
                "{path}:{line} — {}",
                truncate(&d.message, 120)
            )
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ActiveProjectSection, BundleDiagnostic, CargoProjectMeta, ContextBundle,
        DiagnosticsSection, NpmProjectMeta, ProjectIntelligenceSection, ProjectMetadata,
        ProjectSnapshot,
    };

    fn rust_snapshot() -> ProjectSnapshot {
        ProjectSnapshot {
            metadata: ProjectMetadata {
                project_id: Some("p1".into()),
                name: Some("jaymi".into()),
                root_directory: Some("/proj".into()),
                ..Default::default()
            },
            package_manager: Some(PackageManagerKind::Cargo),
            build_system: Some(BuildSystemKind::Cargo),
            cargo: Some(CargoProjectMeta {
                package_name: Some("jaymi".into()),
                edition: Some("2021".into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn npm_snapshot(scripts: Vec<&str>) -> ProjectSnapshot {
        ProjectSnapshot {
            metadata: ProjectMetadata {
                project_id: Some("p2".into()),
                name: Some("web".into()),
                root_directory: Some("/web".into()),
                ..Default::default()
            },
            package_manager: Some(PackageManagerKind::Npm),
            build_system: Some(BuildSystemKind::NpmScripts),
            npm: Some(NpmProjectMeta {
                package_name: Some("web".into()),
                scripts: scripts.into_iter().map(str::to_string).collect(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn session_with(snap: ProjectSnapshot) -> ContextSessionInputs {
        ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(snap),
            ..Default::default()
        }
    }

    fn bundle_with_project(name: &str, root: &str) -> ContextBundle {
        ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("p".into()),
                name: Some(name.into()),
                root_directory: Some(root.into()),
                ..Default::default()
            })
            .build()
    }

    #[test]
    fn detect_example_phrases() {
        for phrase in [
            "Does this build?",
            "Check whether this compiles.",
            "Verify the changes.",
            "What should I run to check this?",
            "Is this project currently buildable?",
        ] {
            assert!(
                detect_build_awareness_request(&UserRequest::new(phrase)).is_some(),
                "should detect: {phrase}"
            );
        }
    }

    #[test]
    fn detect_rejects_creation_and_mutation_payloads() {
        assert!(detect_build_awareness_request(&UserRequest::new("Build Pong.")).is_none());
        assert!(detect_build_awareness_request(&UserRequest::new("build an app")).is_none());
        assert!(detect_build_awareness_request(&UserRequest::new("Fix this.")).is_none());
        let mut req = UserRequest::new("Does this build?");
        req.write_file = Some(jaymi_core::WriteFileRequest {
            path: std::path::PathBuf::from("/tmp/x"),
            content: "x".into(),
        });
        assert!(detect_build_awareness_request(&req).is_none());
    }

    #[test]
    fn known_rust_project_discovers_cargo_check() {
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Does this build?")).unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("jaymi", "/proj"),
            &session_with(rust_snapshot()),
        );
        assert!(
            matches!(
                report.status,
                Some(BuildAwarenessStatus::Ready) | Some(BuildAwarenessStatus::Ambiguous)
            ),
            "{:?}",
            report.status
        );
        assert!(report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref() == Some("cargo check")));
        assert!(report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref() == Some("cargo build")));
        assert!(!report.executed);
        assert!(report
            .targets
            .iter()
            .all(|t| t.working_directory.as_deref() == Some("/proj")));
    }

    #[test]
    fn package_scripts_are_used_not_invented() {
        let assessment =
            detect_build_awareness_request(&UserRequest::new("What should I run to check this?"))
                .unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("web", "/web"),
            &session_with(npm_snapshot(vec!["dev", "build", "lint"])),
        );
        assert!(report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref() == Some("npm run build")));
        assert!(report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref() == Some("npm run lint")));
        assert!(!report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref() == Some("npm run test")));
        assert!(!report.executed);
    }

    #[test]
    fn multiple_possible_checks_listed() {
        let mut snap = rust_snapshot();
        snap.npm = Some(NpmProjectMeta {
            package_name: Some("ui".into()),
            scripts: vec!["test".into(), "build".into()],
            ..Default::default()
        });
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Verify the changes.")).unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("jaymi", "/proj"),
            &session_with(snap),
        );
        assert!(report.targets.len() >= 2);
        assert_eq!(report.status, Some(BuildAwarenessStatus::Ambiguous));
    }

    #[test]
    fn missing_build_configuration() {
        let snap = ProjectSnapshot {
            metadata: ProjectMetadata {
                project_id: Some("p".into()),
                name: Some("notes".into()),
                root_directory: Some("/notes".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Does this build?")).unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("notes", "/notes"),
            &session_with(snap),
        );
        assert_eq!(report.status, Some(BuildAwarenessStatus::MissingBuildConfig));
        assert!(report.targets.is_empty());
        assert!(!report.executed);
    }

    #[test]
    fn no_active_project() {
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Does this build?")).unwrap();
        let report = discover_verification(
            &assessment,
            &ContextBundle::builder().build(),
            &ContextSessionInputs::default(),
        );
        assert_eq!(report.status, Some(BuildAwarenessStatus::NoActiveProject));
        assert!(report.targets.is_empty());
    }

    #[test]
    fn unsupported_cmake_without_inventing() {
        let snap = ProjectSnapshot {
            metadata: ProjectMetadata {
                project_id: Some("p".into()),
                name: Some("native".into()),
                root_directory: Some("/native".into()),
                ..Default::default()
            },
            build_system: Some(BuildSystemKind::CMake),
            ..Default::default()
        };
        let assessment =
            detect_build_awareness_request(&UserRequest::new("What should I run to check this?"))
                .unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("native", "/native"),
            &session_with(snap),
        );
        assert!(report.targets.is_empty());
        assert!(report.gaps.iter().any(|g| g.contains("CMake")));
        assert!(!report
            .targets
            .iter()
            .any(|t| t.candidate_command.as_deref().unwrap_or("").contains("cmake")));
    }

    #[test]
    fn no_command_invention_for_unrelated_scripts() {
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Verify the changes.")).unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("web", "/web"),
            &session_with(npm_snapshot(vec!["dev", "start"])),
        );
        assert!(report.targets.is_empty());
        assert!(report.notes.iter().any(|n| n.contains("not inventing")));
    }

    #[test]
    fn no_mutation_and_markdown_marks_read_only() {
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Does this build?")).unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("jaymi", "/proj"),
            &session_with(rust_snapshot()),
        );
        assert!(!report.executed);
        let md = report.to_markdown();
        assert!(md.contains("no verification command was executed"));
        assert!(!md.contains("## Execution"));
        assert!(report.to_diagnostics_summary().contains("build_awareness"));
        assert!(report.to_diagnostics_summary().contains("executed=false"));
    }

    #[test]
    fn reuses_ambient_diagnostics() {
        let bundle = ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("p".into()),
                name: Some("jaymi".into()),
                root_directory: Some("/proj".into()),
                ..Default::default()
            })
            .diagnostics(DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    severity: "error".into(),
                    path: Some("/proj/main.rs".into()),
                    message: "cannot find value `x`".into(),
                    line: Some(10),
                    column: Some(1),
                    source: None,
                }],
            })
            .project_intelligence(ProjectIntelligenceSection {
                build_system: Some("cargo".into()),
                package_manager: Some("cargo".into()),
                ..Default::default()
            })
            .build();
        let assessment =
            detect_build_awareness_request(&UserRequest::new(
                "Is this project currently buildable?",
            ))
            .unwrap();
        let report = discover_verification(
            &assessment,
            &bundle,
            &session_with(rust_snapshot()),
        );
        assert!(!report.ambient_diagnostics.is_empty());
        assert!(report
            .ambient_diagnostics
            .iter()
            .any(|l| l.contains("cannot find value")));
    }

    #[test]
    fn ambiguous_when_only_make_default() {
        let snap = ProjectSnapshot {
            metadata: ProjectMetadata {
                project_id: Some("p".into()),
                name: Some("legacy".into()),
                root_directory: Some("/legacy".into()),
                ..Default::default()
            },
            build_system: Some(BuildSystemKind::Make),
            ..Default::default()
        };
        let assessment =
            detect_build_awareness_request(&UserRequest::new("Does this build?")).unwrap();
        let report = discover_verification(
            &assessment,
            &bundle_with_project("legacy", "/legacy"),
            &session_with(snap),
        );
        assert_eq!(
            report.targets[0].candidate_command.as_deref(),
            Some("make")
        );
        assert_eq!(
            report.targets[0].confidence,
            Some(VerificationConfidence::Low)
        );
    }
}
