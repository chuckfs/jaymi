//! Verification Summary (Sprint C4.5).
//!
//! Concise conversational summary after coding verification, composed from
//! C3 [`ExecutionSummary`] + C4 [`VerificationEvidence`] / test reports.
//!
//! Answers: what changed, what was verified, what passed/failed, what remains,
//! what should happen next — without dumping diagnostics or inventing runs.
//!
//! Not a soft path: post-process formatter attached when verification completes.
//! Recommended fixes are conversational next actions into C4.4 — never auto-run.

use crate::execution_plan::{ExecutionStatus, ExecutionSummary};
use crate::test_execution::{TestExecutionReport, TestExecutionStatus};
use crate::verification_diagnostics::{
    VerificationEvidence, VerificationFinding, VerificationResultKind,
};

/// High-level verification outcome for conversation (C4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    NotRun,
    Passed,
    Failed,
    Blocked,
    Cancelled,
    Unknown,
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotRun => "Not run",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Blocked => "Blocked",
            Self::Cancelled => "Cancelled",
            Self::Unknown => "Unknown",
        }
    }

    pub fn mark(self) -> &'static str {
        match self {
            Self::Passed => "✓",
            Self::Failed | Self::Blocked => "✗",
            Self::Cancelled => "–",
            Self::NotRun | Self::Unknown => "·",
        }
    }
}

/// One verification command line in the summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VerifiedCommandLine {
    pub command: String,
    pub status: VerificationStatus,
    /// Short detail (`43 tests passed`, `build failed`, …) — never invent counts.
    pub detail: Option<String>,
}

/// Concise verification summary for Conversation First UX.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct VerificationSummary {
    pub status: Option<VerificationStatus>,
    pub goal: String,
    /// From C3 ExecutionSummary when available — never invented.
    pub changed_files: Vec<String>,
    pub verified_commands: Vec<VerifiedCommandLine>,
    pub passed_notes: Vec<String>,
    pub failure_highlights: Vec<String>,
    pub remains: Vec<String>,
    pub next_actions: Vec<String>,
    /// Chip-friendly corrective action label when grounded (e.g. `Review Fix`).
    pub recommended_fix_label: Option<String>,
    /// Conversation text that enters C4.4 when chosen — never auto-executed.
    pub recommended_fix_prompt: Option<String>,
    pub evidence_kind: Option<VerificationResultKind>,
    pub from_execution_summary: bool,
    pub from_verification_evidence: bool,
    /// Always false — summaries never invent verification.
    pub invented: bool,
}

impl VerificationSummary {
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        if !self.changed_files.is_empty() {
            out.push_str("## Implemented\n\n");
            out.push_str(&format!(
                "{} file{} changed.\n",
                self.changed_files.len(),
                if self.changed_files.len() == 1 { "" } else { "s" }
            ));
            for path in self.changed_files.iter().take(12) {
                out.push_str(&format!("- `{path}`\n"));
            }
            if self.changed_files.len() > 12 {
                out.push_str(&format!(
                    "- …and {} more\n",
                    self.changed_files.len() - 12
                ));
            }
            out.push('\n');
        }

        out.push_str("## Verification\n\n");
        match self.status {
            Some(VerificationStatus::NotRun) => {
                out.push_str("Verification was not run.\n");
            }
            Some(VerificationStatus::Cancelled) => {
                out.push_str("Verification was cancelled — nothing ran.\n");
            }
            Some(VerificationStatus::Blocked) => {
                out.push_str("Verification was blocked before it could run.\n");
            }
            Some(VerificationStatus::Unknown) => {
                out.push_str("Verification finished with an unclear result.\n");
            }
            Some(VerificationStatus::Passed) | Some(VerificationStatus::Failed) | None => {}
        }

        if self.verified_commands.is_empty()
            && !matches!(
                self.status,
                Some(
                    VerificationStatus::Passed
                        | VerificationStatus::Failed
                        | VerificationStatus::Unknown
                )
            )
        {
            // Already stated not run / blocked / cancelled.
        } else if self.verified_commands.is_empty() {
            out.push_str("· _(no command recorded)_\n");
        } else {
            for cmd in &self.verified_commands {
                let mark = cmd.status.mark();
                if let Some(detail) = &cmd.detail {
                    out.push_str(&format!("{mark} `{cmd}` — {detail}\n", cmd = cmd.command));
                } else {
                    out.push_str(&format!(
                        "{mark} `{cmd}` — {status}\n",
                        cmd = cmd.command,
                        status = cmd.status.label().to_ascii_lowercase()
                    ));
                }
            }
        }

        if !self.passed_notes.is_empty()
            && matches!(self.status, Some(VerificationStatus::Passed))
            && self.verified_commands.is_empty()
        {
            out.push('\n');
            for note in &self.passed_notes {
                out.push_str(&format!("✓ {note}\n"));
            }
        }

        if !self.failure_highlights.is_empty() {
            out.push_str("\n## Failure\n\n");
            for row in self.failure_highlights.iter().take(8) {
                out.push_str(row);
                if !row.ends_with('\n') {
                    out.push('\n');
                }
            }
        }

        if !self.remains.is_empty() {
            out.push_str("\n## Remains\n\n");
            for row in &self.remains {
                out.push_str(&format!("- {row}\n"));
            }
        }

        if let Some(prompt) = &self.recommended_fix_prompt {
            out.push_str("\n## Recommended next step\n\n");
            if let Some(label) = &self.recommended_fix_label {
                out.push_str(&format!("{prompt}\n\n[{label}]\n"));
            } else {
                out.push_str(prompt);
                out.push('\n');
            }
        } else if !self.next_actions.is_empty() {
            out.push_str("\n## Recommended next step\n\n");
            out.push_str(&self.next_actions[0]);
            out.push('\n');
            for extra in self.next_actions.iter().skip(1).take(3) {
                out.push_str(&format!("- {extra}\n"));
            }
        }

        out.push_str(
            "\n_Detailed diagnostics stay in the Diagnostics panel unless you ask for them._\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "verification_summary status={} changed={} commands={} failures={} next={} invented={}",
            self.status.map(|s| s.as_str()).unwrap_or("unknown"),
            self.changed_files.len(),
            self.verified_commands.len(),
            self.failure_highlights.len(),
            self.next_actions.len(),
            self.invented,
        )
    }
}

/// Inputs for composing a verification summary (no invention beyond these).
#[derive(Debug, Clone, Default)]
pub struct VerificationSummaryInput<'a> {
    pub execution: Option<&'a ExecutionSummary>,
    /// Prior mutation ExecutionSummary when the verification turn has none.
    pub prior_execution: Option<&'a ExecutionSummary>,
    pub evidence: Option<&'a VerificationEvidence>,
    pub test_report: Option<&'a TestExecutionReport>,
    /// Extra verification command lines (e.g. prior check + tests) — caller-supplied only.
    pub additional_commands: Vec<VerifiedCommandLine>,
    /// Explicit cancel (review cancelled before run).
    pub cancelled: bool,
}

/// Compose a concise summary from C3 + C4 data. Never invents verification.
pub fn compose(input: VerificationSummaryInput<'_>) -> VerificationSummary {
    let mut summary = VerificationSummary {
        invented: false,
        ..Default::default()
    };

    let exec = input.execution.or(input.prior_execution);
    if let Some(execution) = exec {
        summary.from_execution_summary = true;
        summary.goal = execution.goal.clone();
        summary.changed_files = execution.files_edited.clone();
        if summary.changed_files.is_empty() {
            // Do not treat generic resources as “files changed” unless they look like paths.
            for r in &execution.resources_changed {
                if r.contains('/') || r.contains('.') {
                    summary.changed_files.push(r.clone());
                }
            }
        }
    }

    if let Some(report) = input.test_report {
        if summary.goal.is_empty() {
            summary.goal = report.goal.clone();
        }
    }

    if input.cancelled {
        summary.status = Some(VerificationStatus::Cancelled);
        summary.remains.push(
            "Verification did not run because the plan was cancelled.".into(),
        );
        summary.next_actions.push(
            "Ask Jaymi to run the tests again when you are ready.".into(),
        );
        return summary;
    }

    if matches!(
        input.execution.map(|e| e.status),
        Some(ExecutionStatus::Cancelled)
    ) && input.test_report.is_some()
        && !matches!(
            input.test_report.and_then(|r| r.status),
            Some(
                TestExecutionStatus::Passed
                    | TestExecutionStatus::Failed
                    | TestExecutionStatus::TimedOut
                    | TestExecutionStatus::CommandError
            )
        )
    {
        summary.status = Some(VerificationStatus::Cancelled);
        summary.remains.push(
            "Verification did not run because the plan was cancelled.".into(),
        );
        summary.next_actions.push(
            "Ask Jaymi to run the tests again when you are ready.".into(),
        );
        return summary;
    }

    if let Some(report) = input.test_report {
        match report.status {
            Some(TestExecutionStatus::AwaitingReview) | Some(TestExecutionStatus::Pending) => {
                summary.status = Some(VerificationStatus::NotRun);
                if let Some(cmd) = &report.command {
                    summary.verified_commands.push(VerifiedCommandLine {
                        command: cmd.clone(),
                        status: VerificationStatus::NotRun,
                        detail: Some("awaiting approval".into()),
                    });
                }
                summary.remains.push(
                    "Verification is ready but has not run yet (Review Before Action)."
                        .into(),
                );
                summary
                    .next_actions
                    .push("Approve the Review Card to run verification.".into());
                return summary;
            }
            Some(TestExecutionStatus::Denied) => {
                summary.status = Some(VerificationStatus::Blocked);
                if let Some(cmd) = &report.command {
                    summary.verified_commands.push(VerifiedCommandLine {
                        command: cmd.clone(),
                        status: VerificationStatus::Blocked,
                        detail: Some("permission/policy blocked".into()),
                    });
                }
                summary.remains.extend(report.gaps.iter().cloned());
                summary
                    .next_actions
                    .push("Adjust permissions or rephrase the request, then try again.".into());
                return summary;
            }
            Some(TestExecutionStatus::MissingCommand) => {
                summary.status = Some(VerificationStatus::NotRun);
                summary.remains.extend(report.gaps.iter().cloned());
                if summary.remains.is_empty() {
                    summary.remains.push(
                        "No grounded project test command is available.".into(),
                    );
                }
                summary.next_actions.push(
                    "Open a project with observed test scripts, or tell Jaymi which command to use."
                        .into(),
                );
                return summary;
            }
            _ => {}
        }
    }

    // Extra commands supplied by caller (never invented here).
    summary
        .verified_commands
        .extend(input.additional_commands);

    if let Some(evidence) = input.evidence {
        summary.from_verification_evidence = true;
        summary.evidence_kind = evidence.kind;
        let status = status_from_evidence(evidence, input.test_report);
        summary.status = Some(status);

        if let Some(cmd) = evidence.command.clone().or_else(|| {
            input
                .test_report
                .and_then(|r| r.command.clone())
        }) {
            let detail = command_detail(evidence, status);
            // Avoid duplicating if additional_commands already listed it.
            if !summary
                .verified_commands
                .iter()
                .any(|c| c.command == cmd)
            {
                summary.verified_commands.push(VerifiedCommandLine {
                    command: cmd,
                    status,
                    detail,
                });
            }
        }

        match status {
            VerificationStatus::Passed => {
                if let Some(note) = pass_note_from_evidence(evidence) {
                    summary.passed_notes.push(note);
                }
                if !evidence.warnings.is_empty() {
                    summary.remains.push(format!(
                        "{} warning{} observed — not treated as failure.",
                        evidence.warnings.len(),
                        if evidence.warnings.len() == 1 { "" } else { "s" }
                    ));
                }
                summary
                    .next_actions
                    .push("Review the changes, or continue with the next task.".into());
            }
            VerificationStatus::Failed => {
                summary.failure_highlights = failure_highlights(evidence);
                if summary.failure_highlights.is_empty() {
                    summary.remains.push(
                        "Failure recorded without parseable file/line findings.".into(),
                    );
                }
                attach_recommended_fix(&mut summary, evidence);
                if summary.next_actions.is_empty() {
                    summary.next_actions.push(
                        "Inspect the failure highlights, then ask Jaymi to fix the failure."
                            .into(),
                    );
                }
            }
            VerificationStatus::Unknown => {
                summary.remains.push(
                    "Exit was non-success, but findings were not structured enough to summarize."
                        .into(),
                );
                if !evidence.output_excerpt.is_empty() {
                    summary.remains.push(
                        "Raw excerpt is available in diagnostics if you need more detail."
                            .into(),
                    );
                }
                summary.next_actions.push(
                    "Re-run verification or paste clearer errors, then ask to fix."
                        .into(),
                );
            }
            VerificationStatus::Blocked
            | VerificationStatus::Cancelled
            | VerificationStatus::NotRun => {}
        }

        // Environment / tool / timeout — never claim a code fix is proven.
        match evidence.kind {
            Some(
                VerificationResultKind::EnvironmentFailure
                    | VerificationResultKind::ToolFailure
                    | VerificationResultKind::Timeout,
            ) => {
                summary.remains.push(format!(
                    "{} — not treated as a project code defect.",
                    evidence.kind.unwrap().label()
                ));
                summary.recommended_fix_label = None;
                summary.recommended_fix_prompt = None;
                summary.next_actions = vec![
                    "Resolve the environment/tool issue, then re-run verification.".into(),
                ];
            }
            _ => {}
        }
    } else if let Some(report) = input.test_report {
        // Report without evidence (should be rare after C4.3).
        summary.status = Some(match report.status {
            Some(TestExecutionStatus::Passed) => VerificationStatus::Passed,
            Some(TestExecutionStatus::Failed)
            | Some(TestExecutionStatus::TimedOut)
            | Some(TestExecutionStatus::CommandError) => VerificationStatus::Failed,
            Some(TestExecutionStatus::Denied) => VerificationStatus::Blocked,
            _ => VerificationStatus::Unknown,
        });
        if let Some(cmd) = &report.command {
            summary.verified_commands.push(VerifiedCommandLine {
                command: cmd.clone(),
                status: summary.status.unwrap_or(VerificationStatus::Unknown),
                detail: None,
            });
        }
        summary.remains.push(
            "Structured verification evidence was unavailable for this turn.".into(),
        );
    } else {
        summary.status = Some(VerificationStatus::NotRun);
        summary.remains.push("No verification ran on this turn.".into());
        if !summary.changed_files.is_empty() {
            summary
                .next_actions
                .push("Run the tests.".into());
            summary
                .next_actions
                .push("Verify the changes.".into());
        }
    }

    summary
}

fn status_from_evidence(
    evidence: &VerificationEvidence,
    report: Option<&TestExecutionReport>,
) -> VerificationStatus {
    if let Some(status) = report.and_then(|r| r.status) {
        match status {
            TestExecutionStatus::Denied => return VerificationStatus::Blocked,
            TestExecutionStatus::AwaitingReview | TestExecutionStatus::Pending => {
                return VerificationStatus::NotRun
            }
            TestExecutionStatus::MissingCommand => return VerificationStatus::NotRun,
            _ => {}
        }
    }
    match evidence.kind {
        Some(VerificationResultKind::Success) | Some(VerificationResultKind::Warning) => {
            VerificationStatus::Passed
        }
        Some(
            VerificationResultKind::BuildFailure
                | VerificationResultKind::TestFailure
                | VerificationResultKind::Timeout
                | VerificationResultKind::ToolFailure
                | VerificationResultKind::EnvironmentFailure,
        ) => VerificationStatus::Failed,
        Some(VerificationResultKind::Unknown) | None => {
            if evidence.exit_code == Some(0) {
                VerificationStatus::Passed
            } else if evidence.exit_code.is_some() {
                VerificationStatus::Unknown
            } else {
                VerificationStatus::Unknown
            }
        }
    }
}

fn command_detail(
    evidence: &VerificationEvidence,
    status: VerificationStatus,
) -> Option<String> {
    match status {
        VerificationStatus::Passed => pass_note_from_evidence(evidence),
        VerificationStatus::Failed => match evidence.kind {
            Some(VerificationResultKind::BuildFailure) => {
                let n = evidence.errors.len();
                Some(if n == 0 {
                    "build failed".into()
                } else {
                    format!(
                        "build failed ({} error{})",
                        n,
                        if n == 1 { "" } else { "s" }
                    )
                })
            }
            Some(VerificationResultKind::TestFailure) => {
                let n = evidence.test_names.len().max(evidence.errors.len());
                Some(if n == 0 {
                    "tests failed".into()
                } else {
                    format!(
                        "{} test{} failed",
                        n,
                        if n == 1 { "" } else { "s" }
                    )
                })
            }
            Some(other) => Some(other.label().to_ascii_lowercase()),
            None => Some("failed".into()),
        },
        other => Some(other.label().to_ascii_lowercase()),
    }
}

fn pass_note_from_evidence(evidence: &VerificationEvidence) -> Option<String> {
    let hay = format!(
        "{} {}",
        evidence.output_excerpt,
        evidence.status_label
    );
    if let Some(n) = parse_passed_count(&hay) {
        return Some(format!(
            "{n} test{} passed",
            if n == 1 { "" } else { "s" }
        ));
    }
    if evidence
        .command
        .as_deref()
        .map(|c| c.contains("check") || c.contains("build"))
        .unwrap_or(false)
    {
        return Some("passed".into());
    }
    Some("passed".into())
}

fn parse_passed_count(text: &str) -> Option<u32> {
    // cargo: `test result: ok. 43 passed; 0 failed`
    let lower = text.to_ascii_lowercase();
    if let Some(idx) = lower.find(" passed") {
        let before = &lower[..idx];
        let digits: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

fn failure_highlights(evidence: &VerificationEvidence) -> Vec<String> {
    let mut out = Vec::new();
    for finding in evidence.errors.iter().take(8) {
        out.push(format_failure_block(finding));
    }
    if out.is_empty() {
        for name in evidence.test_names.iter().take(4) {
            out.push(format!("`{name}`\n"));
        }
        for msg in evidence.failure_messages.iter().take(4) {
            out.push(format!("{msg}\n"));
        }
    }
    out
}

fn format_failure_block(finding: &VerificationFinding) -> String {
    let loc = match (&finding.path, finding.line) {
        (Some(path), Some(line)) => format!("`{path}:{line}`"),
        (Some(path), None) => format!("`{path}`"),
        (None, _) => finding
            .test_name
            .as_ref()
            .map(|t| format!("`{t}`"))
            .unwrap_or_else(|| "_(location unknown)_".into()),
    };
    let mut block = format!("{loc}\n\n");
    if let Some(test) = &finding.test_name {
        if finding.path.is_some() {
            block.push_str(&format!("Test `{test}`\n\n"));
        }
    }
    block.push_str(&finding.message);
    if !finding.message.ends_with('.') {
        block.push('.');
    }
    block.push('\n');
    block
}

fn attach_recommended_fix(summary: &mut VerificationSummary, evidence: &VerificationEvidence) {
    let (label, prompt) = match evidence.kind {
        Some(VerificationResultKind::BuildFailure) => (
            "Review Fix",
            "Fix the build failure.",
        ),
        Some(VerificationResultKind::TestFailure) => (
            "Review Fix",
            "Fix the failing tests.",
        ),
        Some(VerificationResultKind::Warning) => (
            "Review Fix",
            "Fix the verification failure.",
        ),
        _ => (
            "Review Fix",
            "Fix the verification failure.",
        ),
    };
    // Only recommend code fix when findings or files support it.
    let actionable = !evidence.errors.is_empty()
        || !evidence.test_names.is_empty()
        || !evidence.affected_files.is_empty()
        || !evidence.failure_messages.is_empty();
    if !actionable {
        summary.next_actions.push(
            "Gather clearer failure output before asking for a corrective plan.".into(),
        );
        return;
    }
    if matches!(
        evidence.kind,
        Some(
            VerificationResultKind::EnvironmentFailure
                | VerificationResultKind::ToolFailure
                | VerificationResultKind::Timeout
        )
    ) {
        return;
    }
    summary.recommended_fix_label = Some(label.into());
    summary.recommended_fix_prompt = Some(prompt.into());
    summary.next_actions.insert(0, prompt.into());
}

/// Apply summary as the primary conversation body (details stay elsewhere).
pub fn prefer_summary_content(
    summary: &VerificationSummary,
    fallback: impl AsRef<str>,
) -> String {
    let md = summary.to_markdown();
    if md.trim().is_empty() {
        fallback.as_ref().to_string()
    } else {
        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{ExecutionPlanId, ExecutionStatus};
    use crate::verification_diagnostics::VerificationFinding;

    fn exec_with_files(files: &[&str]) -> ExecutionSummary {
        ExecutionSummary {
            plan_id: ExecutionPlanId::from_existing("plan-1"),
            status: ExecutionStatus::Completed,
            goal: "Apply patches".into(),
            actions_performed: vec!["write_file".into()],
            resources_changed: Vec::new(),
            files_edited: files.iter().map(|s| (*s).to_string()).collect(),
            duration_ms: 10,
            warnings: Vec::new(),
            errors: Vec::new(),
            next_suggested_actions: Vec::new(),
            tools_executed: vec!["write_file".into()],
            outputs: Vec::new(),
            partial: false,
            files_moved_to_trash: Vec::new(),
            files_permanently_deleted: Vec::new(),
            recovery_available: None,
            deletion_method: None,
            error: None,
        }
    }

    #[test]
    fn all_tests_pass_summary() {
        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::Success),
            command: Some("cargo test".into()),
            exit_code: Some(0),
            output_excerpt: "test result: ok. 43 passed; 0 failed".into(),
            ..Default::default()
        };
        let report = TestExecutionReport {
            status: Some(TestExecutionStatus::Passed),
            command: Some("cargo test".into()),
            evidence: Some(evidence.clone()),
            ..Default::default()
        };
        let summary = compose(VerificationSummaryInput {
            execution: Some(&exec_with_files(&[
                "src/a.rs",
                "src/b.rs",
                "src/c.rs",
                "src/d.rs",
                "src/e.rs",
                "src/f.rs",
            ])),
            evidence: Some(&evidence),
            test_report: Some(&report),
            ..Default::default()
        });
        assert_eq!(summary.status, Some(VerificationStatus::Passed));
        assert_eq!(summary.changed_files.len(), 6);
        let md = summary.to_markdown();
        assert!(md.contains("## Implemented"));
        assert!(md.contains("6 files changed"));
        assert!(md.contains("43 tests passed"));
        assert!(!md.contains("tests passed") || md.contains("43"));
        assert!(!summary.invented);
    }

    #[test]
    fn never_claims_pass_when_not_run() {
        let report = TestExecutionReport {
            status: Some(TestExecutionStatus::AwaitingReview),
            command: Some("cargo test".into()),
            ..Default::default()
        };
        let summary = compose(VerificationSummaryInput {
            test_report: Some(&report),
            ..Default::default()
        });
        assert_eq!(summary.status, Some(VerificationStatus::NotRun));
        let md = summary.to_markdown();
        assert!(md.contains("not run") || md.contains("Not run") || md.contains("awaiting"));
        assert!(!md.contains("tests passed"));
        assert!(!md.contains("✓ `cargo test` — passed"));
    }

    #[test]
    fn build_failure_links_files_and_recommends_fix() {
        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::BuildFailure),
            command: Some("cargo check".into()),
            exit_code: Some(101),
            errors: vec![VerificationFinding {
                path: Some("src/foo.rs".into()),
                line: Some(42),
                message: "mismatched types".into(),
                code: Some("E0308".into()),
                ..Default::default()
            }],
            affected_files: vec!["src/foo.rs".into()],
            ..Default::default()
        };
        let report = TestExecutionReport {
            status: Some(TestExecutionStatus::Failed),
            command: Some("cargo check".into()),
            evidence: Some(evidence.clone()),
            ..Default::default()
        };
        let summary = compose(VerificationSummaryInput {
            evidence: Some(&evidence),
            test_report: Some(&report),
            ..Default::default()
        });
        assert_eq!(summary.status, Some(VerificationStatus::Failed));
        assert!(summary
            .failure_highlights
            .iter()
            .any(|h| h.contains("src/foo.rs:42")));
        assert_eq!(
            summary.recommended_fix_prompt.as_deref(),
            Some("Fix the build failure.")
        );
        assert_eq!(
            summary.recommended_fix_label.as_deref(),
            Some("Review Fix")
        );
        assert!(summary.to_markdown().contains("[Review Fix]"));
    }

    #[test]
    fn test_failure_partial_detail() {
        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::TestFailure),
            command: Some("cargo test".into()),
            exit_code: Some(101),
            test_names: vec!["settings::provider_selection".into()],
            errors: vec![VerificationFinding {
                path: Some("tests/settings.rs".into()),
                line: Some(84),
                message: "The test still expects the previous provider-selection behavior"
                    .into(),
                test_name: Some("settings::provider_selection".into()),
                ..Default::default()
            }],
            output_excerpt: "test result: FAILED. 43 passed; 1 failed".into(),
            ..Default::default()
        };
        let summary = compose(VerificationSummaryInput {
            evidence: Some(&evidence),
            test_report: Some(&TestExecutionReport {
                status: Some(TestExecutionStatus::Failed),
                command: Some("cargo test".into()),
                evidence: Some(evidence.clone()),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(summary.status, Some(VerificationStatus::Failed));
        let md = summary.to_markdown();
        assert!(md.contains("tests/settings.rs:84"));
        assert!(md.contains("Fix the failing tests"));
    }

    #[test]
    fn cancelled_and_environment() {
        let cancelled = compose(VerificationSummaryInput {
            cancelled: true,
            test_report: Some(&TestExecutionReport {
                status: Some(TestExecutionStatus::AwaitingReview),
                command: Some("cargo test".into()),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(cancelled.status, Some(VerificationStatus::Cancelled));

        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::EnvironmentFailure),
            command: Some("cargo test".into()),
            ..Default::default()
        };
        let env = compose(VerificationSummaryInput {
            evidence: Some(&evidence),
            test_report: Some(&TestExecutionReport {
                status: Some(TestExecutionStatus::CommandError),
                evidence: Some(evidence.clone()),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(env.status, Some(VerificationStatus::Failed));
        assert!(env.recommended_fix_prompt.is_none());
        assert!(env
            .next_actions
            .iter()
            .any(|a| a.to_ascii_lowercase().contains("environment")));
    }

    #[test]
    fn multiple_verification_commands() {
        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::TestFailure),
            command: Some("cargo test".into()),
            errors: vec![VerificationFinding {
                message: "boom".into(),
                path: Some("t.rs".into()),
                line: Some(1),
                ..Default::default()
            }],
            ..Default::default()
        };
        let summary = compose(VerificationSummaryInput {
            evidence: Some(&evidence),
            test_report: Some(&TestExecutionReport {
                status: Some(TestExecutionStatus::Failed),
                command: Some("cargo test".into()),
                evidence: Some(evidence.clone()),
                ..Default::default()
            }),
            additional_commands: vec![VerifiedCommandLine {
                command: "cargo check".into(),
                status: VerificationStatus::Passed,
                detail: Some("passed".into()),
            }],
            ..Default::default()
        });
        assert!(summary.verified_commands.len() >= 2);
        let md = summary.to_markdown();
        assert!(md.contains("cargo check"));
        assert!(md.contains("cargo test"));
    }

    #[test]
    fn coding_apply_without_verification_is_not_run() {
        let summary = compose(VerificationSummaryInput {
            execution: Some(&exec_with_files(&["src/lib.rs"])),
            ..Default::default()
        });
        assert_eq!(summary.status, Some(VerificationStatus::NotRun));
        assert!(summary
            .next_actions
            .iter()
            .any(|a| a.to_ascii_lowercase().contains("test")));
        assert!(!summary.to_markdown().contains("tests passed"));
    }
}
