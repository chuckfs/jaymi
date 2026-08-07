//! Canonical workspace activity memory observation (Sprint B2.9).
//!
//! [`WorkspaceMemorySnapshot`] remembers Coding workspace activity:
//! recent edits, recently opened files, recent builds, recent failures, and
//! the current coding objective.
//!
//! It is **distinct from Conversation Memory** (Memory Engine retrieve /
//! promote). Context Policy decides when this feed participates in a
//! [`crate::ContextBundle`].
//!
//! Observational only:
//!
//! * executes no tools
//! * performs no reasoning
//! * owns no policy
//! * never builds a [`crate::ContextBundle`]
//! * never talks to an LLM
//! * never writes Conversation / Project / Personal memories

use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Cap for paths and command summaries in the contribution section.
pub const WORKSPACE_MEMORY_SECTION_PATH_CAP: usize = 8;
/// Cap for builds / failures in the contribution section.
pub const WORKSPACE_MEMORY_SECTION_COMMAND_CAP: usize = 6;

/// One remembered path with observation time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct WorkspaceMemoryPath {
    /// Path string.
    pub path: String,
    /// Unix seconds when recorded.
    pub timestamp: i64,
}

/// One remembered build / failure summary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct WorkspaceMemoryCommand {
    /// Command line when known.
    pub command: String,
    /// Short human summary.
    pub summary: String,
    /// True when believed successful.
    pub ok: bool,
    /// Unix seconds when recorded.
    pub timestamp: i64,
}

/// Intelligence subset for ContextBundle contribution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceMemorySection {
    /// Current coding objective, when set.
    pub coding_objective: Option<String>,
    /// Recent edit paths (newest first, capped).
    pub recent_edits: Vec<String>,
    /// Recently opened paths (newest first, capped).
    pub recently_opened: Vec<String>,
    /// Recent build summaries (newest first, capped).
    pub recent_builds: Vec<String>,
    /// Recent failure summaries (newest first, capped).
    pub recent_failures: Vec<String>,
}

/// Host-observed workspace activity facts for observation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceMemoryHostFacts {
    /// Current coding objective.
    pub coding_objective: Option<String>,
    /// Recent edits.
    pub recent_edits: Vec<WorkspaceMemoryPath>,
    /// Recently opened files (from editor MRU).
    pub recently_opened: Vec<String>,
    /// Recent builds.
    pub recent_builds: Vec<WorkspaceMemoryCommand>,
    /// Recent failures.
    pub recent_failures: Vec<WorkspaceMemoryCommand>,
}

/// Read-only workspace memory snapshot.
#[derive(Debug, Clone, Eq)]
pub struct WorkspaceMemorySnapshot {
    /// Current coding objective.
    pub coding_objective: Option<String>,
    /// Recent edits (newest first).
    pub recent_edits: Vec<WorkspaceMemoryPath>,
    /// Recently opened files (newest first).
    pub recently_opened: Vec<String>,
    /// Recent builds (newest first).
    pub recent_builds: Vec<WorkspaceMemoryCommand>,
    /// Recent failures (newest first).
    pub recent_failures: Vec<WorkspaceMemoryCommand>,
    /// Unix seconds when captured (ignored by Eq/Hash).
    pub timestamp: i64,
}

impl Default for WorkspaceMemorySnapshot {
    fn default() -> Self {
        Self::empty()
    }
}

impl PartialEq for WorkspaceMemorySnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.coding_objective == other.coding_objective
            && self.recent_edits == other.recent_edits
            && self.recently_opened == other.recently_opened
            && self.recent_builds == other.recent_builds
            && self.recent_failures == other.recent_failures
    }
}

impl Hash for WorkspaceMemorySnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.coding_objective.hash(state);
        self.recent_edits.hash(state);
        self.recently_opened.hash(state);
        self.recent_builds.hash(state);
        self.recent_failures.hash(state);
    }
}

impl WorkspaceMemorySnapshot {
    /// Empty snapshot (no activity).
    pub fn empty() -> Self {
        Self {
            coding_objective: None,
            recent_edits: Vec::new(),
            recently_opened: Vec::new(),
            recent_builds: Vec::new(),
            recent_failures: Vec::new(),
            timestamp: now_unix(),
        }
    }

    /// True when any activity worth contributing is present.
    pub fn has_memory(&self) -> bool {
        self.coding_objective
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
            || !self.recent_edits.is_empty()
            || !self.recently_opened.is_empty()
            || !self.recent_builds.is_empty()
            || !self.recent_failures.is_empty()
    }

    /// Capped section for ContextBundle / LLM contribution.
    pub fn memory_section(&self) -> WorkspaceMemorySection {
        WorkspaceMemorySection {
            coding_objective: self.coding_objective.clone(),
            recent_edits: self
                .recent_edits
                .iter()
                .take(WORKSPACE_MEMORY_SECTION_PATH_CAP)
                .map(|e| e.path.clone())
                .collect(),
            recently_opened: self
                .recently_opened
                .iter()
                .take(WORKSPACE_MEMORY_SECTION_PATH_CAP)
                .cloned()
                .collect(),
            recent_builds: self
                .recent_builds
                .iter()
                .take(WORKSPACE_MEMORY_SECTION_COMMAND_CAP)
                .map(format_command)
                .collect(),
            recent_failures: self
                .recent_failures
                .iter()
                .take(WORKSPACE_MEMORY_SECTION_COMMAND_CAP)
                .map(format_command)
                .collect(),
        }
    }
}

/// Observe workspace memory from host facts (no I/O, no Memory Engine).
pub fn observe_workspace_memory(facts: WorkspaceMemoryHostFacts) -> WorkspaceMemorySnapshot {
    WorkspaceMemorySnapshot {
        coding_objective: facts.coding_objective.and_then(|s| {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }),
        recent_edits: facts.recent_edits,
        recently_opened: facts.recently_opened,
        recent_builds: facts.recent_builds,
        recent_failures: facts.recent_failures,
        timestamp: now_unix(),
    }
}

fn format_command(cmd: &WorkspaceMemoryCommand) -> String {
    if !cmd.summary.trim().is_empty() {
        if cmd.command.trim().is_empty() {
            cmd.summary.clone()
        } else {
            format!("{} — {}", cmd.command, cmd.summary)
        }
    } else if !cmd.command.trim().is_empty() {
        let status = if cmd.ok { "ok" } else { "failed" };
        format!("{} ({status})", cmd.command)
    } else {
        "command".into()
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_has_no_memory() {
        assert!(!WorkspaceMemorySnapshot::empty().has_memory());
    }

    #[test]
    fn observe_copies_host_facts() {
        let snap = observe_workspace_memory(WorkspaceMemoryHostFacts {
            coding_objective: Some("fix the borrow checker".into()),
            recent_edits: vec![WorkspaceMemoryPath {
                path: "/proj/main.rs".into(),
                timestamp: 1,
            }],
            recently_opened: vec!["/proj/lib.rs".into()],
            recent_builds: vec![WorkspaceMemoryCommand {
                command: "cargo build".into(),
                summary: "ok".into(),
                ok: true,
                timestamp: 2,
            }],
            recent_failures: vec![WorkspaceMemoryCommand {
                command: "cargo test".into(),
                summary: "failed".into(),
                ok: false,
                timestamp: 3,
            }],
        });
        assert!(snap.has_memory());
        let section = snap.memory_section();
        assert_eq!(
            section.coding_objective.as_deref(),
            Some("fix the borrow checker")
        );
        assert_eq!(section.recent_edits, vec!["/proj/main.rs".to_string()]);
        assert_eq!(section.recently_opened, vec!["/proj/lib.rs".to_string()]);
        assert!(!section.recent_builds.is_empty());
        assert!(!section.recent_failures.is_empty());
    }

    #[test]
    fn equality_ignores_timestamp() {
        let mut a = observe_workspace_memory(WorkspaceMemoryHostFacts {
            coding_objective: Some("ship B2.9".into()),
            ..Default::default()
        });
        let mut b = a.clone();
        a.timestamp = 1;
        b.timestamp = 99;
        assert_eq!(a, b);
    }
}
