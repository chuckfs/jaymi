//! Canonical read-only runtime intelligence observation (Sprint B2.6).
//!
//! [`RuntimeSnapshot`] is the immutable representation of live Coding runtime
//! state: latest cargo check / build / tests, terminal output summary, running
//! sessions, and recent failures.
//!
//! It is observational only:
//!
//! * executes no tools
//! * performs no reasoning
//! * owns no policy
//! * never builds a [`crate::ContextBundle`]
//! * never talks to an LLM
//! * never re-runs cargo / tests during observation
//!
//! ## Ownership
//!
//! | Role | Owner |
//! |------|--------|
//! | Orchestration (when to assemble) | Planner (via Application host prep) |
//! | Terminal execution | TerminalProvider (via Planner → Tool) |
//! | Ambient refresh | Application `ContextMaintenance` |
//! | Observation contract | [`RuntimeSnapshot`] |
//! | Consumption | Context providers (`RuntimeProvider`) — session only |
//! | Request / Conversation path | **Must not** block waiting for runtime |
//!
//! Distinct from Problems / Diagnostics (LSP / advisories) and from
//! [`crate::GitSnapshot`] (working-tree status).

use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Kind of observed command outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RuntimeCommandKind {
    /// `cargo check` / typecheck-style.
    Check,
    /// Build / compile.
    Build,
    /// Test suite.
    Test,
    /// Other terminal command.
    #[default]
    Other,
}

impl RuntimeCommandKind {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Test => "test",
            Self::Other => "other",
        }
    }
}

/// One observed command outcome (capped excerpt; not a full log dump).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct RuntimeCommandOutcome {
    /// Command line when known.
    pub command: String,
    /// Classified kind.
    pub kind: RuntimeCommandKind,
    /// Exit code when known.
    pub exit_code: Option<i32>,
    /// True when the command is believed to have succeeded.
    pub ok: bool,
    /// Short human summary.
    pub summary: String,
    /// Capped output excerpt.
    pub output_excerpt: String,
    /// Terminal session id when known.
    pub session_id: Option<String>,
    /// Working directory when known.
    pub cwd: Option<String>,
    /// Unix seconds when observed (ignored by parent Eq when on snapshot).
    pub timestamp: i64,
}

/// One running (or recently active) terminal session reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct RuntimeProcessRef {
    /// Session id.
    pub session_id: String,
    /// Display title.
    pub title: String,
    /// Working directory when known.
    pub cwd: Option<String>,
    /// Last command preview.
    pub last_command: Option<String>,
    /// True when the PTY session is still alive.
    pub alive: bool,
}

/// Compact terminal output summary for conversational context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TerminalOutputSummary {
    /// Active terminal session id.
    pub active_session_id: Option<String>,
    /// Number of sessions observed.
    pub session_count: usize,
    /// Number of alive sessions.
    pub alive_count: usize,
    /// Last command across sessions (most recent first preference).
    pub last_command: Option<String>,
    /// Capped tail of the active (or first) session output.
    pub output_tail: String,
}

/// Intelligence subset for ContextBundle contribution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeIntelligenceSection {
    /// Latest cargo check summary, when any.
    pub latest_cargo_check: Option<String>,
    /// Latest build summary, when any.
    pub latest_build: Option<String>,
    /// Latest tests summary, when any.
    pub latest_tests: Option<String>,
    /// Terminal session count.
    pub session_count: usize,
    /// Alive session count.
    pub alive_count: usize,
    /// Last terminal command.
    pub last_command: Option<String>,
    /// Terminal output tail (capped).
    pub output_tail: String,
    /// Running session titles / last commands (capped).
    pub running: Vec<String>,
    /// Recent failure summaries (capped).
    pub recent_failures: Vec<String>,
}

/// Read-only runtime intelligence snapshot.
#[derive(Debug, Clone, Eq)]
pub struct RuntimeSnapshot {
    /// Latest cargo check outcome, when observed.
    pub latest_cargo_check: Option<RuntimeCommandOutcome>,
    /// Latest build outcome, when observed.
    pub latest_build: Option<RuntimeCommandOutcome>,
    /// Latest tests outcome, when observed.
    pub latest_tests: Option<RuntimeCommandOutcome>,
    /// Terminal output summary.
    pub terminal_summary: TerminalOutputSummary,
    /// Running / alive terminal sessions (capped).
    pub running_processes: Vec<RuntimeProcessRef>,
    /// Recent failures (capped; newest first).
    pub recent_failures: Vec<RuntimeCommandOutcome>,
    /// Unix seconds when captured (ignored by Eq/Hash).
    pub timestamp: i64,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            latest_cargo_check: None,
            latest_build: None,
            latest_tests: None,
            terminal_summary: TerminalOutputSummary::default(),
            running_processes: Vec::new(),
            recent_failures: Vec::new(),
            timestamp: 0,
        }
    }
}

impl PartialEq for RuntimeSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.latest_cargo_check == other.latest_cargo_check
            && self.latest_build == other.latest_build
            && self.latest_tests == other.latest_tests
            && self.terminal_summary == other.terminal_summary
            && self.running_processes == other.running_processes
            && self.recent_failures == other.recent_failures
    }
}

impl Hash for RuntimeSnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.latest_cargo_check.hash(state);
        self.latest_build.hash(state);
        self.latest_tests.hash(state);
        self.terminal_summary.hash(state);
        self.running_processes.hash(state);
        self.recent_failures.hash(state);
    }
}

/// One host-observed terminal session for ambient capture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeTerminalSessionFact {
    /// Session id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Working directory.
    pub cwd: Option<String>,
    /// Last command.
    pub last_command: Option<String>,
    /// Full or partial scrollback (will be capped).
    pub output: String,
    /// Command history (oldest first).
    pub history: Vec<String>,
    /// Whether the PTY is alive (from TerminalProvider when known).
    pub alive: bool,
}

/// Host facts for ambient runtime observation (no cargo re-run).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeSnapshotHostFacts {
    /// Active terminal session id.
    pub active_session_id: Option<String>,
    /// Coding / TerminalProvider session observations.
    pub sessions: Vec<RuntimeTerminalSessionFact>,
}

/// Host-observed parts for building a [`RuntimeSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeSnapshotObservation {
    /// Latest cargo check.
    pub latest_cargo_check: Option<RuntimeCommandOutcome>,
    /// Latest build.
    pub latest_build: Option<RuntimeCommandOutcome>,
    /// Latest tests.
    pub latest_tests: Option<RuntimeCommandOutcome>,
    /// Terminal summary.
    pub terminal_summary: TerminalOutputSummary,
    /// Running sessions.
    pub running_processes: Vec<RuntimeProcessRef>,
    /// Recent failures.
    pub recent_failures: Vec<RuntimeCommandOutcome>,
    /// Optional capture timestamp override.
    pub timestamp: Option<i64>,
}

impl RuntimeSnapshot {
    /// Empty observational snapshot.
    pub fn empty() -> Self {
        Self {
            timestamp: now_unix_secs(),
            ..Self::default()
        }
    }

    /// Build from host-observed parts (no tools / reasoning / assemble).
    pub fn from_observation(parts: RuntimeSnapshotObservation) -> Self {
        Self {
            latest_cargo_check: parts.latest_cargo_check,
            latest_build: parts.latest_build,
            latest_tests: parts.latest_tests,
            terminal_summary: parts.terminal_summary,
            running_processes: parts.running_processes,
            recent_failures: parts.recent_failures,
            timestamp: parts.timestamp.unwrap_or_else(now_unix_secs),
        }
    }

    /// True when any runtime intelligence was observed.
    pub fn has_intelligence(&self) -> bool {
        self.latest_cargo_check.is_some()
            || self.latest_build.is_some()
            || self.latest_tests.is_some()
            || self.terminal_summary.session_count > 0
            || !self.running_processes.is_empty()
            || !self.recent_failures.is_empty()
            || !self.terminal_summary.output_tail.is_empty()
            || self.terminal_summary.last_command.is_some()
    }

    /// Intelligence subset for ContextBundle contribution.
    pub fn intelligence_section(&self) -> RuntimeIntelligenceSection {
        RuntimeIntelligenceSection {
            latest_cargo_check: self
                .latest_cargo_check
                .as_ref()
                .map(format_outcome_summary),
            latest_build: self.latest_build.as_ref().map(format_outcome_summary),
            latest_tests: self.latest_tests.as_ref().map(format_outcome_summary),
            session_count: self.terminal_summary.session_count,
            alive_count: self.terminal_summary.alive_count,
            last_command: self.terminal_summary.last_command.clone(),
            output_tail: self.terminal_summary.output_tail.clone(),
            running: self
                .running_processes
                .iter()
                .take(8)
                .map(|proc| {
                    let cmd = proc
                        .last_command
                        .as_deref()
                        .unwrap_or("(idle)");
                    format!("{} · {} · {}", proc.title, if proc.alive { "alive" } else { "dead" }, cmd)
                })
                .collect(),
            recent_failures: self
                .recent_failures
                .iter()
                .take(8)
                .map(format_outcome_summary)
                .collect(),
        }
    }
}

/// Observe runtime intelligence from host terminal facts (ambient only).
///
/// Classifies recent commands heuristically. Never re-runs cargo / tests.
/// **Must not** be called from Context providers during assemble.
pub fn observe_runtime_intelligence(facts: &RuntimeSnapshotHostFacts) -> RuntimeSnapshot {
    let mut latest_check = None;
    let mut latest_build = None;
    let mut latest_tests = None;
    let mut recent_failures = Vec::new();
    let mut running = Vec::new();

    for session in &facts.sessions {
        if session.alive || session.last_command.is_some() || !session.output.is_empty() {
            running.push(RuntimeProcessRef {
                session_id: session.id.clone(),
                title: session.title.clone(),
                cwd: session.cwd.clone(),
                last_command: session.last_command.clone(),
                alive: session.alive,
            });
        }

        // Prefer history (oldest→newest); fall back to last_command.
        let mut commands: Vec<&str> = session
            .history
            .iter()
            .map(String::as_str)
            .collect();
        if commands.is_empty() {
            if let Some(cmd) = session.last_command.as_deref() {
                commands.push(cmd);
            }
        }

        for command in commands {
            let kind = classify_command(command);
            let excerpt = truncate_tail(&session.output, 480);
            let failed = looks_like_failure(&session.output, command);
            let ok = !failed;
            let outcome = RuntimeCommandOutcome {
                command: command.trim().to_string(),
                kind,
                exit_code: None,
                ok,
                summary: outcome_summary(kind, command, ok, &excerpt),
                output_excerpt: excerpt,
                session_id: Some(session.id.clone()),
                cwd: session.cwd.clone(),
                timestamp: now_unix_secs(),
            };

            match kind {
                RuntimeCommandKind::Check => latest_check = Some(outcome.clone()),
                RuntimeCommandKind::Build => latest_build = Some(outcome.clone()),
                RuntimeCommandKind::Test => latest_tests = Some(outcome.clone()),
                RuntimeCommandKind::Other => {}
            }
            if !ok {
                recent_failures.push(outcome);
            }
        }
    }

    // Newest failures first; cap.
    recent_failures.reverse();
    recent_failures.truncate(8);
    running.truncate(16);

    let alive_count = running.iter().filter(|p| p.alive).count();
    let active = facts
        .active_session_id
        .as_ref()
        .and_then(|id| facts.sessions.iter().find(|s| &s.id == id))
        .or_else(|| facts.sessions.first());
    let last_command = active
        .and_then(|s| s.last_command.clone())
        .or_else(|| {
            facts
                .sessions
                .iter()
                .rev()
                .find_map(|s| s.last_command.clone())
        });
    let output_tail = active
        .map(|s| truncate_tail(&s.output, 640))
        .unwrap_or_default();

    RuntimeSnapshot::from_observation(RuntimeSnapshotObservation {
        latest_cargo_check: latest_check,
        latest_build,
        latest_tests,
        terminal_summary: TerminalOutputSummary {
            active_session_id: facts.active_session_id.clone(),
            session_count: facts.sessions.len(),
            alive_count,
            last_command,
            output_tail,
        },
        running_processes: running,
        recent_failures,
        timestamp: None,
    })
}

fn classify_command(command: &str) -> RuntimeCommandKind {
    let lower = command.to_ascii_lowercase();
    let trimmed = lower.trim();
    if trimmed.contains("cargo check")
        || trimmed.contains("cargo clippy")
        || trimmed.starts_with("tsc")
        || trimmed.contains("npm run typecheck")
        || trimmed.contains("pnpm typecheck")
    {
        return RuntimeCommandKind::Check;
    }
    if trimmed.contains("cargo test")
        || trimmed.contains("cargo nextest")
        || trimmed.contains("npm test")
        || trimmed.contains("pnpm test")
        || trimmed.contains("yarn test")
        || trimmed.contains("pytest")
        || trimmed.contains("go test")
    {
        return RuntimeCommandKind::Test;
    }
    if trimmed.contains("cargo build")
        || trimmed.contains("cargo run")
        || trimmed.contains("npm run build")
        || trimmed.contains("pnpm build")
        || trimmed.contains("yarn build")
        || trimmed.contains("cmake --build")
        || trimmed.contains("make ")
        || trimmed == "make"
    {
        return RuntimeCommandKind::Build;
    }
    RuntimeCommandKind::Other
}

fn looks_like_failure(output: &str, command: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    if lower.contains("error: could not compile")
        || lower.contains("error[e")
        || lower.contains("build failed")
        || lower.contains("test result: failed")
        || lower.contains("failures:")
        || lower.contains("panic!")
        || lower.contains("command not found")
        || lower.contains("exited with code 1")
        || lower.contains("exited with code 101")
    {
        return true;
    }
    // Soft signal: cargo check/build/test with "error:" lines.
    let kind = classify_command(command);
    if matches!(
        kind,
        RuntimeCommandKind::Check | RuntimeCommandKind::Build | RuntimeCommandKind::Test
    ) && lower.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("error:") || t.starts_with("error[")
    }) {
        return true;
    }
    false
}

fn outcome_summary(kind: RuntimeCommandKind, command: &str, ok: bool, excerpt: &str) -> String {
    let status = if ok { "ok" } else { "failed" };
    let cmd = truncate_chars(command.trim(), 80);
    if excerpt.is_empty() {
        format!("{} · {} · {}", kind.as_str(), status, cmd)
    } else {
        let tail = excerpt.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        format!(
            "{} · {} · {} · {}",
            kind.as_str(),
            status,
            cmd,
            truncate_chars(tail, 60)
        )
    }
}

fn format_outcome_summary(outcome: &RuntimeCommandOutcome) -> String {
    outcome.summary.clone()
}

fn truncate_tail(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim_end();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut rev = trimmed.chars().rev().take(max_chars).collect::<Vec<_>>();
    rev.reverse();
    let mut out = String::from("…");
    out.extend(rev);
    out
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_has_no_intelligence() {
        let snap = RuntimeSnapshot::empty();
        assert!(!snap.has_intelligence());
    }

    #[test]
    fn observes_cargo_check_and_failure() {
        let snap = observe_runtime_intelligence(&RuntimeSnapshotHostFacts {
            active_session_id: Some("t1".into()),
            sessions: vec![RuntimeTerminalSessionFact {
                id: "t1".into(),
                title: "Terminal".into(),
                cwd: Some("/proj".into()),
                last_command: Some("cargo check".into()),
                output: "error[E0308]: mismatched types\nerror: could not compile `demo`\n".into(),
                history: vec!["cargo check".into()],
                alive: true,
            }],
        });
        assert!(snap.has_intelligence());
        let check = snap.latest_cargo_check.as_ref().expect("check");
        assert_eq!(check.kind, RuntimeCommandKind::Check);
        assert!(!check.ok);
        assert_eq!(snap.recent_failures.len(), 1);
        assert_eq!(snap.running_processes.len(), 1);
        assert!(snap.running_processes[0].alive);
        let section = snap.intelligence_section();
        assert!(section.latest_cargo_check.is_some());
        assert!(!section.recent_failures.is_empty());
    }

    #[test]
    fn classifies_build_and_test() {
        assert_eq!(classify_command("cargo build --release"), RuntimeCommandKind::Build);
        assert_eq!(classify_command("cargo test -p jaymi"), RuntimeCommandKind::Test);
        assert_eq!(classify_command("ls"), RuntimeCommandKind::Other);
    }

    #[test]
    fn snapshot_ignores_timestamp_for_equality() {
        let mut a = observe_runtime_intelligence(&RuntimeSnapshotHostFacts {
            sessions: vec![RuntimeTerminalSessionFact {
                id: "t1".into(),
                title: "Terminal".into(),
                last_command: Some("ls".into()),
                alive: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let mut b = a.clone();
        a.timestamp = 1;
        b.timestamp = 99;
        assert_eq!(a, b);
    }
}
