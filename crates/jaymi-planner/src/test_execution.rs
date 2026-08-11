//! Test Execution (Sprint C4.2).
//!
//! Execute project tests through Planner → Tool → Provider, reusing C4.1
//! [`VerificationTarget`] discovery. Never lets Reasoning run shell commands.
//!
//! Constitutional constraints:
//! - Planner owns test intent and orchestration
//! - Reuse Tool Engine terminal path + permission / risk / review gates
//! - Prefer targeted tests when changed-file evidence supports it
//! - No automatic code mutation or silent retries
//! - Bound large output while preserving actionable errors

use jaymi_context::{
    ContextBundle, ContextSessionInputs, GitSnapshot, ProjectSnapshot, RuntimeCommandKind,
    RuntimeCommandOutcome, RuntimeSnapshot,
};
use jaymi_core::UserRequest;
use jaymi_tools::ToolOutput;

use crate::build_awareness::{
    discover_verification, BuildAwarenessAssessment, BuildAwarenessIntent, BuildAwarenessStatus,
    ExpectedOutputType, VerificationConfidence, VerificationPurpose, VerificationTarget,
};

/// Hard cap for conversation / context excerpts (chars).
pub const OUTPUT_CHAR_CAP: usize = 8_000;
/// Prefer keeping this many trailing characters (error tails).
pub const OUTPUT_TAIL_KEEP: usize = 5_000;
/// Prefer keeping this many leading characters (command banner).
pub const OUTPUT_HEAD_KEEP: usize = 1_500;

/// Scope of the selected suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestScope {
    Targeted,
    FullSuite,
}

impl TestScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Targeted => "targeted",
            Self::FullSuite => "full_suite",
        }
    }
}

/// Lifecycle / outcome of a test-execution turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestExecutionStatus {
    /// Awaiting Review Before Action (terminal permission / ToolRisk).
    AwaitingReview,
    /// Permission / policy denied before run.
    Denied,
    /// No grounded test command in project evidence.
    MissingCommand,
    /// Command finished with exit 0.
    Passed,
    /// Command finished with non-zero exit.
    Failed,
    /// Provider timed out waiting for completion.
    TimedOut,
    /// Tool/provider error (not a test assertion failure).
    CommandError,
    /// Planned but not yet run (discovery-only placeholder).
    Pending,
}

impl TestExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingReview => "awaiting_review",
            Self::Denied => "denied",
            Self::MissingCommand => "missing_command",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::CommandError => "command_error",
            Self::Pending => "pending",
        }
    }
}

/// User intent class for test phrasing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestExecutionIntent {
    /// "Run the tests." / full suite preference.
    RunTests,
    /// "Test what we just changed." / targeted preference.
    Targeted,
    /// "Verify the tests pass." / "Run the relevant tests."
    VerifyPass,
}

impl TestExecutionIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunTests => "run_tests",
            Self::Targeted => "targeted",
            Self::VerifyPass => "verify_pass",
        }
    }

    pub fn prefers_targeted(self) -> bool {
        matches!(self, Self::Targeted | Self::VerifyPass)
    }
}

/// Detected assessment before selection / dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestExecutionAssessment {
    pub goal: String,
    pub intent: TestExecutionIntent,
    pub matched_rules: Vec<&'static str>,
}

/// Planner-owned test execution report (conversation + diagnostics + context).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TestExecutionReport {
    pub status: Option<TestExecutionStatus>,
    pub goal: String,
    pub scope: Option<TestScope>,
    pub target: Option<VerificationTarget>,
    pub command: Option<String>,
    pub working_directory: Option<String>,
    pub exit_status: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub duration_ms: Option<u64>,
    pub affected_project: Option<String>,
    pub truncated: bool,
    pub notes: Vec<String>,
    pub gaps: Vec<String>,
    pub matched_rules: Vec<String>,
    /// True when a terminal tool was invoked (or paused for review).
    pub dispatched: bool,
    /// True when mutation tools were invoked (always false in C4.2).
    pub mutated: bool,
    /// Structured verification evidence (Sprint C4.3), when parsed.
    pub evidence: Option<crate::verification_diagnostics::VerificationEvidence>,
}

impl TestExecutionReport {
    pub fn to_markdown(&self) -> String {
        // Prefer structured verification feedback (C4.3) over raw log dumps.
        if let Some(evidence) = &self.evidence {
            let mut out = evidence.to_conversation_markdown();
            let mut extra = String::new();
            if !self.goal.is_empty() {
                extra.push_str(&format!("\n### Request\n- {}\n", self.goal));
            }
            if let Some(scope) = self.scope {
                extra.push_str(&format!("- Scope: {}\n", scope.as_str()));
            }
            if let Some(project) = &self.affected_project {
                extra.push_str(&format!("- Project: `{project}`\n"));
            }
            if !self.notes.is_empty() {
                extra.push_str("\n### Notes\n");
                for note in &self.notes {
                    extra.push_str(&format!("- {note}\n"));
                }
            }
            if !self.gaps.is_empty() {
                extra.push_str("\n### Gaps\n");
                for gap in &self.gaps {
                    extra.push_str(&format!("- {gap}\n"));
                }
            }
            if !extra.is_empty() {
                if let Some(idx) = out.rfind("\n### Constraints\n") {
                    out.insert_str(idx, &extra);
                } else {
                    out.push_str(&extra);
                }
            }
            return out;
        }

        let mut out = String::from("## Test Execution\n");
        if let Some(status) = self.status {
            out.push_str(&format!(
                "\n### Status\n- {}\n",
                match status {
                    TestExecutionStatus::AwaitingReview => {
                        "Ready to run tests after your approval (Review Before Action)."
                    }
                    TestExecutionStatus::Denied => {
                        "Test execution was denied by permission or policy — nothing ran."
                    }
                    TestExecutionStatus::MissingCommand => {
                        "No project test command is grounded in Workspace Intelligence."
                    }
                    TestExecutionStatus::Passed => "Tests passed.",
                    TestExecutionStatus::Failed => {
                        "Tests failed — no code was modified automatically."
                    }
                    TestExecutionStatus::TimedOut => {
                        "Test command timed out — no automatic retry."
                    }
                    TestExecutionStatus::CommandError => {
                        "Test command could not complete (tool/provider error)."
                    }
                    TestExecutionStatus::Pending => "Test run is prepared but not yet executed.",
                }
            ));
        }

        if !self.goal.is_empty() {
            out.push_str("\n### Request\n- ");
            out.push_str(&self.goal);
            out.push('\n');
        }

        if let Some(cmd) = &self.command {
            out.push_str(&format!("\n### Command\n- `{cmd}`\n"));
        }
        if let Some(cwd) = &self.working_directory {
            out.push_str(&format!("- Working directory: `{cwd}`\n"));
        }
        if let Some(scope) = self.scope {
            out.push_str(&format!("- Scope: {}\n", scope.as_str()));
        }
        if let Some(project) = &self.affected_project {
            out.push_str(&format!("- Project: `{project}`\n"));
        }
        if let Some(code) = self.exit_status {
            out.push_str(&format!("- Exit status: `{code}`\n"));
        }
        if let Some(ms) = self.duration_ms {
            out.push_str(&format!("- Duration: {ms} ms\n"));
        }

        // Pre-evidence path (awaiting review / missing): keep compact — no raw dump.
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
            "\n### Constraints\n- Planner → Terminal Tool → Terminal Provider only.\n- No automatic code changes from failures.\n- No silent retries.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        let evidence = self
            .evidence
            .as_ref()
            .map(|e| e.to_diagnostics_summary())
            .unwrap_or_else(|| "verification_evidence=—".into());
        format!(
            "test_execution status={} scope={} command={:?} exit={:?} ms={:?} truncated={} dispatched={} mutated={} · {}",
            self.status.map(|s| s.as_str()).unwrap_or("unknown"),
            self.scope.map(|s| s.as_str()).unwrap_or("—"),
            self.command.as_deref().unwrap_or("—"),
            self.exit_status,
            self.duration_ms,
            self.truncated,
            self.dispatched,
            self.mutated,
            evidence,
        )
    }

    /// Compact outcome for RuntimeSnapshot / Context pipeline.
    pub fn to_runtime_outcome(&self) -> Option<RuntimeCommandOutcome> {
        let command = self.command.clone()?;
        let excerpt = self
            .stdout
            .clone()
            .or_else(|| self.stderr.clone())
            .unwrap_or_default();
        let ok = matches!(self.status, Some(TestExecutionStatus::Passed));
        let summary = match self.status {
            Some(TestExecutionStatus::Passed) => "tests passed".into(),
            Some(TestExecutionStatus::Failed) => format!(
                "tests failed (exit {})",
                self.exit_status
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".into())
            ),
            Some(TestExecutionStatus::TimedOut) => "tests timed out".into(),
            Some(other) => other.as_str().into(),
            None => "tests".into(),
        };
        Some(RuntimeCommandOutcome {
            command,
            kind: RuntimeCommandKind::Test,
            exit_code: self.exit_status,
            ok,
            summary,
            output_excerpt: excerpt.chars().take(2_000).collect(),
            session_id: None,
            cwd: self.working_directory.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        })
    }
}

/// Detect a natural-language test execution request.
pub fn detect_test_execution_request(request: &UserRequest) -> Option<TestExecutionAssessment> {
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

    // Yield to C4.1 build awareness (discovery-only) and specialized planners.
    // Do not call coding_edit detect here — it may yield back to test_execution.
    if crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
        || crate::coding_review::detect_review_request(request).is_some()
        || crate::multi_file::detect_multi_file_request(request).is_some()
    {
        return None;
    }
    // Mutation / generation phrasing stays out of test execution.
    if trimmed.starts_with("fix ")
        || trimmed.starts_with("write test")
        || trimmed.starts_with("create test")
        || trimmed.starts_with("add test")
        || trimmed.starts_with("generate test")
    {
        return None;
    }

    let (intent, rules) = classify_test_phrasing(trimmed)?;
    Some(TestExecutionAssessment {
        goal: content.to_string(),
        intent,
        matched_rules: rules,
    })
}

fn classify_test_phrasing(trimmed: &str) -> Option<(TestExecutionIntent, Vec<&'static str>)> {
    if trimmed == "test what we just changed"
        || trimmed.starts_with("test what we just changed")
        || trimmed == "test the changes"
        || trimmed.starts_with("test the change")
        || trimmed.contains("test what changed")
        || trimmed.contains("tests for what we changed")
        || trimmed.contains("tests for the changes")
    {
        return Some((TestExecutionIntent::Targeted, vec!["targeted_changed"]));
    }

    if trimmed == "run the relevant tests"
        || trimmed.starts_with("run the relevant test")
        || trimmed == "run relevant tests"
        || trimmed.contains("relevant tests")
    {
        return Some((TestExecutionIntent::VerifyPass, vec!["relevant_tests"]));
    }

    if trimmed == "verify the tests pass"
        || trimmed.starts_with("verify the tests pass")
        || trimmed == "verify tests pass"
        || trimmed.contains("verify the tests")
        || (trimmed.contains("verify") && trimmed.contains("test") && trimmed.contains("pass"))
    {
        return Some((TestExecutionIntent::VerifyPass, vec!["verify_tests_pass"]));
    }

    if trimmed == "run the tests"
        || trimmed == "run tests"
        || trimmed == "run all tests"
        || trimmed == "run the test suite"
        || trimmed.starts_with("run the tests")
        || trimmed.starts_with("run tests ")
        || trimmed == "execute the tests"
        || trimmed.starts_with("execute the tests")
        || trimmed == "please run the tests"
    {
        return Some((TestExecutionIntent::RunTests, vec!["run_the_tests"]));
    }

    // Broader but still test-execution (not "write tests" generation).
    if (trimmed.starts_with("run ") && trimmed.contains("test"))
        && !trimmed.contains("write")
        && !trimmed.contains("create")
        && !trimmed.contains("generate")
        && !trimmed.contains("add ")
    {
        return Some((TestExecutionIntent::RunTests, vec!["run_tests_broad"]));
    }

    None
}

/// Select a grounded test VerificationTarget (C4.1 reuse + targeting).
pub fn select_test_target(
    assessment: &TestExecutionAssessment,
    bundle: &ContextBundle,
    session: &ContextSessionInputs,
) -> TestExecutionReport {
    let mut report = TestExecutionReport {
        goal: assessment.goal.clone(),
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        mutated: false,
        dispatched: false,
        status: Some(TestExecutionStatus::Pending),
        ..Default::default()
    };
    report
        .matched_rules
        .push(format!("intent:{}", assessment.intent.as_str()));

    let awareness = BuildAwarenessAssessment {
        goal: assessment.goal.clone(),
        matched_rules: vec!["test_execution_reuse_c41"],
        intent: BuildAwarenessIntent::Verify,
    };
    let discovery = discover_verification(&awareness, bundle, &session);

    if discovery.status == Some(BuildAwarenessStatus::NoActiveProject) {
        report.status = Some(TestExecutionStatus::MissingCommand);
        report.gaps.push(
            "No active project — cannot determine a test command.".into(),
        );
        return report;
    }

    let test_candidates: Vec<&VerificationTarget> = discovery
        .targets
        .iter()
        .filter(|t| {
            t.purpose == Some(VerificationPurpose::Test)
                || t.expected_output_type == Some(ExpectedOutputType::TestResults)
                || t.candidate_command
                    .as_deref()
                    .map(|c| {
                        let l = c.to_ascii_lowercase();
                        l.contains("test") || l.contains("pytest")
                    })
                    .unwrap_or(false)
        })
        .collect();

    if test_candidates.is_empty() {
        report.status = Some(TestExecutionStatus::MissingCommand);
        report.gaps.push(
            "Project evidence does not expose a test command (no cargo test / test script / go test)."
                .into(),
        );
        for gap in discovery.gaps {
            if !report.gaps.contains(&gap) {
                report.gaps.push(gap);
            }
        }
        return report;
    }

    let snap = session.project_snapshot.as_ref();
    let git = session.git_snapshot.as_ref();
    let current_file = bundle
        .current_file()
        .path
        .clone()
        .or_else(|| session.current_file.path.clone());

    let (scope, chosen) = pick_scope_and_command(
        assessment.intent,
        &test_candidates,
        snap,
        git,
        current_file.as_deref(),
        &mut report,
    );

    report.scope = Some(scope);
    report.target = Some(chosen.clone());
    report.command = chosen.candidate_command.clone();
    report.working_directory = chosen.working_directory.clone();
    report.affected_project = chosen
        .project
        .clone()
        .or_else(|| bundle.active_project().name.clone());
    report.notes.extend(discovery.notes);
    report
}

fn pick_scope_and_command(
    intent: TestExecutionIntent,
    candidates: &[&VerificationTarget],
    snap: Option<&ProjectSnapshot>,
    git: Option<&GitSnapshot>,
    current_file: Option<&str>,
    report: &mut TestExecutionReport,
) -> (TestScope, VerificationTarget) {
    let base = candidates
        .iter()
        .min_by_key(|t| match t.confidence {
            Some(VerificationConfidence::High) => 0,
            Some(VerificationConfidence::Medium) => 1,
            Some(VerificationConfidence::Low) => 2,
            _ => 3,
        })
        .copied()
        .expect("non-empty")
        .clone();

    if !intent.prefers_targeted() {
        return (TestScope::FullSuite, base);
    }

    // Prefer targeted when evidence supports a filter; else full suite.
    if let Some(targeted) =
        try_targeted_command(&base, snap, git, current_file, &mut report.notes)
    {
        return (TestScope::Targeted, targeted);
    }

    report.notes.push(
        "Insufficient evidence for a safe targeted filter — falling back to the full project test command."
            .into(),
    );
    (TestScope::FullSuite, base)
}

fn try_targeted_command(
    base: &VerificationTarget,
    snap: Option<&ProjectSnapshot>,
    git: Option<&GitSnapshot>,
    current_file: Option<&str>,
    notes: &mut Vec<String>,
) -> Option<VerificationTarget> {
    let cmd = base.candidate_command.as_deref()?;
    let lower = cmd.to_ascii_lowercase();

    let changed: Vec<String> = git
        .map(|g| {
            g.dirty
                .iter()
                .chain(g.staged.iter())
                .map(|e| e.path.clone())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let focus = current_file
        .map(str::to_string)
        .or_else(|| changed.first().cloned())?;

    if lower.starts_with("cargo test") {
        let filter = cargo_test_filter(&focus, snap)?;
        let mut targeted = base.clone();
        targeted.candidate_command = Some(format!("cargo test {filter}"));
        targeted.confidence = Some(VerificationConfidence::Medium);
        targeted.evidence.push(format!("targeted_from={focus}"));
        notes.push(format!(
            "Targeted Cargo filter `{filter}` from changed/current file evidence."
        ));
        return Some(targeted);
    }

    if lower.contains("go test") {
        let pkg = go_package_filter(&focus)?;
        let mut targeted = base.clone();
        targeted.candidate_command = Some(format!("go test {pkg}"));
        targeted.confidence = Some(VerificationConfidence::Medium);
        targeted.evidence.push(format!("targeted_from={focus}"));
        notes.push(format!("Targeted Go package `{pkg}` from file evidence."));
        return Some(targeted);
    }

    // npm/pnpm/yarn — only pass a path arg when the script is plainly `test`
    // and the focus looks like a JS/TS test path (do not invent custom runners).
    if (lower.starts_with("npm run test")
        || lower.starts_with("pnpm run test")
        || lower.starts_with("yarn run test")
        || lower == "npm test"
        || lower == "pnpm test"
        || lower == "yarn test")
        && looks_like_js_test_path(&focus)
    {
        let mut targeted = base.clone();
        let runner = if lower.starts_with("pnpm") {
            "pnpm test"
        } else if lower.starts_with("yarn") {
            "yarn test"
        } else {
            "npm test"
        };
        targeted.candidate_command = Some(format!("{runner} -- {focus}"));
        targeted.confidence = Some(VerificationConfidence::Low);
        targeted.evidence.push(format!("targeted_path={focus}"));
        notes.push(
            "Passing a path to the project test script (low confidence — runner-specific)."
                .into(),
        );
        return Some(targeted);
    }

    None
}

fn cargo_test_filter(path: &str, snap: Option<&ProjectSnapshot>) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let file = normalized.rsplit('/').next().unwrap_or(path);
    if !file.ends_with(".rs") {
        return None;
    }
    let stem = file.trim_end_matches(".rs");
    // integration tests / unit file stem as cargo filter (name substring match).
    let mut filter = stem.trim_end_matches("_test").to_string();
    if filter.is_empty() {
        return None;
    }
    // Prefer package when workspace member is clear from path.
    if let Some(snap) = snap {
        if let Some(pkg) = snap.cargo.as_ref().and_then(|c| c.package_name.as_deref()) {
            if normalized.contains(&format!("/{pkg}/")) || normalized.contains("crates/") {
                // Keep filter as module/file stem; cargo test still searches by name.
                let _ = pkg;
            }
        }
        if snap.cargo.as_ref().map(|c| c.is_workspace).unwrap_or(false) {
            if let Some(member) = snap
                .cargo
                .as_ref()
                .map(|c| &c.members)
                .into_iter()
                .flatten()
                .find(|m| normalized.contains(m.as_str()))
            {
                filter = format!("-p {member} {filter}");
            }
        }
    }
    Some(filter)
}

fn go_package_filter(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    if !normalized.ends_with(".go") {
        return None;
    }
    let dir = normalized.rsplit_once('/')?.0;
    if dir.is_empty() {
        Some("./...".into())
    } else {
        Some(format!("./{dir}"))
    }
}

fn looks_like_js_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    (lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx"))
        && (lower.contains(".test.")
            || lower.contains(".spec.")
            || lower.contains("__tests__")
            || lower.contains("/test/")
            || lower.contains("/tests/"))
}

/// Bound large combined PTY output; prefer head + error-ish tail.
pub fn truncate_output(raw: &str) -> (String, bool) {
    if raw.chars().count() <= OUTPUT_CHAR_CAP {
        return (raw.to_string(), false);
    }
    let chars: Vec<char> = raw.chars().collect();
    let head: String = chars.iter().take(OUTPUT_HEAD_KEEP).collect();
    let tail_start = chars.len().saturating_sub(OUTPUT_TAIL_KEEP);
    let tail: String = chars[tail_start..].iter().collect();
    let mut out = String::new();
    out.push_str(&head);
    if !head.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n…[truncated]…\n\n");
    out.push_str(&tail);
    (out, true)
}

/// Finalize a report from a terminal ToolOutput (after Approve / Allowed).
pub fn finalize_from_tool_output(
    mut report: TestExecutionReport,
    output: &ToolOutput,
    provider_error: Option<&str>,
) -> TestExecutionReport {
    report.dispatched = true;
    report.mutated = false;
    report.exit_status = output.terminal_exit_code;
    report.duration_ms = output.terminal_duration_ms.or(report.duration_ms);

    let combined = output
        .terminal_output
        .clone()
        .or_else(|| output.message.clone())
        .unwrap_or_default();
    let (bounded, truncated) = truncate_output(&combined);
    report.truncated = truncated;
    // PTY merges stdout/stderr; keep stderr empty unless we later split streams.
    report.stdout = Some(bounded);
    report.stderr = None;

    if let Some(err) = provider_error {
        if err.to_ascii_lowercase().contains("timed out") {
            report.status = Some(TestExecutionStatus::TimedOut);
            report.gaps.push(err.to_string());
        } else {
            report.status = Some(TestExecutionStatus::CommandError);
            report.gaps.push(err.to_string());
        }
        crate::verification_diagnostics::enrich_test_report(&mut report);
        return report;
    }

    if !output.success {
        let msg = output.message.as_deref().unwrap_or("");
        if msg.to_ascii_lowercase().contains("timed out") {
            report.status = Some(TestExecutionStatus::TimedOut);
        } else if report.exit_status.map(|c| c != 0).unwrap_or(true) {
            report.status = Some(TestExecutionStatus::Failed);
            report.notes.push(
                "Non-zero exit — failures are reported only; no automatic code changes.".into(),
            );
        } else {
            report.status = Some(TestExecutionStatus::CommandError);
        }
        crate::verification_diagnostics::enrich_test_report(&mut report);
        return report;
    }

    match report.exit_status {
        Some(0) | None => report.status = Some(TestExecutionStatus::Passed),
        Some(_) => {
            report.status = Some(TestExecutionStatus::Failed);
            report.notes.push(
                "Non-zero exit — failures are reported only; no automatic code changes.".into(),
            );
        }
    }
    crate::verification_diagnostics::enrich_test_report(&mut report);
    report
}

/// Mark report as awaiting review (permission / ToolRisk gate).
pub fn mark_awaiting_review(mut report: TestExecutionReport) -> TestExecutionReport {
    report.status = Some(TestExecutionStatus::AwaitingReview);
    report.dispatched = true;
    report.notes.push(
        "Terminal execution requires Review Before Action (permission + ToolRisk).".into(),
    );
    report
}

/// Mark report as denied before any run.
pub fn mark_denied(mut report: TestExecutionReport, explanation: impl Into<String>) -> TestExecutionReport {
    report.status = Some(TestExecutionStatus::Denied);
    report.dispatched = false;
    report.gaps.push(explanation.into());
    report
}

/// Publish a completed test outcome into session Context inputs.
///
/// Updates RuntimeSnapshot (C4.2) and merges structured verification
/// diagnostics into the existing diagnostics pipeline (C4.3).
pub fn publish_to_context(
    session: &mut ContextSessionInputs,
    report: &TestExecutionReport,
) {
    merge_into_runtime_snapshot(session, report);
    if let Some(evidence) = &report.evidence {
        crate::verification_diagnostics::merge_evidence_into_session(session, evidence);
    }
}

/// Merge a completed test outcome into session RuntimeSnapshot for Context/Reasoning.
pub fn merge_into_runtime_snapshot(
    session: &mut ContextSessionInputs,
    report: &TestExecutionReport,
) {
    let Some(outcome) = report.to_runtime_outcome() else {
        return;
    };
    let mut snap = session
        .runtime_snapshot
        .take()
        .unwrap_or_else(RuntimeSnapshot::empty);
    if !outcome.ok {
        snap.recent_failures.insert(0, outcome.clone());
        if snap.recent_failures.len() > 8 {
            snap.recent_failures.truncate(8);
        }
    }
    snap.latest_tests = Some(outcome);
    session.runtime_snapshot = Some(snap);
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ActiveProjectSection, BuildSystemKind, CargoProjectMeta, ContextBundle, GitPathEntry,
        NpmProjectMeta, PackageManagerKind, ProjectMetadata, ProjectSnapshot,
    };

    fn session_cargo() -> ContextSessionInputs {
        ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(ProjectSnapshot {
                metadata: ProjectMetadata {
                    project_id: Some("p".into()),
                    name: Some("jaymi".into()),
                    root_directory: Some("/proj".into()),
                    ..Default::default()
                },
                package_manager: Some(PackageManagerKind::Cargo),
                build_system: Some(BuildSystemKind::Cargo),
                cargo: Some(CargoProjectMeta {
                    package_name: Some("jaymi".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn bundle() -> ContextBundle {
        ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("p".into()),
                name: Some("jaymi".into()),
                root_directory: Some("/proj".into()),
                ..Default::default()
            })
            .build()
    }

    #[test]
    fn detect_example_phrases() {
        for phrase in [
            "Run the tests.",
            "Test what we just changed.",
            "Verify the tests pass.",
            "Run the relevant tests.",
        ] {
            assert!(
                detect_test_execution_request(&UserRequest::new(phrase)).is_some(),
                "{phrase}"
            );
        }
    }

    #[test]
    fn detect_yields_to_build_awareness() {
        assert!(detect_test_execution_request(&UserRequest::new("Does this build?")).is_none());
    }

    #[test]
    fn select_full_suite_cargo_test() {
        let assessment =
            detect_test_execution_request(&UserRequest::new("Run the tests.")).unwrap();
        let report = select_test_target(&assessment, &bundle(), &session_cargo());
        assert_eq!(report.command.as_deref(), Some("cargo test"));
        assert_eq!(report.scope, Some(TestScope::FullSuite));
        assert!(!report.mutated);
    }

    #[test]
    fn select_targeted_from_changed_file() {
        let mut session = session_cargo();
        session.git_snapshot = Some(GitSnapshot {
            dirty: vec![GitPathEntry {
                path: "/proj/crates/jaymi-planner/src/lib.rs".into(),
                status: "M".into(),
            }],
            ..Default::default()
        });
        let assessment =
            detect_test_execution_request(&UserRequest::new("Test what we just changed."))
                .unwrap();
        let report = select_test_target(&assessment, &bundle(), &session);
        assert_eq!(report.scope, Some(TestScope::Targeted));
        assert!(
            report
                .command
                .as_deref()
                .unwrap_or("")
                .starts_with("cargo test"),
            "{:?}",
            report.command
        );
        assert_ne!(report.command.as_deref(), Some("cargo test"));
    }

    #[test]
    fn missing_test_command() {
        let session = ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(ProjectSnapshot {
                metadata: ProjectMetadata {
                    project_id: Some("p".into()),
                    name: Some("notes".into()),
                    root_directory: Some("/notes".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let assessment =
            detect_test_execution_request(&UserRequest::new("Run the tests.")).unwrap();
        let report = select_test_target(&assessment, &bundle(), &session);
        assert_eq!(report.status, Some(TestExecutionStatus::MissingCommand));
        assert!(report.command.is_none());
    }

    #[test]
    fn truncate_large_output_keeps_tail() {
        let raw = format!("{}FAILURE at end", "x".repeat(20_000));
        let (out, truncated) = truncate_output(&raw);
        assert!(truncated);
        assert!(out.contains("FAILURE at end"));
        assert!(out.contains("[truncated]"));
        assert!(out.chars().count() < raw.chars().count());
    }

    #[test]
    fn finalize_passing_and_failing() {
        let base = TestExecutionReport {
            command: Some("cargo test".into()),
            working_directory: Some("/proj".into()),
            affected_project: Some("jaymi".into()),
            ..Default::default()
        };
        let pass = finalize_from_tool_output(
            base.clone(),
            &ToolOutput {
                success: true,
                terminal_output: Some("ok".into()),
                terminal_exit_code: Some(0),
                terminal_duration_ms: Some(12),
                ..Default::default()
            },
            None,
        );
        assert_eq!(pass.status, Some(TestExecutionStatus::Passed));
        assert!(!pass.mutated);
        assert!(pass.evidence.is_some());
        assert_eq!(
            pass.evidence.as_ref().and_then(|e| e.kind),
            Some(crate::verification_diagnostics::VerificationResultKind::Success)
        );

        let fail = finalize_from_tool_output(
            base,
            &ToolOutput {
                success: false,
                terminal_output: Some("FAILED".into()),
                terminal_exit_code: Some(101),
                terminal_duration_ms: Some(40),
                ..Default::default()
            },
            None,
        );
        assert_eq!(fail.status, Some(TestExecutionStatus::Failed));
        assert!(!fail.mutated);
        assert!(fail.evidence.is_some());
    }

    #[test]
    fn finalize_timeout() {
        let report = finalize_from_tool_output(
            TestExecutionReport {
                command: Some("cargo test".into()),
                ..Default::default()
            },
            &ToolOutput {
                success: false,
                message: Some("timed out waiting for terminal command: cargo test".into()),
                ..Default::default()
            },
            Some("timed out waiting for terminal command: cargo test"),
        );
        assert_eq!(report.status, Some(TestExecutionStatus::TimedOut));
        assert_eq!(
            report.evidence.as_ref().and_then(|e| e.kind),
            Some(crate::verification_diagnostics::VerificationResultKind::Timeout)
        );
    }

    #[test]
    fn no_npm_invention_without_test_script() {
        let session = ContextSessionInputs {
            project_open: true,
            project_snapshot: Some(ProjectSnapshot {
                metadata: ProjectMetadata {
                    root_directory: Some("/web".into()),
                    name: Some("web".into()),
                    project_id: Some("w".into()),
                    ..Default::default()
                },
                package_manager: Some(PackageManagerKind::Npm),
                build_system: Some(BuildSystemKind::NpmScripts),
                npm: Some(NpmProjectMeta {
                    scripts: vec!["dev".into(), "build".into()],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let assessment =
            detect_test_execution_request(&UserRequest::new("Run the tests.")).unwrap();
        let report = select_test_target(&assessment, &bundle(), &session);
        assert_eq!(report.status, Some(TestExecutionStatus::MissingCommand));
    }

    #[test]
    fn merge_runtime_snapshot_for_context() {
        let mut session = session_cargo();
        let report = TestExecutionReport {
            status: Some(TestExecutionStatus::Passed),
            command: Some("cargo test".into()),
            working_directory: Some("/proj".into()),
            exit_status: Some(0),
            stdout: Some("ok".into()),
            ..Default::default()
        };
        merge_into_runtime_snapshot(&mut session, &report);
        assert!(session
            .runtime_snapshot
            .as_ref()
            .and_then(|s| s.latest_tests.as_ref())
            .is_some());
    }
}
