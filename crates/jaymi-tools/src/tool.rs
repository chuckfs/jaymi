//! Tool interface and structured I/O.

use crate::metadata::ToolMetadata;
use jaymi_core::{DiscoveryQueryKind, Document, FileEntry, JaymiResult};

/// Structured input supplied by the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolInput {
    /// Directory or file path for filesystem tools.
    pub path: Option<std::path::PathBuf>,
    /// Structured discovery query for inventory tools.
    pub discovery: Option<DiscoveryQueryKind>,
}

impl ToolInput {
    /// Create input for a single-directory listing operation.
    pub fn list_directory(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            discovery: None,
        }
    }

    /// Create input for a single-file read operation.
    pub fn read_file(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            discovery: None,
        }
    }

    /// Create input for a discovery inventory query.
    pub fn discover(kind: DiscoveryQueryKind) -> Self {
        let path = match &kind {
            DiscoveryQueryKind::ByFolder { path, .. } => Some(path.clone()),
            _ => None,
        };
        Self {
            path,
            discovery: Some(kind),
        }
    }
}

/// Structured result returned to the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolOutput {
    /// Whether the tool completed successfully.
    pub success: bool,
    /// Directory listing entries when applicable.
    pub entries: Vec<FileEntry>,
    /// Unified document produced by the Read pipeline.
    pub document: Option<Document>,
    /// Parser selected for a read operation, when any.
    pub parser_id: Option<String>,
    /// Optional human-readable message.
    pub message: Option<String>,
}

impl ToolOutput {
    /// Successful directory listing.
    pub fn directory_listing(entries: Vec<FileEntry>) -> Self {
        Self {
            success: true,
            entries,
            document: None,
            parser_id: None,
            message: None,
        }
    }

    /// Successful document read.
    pub fn document(document: Document) -> Self {
        let parser_id = document.parser_id.clone();
        Self {
            success: true,
            entries: Vec::new(),
            document: Some(document),
            parser_id: Some(parser_id),
            message: None,
        }
    }

    /// Failed tool execution with an explanatory message.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            entries: Vec::new(),
            document: None,
            parser_id: None,
            message: Some(message.into()),
        }
    }
}

/// Tool trait — validate, execute, return structured output.
///
/// A Tool is not responsible for planning, choosing providers, memory,
/// permissions, or user interaction.
pub trait Tool: Send + Sync {
    /// Describe this tool for Planner selection.
    fn metadata(&self) -> &ToolMetadata;

    /// Validate input before execution.
    fn validate(&self, input: &ToolInput) -> JaymiResult<()>;

    /// Execute the operation through the appropriate provider.
    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput>;
}
