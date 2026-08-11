//! Verification Diagnostics Feedback (Sprint C4.3).
//!
//! Turn build/test/verification command output into structured evidence for
//! conversation and the canonical ContextEngine diagnostics pipeline —
//! without dumping raw logs or creating a second diagnostics system.
//!
//! Pipeline:
//! Tool Execution → Verification Result → Planner → Context → Reasoning
//!
//! Constitutional constraints:
//! - Preserve raw excerpts for debugging (bounded)
//! - Do not infer failures unsupported by the output
//! - Observational only — never decides to mutate code
//! - Reasoning may interpret; never executes commands
//! - Reuse [`jaymi_context::BundleDiagnostic`] / DiagnosticsSection

use jaymi_context::{
    BundleDiagnostic, ContextSessionInputs, DiagnosticsSection, RuntimeCommandKind,
};

use crate::test_execution::{TestExecutionReport, TestExecutionStatus};

/// Hard cap for raw excerpts retained on evidence (chars).
pub const RAW_EXCERPT_CAP: usize = 4_000;
/// Max structured findings kept for conversation / context.
pub const FINDING_CAP: usize = 32;

/// High-level verification outcome class (C4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationResultKind {
    Success,
    BuildFailure,
    TestFailure,
    Warning,
    ToolFailure,
    Timeout,
    EnvironmentFailure,
    Unknown,
}

impl VerificationResultKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::BuildFailure => "build_failure",
            Self::TestFailure => "test_failure",
            Self::Warning => "warning",
            Self::ToolFailure => "tool_failure",
            Self::Timeout => "timeout",
            Self::EnvironmentFailure => "environment_failure",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::BuildFailure => "Build failure",
            Self::TestFailure => "Test failure",
            Self::Warning => "Warning",
            Self::ToolFailure => "Tool failure",
            Self::Timeout => "Timeout",
            Self::EnvironmentFailure => "Environment failure",
            Self::Unknown => "Unknown result",
        }
    }
}

/// Severity of one structured finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Error,
    Warning,
    Info,
}

impl FindingSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One structured diagnostic finding from verification output.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct VerificationFinding {
    pub severity: Option<FindingSeverity>,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub message: String,
    pub test_name: Option<String>,
    /// Compiler code when present (`E0308`, …).
    pub code: Option<String>,
}

/// Structured verification evidence for conversation + Context.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct VerificationEvidence {
    pub kind: Option<VerificationResultKind>,
    pub status_label: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub errors: Vec<VerificationFinding>,
    pub warnings: Vec<VerificationFinding>,
    pub test_names: Vec<String>,
    pub failure_messages: Vec<String>,
    pub affected_files: Vec<String>,
    /// Bounded raw excerpt (not the full dump).
    pub output_excerpt: String,
    pub truncated: bool,
    pub notes: Vec<String>,
}

impl VerificationEvidence {
    pub fn finding_count(&self) -> usize {
        self.errors.len() + self.warnings.len()
    }

    /// Conversation-facing summary — not a raw log dump.
    pub fn to_conversation_markdown(&self) -> String {
        let mut out = String::from("## Verification\n");
        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Result\n- {}\n", kind.label()));
        } else if !self.status_label.is_empty() {
            out.push_str(&format!("\n### Result\n- {}\n", self.status_label));
        }

        match self.kind {
            Some(VerificationResultKind::Success) => {
                if self.warnings.is_empty() {
                    out.push_str("\nVerification succeeded.\n");
                } else {
                    out.push_str(&format!(
                        "\nVerification succeeded with {} warning{}.\n",
                        self.warnings.len(),
                        if self.warnings.len() == 1 { "" } else { "s" }
                    ));
                }
            }
            Some(VerificationResultKind::BuildFailure) => {
                out.push_str(&format!(
                    "\nBuild failed with {} error{}.\n",
                    self.errors.len().max(1),
                    if self.errors.len() == 1 { "" } else { "s" }
                ));
            }
            Some(VerificationResultKind::TestFailure) => {
                out.push_str(&format!(
                    "\nTests failed ({} failure{}).\n",
                    self.errors.len().max(1),
                    if self.errors.len() == 1 { "" } else { "s" }
                ));
            }
            Some(VerificationResultKind::Warning) => {
                out.push_str(&format!(
                    "\nVerification reported {} warning{}.\n",
                    self.warnings.len(),
                    if self.warnings.len() == 1 { "" } else { "s" }
                ));
            }
            Some(VerificationResultKind::Timeout) => {
                out.push_str("\nVerification timed out — no automatic retry.\n");
            }
            Some(VerificationResultKind::ToolFailure) => {
                out.push_str("\nVerification tool/provider failed before producing a suite result.\n");
            }
            Some(VerificationResultKind::EnvironmentFailure) => {
                out.push_str("\nVerification could not run due to an environment problem.\n");
            }
            Some(VerificationResultKind::Unknown) | None => {
                out.push_str(
                    "\nVerification finished with an unclear result — see excerpt / diagnostics.\n",
                );
            }
        }

        if let Some(cmd) = &self.command {
            out.push_str(&format!("\n### Command\n- `{cmd}`\n"));
        }
        if let Some(code) = self.exit_code {
            out.push_str(&format!("- Exit code: `{code}`\n"));
        }
        if let Some(ms) = self.duration_ms {
            out.push_str(&format!("- Duration: {ms} ms\n"));
        }

        if !self.errors.is_empty() {
            out.push_str("\n### Errors\n");
            for finding in self.errors.iter().take(FINDING_CAP) {
                out.push_str(&format!("- {}\n", format_finding_line(finding)));
            }
        }
        if !self.warnings.is_empty() {
            out.push_str("\n### Warnings\n");
            for finding in self.warnings.iter().take(FINDING_CAP) {
                out.push_str(&format!("- {}\n", format_finding_line(finding)));
            }
        }
        if !self.test_names.is_empty() && self.errors.is_empty() {
            out.push_str("\n### Failed tests\n");
            for name in self.test_names.iter().take(FINDING_CAP) {
                out.push_str(&format!("- `{name}`\n"));
            }
        }
        if !self.failure_messages.is_empty() && self.errors.is_empty() {
            out.push_str("\n### Failure messages\n");
            for msg in self.failure_messages.iter().take(8) {
                out.push_str(&format!("- {msg}\n"));
            }
        }

        if !self.output_excerpt.is_empty()
            && self.errors.is_empty()
            && self.warnings.is_empty()
            && matches!(
                self.kind,
                Some(
                    VerificationResultKind::Unknown
                        | VerificationResultKind::ToolFailure
                        | VerificationResultKind::EnvironmentFailure
                        | VerificationResultKind::Timeout
                ) | None
            )
        {
            out.push_str("\n### Excerpt\n```\n");
            out.push_str(&self.output_excerpt);
            if !self.output_excerpt.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }

        if self.truncated {
            out.push_str("\n_Raw output was bounded; full stream remains in diagnostics / activity._\n");
        }

        out.push_str(
            "\n### Constraints\n- Diagnostics are observational — no automatic repair.\n- Planner → Terminal Tool → Terminal Provider only.\n- Reasoning may interpret this evidence; it does not run commands.\n- No silent retries.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "verification_evidence kind={} errors={} warnings={} files={} tests={} truncated={}",
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.errors.len(),
            self.warnings.len(),
            self.affected_files.len(),
            self.test_names.len(),
            self.truncated,
        )
    }

    /// Map findings into existing [`BundleDiagnostic`] wire format.
    pub fn to_bundle_diagnostics(&self) -> Vec<BundleDiagnostic> {
        let mut out = Vec::new();
        for finding in self.errors.iter().chain(self.warnings.iter()).take(FINDING_CAP) {
            out.push(BundleDiagnostic {
                path: finding.path.clone(),
                severity: finding
                    .severity
                    .unwrap_or(FindingSeverity::Error)
                    .as_str()
                    .to_string(),
                message: {
                    let mut msg = finding.message.clone();
                    if let Some(test) = &finding.test_name {
                        msg = format!("[{test}] {msg}");
                    }
                    if let Some(code) = &finding.code {
                        msg = format!("{code}: {msg}");
                    }
                    msg
                },
                line: finding.line,
                column: finding.column,
                source: Some("verification".into()),
            });
        }
        if out.is_empty() {
            if let Some(kind) = self.kind {
                if !matches!(kind, VerificationResultKind::Success) {
                    out.push(BundleDiagnostic {
                        path: None,
                        severity: match kind {
                            VerificationResultKind::Warning => "warning".into(),
                            VerificationResultKind::Success => "info".into(),
                            _ => "error".into(),
                        },
                        message: format!(
                            "{}{}",
                            kind.label(),
                            self.command
                                .as_ref()
                                .map(|c| format!(" (`{c}`)"))
                                .unwrap_or_default()
                        ),
                        line: None,
                        column: None,
                        source: Some("verification".into()),
                    });
                }
            }
        }
        out
    }
}

fn format_finding_line(finding: &VerificationFinding) -> String {
    let loc = match (&finding.path, finding.line) {
        (Some(path), Some(line)) => format!("`{path}:{line}`"),
        (Some(path), None) => format!("`{path}`"),
        (None, Some(line)) => format!("line {line}"),
        (None, None) => finding
            .test_name
            .as_ref()
            .map(|t| format!("`{t}`"))
            .unwrap_or_else(|| "—".into()),
    };
    let mut msg = finding.message.clone();
    if let Some(code) = &finding.code {
        if !msg.contains(code) {
            msg = format!("{code} — {msg}");
        }
    }
    format!("{loc} — {msg}")
}

/// Build structured evidence from a finalized [`TestExecutionReport`].
pub fn evidence_from_test_report(report: &TestExecutionReport) -> VerificationEvidence {
    let combined = join_output(report);
    let mut evidence = parse_verification_output(
        report.command.as_deref(),
        report.exit_status,
        report.duration_ms,
        &combined,
        report.truncated,
        report.status,
        report.gaps.first().map(String::as_str),
    );
    if evidence.command.is_none() {
        evidence.command = report.command.clone();
    }
    evidence
}

fn join_output(report: &TestExecutionReport) -> String {
    let mut parts = Vec::new();
    if let Some(stdout) = &report.stdout {
        if !stdout.is_empty() {
            parts.push(stdout.as_str());
        }
    }
    if let Some(stderr) = &report.stderr {
        if !stderr.is_empty() {
            parts.push(stderr.as_str());
        }
    }
    for gap in &report.gaps {
        parts.push(gap.as_str());
    }
    parts.join("\n")
}

/// Parse verification command output into structured evidence.
pub fn parse_verification_output(
    command: Option<&str>,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    output: &str,
    already_truncated: bool,
    exec_status: Option<TestExecutionStatus>,
    provider_error: Option<&str>,
) -> VerificationEvidence {
    let (excerpt, excerpt_truncated) = bound_excerpt(output);
    let truncated = already_truncated || excerpt_truncated;

    let mut errors = parse_compiler_diagnostics(output, FindingSeverity::Error);
    let warnings = parse_compiler_diagnostics(output, FindingSeverity::Warning);
    let (test_names, test_findings, failure_messages) = parse_test_failures(output);
    for finding in test_findings {
        if !errors.iter().any(|e| e.message == finding.message && e.test_name == finding.test_name)
        {
            errors.push(finding);
        }
    }
    errors.truncate(FINDING_CAP);

    let kind = classify_kind(
        exec_status,
        exit_code,
        provider_error.or_else(|| {
            if output.is_empty() {
                None
            } else {
                None
            }
        }),
        output,
        &errors,
        &warnings,
        &test_names,
    );

    // Environment / tool cues from provider error or output.
    let mut notes = Vec::new();
    if matches!(kind, VerificationResultKind::Unknown) && exit_code.map(|c| c != 0).unwrap_or(false)
    {
        notes.push(
            "Non-zero exit without parseable compiler/test findings — not inventing failure details."
                .into(),
        );
    }

    let affected_files = collect_affected_files(&errors, &warnings);

    VerificationEvidence {
        kind: Some(kind),
        status_label: kind.label().to_string(),
        command: command.map(str::to_string),
        exit_code,
        duration_ms,
        errors,
        warnings,
        test_names,
        failure_messages,
        affected_files,
        output_excerpt: excerpt,
        truncated,
        notes,
    }
}

fn classify_kind(
    exec_status: Option<TestExecutionStatus>,
    exit_code: Option<i32>,
    provider_error: Option<&str>,
    output: &str,
    errors: &[VerificationFinding],
    warnings: &[VerificationFinding],
    test_names: &[String],
) -> VerificationResultKind {
    let lower = output.to_ascii_lowercase();
    let err_lower = provider_error.unwrap_or("").to_ascii_lowercase();

    if matches!(exec_status, Some(TestExecutionStatus::TimedOut))
        || err_lower.contains("timed out")
        || lower.contains("timed out waiting for terminal command")
    {
        return VerificationResultKind::Timeout;
    }

    if matches!(
        exec_status,
        Some(TestExecutionStatus::CommandError | TestExecutionStatus::Denied)
    ) || !err_lower.is_empty()
    {
        if looks_like_environment(&err_lower) || looks_like_environment(&lower) {
            return VerificationResultKind::EnvironmentFailure;
        }
        if matches!(exec_status, Some(TestExecutionStatus::CommandError)) || !err_lower.is_empty() {
            return VerificationResultKind::ToolFailure;
        }
    }

    if looks_like_environment(&lower)
        && exit_code.map(|c| c != 0).unwrap_or(true)
        && errors.is_empty()
        && test_names.is_empty()
    {
        return VerificationResultKind::EnvironmentFailure;
    }

    let has_compile = output_has_compile_errors(output) || errors.iter().any(|e| e.code.is_some());
    let has_test_fail = !test_names.is_empty()
        || lower.contains("test result: failed")
        || (lower.contains("failures:") && lower.contains("test "))
        || errors.iter().any(|e| e.test_name.is_some());

    let success = matches!(exec_status, Some(TestExecutionStatus::Passed))
        || exit_code == Some(0);

    if success {
        return if !warnings.is_empty() {
            VerificationResultKind::Warning
        } else {
            VerificationResultKind::Success
        };
    }

    // Failed / non-zero / unknown status — only claim kinds supported by output.
    if has_compile && !has_test_fail {
        return VerificationResultKind::BuildFailure;
    }
    if has_test_fail && !has_compile {
        return VerificationResultKind::TestFailure;
    }
    if has_compile && has_test_fail {
        return if lower.contains("could not compile") {
            VerificationResultKind::BuildFailure
        } else {
            VerificationResultKind::TestFailure
        };
    }
    if !warnings.is_empty() && errors.is_empty() {
        return VerificationResultKind::Warning;
    }
    if !errors.is_empty() {
        return VerificationResultKind::BuildFailure;
    }
    VerificationResultKind::Unknown
}

fn looks_like_environment(text: &str) -> bool {
    text.contains("command not found")
        || text.contains("no such file or directory")
        || text.contains("permission denied")
        || text.contains("not a git repository")
        || text.contains("can't open")
        || text.contains("cannot find")
            && (text.contains("cargo") || text.contains("npm") || text.contains("go"))
}

fn output_has_compile_errors(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("error[e")
        || lower.contains("error: could not compile")
        || lower.contains("could not compile")
        || lower.contains("aborting due to")
}

/// Parse rustc / cargo-style `error` / `warning` blocks with `--> file:line:col`.
pub fn parse_compiler_diagnostics(
    output: &str,
    severity: FindingSeverity,
) -> Vec<VerificationFinding> {
    let needle = match severity {
        FindingSeverity::Error => "error",
        FindingSeverity::Warning => "warning",
        FindingSeverity::Info => "info",
    };
    let mut findings = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        let lower = line.to_ascii_lowercase();
        let is_match = match severity {
            FindingSeverity::Error => {
                lower.starts_with("error[") || lower.starts_with("error:")
            }
            FindingSeverity::Warning => {
                lower.starts_with("warning:") || lower.starts_with("warning[")
            }
            FindingSeverity::Info => lower.starts_with("info:"),
        };
        if !is_match {
            i += 1;
            continue;
        }

        let (code, message) = split_compiler_header(line, needle);
        let mut path = None;
        let mut line_no = None;
        let mut column = None;
        // Look ahead for `--> path:line:col`
        for look in lines.iter().skip(i + 1).take(6) {
            if let Some((p, l, c)) = parse_arrow_location(look) {
                path = Some(p);
                line_no = l;
                column = c;
                break;
            }
            let t = look.trim_start();
            if t.starts_with("error") || t.starts_with("warning") || t.starts_with("note:") {
                break;
            }
        }

        findings.push(VerificationFinding {
            severity: Some(severity),
            path,
            line: line_no,
            column,
            end_line: None,
            message,
            test_name: None,
            code,
        });
        if findings.len() >= FINDING_CAP {
            break;
        }
        i += 1;
    }
    findings
}

fn split_compiler_header(line: &str, _kind: &str) -> (Option<String>, String) {
    let trimmed = line.trim();
    // error[E0308]: mismatched types
    if let Some(rest) = trimmed
        .strip_prefix("error[")
        .or_else(|| trimmed.strip_prefix("warning["))
    {
        if let Some((code, after)) = rest.split_once(']') {
            let msg = after.trim_start_matches(':').trim();
            return (Some(code.to_string()), msg.to_string());
        }
    }
    if let Some(msg) = trimmed
        .strip_prefix("error:")
        .or_else(|| trimmed.strip_prefix("warning:"))
        .or_else(|| trimmed.strip_prefix("info:"))
    {
        return (None, msg.trim().to_string());
    }
    (None, trimmed.to_string())
}

fn parse_arrow_location(line: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("-->")?;
    let rest = rest.trim();
    // path:line:col  or path:line
    let mut parts = rest.rsplitn(3, ':');
    let third = parts.next()?;
    let second = parts.next();
    let first = parts.next();
    match (first, second, third) {
        (Some(path), Some(line_s), col_s) => {
            let line_n = line_s.parse().ok();
            let col_n = col_s.parse().ok();
            if line_n.is_some() {
                Some((path.to_string(), line_n, col_n))
            } else {
                // path might contain drive letters; treat whole as path
                Some((rest.to_string(), None, None))
            }
        }
        (None, Some(path), line_s) => {
            let line_n = line_s.parse().ok();
            Some((path.to_string(), line_n, None))
        }
        _ => Some((rest.to_string(), None, None)),
    }
}

/// Parse cargo/libtest failure names and panic locations when present.
pub fn parse_test_failures(
    output: &str,
) -> (Vec<String>, Vec<VerificationFinding>, Vec<String>) {
    let mut names = Vec::new();
    let mut findings = Vec::new();
    let mut messages = Vec::new();
    let lines: Vec<&str> = output.lines().collect();

    // `failures:` block listing test names
    let mut in_failures = false;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures = true;
            continue;
        }
        if in_failures {
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("test result:")
                || trimmed.starts_with("error:")
                || trimmed.starts_with("failures:")
            {
                in_failures = false;
                continue;
            }
            // lines like `    crate::tests::foo` or bare names
            let name = trimmed.trim_start_matches("----").trim();
            if name.ends_with(" stdout ----") {
                let n = name.trim_end_matches(" stdout ----").trim();
                push_unique(&mut names, n.to_string());
            } else if !name.contains(' ') && name.contains("::") || name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':') {
                push_unique(&mut names, name.to_string());
            }
        }
    }

    // `---- name stdout ----` + panic line
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(name) = trimmed
            .strip_prefix("---- ")
            .and_then(|r| r.strip_suffix(" stdout ----"))
        {
            let test_name = name.trim().to_string();
            push_unique(&mut names, test_name.clone());
            let mut panic_msg = None;
            let mut path = None;
            let mut line_no = None;
            for look in lines.iter().skip(i + 1).take(12) {
                let t = look.trim();
                if t.starts_with("---- ") || t == "failures:" || t.starts_with("test result:") {
                    break;
                }
                if let Some(rest) = t.strip_prefix("thread '") {
                    // thread 'x' panicked at 'msg', path:line:col
                    if let Some((_, after_at)) = rest.split_once("panicked at") {
                        let after_at = after_at.trim().trim_start_matches('\'').trim();
                        if let Some((msg, loc)) = after_at.rsplit_once("', ") {
                            panic_msg = Some(msg.trim().trim_matches('\'').to_string());
                            if let Some((p, l, c)) = parse_plain_location(loc) {
                                path = Some(p);
                                line_no = l;
                                let _ = c;
                            }
                        } else {
                            panic_msg = Some(after_at.trim_matches('\'').to_string());
                        }
                    }
                }
            }
            let message = panic_msg
                .clone()
                .unwrap_or_else(|| format!("test `{test_name}` failed"));
            if let Some(msg) = panic_msg {
                push_unique(&mut messages, msg);
            }
            findings.push(VerificationFinding {
                severity: Some(FindingSeverity::Error),
                path,
                line: line_no,
                column: None,
                end_line: None,
                message,
                test_name: Some(test_name),
                code: None,
            });
        }
        i += 1;
    }

    names.truncate(FINDING_CAP);
    findings.truncate(FINDING_CAP);
    messages.truncate(16);
    (names, findings, messages)
}

fn parse_plain_location(loc: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    let loc = loc.trim().trim_end_matches(':');
    let mut parts = loc.rsplitn(3, ':');
    let a = parts.next()?;
    let b = parts.next();
    let c = parts.next();
    match (c, b, a) {
        (Some(path), Some(line_s), col_s) => {
            let line_n = line_s.parse().ok();
            let col_n = col_s.parse().ok();
            if line_n.is_some() {
                Some((path.to_string(), line_n, col_n))
            } else {
                None
            }
        }
        (None, Some(path), line_s) => {
            let line_n = line_s.parse().ok();
            Some((path.to_string(), line_n, None))
        }
        _ => None,
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !value.is_empty() && !list.iter().any(|x| x == &value) {
        list.push(value);
    }
}

fn collect_affected_files(
    errors: &[VerificationFinding],
    warnings: &[VerificationFinding],
) -> Vec<String> {
    let mut files = Vec::new();
    for finding in errors.iter().chain(warnings.iter()) {
        if let Some(path) = &finding.path {
            push_unique(&mut files, path.clone());
        }
    }
    files.truncate(FINDING_CAP);
    files
}

fn bound_excerpt(raw: &str) -> (String, bool) {
    if raw.chars().count() <= RAW_EXCERPT_CAP {
        return (raw.to_string(), false);
    }
    let chars: Vec<char> = raw.chars().collect();
    let head: String = chars.iter().take(800).collect();
    let tail_start = chars.len().saturating_sub(RAW_EXCERPT_CAP.saturating_sub(900));
    let tail: String = chars[tail_start..].iter().collect();
    (
        format!("{head}\n…[truncated]…\n{tail}"),
        true,
    )
}

/// Merge verification diagnostics into session inputs for ContextEngine.
///
/// Reuses [`DiagnosticsSection`] / editor snapshot diagnostics — no second system.
/// Replaces prior `source == "verification"` entries only.
pub fn merge_evidence_into_session(
    session: &mut ContextSessionInputs,
    evidence: &VerificationEvidence,
) {
    let new_diags = evidence.to_bundle_diagnostics();
    retain_non_verification(&mut session.diagnostics.diagnostics);
    session.diagnostics.diagnostics.extend(new_diags.clone());

    if let Some(editor) = session.editor_snapshot.as_mut() {
        retain_non_verification(&mut editor.diagnostics);
        editor.diagnostics.extend(new_diags);
    }

    // Enrich runtime summary label when we have a clearer kind.
    if let Some(snap) = session.runtime_snapshot.as_mut() {
        if let Some(latest) = snap.latest_tests.as_mut() {
            if latest.kind == RuntimeCommandKind::Test {
                if let Some(kind) = evidence.kind {
                    latest.summary = format!("{} · {}", kind.label(), latest.summary);
                }
            }
        }
    }

    let _ = DiagnosticsSection {
        diagnostics: session.diagnostics.diagnostics.clone(),
    };
}

fn retain_non_verification(diags: &mut Vec<BundleDiagnostic>) {
    diags.retain(|d| d.source.as_deref() != Some("verification"));
}

/// Attach evidence onto a test report and refresh conversation-oriented fields.
pub fn enrich_test_report(report: &mut TestExecutionReport) {
    if !matches!(
        report.status,
        Some(
            TestExecutionStatus::Passed
                | TestExecutionStatus::Failed
                | TestExecutionStatus::TimedOut
                | TestExecutionStatus::CommandError
        )
    ) {
        return;
    }
    let evidence = evidence_from_test_report(report);
    report.evidence = Some(evidence);
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPILER_OUT: &str = r#"
error[E0308]: mismatched types
  --> src/foo.rs:42:5
   |
42 |     x
   |     ^ expected `i32`, found `&str`

error[E0382]: borrow of moved value
  --> src/bar.rs:91:12

error[E0425]: cannot find value `token`
  --> tests/auth.rs:18:9

warning: unused variable: `tmp`
  --> src/foo.rs:10:9

error: aborting due to 3 previous errors
"#;

    const TEST_OUT: &str = r#"
running 2 tests
test suite::ok ... ok
test suite::bad ... FAILED

failures:

---- suite::bad stdout ----
thread 'suite::bad' panicked at 'assertion failed: `(left == right)`', src/lib.rs:10:5

failures:
    suite::bad

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
"#;

    #[test]
    fn parses_compiler_errors_multiple_files() {
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(101),
            Some(1200),
            COMPILER_OUT,
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        assert_eq!(evidence.kind, Some(VerificationResultKind::BuildFailure));
        assert!(evidence.errors.len() >= 3);
        assert!(evidence.warnings.iter().any(|w| w.message.contains("unused")));
        assert!(evidence.affected_files.iter().any(|f| f.contains("foo.rs")));
        assert!(evidence.affected_files.iter().any(|f| f.contains("bar.rs")));
        assert!(evidence.affected_files.iter().any(|f| f.contains("auth.rs")));
        let md = evidence.to_conversation_markdown();
        assert!(md.contains("Build failed"));
        assert!(md.contains("src/foo.rs:42"));
        assert!(!md.contains("aborting due to 3 previous errors") || md.contains("Errors"));
    }

    #[test]
    fn parses_test_failures() {
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(101),
            Some(800),
            TEST_OUT,
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        assert_eq!(evidence.kind, Some(VerificationResultKind::TestFailure));
        assert!(evidence.test_names.iter().any(|n| n.contains("suite::bad")));
        assert!(evidence.errors.iter().any(|e| e.test_name.as_deref() == Some("suite::bad")));
        assert!(evidence
            .errors
            .iter()
            .any(|e| e.path.as_deref() == Some("src/lib.rs") && e.line == Some(10)));
    }

    #[test]
    fn successful_verification() {
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(0),
            Some(50),
            "test result: ok. 3 passed; 0 failed",
            false,
            Some(TestExecutionStatus::Passed),
            None,
        );
        assert_eq!(evidence.kind, Some(VerificationResultKind::Success));
        assert!(evidence.errors.is_empty());
        assert!(evidence.to_conversation_markdown().contains("succeeded"));
    }

    #[test]
    fn warnings_on_success() {
        let out = "warning: unused import: `std::mem`\n  --> src/a.rs:1:5\ntest result: ok. 1 passed; 0 failed\n";
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(0),
            None,
            out,
            false,
            Some(TestExecutionStatus::Passed),
            None,
        );
        assert_eq!(evidence.kind, Some(VerificationResultKind::Warning));
        assert_eq!(evidence.warnings.len(), 1);
    }

    #[test]
    fn malformed_output_is_unknown_not_invented() {
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(2),
            None,
            "blargle frobnitz exited weirdly",
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        assert_eq!(evidence.kind, Some(VerificationResultKind::Unknown));
        assert!(evidence.errors.is_empty());
        assert!(evidence
            .notes
            .iter()
            .any(|n| n.contains("not inventing")));
    }

    #[test]
    fn large_output_is_bounded() {
        let raw = format!(
            "{}\nerror[E0001]: boom\n  --> a.rs:1:1\n",
            "x".repeat(20_000)
        );
        let evidence = parse_verification_output(
            Some("cargo check"),
            Some(1),
            None,
            &raw,
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        assert!(evidence.truncated);
        assert!(evidence.output_excerpt.chars().count() < raw.chars().count());
        assert!(!evidence.errors.is_empty());
    }

    #[test]
    fn missing_line_information_still_surfaces_message() {
        let out = "error: could not compile `jaymi` due to previous errors\n";
        let evidence = parse_verification_output(
            Some("cargo build"),
            Some(101),
            None,
            out,
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        assert_eq!(evidence.kind, Some(VerificationResultKind::BuildFailure));
        assert!(evidence.errors.iter().any(|e| e.line.is_none()));
        assert!(evidence.to_conversation_markdown().contains("could not compile"));
    }

    #[test]
    fn tool_failure_and_timeout() {
        let timeout = parse_verification_output(
            Some("cargo test"),
            None,
            None,
            "",
            false,
            Some(TestExecutionStatus::TimedOut),
            Some("timed out waiting for terminal command: cargo test"),
        );
        assert_eq!(timeout.kind, Some(VerificationResultKind::Timeout));

        let tool = parse_verification_output(
            Some("cargo test"),
            None,
            None,
            "",
            false,
            Some(TestExecutionStatus::CommandError),
            Some("pty write failed"),
        );
        assert_eq!(tool.kind, Some(VerificationResultKind::ToolFailure));
    }

    #[test]
    fn environment_failure() {
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(127),
            None,
            "bash: cargo: command not found",
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        assert_eq!(
            evidence.kind,
            Some(VerificationResultKind::EnvironmentFailure)
        );
    }

    #[test]
    fn bundle_diagnostics_use_verification_source() {
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(101),
            None,
            COMPILER_OUT,
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        let diags = evidence.to_bundle_diagnostics();
        assert!(diags.len() >= 3);
        assert!(diags.iter().all(|d| d.source.as_deref() == Some("verification")));
    }

    #[test]
    fn merge_replaces_prior_verification_only() {
        let mut session = ContextSessionInputs {
            diagnostics: DiagnosticsSection {
                diagnostics: vec![
                    BundleDiagnostic {
                        path: Some("a.rs".into()),
                        severity: "error".into(),
                        message: "lsp".into(),
                        line: Some(1),
                        column: None,
                        source: Some("rust-analyzer".into()),
                    },
                    BundleDiagnostic {
                        path: Some("old.rs".into()),
                        severity: "error".into(),
                        message: "old".into(),
                        line: Some(2),
                        column: None,
                        source: Some("verification".into()),
                    },
                ],
            },
            ..Default::default()
        };
        let evidence = parse_verification_output(
            Some("cargo test"),
            Some(101),
            None,
            "error[E0001]: x\n  --> new.rs:3:1\n",
            false,
            Some(TestExecutionStatus::Failed),
            None,
        );
        merge_evidence_into_session(&mut session, &evidence);
        assert!(session
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.source.as_deref() == Some("rust-analyzer")));
        assert!(session
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.path.as_deref() == Some("new.rs")));
        assert!(!session
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.path.as_deref() == Some("old.rs")));
    }
}
