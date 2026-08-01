//! Capability registry for Jaymi.
//!
//! Capabilities describe behavior independently from tools and providers.
//! The Planner discovers available capabilities through this registry.

#![forbid(unsafe_code)]

mod registry;

pub use registry::CapabilityRegistry;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

const NAME: &str = "capability_registry";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
    "context_engine",
];

/// Stable capability identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Conversational interaction.
    Chat,
    /// Semantic or keyword search.
    Search,
    /// Software development assistance.
    Code,
    /// Visual understanding.
    Vision,
    /// Image generation.
    GenerateImages,
    /// Web browsing.
    BrowseTheWeb,
    /// Document reading and parsing.
    ReadDocuments,
    /// File organization.
    OrganizeFiles,
    /// Terminal command execution.
    ExecuteTerminalCommands,
    /// Task automation.
    AutomateTasks,
    /// General file management.
    FileManagement,
    /// Internet access.
    Internet,
    /// General automation.
    Automation,
}

impl Capability {
    /// Stable string identity for diagnostics and registries.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Search => "search",
            Self::Code => "code",
            Self::Vision => "vision",
            Self::GenerateImages => "generate_images",
            Self::BrowseTheWeb => "browse_the_web",
            Self::ReadDocuments => "read_documents",
            Self::OrganizeFiles => "organize_files",
            Self::ExecuteTerminalCommands => "execute_terminal_commands",
            Self::AutomateTasks => "automate_tasks",
            Self::FileManagement => "file_management",
            Self::Internet => "internet",
            Self::Automation => "automation",
        }
    }
}

impl Lifecycle for CapabilityRegistry {
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
        self.clear();
        Ok(())
    }
}

/// Ensure a registry has been initialized before mutation.
pub(crate) fn ensure_initialized(initialized: bool) -> JaymiResult<()> {
    if initialized {
        Ok(())
    } else {
        Err(JaymiError::new(
            "capability registry is not initialized".to_string(),
        ))
    }
}
