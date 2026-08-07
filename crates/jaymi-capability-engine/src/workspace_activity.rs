//! Workspace activity memory owned by CodingState (Sprint B2.9).
//!
//! Distinct from Conversation Memory (Memory Engine). This ring buffer tracks
//! recent edits, opens, builds, failures, and the current coding objective for
//! the Coding workspace session. Context Policy decides when it enters a
//! ContextBundle.

use std::time::{SystemTime, UNIX_EPOCH};

/// Cap for recent edit / open paths.
pub const WORKSPACE_ACTIVITY_PATH_CAP: usize = 12;
/// Cap for recent build / failure command summaries.
pub const WORKSPACE_ACTIVITY_COMMAND_CAP: usize = 8;

/// One remembered path with observation time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceActivityPath {
    /// Absolute or project-relative path.
    pub path: String,
    /// Unix seconds when recorded.
    pub timestamp: i64,
}

/// One remembered build / command outcome summary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceActivityCommand {
    /// Command line when known.
    pub command: String,
    /// Short human summary.
    pub summary: String,
    /// True when believed successful.
    pub ok: bool,
    /// Unix seconds when recorded.
    pub timestamp: i64,
}

/// Session-scoped workspace activity memory (not Conversation Memory).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct WorkspaceActivityState {
    /// Recently edited files (newest first).
    pub recent_edits: Vec<WorkspaceActivityPath>,
    /// Recent build / check / test outcomes (newest first).
    pub recent_builds: Vec<WorkspaceActivityCommand>,
    /// Recent failed commands (newest first).
    pub recent_failures: Vec<WorkspaceActivityCommand>,
    /// Current coding objective when the host has set one.
    pub coding_objective: Option<String>,
}

impl WorkspaceActivityState {
    /// True when any activity or objective is present.
    pub fn has_activity(&self) -> bool {
        self.coding_objective
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
            || !self.recent_edits.is_empty()
            || !self.recent_builds.is_empty()
            || !self.recent_failures.is_empty()
    }

    /// Record a path edit (MRU; capped).
    pub fn record_edit(&mut self, path: &str) {
        if path.trim().is_empty() {
            return;
        }
        let ts = now_unix();
        self.recent_edits.retain(|entry| entry.path != path);
        self.recent_edits.insert(
            0,
            WorkspaceActivityPath {
                path: path.to_string(),
                timestamp: ts,
            },
        );
        self.recent_edits.truncate(WORKSPACE_ACTIVITY_PATH_CAP);
    }

    /// Record a build / check / test outcome.
    pub fn record_build(&mut self, command: &str, summary: &str, ok: bool) {
        let ts = now_unix();
        let entry = WorkspaceActivityCommand {
            command: command.to_string(),
            summary: summary.to_string(),
            ok,
            timestamp: ts,
        };
        self.recent_builds.insert(0, entry.clone());
        self.recent_builds.truncate(WORKSPACE_ACTIVITY_COMMAND_CAP);
        if !ok {
            self.recent_failures.insert(0, entry);
            self.recent_failures
                .truncate(WORKSPACE_ACTIVITY_COMMAND_CAP);
        }
    }

    /// Record a failure that is not necessarily a "build".
    pub fn record_failure(&mut self, command: &str, summary: &str) {
        let ts = now_unix();
        self.recent_failures.insert(
            0,
            WorkspaceActivityCommand {
                command: command.to_string(),
                summary: summary.to_string(),
                ok: false,
                timestamp: ts,
            },
        );
        self.recent_failures
            .truncate(WORKSPACE_ACTIVITY_COMMAND_CAP);
    }

    /// Set or clear the current coding objective.
    pub fn set_coding_objective(&mut self, objective: Option<String>) {
        self.coding_objective = objective.and_then(|s| {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
    }

    /// Clear all activity (workspace / coding close).
    pub fn clear(&mut self) {
        *self = Self::default();
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
    fn records_edits_mru_and_cap() {
        let mut activity = WorkspaceActivityState::default();
        activity.record_edit("/a.rs");
        activity.record_edit("/b.rs");
        activity.record_edit("/a.rs");
        assert_eq!(activity.recent_edits[0].path, "/a.rs");
        assert_eq!(activity.recent_edits.len(), 2);
        for i in 0..20 {
            activity.record_edit(&format!("/f{i}.rs"));
        }
        assert_eq!(activity.recent_edits.len(), WORKSPACE_ACTIVITY_PATH_CAP);
    }

    #[test]
    fn failed_build_also_records_failure() {
        let mut activity = WorkspaceActivityState::default();
        activity.record_build("cargo build", "build failed", false);
        assert_eq!(activity.recent_builds.len(), 1);
        assert_eq!(activity.recent_failures.len(), 1);
        assert!(!activity.recent_builds[0].ok);
    }
}
