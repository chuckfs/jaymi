//! Git Provider — local repository status and mutation via the `git` CLI.
//!
//! Architecture path:
//! Planner → Code → Git Tool → Git Provider → `git`
//!
//! The Planner never shells out to git directly. Tools mediate all access.

use std::path::{Path, PathBuf};
use std::process::Command;

use jaymi_capabilities::Capability;
use jaymi_core::{GitOperation, GitPathStatus, JaymiError, JaymiResult};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Provider ID used for registration and tool metadata.
pub const GIT_PROVIDER_ID: &str = "git";

/// Structured repository status returned by the Git provider.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatusSnapshot {
    /// Absolute repository root.
    pub repo_root: PathBuf,
    /// Current branch name, when known.
    pub branch: Option<String>,
    /// Short human-readable summary.
    pub summary: String,
    /// Unstaged worktree modifications (tracked files).
    pub modified: Vec<GitPathStatus>,
    /// Staged index changes.
    pub staged: Vec<GitPathStatus>,
    /// Untracked paths.
    pub untracked: Vec<GitPathStatus>,
}

impl GitStatusSnapshot {
    fn with_summary(mut self) -> Self {
        self.summary = summarize(&self.modified, &self.staged, &self.untracked);
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

    /// Run a Git operation and return refreshed status.
    pub fn execute(
        &self,
        repo_root: &Path,
        operation: GitOperation,
        paths: &[PathBuf],
        message: Option<&str>,
    ) -> JaymiResult<GitStatusSnapshot> {
        self.require_initialized()?;
        let repo = normalize_repo(repo_root)?;

        match operation {
            GitOperation::Status => {}
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
                "git {} repo={} branch={:?} modified={} staged={} untracked={}",
                operation.as_str(),
                repo.display(),
                snapshot.branch,
                snapshot.modified.len(),
                snapshot.staged.len(),
                snapshot.untracked.len()
            ),
        );
        Ok(snapshot)
    }

    /// Convenience: repository status only.
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

fn normalize_repo(path: &Path) -> JaymiResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(JaymiError::new("git repo root must not be empty"));
    }
    let meta = std::fs::metadata(path).map_err(|error| {
        JaymiError::new(format!("cannot access git repo {}: {error}", path.display()))
    })?;
    if !meta.is_dir() {
        return Err(JaymiError::new(format!(
            "git repo root is not a directory: {}",
            path.display()
        )));
    }
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Confirm this is a git work tree.
    let output = Command::new("git")
        .args(["-C"])
        .arg(&canonical)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|error| JaymiError::new(format!("failed to probe git repo: {error}")))?;
    if !output.status.success() {
        return Err(JaymiError::new(format!(
            "not a git repository: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
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

    let mut tracked = Vec::new();
    let mut clean = Vec::new();
    for path in paths {
        let key = path.to_string_lossy();
        if untracked.contains(key.as_ref()) {
            clean.push(path.clone());
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
    Ok(parse_porcelain(repo, &text).with_summary())
}

fn parse_porcelain(repo: &Path, text: &str) -> GitStatusSnapshot {
    let mut branch = None;
    let mut modified = Vec::new();
    let mut staged = Vec::new();
    let mut untracked = Vec::new();

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

        if index != ' ' && index != '?' {
            staged.push(GitPathStatus {
                path: path.clone(),
                status: index.to_string(),
            });
        }
        if worktree != ' ' && worktree != '?' {
            modified.push(GitPathStatus {
                path,
                status: worktree.to_string(),
            });
        }
    }

    GitStatusSnapshot {
        repo_root: repo.to_path_buf(),
        branch,
        summary: String::new(),
        modified,
        staged,
        untracked,
    }
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
    staged: &[GitPathStatus],
    untracked: &[GitPathStatus],
) -> String {
    if modified.is_empty() && staged.is_empty() && untracked.is_empty() {
        return "clean".to_string();
    }
    let mut parts = Vec::new();
    if !modified.is_empty() {
        parts.push(format!("{} modified", modified.len()));
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
        assert!(status.branch.is_some());
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.untracked[0].path, "note.txt");

        let staged = provider
            .execute(
                &repo,
                GitOperation::Stage,
                &[PathBuf::from("note.txt")],
                None,
            )
            .unwrap();
        assert_eq!(staged.staged.len(), 1);
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
            .execute(
                &repo,
                GitOperation::Commit,
                &[],
                Some("add note"),
            )
            .unwrap();
        assert!(committed.staged.is_empty());
        assert!(committed.untracked.is_empty());
        assert_eq!(committed.summary, "clean");

        let log = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["log", "-1", "--pretty=%s"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "add note");
    }

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-git-provider-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
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
