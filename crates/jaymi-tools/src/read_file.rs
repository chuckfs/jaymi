//! Read File Tool — reads a file through the Filesystem Provider and parsers.
//!
//! Architecture path:
//! Planner → ReadDocuments → Read File Tool → Filesystem Provider →
//! Parser Registry → Specific Parser → Unified Document

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_parsers::ParserRegistry;
use jaymi_providers::{FilesystemProvider, FILESYSTEM_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const READ_FILE_TOOL_ID: &str = "read_file";

/// Tool that reads one supported file into a unified [`jaymi_core::Document`].
#[derive(Debug)]
pub struct ReadFileTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
    parsers: Arc<ParserRegistry>,
}

impl ReadFileTool {
    /// Create a Read File tool bound to filesystem and parser services.
    pub fn new(filesystem: Arc<FilesystemProvider>, parsers: Arc<ParserRegistry>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: READ_FILE_TOOL_ID.to_string(),
                name: "Read File".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Read a supported file into a unified document".to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::ReadDocuments],
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
            parsers,
        }
    }
}

impl Tool for ReadFileTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => Ok(()),
            Some(_) => Err(JaymiError::new("file path must not be empty")),
            None => Err(JaymiError::new("read file tool requires a file path")),
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let path = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("file path is required"))?;

        let file_type = ParserRegistry::detect_type(path).ok_or_else(|| {
            JaymiError::new(format!(
                "cannot detect file type for {}",
                path.display()
            ))
        })?;

        let parser = self.parsers.resolve(&file_type)?;
        let bytes = self.filesystem.read_file(path)?;
        let document = parser.parse(path, &bytes)?;
        Ok(ToolOutput::document(document))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::FileType;
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
        let parsers = Arc::new(default_registry().unwrap());
        let tool = ReadFileTool::new(Arc::new(filesystem), parsers);

        let output = tool.execute(&ToolInput::read_file(&path)).unwrap();
        assert!(output.success);
        let document = output.document.unwrap();
        assert_eq!(document.file_type, FileType::Markdown);
        assert_eq!(document.title.as_deref(), Some("Hello"));
        assert_eq!(output.parser_id.as_deref(), Some("markdown"));
    }

    #[test]
    fn rejects_unsupported_extension() {
        let dir = temp_dir();
        let path = dir.join("scan.pdf");
        File::create(&path).unwrap();

        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let parsers = Arc::new(default_registry().unwrap());
        let tool = ReadFileTool::new(Arc::new(filesystem), parsers);

        let error = tool.execute(&ToolInput::read_file(&path)).unwrap_err();
        assert!(error.message().contains("no parser registered"));
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-read-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
