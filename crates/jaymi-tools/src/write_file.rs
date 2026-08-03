//! Write File Tool — overwrite or create a text file through the Filesystem Provider.
//!
//! Architecture path:
//! Planner → FileManagement → Write File Tool → Filesystem Provider → Filesystem

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
pub const WRITE_FILE_TOOL_ID: &str = "write_file";

/// Tool that writes text content to one local file.
#[derive(Debug)]
pub struct WriteFileTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
}

impl WriteFileTool {
    /// Create a Write File tool bound to a filesystem provider instance.
    pub fn new(filesystem: Arc<FilesystemProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: WRITE_FILE_TOOL_ID.to_string(),
                name: "Write File".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Write or overwrite a local text file".to_string(),
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

impl Tool for WriteFileTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => {}
            Some(_) => return Err(JaymiError::new("file path must not be empty")),
            None => return Err(JaymiError::new("write file tool requires a file path")),
        }
        if input.content.is_none() {
            return Err(JaymiError::new("write file tool requires content"));
        }
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("file path is required"))?;
        let content = input
            .content
            .as_ref()
            .ok_or_else(|| JaymiError::new("content is required"))?;
        self.filesystem.write_file(path, content.as_bytes())?;
        Ok(ToolOutput {
            success: true,
            message: Some(format!(
                "Wrote {} bytes to {}",
                content.len(),
                path.display()
            )),
            listed_path: Some(path.clone()),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_providers::Provider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_through_filesystem_provider() {
        let dir = temp_dir();
        let path = dir.join("note.txt");

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let tool = WriteFileTool::new(Arc::new(provider));

        let output = tool
            .execute(&ToolInput::write_file(&path, "hello write"))
            .unwrap();
        assert!(output.success);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello write");
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-write-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
