//! Git Provider — local repository status and mutation via the `git` CLI.
//!
//! Architecture path:
//! Planner → Code → Git Tool → Git Provider → `git`
//!
//! The Planner never shells out to git directly. Tools mediate all access.
//! Coding Workspace consumes Provider data only through that orchestration path.

use std::path::{Path, PathBuf};
use std::process::Command;

use jaymi_capabilities::Capability;
use jaymi_core::{GitCommitSummary, GitOperation, GitPathStatus, JaymiError, JaymiResult};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Provider ID used for registration and tool metadata.
pub const GIT_PROVIDER_ID: &str = "git";

/// Structured repository status returned by the Git provider.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatusSnapshot {
    /// Whether [`Self::repo_root`] is inside a Git work tree.
    pub is_repository: bool,
    /// Absolute repository root (toplevel), when detected.
    pub repo_root: PathBuf,
    /// Current branch name, when known.
    pub branch: Option<String>,
    /// Full HEAD object name, when known.
    pub head_sha: Option<String>,
    /// Abbreviated HEAD, when known.
    pub head_short: Option<String>,
    /// Short human-readable summary.
    pub summary: String,
    /// Unstaged worktree modifications (tracked files, not deletes / conflicts).
    pub modified: Vec<GitPathStatus>,
    /// Newly staged paths (index status `A`).
    pub added: Vec<GitPathStatus>,
    /// Deleted paths (worktree and/or index status `D`).
    pub deleted: Vec<GitPathStatus>,
    /// Staged index changes (includes added / staged deletes).
    pub staged: Vec<GitPathStatus>,
    /// Untracked paths.
    pub untracked: Vec<GitPathStatus>,
    /// Merge conflict / unmerged paths.
    pub conflicts: Vec<GitPathStatus>,
    /// Recent commits (newest first; capped).
    pub recent_commits: Vec<GitCommitSummary>,
}

impl GitStatusSnapshot {
    fn with_summary(mut self) -> Self {
        self.summary = if !self.is_repository {
            "not a git repository".to_string()
        } else {
            summarize(
                &self.modified,
                &self.added,
                &self.deleted,
                &self.staged,
                &self.untracked,
                &self.conflicts,
            )
        };
        self
    }
}

/// Local Git provider backed by the system `git` binary.
#[derive(Debug)]
pub struct GitProvider {
    identity: ProviderIdentity,
    initialized: bool,
}

impl GitProvider {
    /// Create an uninitialized git provider.
    pub fn new() -> Self {
        Self {
            identity: ProviderIdentity {
                id: GIT_PROVIDER_ID.to_string(),
                name: "Git".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Local Git repository status and mutations".to_string(),
                category: ProviderCategory::Local,
                author: "jaymi".to_string(),
                capabilities: vec![Capability::Code],
            },
            initialized: false,
        }
    }

    /// Returns true after [`Provider::initialize`] succeeds.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Detect whether `path` is inside a Git work tree (no mutations).
    ///
    /// Returns a snapshot with [`GitStatusSnapshot::is_repository`] set. When
    /// the path is a repository, status lists are populated; otherwise lists
    /// are empty and `summary` explains the miss.
    pub fn detect(&self, path: &Path) -> JaymiResult<GitStatusSnapshot> {
        self.require_initialized()?;
        detect_snapshot(path)
    }

    /// Run a Git operation and return refreshed status.
    ///
    /// [`GitOperation::Status`] soft-detects repositories (never errors solely
    /// because the folder is not a git work tree). Mutating operations still
    /// require a real repository.
    pub fn execute(
        &self,
        repo_root: &Path,
        operation: GitOperation,
        paths: &[PathBuf],
        message: Option<&str>,
    ) -> JaymiResult<GitStatusSnapshot> {
        self.require_initialized()?;

        if matches!(operation, GitOperation::Status) {
            let snapshot = detect_snapshot(repo_root)?;
            jaymi_logging::info(
                "providers",
                format!(
                    "git status path={} is_repo={} branch={:?} modified={} added={} deleted={} staged={} untracked={}",
                    repo_root.display(),
                    snapshot.is_repository,
                    snapshot.branch,
                    snapshot.modified.len(),
                    snapshot.added.len(),
                    snapshot.deleted.len(),
                    snapshot.staged.len(),
                    snapshot.untracked.len()
                ),
            );
            return Ok(snapshot);
        }

        let repo = normalize_repo(repo_root)?;

        match operation {
            GitOperation::Status => unreachable!("handled above"),
            GitOperation::Stage => {
                ensure_paths(paths, "stage")?;
                run_git(&repo, &["add", "--"], paths)?;
            }
            GitOperation::Unstage => {
                ensure_paths(paths, "unstage")?;
                unstage_paths(&repo, paths)?;
            }
            GitOperation::Discard => {
                ensure_paths(paths, "discard")?;
                discard_paths(&repo, paths)?;
            }
            GitOperation::Commit => {
                let message = message
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| JaymiError::new("commit requires a message"))?;
                run_git_commit(&repo, message)?;
            }
        }

        let snapshot = status_snapshot(&repo)?;
        jaymi_logging::info(
            "providers",
            format!(
                "git {} repo={} branch={:?} modified={} added={} deleted={} staged={} untracked={}",
                operation.as_str(),
                repo.display(),
                snapshot.branch,
                snapshot.modified.len(),
                snapshot.added.len(),
                snapshot.deleted.len(),
                snapshot.staged.len(),
                snapshot.untracked.len()
            ),
        );
        Ok(snapshot)
    }

    /// Convenience: repository status / detection only.
    pub fn status(&self, repo_root: &Path) -> JaymiResult<GitStatusSnapshot> {
        self.execute(repo_root, GitOperation::Status, &[], None)
    }

    fn require_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("git provider is not initialized"))
        }
    }
}

impl Default for GitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GitProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> JaymiResult<()> {
        self.require_initialized()?;
        let output = Command::new("git")
            .args(["--version"])
            .output()
            .map_err(|error| JaymiError::new(format!("git is not available: {error}")))?;
        if !output.status.success() {
            return Err(JaymiError::new("git --version failed"));
        }
        Ok(())
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

fn detect_snapshot(path: &Path) -> JaymiResult<GitStatusSnapshot> {
    if path.as_os_str().is_empty() {
        return Err(JaymiError::new("git repo root must not be empty"));
    }
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => {
            return Ok(GitStatusSnapshot {
                is_repository: false,
                repo_root: path.to_path_buf(),
                summary: format!("cannot access path: {error}"),
                ..GitStatusSnapshot::default()
            }
            .with_summary());
        }
    };
    if !meta.is_dir() {
        return Ok(GitStatusSnapshot {
            is_repository: false,
            repo_root: path.to_path_buf(),
            summary: "path is not a directory".into(),
            ..GitStatusSnapshot::default()
        }
        .with_summary());
    }

    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match resolve_toplevel(&canonical) {
        Ok(repo) => status_snapshot(&repo),
        Err(_) => Ok(GitStatusSnapshot {
            is_repository: false,
            repo_root: canonical,
            summary: "not a git repository".into(),
            ..GitStatusSnapshot::default()
        }
        .with_summary()),
    }
}

fn normalize_repo(path: &Path) -> JaymiResult<PathBuf> {
    let snapshot = detect_snapshot(path)?;
    if snapshot.is_repository {
        Ok(snapshot.repo_root)
    } else {
        Err(JaymiError::new(format!(
            "not a git repository: {}",
            path.display()
        )))
    }
}

fn resolve_toplevel(path: &Path) -> JaymiResult<PathBuf> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| JaymiError::new(format!("failed to probe git repo: {error}")))?;
    if !output.status.success() {
        return Err(JaymiError::new(format!(
            "not a git repository: {}",
            path.display()
        )));
    }
    let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if toplevel.is_empty() {
        return Err(JaymiError::new(format!(
            "not a git repository: {}",
            path.display()
        )));
    }
    Ok(PathBuf::from(toplevel))
}

fn ensure_paths(paths: &[PathBuf], operation: &str) -> JaymiResult<()> {
    if paths.is_empty() {
        return Err(JaymiError::new(format!(
            "git {operation} requires at least one path"
        )));
    }
    Ok(())
}

fn run_git(repo: &Path, args_prefix: &[&str], paths: &[PathBuf]) -> JaymiResult<()> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo);
    for arg in args_prefix {
        command.arg(arg);
    }
    for path in paths {
        command.arg(path);
    }
    let output = command
        .output()
        .map_err(|error| JaymiError::new(format!("failed to run git: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(JaymiError::new(format!(
            "git {} failed: {}",
            args_prefix.first().copied().unwrap_or("command"),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn run_git_commit(repo: &Path, message: &str) -> JaymiResult<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=Jaymi",
            "-c",
            "user.email=jaymi@local",
            "commit",
            "-m",
            message,
        ])
        .output()
        .map_err(|error| JaymiError::new(format!("failed to run git commit: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(JaymiError::new(format!(
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn unstage_paths(repo: &Path, paths: &[PathBuf]) -> JaymiResult<()> {
    // `git restore --staged` needs HEAD. Brand-new repos (no commits) must use
    // `git rm --cached` to remove paths from the index.
    match run_git(repo, &["restore", "--staged", "--"], paths) {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = error.message().to_ascii_lowercase();
            if message.contains("could not resolve")
                || message.contains("needed a single revision")
                || message.contains("unknown revision")
            {
                run_git(repo, &["rm", "--cached", "-q", "--"], paths)
            } else {
                Err(error)
            }
        }
    }
}

fn discard_paths(repo: &Path, paths: &[PathBuf]) -> JaymiResult<()> {
    let snapshot = status_snapshot(repo)?;
    let untracked: std::collections::HashSet<&str> = snapshot
        .untracked
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    let deleted: std::collections::HashSet<&str> = snapshot
        .deleted
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();

    let mut tracked = Vec::new();
    let mut clean = Vec::new();
    for path in paths {
        let key = path.to_string_lossy();
        if untracked.contains(key.as_ref()) {
            clean.push(path.clone());
        } else if deleted.contains(key.as_ref()) {
            // Restoring a deleted worktree path brings the file back.
            tracked.push(path.clone());
        } else {
            tracked.push(path.clone());
        }
    }

    if !tracked.is_empty() {
        run_git(repo, &["restore", "--"], &tracked)?;
    }
    if !clean.is_empty() {
        run_git(repo, &["clean", "-f", "--"], &clean)?;
    }
    Ok(())
}

fn status_snapshot(repo: &Path) -> JaymiResult<GitStatusSnapshot> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain=v1", "-b"])
        .output()
        .map_err(|error| JaymiError::new(format!("failed to run git status: {error}")))?;
    if !output.status.success() {
        return Err(JaymiError::new(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut snapshot = parse_porcelain(repo, &text).with_summary();
    let (head_sha, head_short) = resolve_head(repo);
    snapshot.head_sha = head_sha;
    snapshot.head_short = head_short;
    snapshot.recent_commits = recent_commits(repo, 8);
    Ok(snapshot)
}

fn resolve_head(repo: &Path) -> (Option<String>, Option<String>) {
    let full = run_git_stdout(repo, &["rev-parse", "HEAD"]).ok();
    let short = run_git_stdout(repo, &["rev-parse", "--short", "HEAD"]).ok();
    (full, short)
}

fn recent_commits(repo: &Path, limit: usize) -> Vec<GitCommitSummary> {
    let limit = limit.max(1).min(32);
    let n = format!("-n{limit}");
    let pretty = format!("--pretty=format:{}", "%H%x00%h%x00%s%x00%an%x00%cr");
    let Ok(text) = run_git_stdout(repo, &["log", &n, &pretty]) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let sha = parts.next()?.trim();
            let short_sha = parts.next()?.trim();
            let subject = parts.next()?.trim();
            if sha.is_empty() || subject.is_empty() {
                return None;
            }
            let author = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let relative_time = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Some(GitCommitSummary {
                sha: sha.to_string(),
                short_sha: if short_sha.is_empty() {
                    sha.chars().take(7).collect()
                } else {
                    short_sha.to_string()
                },
                subject: subject.to_string(),
                author,
                relative_time,
            })
        })
        .collect()
}

fn run_git_stdout(repo: &Path, args: &[&str]) -> JaymiResult<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| JaymiError::new(format!("failed to run git: {error}")))?;
    if !output.status.success() {
        return Err(JaymiError::new(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or("command"),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_porcelain(repo: &Path, text: &str) -> GitStatusSnapshot {
    let mut branch = None;
    let mut modified = Vec::new();
    let mut added = Vec::new();
    let mut deleted = Vec::new();
    let mut staged = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicts = Vec::new();

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            branch = Some(parse_branch_line(rest));
            continue;
        }
        if line.len() < 3 {
            continue;
        }
        let index = line.as_bytes()[0] as char;
        let worktree = line.as_bytes()[1] as char;
        let path = normalize_status_path(&line[3..]);

        if index == '?' && worktree == '?' {
            untracked.push(GitPathStatus {
                path,
                status: "??".into(),
            });
            continue;
        }

        if is_unmerged(index, worktree) {
            conflicts.push(GitPathStatus {
                path,
                status: format!("{index}{worktree}"),
            });
            continue;
        }

        if index != ' ' && index != '?' {
            staged.push(GitPathStatus {
                path: path.clone(),
                status: index.to_string(),
            });
            if index == 'A' {
                added.push(GitPathStatus {
                    path: path.clone(),
                    status: "A".into(),
                });
            }
            if index == 'D' {
                deleted.push(GitPathStatus {
                    path: path.clone(),
                    status: "D".into(),
                });
            }
        }
        if worktree != ' ' && worktree != '?' {
            match worktree {
                'D' => {
                    if !deleted.iter().any(|entry| entry.path == path) {
                        deleted.push(GitPathStatus {
                            path: path.clone(),
                            status: "D".into(),
                        });
                    }
                }
                'M' | 'T' | 'R' | 'C' => {
                    modified.push(GitPathStatus {
                        path: path.clone(),
                        status: worktree.to_string(),
                    });
                }
                other => {
                    modified.push(GitPathStatus {
                        path,
                        status: other.to_string(),
                    });
                }
            }
        }
    }

    GitStatusSnapshot {
        is_repository: true,
        repo_root: repo.to_path_buf(),
        branch,
        head_sha: None,
        head_short: None,
        summary: String::new(),
        modified,
        added,
        deleted,
        staged,
        untracked,
        conflicts,
        recent_commits: Vec::new(),
    }
}

fn is_unmerged(index: char, worktree: char) -> bool {
    index == 'U'
        || worktree == 'U'
        || (index == 'A' && worktree == 'A')
        || (index == 'D' && worktree == 'D')
}

fn parse_branch_line(rest: &str) -> String {
    // Formats: "main", "main...origin/main [ahead 1]", "HEAD (no branch)"
    let head = rest.split_whitespace().next().unwrap_or(rest);
    head.split("...").next().unwrap_or(head).to_string()
}

fn normalize_status_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some((from, to)) = trimmed.split_once(" -> ") {
        // Rename: keep destination path for UI actions.
        let _ = from;
        return strip_quotes(to.trim()).to_string();
    }
    strip_quotes(trimmed).to_string()
}

fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
}

fn summarize(
    modified: &[GitPathStatus],
    added: &[GitPathStatus],
    deleted: &[GitPathStatus],
    staged: &[GitPathStatus],
    untracked: &[GitPathStatus],
    conflicts: &[GitPathStatus],
) -> String {
    if modified.is_empty()
        && added.is_empty()
        && deleted.is_empty()
        && staged.is_empty()
        && untracked.is_empty()
        && conflicts.is_empty()
    {
        return "clean".to_string();
    }
    let mut parts = Vec::new();
    if !conflicts.is_empty() {
        parts.push(format!("{} conflict{}", conflicts.len(), if conflicts.len() == 1 { "" } else { "s" }));
    }
    if !modified.is_empty() {
        parts.push(format!("{} modified", modified.len()));
    }
    if !added.is_empty() {
        parts.push(format!("{} added", added.len()));
    }
    if !deleted.is_empty() {
        parts.push(format!("{} deleted", deleted.len()));
    }
    if !staged.is_empty() {
        parts.push(format!("{} staged", staged.len()));
    }
    if !untracked.is_empty() {
        parts.push(format!("{} untracked", untracked.len()));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn status_stage_unstage_and_commit() {
        let repo = temp_repo();
        let mut provider = GitProvider::new();
        provider.initialize().unwrap();

        fs::write(repo.join("note.txt"), "one\n").unwrap();
        let status = provider.status(&repo).unwrap();
        assert!(status.is_repository);
        assert!(status.branch.is_some());
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.untracked[0].path, "note.txt");
        assert!(status.conflicts.is_empty());
        // Brand-new repo may not have HEAD until first commit.
        assert!(status.recent_commits.is_empty() || status.head_sha.is_some());

        let staged = provider
            .execute(
                &repo,
                GitOperation::Stage,
                &[PathBuf::from("note.txt")],
                None,
            )
            .unwrap();
        assert_eq!(staged.staged.len(), 1);
        assert_eq!(staged.added.len(), 1);
        assert_eq!(staged.added[0].path, "note.txt");
        assert!(staged.untracked.is_empty());

        let unstaged = provider
            .execute(
                &repo,
                GitOperation::Unstage,
                &[PathBuf::from("note.txt")],
                None,
            )
            .unwrap();
        assert!(unstaged.staged.is_empty());
        assert!(unstaged.added.is_empty());
        assert_eq!(unstaged.untracked.len(), 1);

        provider
            .execute(
                &repo,
                GitOperation::Stage,
                &[PathBuf::from("note.txt")],
                None,
            )
            .unwrap();
        let committed = provider
            .execute(&repo, GitOperation::Commit, &[], Some("add note"))
            .unwrap();
        assert!(committed.staged.is_empty());
        assert!(committed.added.is_empty());
        assert!(committed.untracked.is_empty());
        assert_eq!(committed.summary, "clean");
        assert!(committed.head_sha.is_some());
        assert!(committed.head_short.is_some());
        assert!(!committed.recent_commits.is_empty());
        assert_eq!(committed.recent_commits[0].subject, "add note");

        let log = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["log", "-1", "--pretty=%s"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "add note");
    }

    #[test]
    fn parse_porcelain_classifies_merge_conflicts() {
        let repo = PathBuf::from("/tmp/fake-repo");
        let snap = parse_porcelain(
            &repo,
            "## main\nUU conflict.rs\n M dirty.rs\nA  staged.rs\n?? new.rs\n",
        );
        assert_eq!(snap.conflicts.len(), 1);
        assert_eq!(snap.conflicts[0].path, "conflict.rs");
        assert_eq!(snap.modified.len(), 1);
        assert_eq!(snap.staged.len(), 1);
        assert_eq!(snap.untracked.len(), 1);
        assert!(!snap.summary.is_empty() || snap.summary.is_empty()); // filled by with_summary
        let summarized = snap.with_summary();
        assert!(summarized.summary.contains("conflict"));
    }

    #[test]
    fn detect_non_repository_and_classify_modified_deleted() {
        let mut provider = GitProvider::new();
        provider.initialize().unwrap();

        let plain = temp_dir("not-a-repo");
        let miss = provider.detect(&plain).unwrap();
        assert!(!miss.is_repository);
        assert!(miss.summary.contains("not a git repository"));

        let repo = temp_repo();
        fs::write(repo.join("keep.txt"), "v1\n").unwrap();
        fs::write(repo.join("gone.txt"), "x\n").unwrap();
        provider
            .execute(
                &repo,
                GitOperation::Stage,
                &[PathBuf::from("keep.txt"), PathBuf::from("gone.txt")],
                None,
            )
            .unwrap();
        provider
            .execute(&repo, GitOperation::Commit, &[], Some("init"))
            .unwrap();

        fs::write(repo.join("keep.txt"), "v2\n").unwrap();
        fs::remove_file(repo.join("gone.txt")).unwrap();

        let status = provider.status(&repo).unwrap();
        assert!(status.is_repository);
        assert_eq!(status.modified.len(), 1);
        assert_eq!(status.modified[0].path, "keep.txt");
        assert_eq!(status.deleted.len(), 1);
        assert_eq!(status.deleted[0].path, "gone.txt");
        assert!(status.added.is_empty());
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-git-provider-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_repo() -> PathBuf {
        let dir = temp_dir("repo");
        let init = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&dir)
            .output()
            .unwrap();
        if !init.status.success() {
            // Older git without -b.
            Command::new("git")
                .args(["init"])
                .current_dir(&dir)
                .output()
                .unwrap();
            let _ = Command::new("git")
                .args(["branch", "-M", "main"])
                .current_dir(&dir)
                .output();
        }
        dir
    }
}
