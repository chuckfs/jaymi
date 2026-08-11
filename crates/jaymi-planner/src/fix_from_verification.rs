//! Fix From Verification Failure (Sprint C4.4).
//!
//! Bridge verification failures into the existing C2 investigation + C3
//! Coding Plan pipeline — without autonomous repair.
//!
//! Canonical loop:
//! Verification Failure → Investigation → Diagnosis → Coding Plan →
//! Review → Approval → Existing C3 execution
//!
//! Constitutional constraints:
//! - Never automatically modify files
//! - Never auto-approve a corrective plan
//! - Never silently retry mutations or re-run verification
//! - Reasoning never executes tools
//! - Reuse C2/C3 — no parallel repair architecture

use jaymi_context::{BundleDiagnostic, ContextSessionInputs};
use jaymi_core::UserRequest;

use crate::change_investigation::ChangeInvestigationReport;
use crate::coding_edit::{
    CodingEditKind, CodingEditObjective, CodingEditStatus, EditScope,
};
use crate::coding_plan::CodingPlan;
use crate::test_execution::TestExecutionStatus;
use crate::verification_diagnostics::{
    parse_verification_output, VerificationEvidence, VerificationFinding, VerificationResultKind,
};

/// Soft-path intent for verification-grounded fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixFromVerificationIntent {
    FixBuildFailure,
    FixTestFailure,
    FixVerificationFailure,
}

impl FixFromVerificationIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FixBuildFailure => "fix_build_failure",
            Self::FixTestFailure => "fix_test_failure",
            Self::FixVerificationFailure => "fix_verification_failure",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FixBuildFailure => "Fix build failure",
            Self::FixTestFailure => "Fix test failure",
            Self::FixVerificationFailure => "Fix verification failure",
        }
    }
}

/// Detected assessment before diagnosis / planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixFromVerificationAssessment {
    pub goal: String,
    pub intent: FixFromVerificationIntent,
    pub matched_rules: Vec<&'static str>,
}

/// Whether a corrective coding plan is appropriate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixActionability {
    /// Structured failure evidence supports a corrective Coding Plan.
    Actionable,
    /// Failure exists but findings/files are missing — do not invent targets.
    InsufficientEvidence,
    /// Environment / tool / timeout — not a code mutation problem.
    NotCodeRelated,
    /// No recent verification failure to ground a fix.
    NoFailure,
}

impl FixActionability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actionable => "actionable",
            Self::InsufficientEvidence => "insufficient_evidence",
            Self::NotCodeRelated => "not_code_related",
            Self::NoFailure => "no_failure",
        }
    }
}

/// Diagnosis + C3 plan produced from a verification failure (planning only).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct FixFromVerificationReport {
    pub status: Option<FixActionability>,
    pub goal: String,
    pub intent: Option<FixFromVerificationIntent>,
    pub evidence: Option<VerificationEvidence>,
    pub diagnosis_summary: String,
    pub failing_files: Vec<String>,
    pub failing_symbols: Vec<String>,
    pub likely_cause: Option<String>,
    pub supporting_evidence: Vec<String>,
    pub gaps: Vec<String>,
    pub objective: Option<CodingEditObjective>,
    pub investigation: Option<ChangeInvestigationReport>,
    pub coding_plan: Option<CodingPlan>,
    pub matched_rules: Vec<String>,
    /// Always false in C4.4 — no automatic mutation.
    pub mutated: bool,
    /// Always false — no silent verification / mutation retry.
    pub auto_retried: bool,
    /// Always false — corrective ExecutionPlan is not auto-approved.
    pub auto_approved: bool,
}

impl FixFromVerificationReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Fix From Verification Failure\n");
        if let Some(status) = self.status {
            out.push_str(&format!(
                "\n### Status\n- {}\n",
                match status {
                    FixActionability::Actionable => {
                        "Verification failure looks actionable — corrective Coding Plan prepared (Review Before Action required before any mutation)."
                    }
                    FixActionability::InsufficientEvidence => {
                        "Verification failed, but evidence is insufficient to plan a safe correction."
                    }
                    FixActionability::NotCodeRelated => {
                        "This failure does not look like a code defect Jaymi should mutate around."
                    }
                    FixActionability::NoFailure => {
                        "No recent verification failure is available to ground a fix."
                    }
                }
            ));
        }

        if let Some(intent) = self.intent {
            out.push_str(&format!("\n### Intent\n- {}\n", intent.label()));
        }
        if !self.goal.is_empty() {
            out.push_str(&format!("\n### Request\n- {}\n", self.goal));
        }
        if !self.diagnosis_summary.is_empty() {
            out.push_str(&format!("\n### Diagnosis\n{}\n", self.diagnosis_summary));
        }
        if let Some(cause) = &self.likely_cause {
            out.push_str(&format!("\n### Likely cause\n- {cause}\n"));
        }
        if !self.failing_files.is_empty() {
            out.push_str("\n### Failing files\n");
            for f in &self.failing_files {
                out.push_str(&format!("- `{f}`\n"));
            }
        }
        if !self.failing_symbols.is_empty() {
            out.push_str("\n### Failing symbols / tests\n");
            for s in &self.failing_symbols {
                out.push_str(&format!("- `{s}`\n"));
            }
        }
        if !self.supporting_evidence.is_empty() {
            out.push_str("\n### Supporting evidence\n");
            for row in self.supporting_evidence.iter().take(12) {
                out.push_str(&format!("- {row}\n"));
            }
        }
        if !self.gaps.is_empty() {
            out.push_str("\n### Unknown / gaps\n");
            for gap in &self.gaps {
                out.push_str(&format!("- {gap}\n"));
            }
        }

        out.push_str(
            "\n### Constraints\n\
             - No automatic file modifications.\n\
             - No automatic approval of the corrective plan.\n\
             - No silent verification or mutation retries.\n\
             - Mutations use the existing C3 path under Review Before Action.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "fix_from_verification status={} intent={} files={} symbols={} mutated={} auto_retried={} auto_approved={} gaps={}",
            self.status.map(|s| s.as_str()).unwrap_or("unknown"),
            self.intent.map(|i| i.as_str()).unwrap_or("—"),
            self.failing_files.len(),
            self.failing_symbols.len(),
            self.mutated,
            self.auto_retried,
            self.auto_approved,
            self.gaps.len(),
        )
    }
}

/// Detect a verification-grounded fix request.
///
/// Explicit build/test/verification fix phrases always claim this soft path.
/// Deictic “fix it / fix this” only claims when failure evidence is present.
pub fn detect_fix_from_verification_request(
    request: &UserRequest,
    evidence: Option<&VerificationEvidence>,
) -> Option<FixFromVerificationAssessment> {
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

    // Never steal C4.1 / C4.2 / generation / multi-file / C5.1 multi-step goals.
    if crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::coding_goal::steals_from_coding_plan_detect(trimmed)
    {
        return None;
    }

    if let Some((intent, rules)) = classify_explicit_fix(trimmed) {
        return Some(FixFromVerificationAssessment {
            goal: content.to_string(),
            intent,
            matched_rules: rules,
        });
    }

    // Contextual deixis only when a recent failure is available.
    if looks_like_deictic_fix(trimmed) && evidence.map(is_failure_evidence).unwrap_or(false) {
        let intent = intent_from_evidence(evidence);
        return Some(FixFromVerificationAssessment {
            goal: content.to_string(),
            intent,
            matched_rules: vec!["deictic_fix_with_verification_evidence"],
        });
    }

    None
}

fn classify_explicit_fix(trimmed: &str) -> Option<(FixFromVerificationIntent, Vec<&'static str>)> {
    let buildish = trimmed.contains("build")
        || trimmed.contains("compile")
        || trimmed.contains("compiler")
        || trimmed.contains("typecheck")
        || trimmed.contains("type check");
    let testish = trimmed.contains("test") || trimmed.contains("suite");
    let verifyish = trimmed.contains("verification")
        || trimmed.contains("verify")
        || (trimmed.contains("failure") && (buildish || testish || trimmed.contains("error")));

    let fix_verb = trimmed.starts_with("fix ")
        || trimmed.starts_with("repair ")
        || trimmed.starts_with("address ")
        || trimmed.starts_with("resolve ");

    if !fix_verb {
        return None;
    }

    // Require failure / error / failing language so "fix the tests" (add tests)
    // stays with coding_edit / generation — not C4.4.
    let failure_language = trimmed.contains("fail")
        || trimmed.contains("error")
        || trimmed.contains("broken")
        || trimmed.contains("regression")
        || trimmed.contains("verification");

    if !failure_language && !verifyish {
        return None;
    }

    if buildish && !testish {
        return Some((
            FixFromVerificationIntent::FixBuildFailure,
            vec!["fix_build_failure"],
        ));
    }
    if testish {
        return Some((
            FixFromVerificationIntent::FixTestFailure,
            vec!["fix_test_failure"],
        ));
    }
    if verifyish || failure_language {
        return Some((
            FixFromVerificationIntent::FixVerificationFailure,
            vec!["fix_verification_failure"],
        ));
    }
    None
}

fn looks_like_deictic_fix(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "fix it"
            | "fix this"
            | "fix that"
            | "fix this failure"
            | "fix the failure"
            | "fix this error"
            | "fix the error"
            | "repair it"
            | "repair this"
    ) || trimmed.starts_with("fix it ")
        || (trimmed.starts_with("fix this ")
            && (trimmed.contains("fail")
                || trimmed.contains("error")
                || trimmed.contains("build")
                || trimmed.contains("test")))
}

fn is_failure_evidence(evidence: &VerificationEvidence) -> bool {
    matches!(
        evidence.kind,
        Some(
            VerificationResultKind::BuildFailure
                | VerificationResultKind::TestFailure
                | VerificationResultKind::Warning
                | VerificationResultKind::Unknown
                | VerificationResultKind::ToolFailure
                | VerificationResultKind::Timeout
                | VerificationResultKind::EnvironmentFailure
        )
    ) || evidence.exit_code.map(|c| c != 0).unwrap_or(false)
}

fn intent_from_evidence(evidence: Option<&VerificationEvidence>) -> FixFromVerificationIntent {
    match evidence.and_then(|e| e.kind) {
        Some(VerificationResultKind::BuildFailure) => FixFromVerificationIntent::FixBuildFailure,
        Some(VerificationResultKind::TestFailure) => FixFromVerificationIntent::FixTestFailure,
        _ => FixFromVerificationIntent::FixVerificationFailure,
    }
}

/// True when coding-edit / other soft paths should yield to C4.4 (explicit phrases).
pub fn steals_from_other_detect(request: &UserRequest) -> bool {
    detect_fix_from_verification_request(request, None).is_some()
}

/// Classify whether a corrective plan is appropriate from evidence.
pub fn classify_actionability(evidence: Option<&VerificationEvidence>) -> FixActionability {
    let Some(evidence) = evidence else {
        return FixActionability::NoFailure;
    };
    match evidence.kind {
        Some(VerificationResultKind::Success) | None => {
            if evidence.exit_code == Some(0) {
                FixActionability::NoFailure
            } else if evidence.errors.is_empty()
                && evidence.test_names.is_empty()
                && evidence.affected_files.is_empty()
            {
                FixActionability::InsufficientEvidence
            } else {
                FixActionability::Actionable
            }
        }
        Some(
            VerificationResultKind::EnvironmentFailure
                | VerificationResultKind::ToolFailure
                | VerificationResultKind::Timeout,
        ) => FixActionability::NotCodeRelated,
        Some(VerificationResultKind::Warning) => {
            if evidence.warnings.is_empty() && evidence.errors.is_empty() {
                FixActionability::InsufficientEvidence
            } else {
                FixActionability::Actionable
            }
        }
        Some(VerificationResultKind::Unknown) => {
            if evidence.errors.is_empty()
                && evidence.test_names.is_empty()
                && evidence.affected_files.is_empty()
            {
                FixActionability::InsufficientEvidence
            } else {
                FixActionability::Actionable
            }
        }
        Some(
            VerificationResultKind::BuildFailure | VerificationResultKind::TestFailure,
        ) => {
            if evidence.errors.is_empty()
                && evidence.test_names.is_empty()
                && evidence.affected_files.is_empty()
                && evidence.failure_messages.is_empty()
            {
                FixActionability::InsufficientEvidence
            } else {
                FixActionability::Actionable
            }
        }
    }
}

/// Load / rebuild structured evidence from Planner memory + session Context.
pub fn load_verification_evidence(
    remembered: Option<&VerificationEvidence>,
    session: &ContextSessionInputs,
) -> Option<VerificationEvidence> {
    if let Some(evidence) = remembered {
        if is_failure_evidence(evidence) || evidence.kind == Some(VerificationResultKind::Warning) {
            return Some(evidence.clone());
        }
    }

    let snap = session.runtime_snapshot.as_ref()?;
    let outcome = snap
        .latest_tests
        .as_ref()
        .filter(|o| !o.ok)
        .or_else(|| snap.recent_failures.first())?;

    let exec_status = if outcome.ok {
        Some(TestExecutionStatus::Passed)
    } else if outcome.summary.to_ascii_lowercase().contains("timed out") {
        Some(TestExecutionStatus::TimedOut)
    } else {
        Some(TestExecutionStatus::Failed)
    };

    let mut evidence = parse_verification_output(
        Some(outcome.command.as_str()),
        outcome.exit_code,
        None,
        &outcome.output_excerpt,
        outcome.output_excerpt.contains("[truncated]"),
        exec_status,
        None,
    );

    // Fold session diagnostics with source=verification when the excerpt is thin.
    enrich_from_session_diagnostics(&mut evidence, session);
    Some(evidence)
}

fn enrich_from_session_diagnostics(
    evidence: &mut VerificationEvidence,
    session: &ContextSessionInputs,
) {
    for diag in session
        .diagnostics
        .diagnostics
        .iter()
        .chain(
            session
                .editor_snapshot
                .as_ref()
                .map(|e| e.diagnostics.as_slice())
                .unwrap_or(&[]),
        )
    {
        if diag.source.as_deref() != Some("verification") {
            continue;
        }
        let finding = finding_from_bundle_diagnostic(diag);
        let is_warning = diag.severity.eq_ignore_ascii_case("warning");
        if is_warning {
            if !evidence
                .warnings
                .iter()
                .any(|w| w.message == finding.message && w.path == finding.path)
            {
                evidence.warnings.push(finding);
            }
        } else if !evidence
            .errors
            .iter()
            .any(|e| e.message == finding.message && e.path == finding.path)
        {
            evidence.errors.push(finding);
        }
    }
    for f in evidence
        .errors
        .iter()
        .chain(evidence.warnings.iter())
    {
        if let Some(path) = &f.path {
            if !evidence.affected_files.iter().any(|p| p == path) {
                evidence.affected_files.push(path.clone());
            }
        }
        if let Some(test) = &f.test_name {
            if !evidence.test_names.iter().any(|t| t == test) {
                evidence.test_names.push(test.clone());
            }
        }
    }
}

fn finding_from_bundle_diagnostic(diag: &BundleDiagnostic) -> VerificationFinding {
    let mut message = diag.message.clone();
    let mut test_name = None;
    if let Some(rest) = message.strip_prefix('[') {
        if let Some((name, rem)) = rest.split_once("] ") {
            test_name = Some(name.to_string());
            message = rem.to_string();
        }
    }
    VerificationFinding {
        severity: None,
        path: diag.path.clone(),
        line: diag.line,
        column: diag.column,
        end_line: None,
        message,
        test_name,
        code: None,
    }
}

/// Seed a C3.1 Coding Edit objective from verification evidence.
pub fn objective_from_evidence(
    assessment: &FixFromVerificationAssessment,
    evidence: &VerificationEvidence,
    session: &ContextSessionInputs,
    actionability: FixActionability,
) -> CodingEditObjective {
    let mut obj = CodingEditObjective {
        status: match actionability {
            FixActionability::Actionable => CodingEditStatus::Understood,
            _ => CodingEditStatus::NeedsClarification,
        },
        kind: Some(CodingEditKind::Fix),
        goal: assessment.goal.clone(),
        requested_change: format!(
            "Correct verification failure ({})",
            evidence
                .kind
                .map(|k| k.label())
                .unwrap_or("unknown result")
        ),
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        constraints: vec![
            "No automatic file modifications (C4.4).".into(),
            "Corrective mutations require Review Before Action via existing C3 path.".into(),
            "Do not silently re-run verification or retry mutations.".into(),
        ],
        ..CodingEditObjective::default()
    };
    obj.matched_rules
        .push(format!("intent:{}", assessment.intent.as_str()));
    if let Some(kind) = evidence.kind {
        obj.matched_rules
            .push(format!("verification_kind:{}", kind.as_str()));
    }

    if let Some(name) = session
        .project_snapshot
        .as_ref()
        .and_then(|p| p.metadata.name.clone())
    {
        obj.target_project = Some(name);
    }

    for path in &evidence.affected_files {
        push_unique(&mut obj.relevant_files, path.clone());
    }
    for err in evidence.errors.iter().chain(evidence.warnings.iter()) {
        if let Some(path) = &err.path {
            push_unique(&mut obj.relevant_files, path.clone());
        }
    }
    obj.target_file = obj.relevant_files.first().cloned();

    for name in &evidence.test_names {
        push_unique(&mut obj.target_symbols, name.clone());
    }
    // Heuristic: compiler codes are not symbols; skip. Prefer path stems later via C2.

    obj.expected_scope = Some(if obj.relevant_files.len() > 1 {
        EditScope::MultiFile
    } else if obj.target_file.is_some() {
        EditScope::File
    } else if !obj.target_symbols.is_empty() {
        EditScope::Symbol
    } else {
        EditScope::Unknown
    });

    if let Some(cmd) = &evidence.command {
        obj.relevant_context
            .push(format!("verification command: `{cmd}`"));
    }
    if let Some(code) = evidence.exit_code {
        obj.relevant_context
            .push(format!("verification exit code: {code}"));
    }
    for err in evidence.errors.iter().take(8) {
        obj.investigation_notes.push(format_finding_note(err));
    }
    for warn in evidence.warnings.iter().take(4) {
        obj.investigation_notes
            .push(format!("warning: {}", format_finding_note(warn)));
    }

    if let Some(overlap) = changed_code_overlap_note(session, &obj.relevant_files) {
        obj.investigation_notes.push(overlap);
    }

    match actionability {
        FixActionability::InsufficientEvidence => {
            obj.ambiguities.push(
                "Verification failed without parseable file/test findings — do not invent targets."
                    .into(),
            );
            obj.clarification = Some(
                "Re-run the failing verification (or paste the structured errors) so Jaymi can bind concrete files."
                    .into(),
            );
        }
        FixActionability::NotCodeRelated => {
            obj.ambiguities.push(
                "Failure kind is environment / tool / timeout — a code mutation plan is not appropriate."
                    .into(),
            );
            obj.clarification = Some(
                "Fix the environment or re-run after the tool/timeout issue is resolved; then ask again if code still fails."
                    .into(),
            );
        }
        FixActionability::NoFailure => {
            obj.ambiguities
                .push("No recent verification failure is available.".into());
            obj.clarification = Some(
                "Run the tests or a build check first, then ask to fix the failure.".into(),
            );
        }
        FixActionability::Actionable => {}
    }

    obj
}

fn format_finding_note(finding: &VerificationFinding) -> String {
    let loc = match (&finding.path, finding.line) {
        (Some(path), Some(line)) => format!("{path}:{line}"),
        (Some(path), None) => path.clone(),
        _ => finding
            .test_name
            .clone()
            .unwrap_or_else(|| "unknown".into()),
    };
    format!("{loc} — {}", finding.message)
}

fn changed_code_overlap_note(
    session: &ContextSessionInputs,
    failing_files: &[String],
) -> Option<String> {
    if failing_files.is_empty() {
        return None;
    }
    let git = session.git_snapshot.as_ref()?;
    let mut changed = Vec::new();
    for entry in git
        .dirty
        .iter()
        .chain(git.staged.iter())
        .chain(git.untracked.iter())
    {
        let path = entry.path.as_str();
        if failing_files.iter().any(|f| {
            path.ends_with(f) || f.ends_with(path) || path.contains(f) || f.contains(path)
        }) {
            changed.push(path.to_string());
        }
    }
    if changed.is_empty() {
        None
    } else {
        Some(format!(
            "Overlaps recent git changes (possible cause from changed code): {}",
            changed.into_iter().take(6).collect::<Vec<_>>().join(", ")
        ))
    }
}

/// Build diagnosis fields from evidence + actionability (no tools).
pub fn diagnose(
    assessment: &FixFromVerificationAssessment,
    evidence: Option<&VerificationEvidence>,
    actionability: FixActionability,
) -> FixFromVerificationReport {
    let mut report = FixFromVerificationReport {
        status: Some(actionability),
        goal: assessment.goal.clone(),
        intent: Some(assessment.intent),
        evidence: evidence.cloned(),
        matched_rules: assessment
            .matched_rules
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        mutated: false,
        auto_retried: false,
        auto_approved: false,
        ..Default::default()
    };

    if let Some(evidence) = evidence {
        report.failing_files = evidence.affected_files.clone();
        report.failing_symbols = evidence.test_names.clone();
        for err in &evidence.errors {
            report
                .supporting_evidence
                .push(format_finding_note(err));
            if let Some(path) = &err.path {
                push_unique(&mut report.failing_files, path.clone());
            }
        }
        for msg in evidence.failure_messages.iter().take(6) {
            push_unique(&mut report.supporting_evidence, msg.clone());
        }
        report.likely_cause = likely_cause_from_evidence(evidence, actionability);
        report.diagnosis_summary = match actionability {
            FixActionability::Actionable => format!(
                "{} with {} error(s) / {} warning(s) across {} file(s).",
                evidence
                    .kind
                    .map(|k| k.label())
                    .unwrap_or("Verification failure"),
                evidence.errors.len(),
                evidence.warnings.len(),
                report.failing_files.len(),
            ),
            FixActionability::InsufficientEvidence => {
                "A non-success verification result was observed, but Jaymi could not bind concrete failing files or tests from the output."
                    .into()
            }
            FixActionability::NotCodeRelated => format!(
                "{} — this is not treated as an actionable code defect.",
                evidence
                    .kind
                    .map(|k| k.label())
                    .unwrap_or("Verification problem")
            ),
            FixActionability::NoFailure => {
                "No verification failure is available.".into()
            }
        };
    } else {
        report.diagnosis_summary =
            "No VerificationEvidence is available for this turn.".into();
        report.gaps.push(
            "Run tests / a build check first so structured verification evidence exists.".into(),
        );
    }

    match actionability {
        FixActionability::InsufficientEvidence => {
            report.gaps.push(
                "Insufficient structured findings — will not invent failing files or symbols."
                    .into(),
            );
        }
        FixActionability::NotCodeRelated => {
            report.gaps.push(
                "Environment / tool / timeout failures require fixing the runtime condition, not mutating project source."
                    .into(),
            );
        }
        FixActionability::NoFailure => {
            report
                .gaps
                .push("Ask Jaymi to run verification before requesting a fix.".into());
        }
        FixActionability::Actionable => {}
    }

    report
}

fn likely_cause_from_evidence(
    evidence: &VerificationEvidence,
    actionability: FixActionability,
) -> Option<String> {
    match actionability {
        FixActionability::NotCodeRelated => evidence.kind.map(|k| k.label().to_string()),
        FixActionability::Actionable | FixActionability::InsufficientEvidence => {
            if let Some(err) = evidence.errors.first() {
                Some(format_finding_note(err))
            } else if let Some(msg) = evidence.failure_messages.first() {
                Some(msg.clone())
            } else if let Some(name) = evidence.test_names.first() {
                Some(format!("Failing test `{name}`"))
            } else {
                evidence.kind.map(|k| k.label().to_string())
            }
        }
        FixActionability::NoFailure => None,
    }
}

/// Attach investigation + coding plan onto a diagnosis report and enforce C4.4 flags.
pub fn attach_plan_artifacts(
    mut report: FixFromVerificationReport,
    objective: CodingEditObjective,
    investigation: ChangeInvestigationReport,
    mut plan: CodingPlan,
) -> FixFromVerificationReport {
    plan.rationale.insert(
        0,
        format!(
            "Grounded in VerificationEvidence ({}).",
            report
                .evidence
                .as_ref()
                .and_then(|e| e.kind)
                .map(|k| k.label())
                .unwrap_or("unknown")
        ),
    );
    if !plan.plan_steps.iter().any(|s| s.contains("Review Before Action")) {
        plan.plan_steps.push(
            "Constitutional gate: Review Before Action required before any file mutation (C4.4)."
                .into(),
        );
    }
    // Fold investigation symbols into diagnosis when C2 found more.
    for f in &investigation.affected_files {
        push_unique(&mut report.failing_files, f.clone());
    }
    for s in investigation
        .primary_symbols
        .iter()
        .chain(investigation.affected_symbols.iter())
    {
        push_unique(&mut report.failing_symbols, s.clone());
    }
    for dep in investigation.related_dependencies.iter().take(8) {
        push_unique(
            &mut report.supporting_evidence,
            format!("dependency: {dep}"),
        );
    }
    for u in &investigation.unknowns {
        push_unique(&mut report.gaps, u.clone());
    }

    report.objective = Some(objective);
    report.investigation = Some(investigation);
    report.coding_plan = Some(plan);
    report.mutated = false;
    report.auto_retried = false;
    report.auto_approved = false;
    report
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|v| v == &value) {
        list.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification_diagnostics::VerificationResultKind;

    fn failed_build_evidence() -> VerificationEvidence {
        VerificationEvidence {
            kind: Some(VerificationResultKind::BuildFailure),
            status_label: "Build failure".into(),
            command: Some("cargo test".into()),
            exit_code: Some(101),
            errors: vec![VerificationFinding {
                path: Some("src/foo.rs".into()),
                line: Some(42),
                message: "mismatched types".into(),
                code: Some("E0308".into()),
                ..Default::default()
            }],
            affected_files: vec!["src/foo.rs".into(), "src/bar.rs".into()],
            ..Default::default()
        }
    }

    #[test]
    fn detect_compiler_and_test_fix_phrases() {
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Fix the build failure."),
            None
        )
        .is_some());
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Fix the failing tests."),
            None
        )
        .is_some());
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Fix the compiler errors."),
            None
        )
        .is_some());
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Run the tests."),
            None
        )
        .is_none());
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Fix this."),
            None
        )
        .is_none());
    }

    #[test]
    fn deictic_fix_requires_evidence() {
        let evidence = failed_build_evidence();
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Fix it."),
            Some(&evidence)
        )
        .is_some());
        assert!(detect_fix_from_verification_request(
            &UserRequest::new("Fix it."),
            None
        )
        .is_none());
    }

    #[test]
    fn actionability_distinguishes_kinds() {
        assert_eq!(
            classify_actionability(Some(&failed_build_evidence())),
            FixActionability::Actionable
        );
        let env = VerificationEvidence {
            kind: Some(VerificationResultKind::EnvironmentFailure),
            ..Default::default()
        };
        assert_eq!(
            classify_actionability(Some(&env)),
            FixActionability::NotCodeRelated
        );
        let unknown = VerificationEvidence {
            kind: Some(VerificationResultKind::Unknown),
            exit_code: Some(2),
            ..Default::default()
        };
        assert_eq!(
            classify_actionability(Some(&unknown)),
            FixActionability::InsufficientEvidence
        );
        assert_eq!(classify_actionability(None), FixActionability::NoFailure);
    }

    #[test]
    fn objective_seeds_multiple_files() {
        let assessment = FixFromVerificationAssessment {
            goal: "Fix the build failure.".into(),
            intent: FixFromVerificationIntent::FixBuildFailure,
            matched_rules: vec!["fix_build_failure"],
        };
        let evidence = failed_build_evidence();
        let obj = objective_from_evidence(
            &assessment,
            &evidence,
            &ContextSessionInputs::default(),
            FixActionability::Actionable,
        );
        assert_eq!(obj.status, CodingEditStatus::Understood);
        assert_eq!(obj.kind, Some(CodingEditKind::Fix));
        assert_eq!(obj.expected_scope, Some(EditScope::MultiFile));
        assert!(obj.relevant_files.iter().any(|f| f.contains("foo.rs")));
        assert!(obj.relevant_files.iter().any(|f| f.contains("bar.rs")));
        assert!(obj
            .constraints
            .iter()
            .any(|c| c.contains("Review Before Action")));
    }

    #[test]
    fn diagnose_does_not_invent_for_unknown() {
        let assessment = FixFromVerificationAssessment {
            goal: "Fix the failure.".into(),
            intent: FixFromVerificationIntent::FixVerificationFailure,
            matched_rules: vec!["fix_verification_failure"],
        };
        let evidence = VerificationEvidence {
            kind: Some(VerificationResultKind::Unknown),
            exit_code: Some(2),
            output_excerpt: "blargle".into(),
            ..Default::default()
        };
        let report = diagnose(
            &assessment,
            Some(&evidence),
            FixActionability::InsufficientEvidence,
        );
        assert!(report.failing_files.is_empty());
        assert!(!report.gaps.is_empty());
        assert!(!report.mutated);
        assert!(!report.auto_approved);
        assert!(!report.auto_retried);
    }

    #[test]
    fn changed_code_overlap_is_noted() {
        use jaymi_context::{GitPathEntry, GitSnapshot};

        let assessment = FixFromVerificationAssessment {
            goal: "Fix the build failure.".into(),
            intent: FixFromVerificationIntent::FixBuildFailure,
            matched_rules: vec!["fix_build_failure"],
        };
        let evidence = failed_build_evidence();
        let session = ContextSessionInputs {
            git_snapshot: Some(GitSnapshot {
                dirty: vec![GitPathEntry {
                    path: "src/foo.rs".into(),
                    status: "M".into(),
                }],
                ..GitSnapshot::empty()
            }),
            ..Default::default()
        };
        let obj = objective_from_evidence(
            &assessment,
            &evidence,
            &session,
            FixActionability::Actionable,
        );
        assert!(obj
            .investigation_notes
            .iter()
            .any(|n| n.contains("Overlaps recent git changes")));
    }

    #[test]
    fn unrelated_editor_does_not_override_verification_files() {
        let assessment = FixFromVerificationAssessment {
            goal: "Fix the build failure.".into(),
            intent: FixFromVerificationIntent::FixBuildFailure,
            matched_rules: vec!["fix_build_failure"],
        };
        let evidence = failed_build_evidence();
        let session = ContextSessionInputs {
            current_file: jaymi_context::CurrentFileSection {
                path: Some("/proj/README.md".into()),
                dirty: false,
                language: Some("markdown".into()),
            },
            ..Default::default()
        };
        let obj = objective_from_evidence(
            &assessment,
            &evidence,
            &session,
            FixActionability::Actionable,
        );
        assert_eq!(obj.target_file.as_deref(), Some("src/foo.rs"));
        assert!(!obj.relevant_files.iter().any(|f| f.contains("README")));
    }
}
