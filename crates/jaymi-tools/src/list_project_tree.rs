//! List Project Tree Tool — recursive project directory listing for Coding Explorer.
//!
//! Architecture path:
//! Planner → Search → List Project Tree Tool → Filesystem Provider → Filesystem

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
pub const LIST_PROJECT_TREE_TOOL_ID: &str = "list_project_tree";

/// Tool that recursively lists a project directory for the Coding Explorer.
#[derive(Debug)]
pub struct ListProjectTreeTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
}

impl ListProjectTreeTool {
    /// Create a List Project Tree tool bound to a filesystem provider instance.
    pub fn new(filesystem: Arc<FilesystemProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: LIST_PROJECT_TREE_TOOL_ID.to_string(),
                name: "List Project Tree".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Recursively list a project directory tree (skips hidden files and .git)"
                        .to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Search],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Fast,
                resource_cost: ResourceCost::VeryLow,
                memory_usage: MemoryUsage::Small,
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

impl Tool for ListProjectTreeTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => Ok(()),
            Some(_) => Err(JaymiError::new("project tree root must not be empty")),
            None => Err(JaymiError::new(
                "list project tree tool requires a directory path",
            )),
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("directory path is required"))?;
        let entries = self.filesystem.list_directory_tree(path)?;
        Ok(ToolOutput::project_tree(entries.0, entries.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::EntryType;
    use jaymi_providers::Provider;
    use std::fs::{self, File};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn executes_through_filesystem_provider() {
        let dir = temp_dir();
        fs::create_dir(dir.join("src")).unwrap();
        File::create(dir.join("src").join("main.rs")).unwrap();
        File::create(dir.join(".gitignore")).unwrap();

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = ListProjectTreeTool::new(Arc::new(provider));

        let output = tool
            .execute(&ToolInput::list_directory(&dir))
            .unwrap();
        assert!(output.success);
        assert!(output
            .entries
            .iter()
            .any(|entry| entry.name == "main.rs" && entry.entry_type == EntryType::File));
        assert!(output
            .entries
            .iter()
            .any(|entry| entry.name == "src" && entry.entry_type == EntryType::Directory));
        assert!(!output
            .entries
            .iter()
            .any(|entry| entry.name == ".gitignore"));
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-list-tree-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
