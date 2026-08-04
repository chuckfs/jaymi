//! Manage Path Tool — mkdir / rename / delete through the Filesystem Provider.
//!
//! Architecture path:
//! Planner → FileManagement → Manage Path Tool → Filesystem Provider → Filesystem

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_providers::{FilesystemProvider, FILESYSTEM_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

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
            "mkdir" | "delete" => Ok(()),
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
                    listed_path: Some(to),
                    ..Default::default()
                })
            }
            "delete" => {
                self.filesystem.delete_path(path)?;
                Ok(ToolOutput {
                    success: true,
                    message: Some(format!("Deleted {}", path.display())),
                    listed_path: Some(path.clone()),
                    ..Default::default()
                })
            }
            other => Err(JaymiError::new(format!(
                "unsupported manage_path command: {other}"
            ))),
        }
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
            .execute(&ToolInput::manage_path("delete", &renamed, None::<String>))
            .unwrap();
        assert!(output.success);
        assert!(!renamed.exists());
        let _ = fs::remove_dir_all(&dir);
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
}
