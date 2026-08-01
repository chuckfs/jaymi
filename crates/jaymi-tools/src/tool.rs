//! Tool interface and structured I/O.

use crate::metadata::ToolMetadata;
use jaymi_core::{Content, FileEntry, JaymiResult};

/// Structured input supplied by the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolInput {
    /// Path for tools that operate on filesystem resources.
    pub path: Option<std::path::PathBuf>,
    /// Free-text query for index search.
    pub query: Option<String>,
    /// Optional index root label (`downloads`, `documents`, `workspace`).
    pub source_root: Option<String>,
    /// Optional result limit.
    pub limit: Option<usize>,
}

impl ToolInput {
    /// Create input for a single-directory listing operation.
    pub fn list_directory(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            ..Self::default()
        }
    }

    /// Create input for a single-file content read operation.
    pub fn read_file(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            ..Self::default()
        }
    }

    /// Create input for indexing configured knowledge roots.
    pub fn index_roots() -> Self {
        Self::default()
    }

    /// Create input for querying the knowledge index.
    pub fn search_index(
        query: Option<String>,
        source_root: Option<String>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            query,
            source_root,
            limit,
            ..Self::default()
        }
    }
}

/// Structured result returned to the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolOutput {
    /// Whether the tool completed successfully.
    pub success: bool,
    /// Directory listing / index entries when applicable.
    pub entries: Vec<FileEntry>,
    /// Unified content produced by the Read pipeline.
    pub content: Option<Content>,
    /// Content parser selected for a read operation, when any.
    pub parser_id: Option<String>,
    /// Optional human-readable message.
    pub message: Option<String>,
    /// Number of indexed entries written by an index operation.
    pub indexed_count: Option<usize>,
}

impl ToolOutput {
    /// Successful directory listing.
    pub fn directory_listing(entries: Vec<FileEntry>) -> Self {
        Self {
            success: true,
            entries,
            content: None,
            parser_id: None,
            message: None,
            indexed_count: None,
        }
    }

    /// Successful index search / existence query.
    pub fn index_results(entries: Vec<FileEntry>, message: impl Into<String>) -> Self {
        Self {
            success: true,
            entries,
            content: None,
            parser_id: None,
            message: Some(message.into()),
            indexed_count: None,
        }
    }

    /// Successful indexing scan.
    pub fn indexed(count: usize, message: impl Into<String>) -> Self {
        Self {
            success: true,
            entries: Vec::new(),
            content: None,
            parser_id: None,
            message: Some(message.into()),
            indexed_count: Some(count),
        }
    }

    /// Successful content read.
    pub fn content(content: Content) -> Self {
        let parser_id = content.parser_id.clone();
        Self {
            success: true,
            entries: Vec::new(),
            content: Some(content),
            parser_id: Some(parser_id),
            message: None,
            indexed_count: None,
        }
    }

    /// Failed tool execution with an explanatory message.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            entries: Vec::new(),
            content: None,
            parser_id: None,
            message: Some(message.into()),
            indexed_count: None,
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
