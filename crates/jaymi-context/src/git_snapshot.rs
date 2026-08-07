//! Canonical read-only Git intelligence observation (Sprint B2.5).
//!
//! [`GitSnapshot`] is the immutable representation of working-tree Git state:
//! branch, HEAD, dirty / staged / untracked / conflict paths, and recent commits.
//!
//! It is observational only:
//!
//! * executes no tools
//! * performs no reasoning
//! * owns no policy
//! * never builds a [`crate::ContextBundle`]
//! * never talks to an LLM
//!
//! ## Ownership
//!
//! | Role | Owner |
//! |------|--------|
//! | Orchestration (when to assemble) | Planner (via Application host prep) |
//! | Ambient refresh / git CLI | Application `ContextMaintenance` (read-only `GitProvider`) |
//! | Observation contract | [`GitSnapshot`] |
//! | Consumption | Context providers (`GitStatusProvider`) — session only |
//! | Request / Reasoning path | **Must not** run git commands |
//!
//! Distinct from [`crate::ProjectSnapshot`] repository metadata (cheap `.git`
//! markers) and from Coding `GitStatusState` (UI dock SoT).

use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::GitCommitSummary;

use crate::GitStatusSection;

/// One path entry in a GitSnapshot path list (capped at capture time).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct GitPathEntry {
    /// Repository-relative path.
    pub path: String,
    /// Short status label (`M`, `A`, `UU`, `??`, …).
    pub status: String,
}

/// Read-only Git intelligence snapshot.
#[derive(Debug, Clone, Eq)]
pub struct GitSnapshot {
    /// Whether the root is inside a Git work tree.
    pub is_repository: bool,
    /// Absolute repository toplevel, when known.
    pub repo_root: Option<String>,
    /// Current branch name, when known.
    pub branch: Option<String>,
    /// Full HEAD object name.
    pub head_sha: Option<String>,
    /// Abbreviated HEAD.
    pub head_short: Option<String>,
    /// Short human-readable summary (`clean`, `2 modified`, …).
    pub summary: String,
    /// Dirty / unstaged worktree paths (capped).
    pub dirty: Vec<GitPathEntry>,
    /// Staged index paths (capped).
    pub staged: Vec<GitPathEntry>,
    /// Untracked paths (capped).
    pub untracked: Vec<GitPathEntry>,
    /// Merge conflict / unmerged paths (capped).
    pub conflicts: Vec<GitPathEntry>,
    /// Recent commits (capped; newest first).
    pub recent_commits: Vec<GitCommitSummary>,
    /// Unix seconds when captured (ignored by Eq/Hash).
    pub timestamp: i64,
}

impl Default for GitSnapshot {
    fn default() -> Self {
        Self {
            is_repository: false,
            repo_root: None,
            branch: None,
            head_sha: None,
            head_short: None,
            summary: String::new(),
            dirty: Vec::new(),
            staged: Vec::new(),
            untracked: Vec::new(),
            conflicts: Vec::new(),
            recent_commits: Vec::new(),
            timestamp: 0,
        }
    }
}

impl PartialEq for GitSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.is_repository == other.is_repository
            && self.repo_root == other.repo_root
            && self.branch == other.branch
            && self.head_sha == other.head_sha
            && self.head_short == other.head_short
            && self.summary == other.summary
            && self.dirty == other.dirty
            && self.staged == other.staged
            && self.untracked == other.untracked
            && self.conflicts == other.conflicts
            && self.recent_commits == other.recent_commits
    }
}

impl Hash for GitSnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.is_repository.hash(state);
        self.repo_root.hash(state);
        self.branch.hash(state);
        self.head_sha.hash(state);
        self.head_short.hash(state);
        self.summary.hash(state);
        self.dirty.hash(state);
        self.staged.hash(state);
        self.untracked.hash(state);
        self.conflicts.hash(state);
        self.recent_commits.hash(state);
    }
}

/// Host-observed parts for building a [`GitSnapshot`] (no git CLI here).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitSnapshotObservation {
    /// Whether the root is a Git work tree.
    pub is_repository: bool,
    /// Absolute repository toplevel.
    pub repo_root: Option<String>,
    /// Current branch.
    pub branch: Option<String>,
    /// Full HEAD SHA.
    pub head_sha: Option<String>,
    /// Short HEAD SHA.
    pub head_short: Option<String>,
    /// Summary label.
    pub summary: String,
    /// Dirty paths.
    pub dirty: Vec<GitPathEntry>,
    /// Staged paths.
    pub staged: Vec<GitPathEntry>,
    /// Untracked paths.
    pub untracked: Vec<GitPathEntry>,
    /// Conflict paths.
    pub conflicts: Vec<GitPathEntry>,
    /// Recent commits.
    pub recent_commits: Vec<GitCommitSummary>,
    /// Optional capture timestamp override.
    pub timestamp: Option<i64>,
}

impl GitSnapshot {
    /// Empty observational snapshot.
    pub fn empty() -> Self {
        Self {
            timestamp: now_unix_secs(),
            ..Self::default()
        }
    }

    /// Build from host-observed parts (no tools / reasoning / assemble).
    pub fn from_observation(parts: GitSnapshotObservation) -> Self {
        Self {
            is_repository: parts.is_repository,
            repo_root: parts.repo_root,
            branch: parts.branch,
            head_sha: parts.head_sha,
            head_short: parts.head_short,
            summary: parts.summary,
            dirty: parts.dirty,
            staged: parts.staged,
            untracked: parts.untracked,
            conflicts: parts.conflicts,
            recent_commits: parts.recent_commits,
            timestamp: parts.timestamp.unwrap_or_else(now_unix_secs),
        }
    }

    /// True when any Git intelligence beyond “empty” was observed.
    pub fn has_intelligence(&self) -> bool {
        self.is_repository
            || self.branch.is_some()
            || self.head_sha.is_some()
            || !self.summary.is_empty()
            || !self.dirty.is_empty()
            || !self.staged.is_empty()
            || !self.untracked.is_empty()
            || !self.conflicts.is_empty()
            || !self.recent_commits.is_empty()
    }

    /// Compact conversational section derived from this snapshot.
    pub fn status_section(&self) -> GitStatusSection {
        let mut sample_paths = Vec::new();
        for path in self
            .conflicts
            .iter()
            .chain(self.dirty.iter())
            .chain(self.staged.iter())
            .chain(self.untracked.iter())
            .map(|entry| entry.path.clone())
        {
            if sample_paths.len() >= 8 {
                break;
            }
            if !sample_paths.contains(&path) {
                sample_paths.push(path);
            }
        }

        GitStatusSection {
            is_repository: self.is_repository,
            branch: self.branch.clone(),
            summary: self.summary.clone(),
            modified_count: self.dirty.len(),
            staged_count: self.staged.len(),
            untracked_count: self.untracked.len(),
            conflict_count: self.conflicts.len(),
            head_sha: self.head_sha.clone(),
            head_short: self.head_short.clone(),
            dirty_paths: self.dirty.iter().map(|e| e.path.clone()).take(16).collect(),
            staged_paths: self.staged.iter().map(|e| e.path.clone()).take(16).collect(),
            untracked_paths: self
                .untracked
                .iter()
                .map(|e| e.path.clone())
                .take(16)
                .collect(),
            conflict_paths: self
                .conflicts
                .iter()
                .map(|e| e.path.clone())
                .take(16)
                .collect(),
            recent_commits: self.recent_commits.iter().take(8).cloned().collect(),
            sample_paths,
        }
    }
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
        let snap = GitSnapshot::empty();
        assert!(!snap.has_intelligence());
        assert!(!snap.status_section().is_repository);
    }

    #[test]
    fn status_section_caps_and_counts() {
        let snap = GitSnapshot::from_observation(GitSnapshotObservation {
            is_repository: true,
            branch: Some("main".into()),
            head_sha: Some("abcdef0123456789".into()),
            head_short: Some("abcdef0".into()),
            summary: "1 modified, 1 conflict".into(),
            dirty: vec![GitPathEntry {
                path: "a.rs".into(),
                status: "M".into(),
            }],
            conflicts: vec![GitPathEntry {
                path: "b.rs".into(),
                status: "UU".into(),
            }],
            recent_commits: vec![GitCommitSummary {
                sha: "abcdef0123456789".into(),
                short_sha: "abcdef0".into(),
                subject: "init".into(),
                author: Some("jaymi".into()),
                relative_time: Some("2 days ago".into()),
            }],
            ..GitSnapshotObservation::default()
        });
        let section = snap.status_section();
        assert_eq!(section.modified_count, 1);
        assert_eq!(section.conflict_count, 1);
        assert_eq!(section.head_short.as_deref(), Some("abcdef0"));
        assert_eq!(section.recent_commits.len(), 1);
        assert!(section.sample_paths.contains(&"b.rs".into()));
    }

    #[test]
    fn snapshot_ignores_timestamp_for_equality() {
        let mut a = GitSnapshot::from_observation(GitSnapshotObservation {
            is_repository: true,
            branch: Some("main".into()),
            summary: "clean".into(),
            ..GitSnapshotObservation::default()
        });
        let mut b = a.clone();
        a.timestamp = 1;
        b.timestamp = 99;
        assert_eq!(a, b);
    }
}
