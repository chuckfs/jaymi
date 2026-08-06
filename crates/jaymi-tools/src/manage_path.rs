//! Manage Path Tool — mkdir / rename / delete through the Filesystem Provider.
//!
//! Architecture path:
//! Planner → FileManagement → Manage Path Tool → Filesystem Provider → Filesystem
//!
//! Deletion strategy is **Planner-owned**. This tool executes
//! [`DeletionMethod`](jaymi_core::DeletionMethod) from [`ToolInput`] and never
//! invents Trash vs Permanent on its own.

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{ActionPreview, DeletionMethod, JaymiError, JaymiResult, PreviewKind};
use jaymi_providers::{FilesystemProvider, FILESYSTEM_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata, ToolRisk,
};
use crate::tool::{Tool, ToolExecutionMetadata, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const MANAGE_PATH_TOOL_ID: &str = "manage_path";

/// Tool that creates directories, renames paths, or deletes files/folders.
#[derive(Debug)]
pub struct ManagePathTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
}

impl ManagePathTool {
    /// Create a Manage Path tool bound to a filesystem provider instance.
    pub fn new(filesystem: Arc<FilesystemProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: MANAGE_PATH_TOOL_ID.to_string(),
                name: "Manage Path".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Create, rename, or delete a local file or directory".to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::FileManagement],
                risk: ToolRisk::Modify,
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
            filesystem,
        }
    }
}

impl Tool for ManagePathTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn supports_recoverable_delete(&self) -> bool {
        self.filesystem.supports_trash()
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => {}
            Some(_) => return Err(JaymiError::new("path must not be empty")),
            None => return Err(JaymiError::new("manage path tool requires a path")),
        }
        let command = input
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| JaymiError::new("manage path tool requires command"))?;
        match command {
            "mkdir" => Ok(()),
            "delete" => {
                if input.deletion_method.is_none() {
                    Err(JaymiError::new(
                        "delete requires Planner-chosen deletion_method (trash|permanent)",
                    ))
                } else {
                    Ok(())
                }
            }
            "rename" => {
                if input
                    .content
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
                {
                    Ok(())
                } else {
                    Err(JaymiError::new(
                        "rename requires destination path in content",
                    ))
                }
            }
            other => Err(JaymiError::new(format!(
                "unsupported manage_path command: {other} (expected mkdir|rename|delete)"
            ))),
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("path is required"))?;
        let command = input
            .command
            .as_deref()
            .ok_or_else(|| JaymiError::new("command is required"))?;

        match command {
            "mkdir" => {
                self.filesystem.create_directory(path)?;
                Ok(ToolOutput {
                    success: true,
                    message: Some(format!("Created directory {}", path.display())),
                    listed_path: Some(path.clone()),
                    metadata: ToolExecutionMetadata::path_change(
                        format!("Created directory {}", path.display()),
                        [path.display().to_string()],
                    ),
                    ..Default::default()
                })
            }
            "rename" => {
                let destination = input
                    .content
                    .as_deref()
                    .ok_or_else(|| JaymiError::new("destination is required"))?;
                let to = std::path::PathBuf::from(destination);
                self.filesystem.rename_path(path, &to)?;
                Ok(ToolOutput {
                    success: true,
                    message: Some(format!("Renamed {} → {}", path.display(), to.display())),
                    listed_path: Some(to.clone()),
                    metadata: ToolExecutionMetadata::path_change(
                        format!("Renamed {} → {}", path.display(), to.display()),
                        [path.display().to_string(), to.display().to_string()],
                    ),
                    ..Default::default()
                })
            }
            "delete" => {
                let method = input.deletion_method.ok_or_else(|| {
                    JaymiError::new("delete requires Planner-chosen deletion_method")
                })?;
                match method {
                    DeletionMethod::Trash => {
                        let result = self.filesystem.move_to_trash(path)?;
                        Ok(ToolOutput {
                            success: true,
                            message: Some(format!(
                                "Moved {} to Trash (recoverable)",
                                result.original_path.display()
                            )),
                            listed_path: Some(result.original_path.clone()),
                            metadata: ToolExecutionMetadata::moved_to_trash(&result.original_path),
                            ..Default::default()
                        })
                    }
                    DeletionMethod::Permanent => {
                        self.filesystem.delete_permanently(path)?;
                        Ok(ToolOutput {
                            success: true,
                            message: Some(format!("Permanently deleted {}", path.display())),
                            listed_path: Some(path.clone()),
                            metadata: ToolExecutionMetadata::permanently_deleted(path),
                            ..Default::default()
                        })
                    }
                }
            }
            other => Err(JaymiError::new(format!(
                "unsupported manage_path command: {other}"
            ))),
        }
    }

    fn preview(&self, input: &ToolInput) -> JaymiResult<Option<ActionPreview>> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("path is required"))?;
        let command = input
            .command
            .as_deref()
            .ok_or_else(|| JaymiError::new("command is required"))?;
        let from = path.display().to_string();

        Ok(Some(match command {
            "mkdir" => ActionPreview {
                kind: PreviewKind::PathCreate,
                title: format!("Create directory {from}"),
                summary_lines: vec![format!("Create {from}")],
                body: None,
                truncated: false,
                total_lines: None,
                added_lines: None,
                removed_lines: None,
                resources: vec![from],
            },
            "rename" => {
                let destination = input
                    .content
                    .as_deref()
                    .ok_or_else(|| JaymiError::new("destination is required"))?;
                let to = destination.to_string();
                // Treat cross-directory renames as moves for preview labeling.
                let kind = if std::path::Path::new(&from).parent()
                    != std::path::Path::new(&to).parent()
                {
                    PreviewKind::PathMove
                } else {
                    PreviewKind::PathRename
                };
                let (title, before_label, after_label) = if kind == PreviewKind::PathMove {
                    (
                        format!("Move {from} → {to}"),
                        format!("Source: {from}"),
                        format!("Destination: {to}"),
                    )
                } else {
                    (
                        format!("Rename {from} → {to}"),
                        format!("Before: {from}"),
                        format!("After: {to}"),
                    )
                };
                ActionPreview {
                    kind,
                    title,
                    summary_lines: vec![before_label, after_label],
                    body: Some(format!("{from}\n→\n{to}")),
                    truncated: false,
                    total_lines: Some(3),
                    added_lines: None,
                    removed_lines: None,
                    resources: vec![from, to],
                }
            }
            "delete" => {
                let method = input.deletion_method.unwrap_or(DeletionMethod::Permanent);
                let method_label = method.as_str();
                ActionPreview {
                    kind: PreviewKind::PathDelete,
                    title: format!("Delete {from}"),
                    summary_lines: vec![
                        format!("Path: {from}"),
                        format!("Deletion Method: {method_label}"),
                    ],
                    body: None,
                    truncated: false,
                    total_lines: None,
                    added_lines: None,
                    removed_lines: None,
                    resources: vec![from],
                }
            }
            other => {
                return Ok(Some(ActionPreview::unavailable(
                    format!("Manage {from}"),
                    format!("No preview for command '{other}'"),
                )));
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_providers::Provider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mkdir_rename_delete_through_provider() {
        let dir = temp_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = ManagePathTool::new(Arc::new(provider));

        let folder = dir.join("folder");
        let output = tool
            .execute(&ToolInput::manage_path("mkdir", &folder, None::<String>))
            .unwrap();
        assert!(output.success);
        assert!(folder.is_dir());

        let renamed = dir.join("renamed");
        let output = tool
            .execute(&ToolInput::manage_path(
                "rename",
                &folder,
                Some(renamed.to_string_lossy().into_owned()),
            ))
            .unwrap();
        assert!(output.success);
        assert!(renamed.is_dir());
        assert!(!folder.exists());

        let output = tool
            .execute(&ToolInput::manage_delete(
                &renamed,
                DeletionMethod::Permanent,
            ))
            .unwrap();
        assert!(output.success);
        assert!(!renamed.exists());
        assert_eq!(
            output.metadata.deletion_method,
            Some(DeletionMethod::Permanent)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn trash_delete() {
        let dir = temp_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let filesystem = Arc::new(provider);
        let tool = ManagePathTool::new(Arc::clone(&filesystem));
        if !tool.supports_recoverable_delete() {
            return;
        }

        let file = dir.join("trash-tool.txt");
        filesystem.write_file(&file, b"x").unwrap();
        match tool.execute(&ToolInput::manage_delete(&file, DeletionMethod::Trash)) {
            Ok(output) => {
                assert!(output.success);
                assert!(!file.exists());
                assert_eq!(output.metadata.files_moved_to_trash.len(), 1);
                assert_eq!(output.metadata.recovery_available, Some(true));
                assert_eq!(
                    output.metadata.deletion_method,
                    Some(DeletionMethod::Trash)
                );
            }
            Err(error) if trash_environment_unavailable(error.message()) => {
                // Some hosts (CI / restricted Finder) cannot complete Trash moves.
            }
            Err(error) => panic!("unexpected trash failure: {error}"),
        }
    }

    #[test]
    fn permanent_delete() {
        let dir = temp_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = ManagePathTool::new(Arc::new(provider));

        let file = dir.join("perm-tool.txt");
        fs::write(&file, b"x").unwrap();
        let output = tool
            .execute(&ToolInput::manage_delete(
                &file,
                DeletionMethod::Permanent,
            ))
            .unwrap();
        assert!(output.success);
        assert!(!file.exists());
        assert_eq!(output.metadata.files_permanently_deleted.len(), 1);
        assert_eq!(output.metadata.recovery_available, Some(false));
    }

    #[test]
    fn trash_unavailable() {
        let dir = temp_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        provider.set_trash_available(false);
        let tool = ManagePathTool::new(Arc::new(provider));
        assert!(!tool.supports_recoverable_delete());

        let file = dir.join("no-trash-tool.txt");
        fs::write(&file, b"x").unwrap();
        let error = tool
            .execute(&ToolInput::manage_delete(&file, DeletionMethod::Trash))
            .unwrap_err();
        assert!(error.message().contains("Trash is unavailable"));
        assert!(file.exists());
    }

    #[test]
    fn delete_without_method_is_rejected() {
        let dir = temp_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = ManagePathTool::new(Arc::new(provider));
        let file = dir.join("needs-method.txt");
        fs::write(&file, b"x").unwrap();
        let error = tool
            .execute(&ToolInput::manage_path("delete", &file, None::<String>))
            .unwrap_err();
        assert!(error.message().contains("deletion_method"));
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-manage-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn trash_environment_unavailable(message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        lower.contains("finder")
            || lower.contains("osascript")
            || lower.contains("trash is unavailable")
            || lower.contains("connection invalid")
    }
}
