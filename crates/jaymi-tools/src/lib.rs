//! Tool framework and Tool Orchestrator for Jaymi.
//!
//! Tools are executable building blocks. They do not make decisions, reason,
//! or remember. The Planner decides; the Tool performs.

#![forbid(unsafe_code)]

pub mod categories;
pub mod git;
pub mod list_project_tree;
pub mod lsp;
pub mod manage_path;
pub mod metadata;
pub mod orchestrator;
pub mod query_inventory;
pub mod read_file;
pub mod registry;
pub mod scan_filesystem;
pub mod search_files;
pub mod search_knowledge;
pub mod search_project_knowledge;
pub mod terminal;
pub mod tool;
pub mod write_file;

pub use git::{GitTool, GIT_TOOL_ID};
pub use list_project_tree::{ListProjectTreeTool, LIST_PROJECT_TREE_TOOL_ID};
pub use lsp::{LanguageServerTool, LANGUAGE_SERVER_TOOL_ID};
pub use manage_path::{ManagePathTool, MANAGE_PATH_TOOL_ID};
pub use metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
pub use orchestrator::ToolOrchestrator;
pub use query_inventory::{QueryInventoryTool, QUERY_INVENTORY_TOOL_ID};
pub use read_file::{ReadFileTool, READ_FILE_TOOL_ID};
pub use registry::ToolRegistry;
pub use scan_filesystem::{ScanFilesystemTool, SCAN_FILESYSTEM_TOOL_ID};
pub use search_files::{SearchFilesTool, SEARCH_FILES_TOOL_ID};
pub use search_knowledge::{SearchKnowledgeTool, SEARCH_KNOWLEDGE_TOOL_ID};
pub use search_project_knowledge::{SearchProjectKnowledgeTool, SEARCH_PROJECT_KNOWLEDGE_TOOL_ID};
pub use terminal::{TerminalTool, TERMINAL_TOOL_ID};
pub use tool::{Tool, ToolInput, ToolOutput};
pub use write_file::{WriteFileTool, WRITE_FILE_TOOL_ID};

use jaymi_core::{HealthReport, JaymiResult, Lifecycle};

const NAME: &str = "tool_registry";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
    "context_engine",
    "capability_engine",
    "provider_registry",
];

impl Lifecycle for ToolRegistry {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.mark_initialized();
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.is_initialized(),
            self.is_initialized(),
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.clear()
    }
}
