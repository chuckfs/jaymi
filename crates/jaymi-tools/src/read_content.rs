//! Content Tool — reads a resource through a Provider and the Content Registry.
//!
//! Architecture path:
//! Planner → Read Capability → Content Tool → Provider →
//! Content Registry → Content Parser → Unified Content

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_parsers::{ContentRegistry, ParseRequest};
use jaymi_providers::{FilesystemProvider, FILESYSTEM_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const READ_CONTENT_TOOL_ID: &str = "read_content";

/// Backward-compatible alias for Slice 3 call sites.
pub const READ_FILE_TOOL_ID: &str = READ_CONTENT_TOOL_ID;

/// Tool that reads one supported file into unified [`jaymi_core::Content`].
///
/// Today this tool is file-backed. Future content tools can target other
/// providers while still returning Content to the Planner.
#[derive(Debug)]
pub struct ReadContentTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
    contents: Arc<ContentRegistry>,
}

/// Backward-compatible alias for Slice 3 naming.
pub type ReadFileTool = ReadContentTool;

impl ReadContentTool {
    /// Create a Content Tool bound to filesystem and content-registry services.
    pub fn new(filesystem: Arc<FilesystemProvider>, contents: Arc<ContentRegistry>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: READ_CONTENT_TOOL_ID.to_string(),
                name: "Read Content".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Read a supported resource into unified Content".to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::ReadContent],
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
            contents,
        }
    }
}

impl Tool for ReadContentTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => Ok(()),
            Some(_) => Err(JaymiError::new("content path must not be empty")),
            None => Err(JaymiError::new("content tool requires a path")),
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("content path is required"))?;

        let content_type = ContentRegistry::detect_type(path).ok_or_else(|| {
            JaymiError::new(format!(
                "cannot detect content type for {}",
                path.display()
            ))
        })?;

        let parser = self.contents.resolve(&content_type)?;
        let bytes = self.filesystem.read_file(path)?;
        let content = parser.parse(&ParseRequest::file(path, &bytes))?;
        Ok(ToolOutput::content(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{ContentSource, ContentType};
    use jaymi_parsers::default_registry;
    use jaymi_providers::Provider;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_markdown_through_provider_and_parser() {
        let dir = temp_dir();
        let path = dir.join("note.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "# Hello\n\nWorld").unwrap();

        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let contents = Arc::new(default_registry().unwrap());
        let tool = ReadContentTool::new(Arc::new(filesystem), contents);

        let output = tool.execute(&ToolInput::read_file(&path)).unwrap();
        assert!(output.success);
        let content = output.content.unwrap();
        assert_eq!(content.source, ContentSource::File);
        assert_eq!(content.content_type, ContentType::Markdown);
        assert_eq!(content.title.as_deref(), Some("Hello"));
        assert_eq!(output.parser_id.as_deref(), Some("markdown"));
        assert!(content.path.is_some());
    }

    #[test]
    fn rejects_unsupported_extension() {
        let dir = temp_dir();
        let path = dir.join("scan.pdf");
        File::create(&path).unwrap();

        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let contents = Arc::new(default_registry().unwrap());
        let tool = ReadContentTool::new(Arc::new(filesystem), contents);

        let error = tool.execute(&ToolInput::read_file(&path)).unwrap_err();
        assert!(error.message().contains("no content parser registered"));
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-read-content-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
