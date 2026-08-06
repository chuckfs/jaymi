//! Git Tool — repository status and mutations through the Git Provider.
//!
//! Architecture path:
//! Planner → Code → Git Tool → Git Provider → `git`

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{ActionPreview, GitOperation, JaymiError, JaymiResult, PreviewKind};
use jaymi_providers::{GitProvider, GIT_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata, ToolRisk,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const GIT_TOOL_ID: &str = "git";

/// Tool that reads and mutates a local Git repository.
#[derive(Debug)]
pub struct GitTool {
    metadata: ToolMetadata,
    git: Arc<GitProvider>,
}

impl GitTool {
    /// Create a Git tool bound to a git provider instance.
    pub fn new(git: Arc<GitProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: GIT_TOOL_ID.to_string(),
                name: "Git".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Inspect and mutate a local Git repository".to_string(),
                provider: GIT_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Code],
                risk: ToolRisk::Workspace,
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Fast,
                resource_cost: ResourceCost::VeryLow,
                memory_usage: MemoryUsage::Tiny,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::StructuredData,
            },
            git,
        }
    }
}

impl Tool for GitTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => {}
            Some(_) => return Err(JaymiError::new("git repo root must not be empty")),
            None => return Err(JaymiError::new("git tool requires a repository root")),
        }
        let Some(operation) = input.git_operation else {
            return Err(JaymiError::new("git tool requires an operation"));
        };
        match operation {
            GitOperation::Stage | GitOperation::Unstage | GitOperation::Discard
                if input.paths.is_empty() =>
            {
                return Err(JaymiError::new(format!(
                    "git {} requires at least one path",
                    operation.as_str()
                )));
            }
            GitOperation::Commit => {
                let message = input.content.as_deref().map(str::trim).unwrap_or("");
                if message.is_empty() {
                    return Err(JaymiError::new("git commit requires a message"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let repo = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("git repo root is required"))?;
        let operation = input
            .git_operation
            .ok_or_else(|| JaymiError::new("git operation is required"))?;

        let snapshot = self
            .git
            .execute(repo, operation, &input.paths, input.content.as_deref())?;

        Ok(ToolOutput::git_status(
            snapshot.repo_root,
            snapshot.is_repository,
            snapshot.branch,
            snapshot.summary.clone(),
            snapshot.modified,
            snapshot.added,
            snapshot.deleted,
            snapshot.staged,
            snapshot.untracked,
            format!("Git {} · {}", operation.as_str(), snapshot.summary),
        ))
    }

    fn preview(&self, input: &ToolInput) -> JaymiResult<Option<ActionPreview>> {
        self.validate(input)?;
        let repo = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("git repo root is required"))?;
        let operation = input
            .git_operation
            .ok_or_else(|| JaymiError::new("git operation is required"))?;
        if matches!(operation, GitOperation::Status) {
            return Ok(None);
        }

        let snapshot = self.git.status(repo)?;
        let mut summary = vec![format!("Operation: {}", operation.as_str())];
        let mut body_lines = Vec::new();

        let path_filter: Option<std::collections::HashSet<String>> = if input.paths.is_empty() {
            None
        } else {
            Some(
                input
                    .paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            )
        };
        let matches_filter = |path: &str| {
            path_filter
                .as_ref()
                .map(|set| set.contains(path))
                .unwrap_or(true)
        };

        let modified: Vec<_> = snapshot
            .modified
            .iter()
            .filter(|entry| matches_filter(&entry.path))
            .map(|entry| entry.path.clone())
            .collect();
        let staged: Vec<_> = snapshot
            .staged
            .iter()
            .filter(|entry| matches_filter(&entry.path))
            .map(|entry| entry.path.clone())
            .collect();
        let added: Vec<_> = snapshot
            .added
            .iter()
            .filter(|entry| matches_filter(&entry.path))
            .map(|entry| entry.path.clone())
            .collect();
        let deleted: Vec<_> = snapshot
            .deleted
            .iter()
            .filter(|entry| matches_filter(&entry.path))
            .map(|entry| entry.path.clone())
            .collect();

        if !input.paths.is_empty() {
            summary.push(format!(
                "Paths: {}",
                input
                    .paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        summary.push(format!("Files modified: {}", modified.len()));
        summary.push(format!("Staged changes: {}", staged.len()));
        if !added.is_empty() {
            summary.push(format!("Added: {}", added.len()));
        }
        if !deleted.is_empty() {
            summary.push(format!("Deleted: {}", deleted.len()));
        }
        if let Some(message) = input.content.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            summary.push(format!("Commit message: {message}"));
        }

        if !modified.is_empty() {
            body_lines.push("Modified:".into());
            for path in &modified {
                body_lines.push(format!("  {path}"));
            }
        }
        if !staged.is_empty() {
            body_lines.push("Staged:".into());
            for path in &staged {
                body_lines.push(format!("  {path}"));
            }
        }
        if !added.is_empty() {
            body_lines.push("Added:".into());
            for path in &added {
                body_lines.push(format!("  {path}"));
            }
        }
        if !deleted.is_empty() {
            body_lines.push("Deleted:".into());
            for path in &deleted {
                body_lines.push(format!("  {path}"));
            }
        }

        let body = if body_lines.is_empty() {
            None
        } else {
            Some(body_lines.join("\n"))
        };
        let resources = input
            .paths
            .iter()
            .map(|path| path.display().to_string())
            .chain(std::iter::once(repo.display().to_string()))
            .collect();

        Ok(Some(ActionPreview {
            kind: PreviewKind::GitImpact,
            title: format!("Git {}", operation.as_str()),
            summary_lines: summary,
            body,
            truncated: false,
            total_lines: None,
            added_lines: None,
            removed_lines: None,
            resources,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_providers::Provider;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stages_through_git_provider() {
        let repo = temp_repo();
        fs::write(repo.join("a.txt"), "a\n").unwrap();

        let mut provider = GitProvider::new();
        provider.initialize().unwrap();
        let tool = GitTool::new(Arc::new(provider));

        let output = tool
            .execute(&ToolInput::git(
                &repo,
                GitOperation::Stage,
                vec![PathBuf::from("a.txt")],
                None,
            ))
            .unwrap();
        assert!(output.success);
        assert_eq!(output.git_staged.len(), 1);
        assert_eq!(output.git_staged[0].path, "a.txt");
    }

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-git-tool-{}",
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
            Command::new("git")
                .args(["init"])
                .current_dir(&dir)
                .output()
                .unwrap();
        }
        dir
    }
}
