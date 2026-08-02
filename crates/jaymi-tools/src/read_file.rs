//! Read File Tool — prefers normalized content via Content Intelligence API.
//!
//! Architecture path:
//! Planner → ReadDocuments → Read File Tool → Content Intelligence API →
//! Normalized Content Store / Understanding pipeline
//! (parsers remain hidden behind the API)

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;
use jaymi_understanding::{ContentIntelligence, ContentIntelligenceApi};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const READ_FILE_TOOL_ID: &str = "read_file";

/// Tool that reads one supported file into a unified [`jaymi_core::Document`].
pub struct ReadFileTool {
    metadata: ToolMetadata,
    content: Arc<ContentIntelligenceApi>,
}

impl ReadFileTool {
    /// Create a Read File tool bound to the Content Intelligence API.
    pub fn new(content: Arc<ContentIntelligenceApi>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: READ_FILE_TOOL_ID.to_string(),
                name: "Read File".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Read a supported file as normalized content when available, otherwise parse"
                        .to_string(),
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
            content,
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

        let loaded = self.content.load_content(path)?;
        let document = loaded.content.to_document();
        let mut output = ToolOutput::document(document);
        output.message = Some(format!("content_source={}", loaded.source.as_str()));
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{FileType, Lifecycle};
    use jaymi_database::Database;
    use jaymi_knowledge::{normalize_path, KnowledgeItem, KnowledgeStore, SqliteKnowledgeStore};
    use jaymi_parsers::default_registry;
    use jaymi_providers::{FilesystemProvider, Provider};
    use jaymi_understanding::{SqliteContentStore, UnderstandingEngine};
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn boot_tool(data: &std::path::Path) -> (Arc<SqliteKnowledgeStore>, ReadFileTool) {
        let mut db = Database::with_data_dir(data);
        db.initialize().unwrap();
        let db = Arc::new(db);
        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);
        let content = Arc::new(SqliteContentStore::new(Arc::clone(&db)));
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let parsers = Arc::new(default_registry().unwrap());
        let mut understanding = UnderstandingEngine::new(
            Arc::clone(&knowledge),
            content,
            Arc::new(filesystem),
            parsers,
        );
        understanding.initialize().unwrap();
        let api = Arc::new(ContentIntelligenceApi::new(Arc::new(understanding)));
        (knowledge, ReadFileTool::new(api))
    }

    #[test]
    fn reads_markdown_through_content_intelligence_api() {
        let data = temp_dir("read-tool-data");
        let dir = temp_dir("read-tool-files");
        let path = dir.join("note.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "# Hello\n\nWorld").unwrap();

        let (knowledge, tool) = boot_tool(&data);
        knowledge
            .publish(
                &KnowledgeItem {
                    path: normalize_path(&path).unwrap(),
                    filename: "note.md".into(),
                    extension: Some("md".into()),
                    size: 14,
                    created: Some(1),
                    modified: Some(1),
                    is_directory: false,
                    hidden: false,
                    parent: Some(normalize_path(&dir).unwrap()),
                    first_discovered: Some(1),
                    last_indexed: Some(1),
                    last_modified: Some(1),
                    last_verified: Some(1),
                    device_id: None,
                    inode: None,
                },
                1,
            )
            .unwrap();

        let output = tool.execute(&ToolInput::read_file(&path)).unwrap();
        assert!(output.success);
        let document = output.document.unwrap();
        assert_eq!(document.file_type, FileType::Markdown);
        assert_eq!(document.title.as_deref(), Some("Hello"));
        assert_eq!(output.parser_id.as_deref(), Some("markdown"));
        assert_eq!(output.message.as_deref(), Some("content_source=parsed"));

        let second = tool.execute(&ToolInput::read_file(&path)).unwrap();
        assert_eq!(second.message.as_deref(), Some("content_source=stored"));
    }

    #[test]
    fn rejects_unsupported_extension() {
        let data = temp_dir("read-unsup-data");
        let dir = temp_dir("read-unsup-files");
        let path = dir.join("archive.bin");
        File::create(&path).unwrap();

        let (_knowledge, tool) = boot_tool(&data);
        let error = tool.execute(&ToolInput::read_file(&path)).unwrap_err();
        assert!(
            error.message().contains("no parser")
                || error.message().contains("cannot detect")
                || error.message().contains("Unsupported")
                || error.message().contains("bin"),
            "{}",
            error.message()
        );
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-read-tool-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
