//! Search Files Tool — lists a single directory through the Filesystem Provider.

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
pub const SEARCH_FILES_TOOL_ID: &str = "search_files";

/// Tool that lists the contents of one directory.
///
/// Architecture path:
/// Planner → Search capability → Search Files Tool → Filesystem Provider → Filesystem
#[derive(Debug)]
pub struct SearchFilesTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
}

impl SearchFilesTool {
    /// Create a Search Files tool bound to a filesystem provider instance.
    pub fn new(filesystem: Arc<FilesystemProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: SEARCH_FILES_TOOL_ID.to_string(),
                name: "Search Files".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "List the contents of a single local directory".to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Search],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Fast,
                resource_cost: ResourceCost::VeryLow,
                memory_usage: MemoryUsage::Tiny,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::SearchResults,
            },
            filesystem,
        }
    }
}

impl Tool for SearchFilesTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => Ok(()),
            Some(_) => Err(JaymiError::new("directory path must not be empty")),
            None => Err(JaymiError::new(
                "search files tool requires a directory path",
            )),
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("directory path is required"))?;
        let entries = self.filesystem.list_directory(path)?;
        Ok(ToolOutput::directory_listing(entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::EntryType;
    use jaymi_providers::Provider;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn executes_through_filesystem_provider() {
        let dir = temp_dir();
        let mut file = File::create(dir.join("a.txt")).unwrap();
        write!(file, "data").unwrap();

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = SearchFilesTool::new(Arc::new(provider));

        let output = tool
            .execute(&ToolInput::list_directory(&dir))
            .unwrap();
        assert!(output.success);
        assert_eq!(output.entries.len(), 1);
        assert_eq!(output.entries[0].name, "a.txt");
        assert_eq!(output.entries[0].entry_type, EntryType::File);
    }

    #[test]
    fn validate_rejects_missing_path() {
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = SearchFilesTool::new(Arc::new(provider));
        assert!(tool.validate(&ToolInput::default()).is_err());
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-search-files-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
