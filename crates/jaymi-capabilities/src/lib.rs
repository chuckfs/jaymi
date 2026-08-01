//! Capability Engine for Jaymi.
//!
//! Capabilities describe behavior. They do not describe implementation.
//! Tools and providers fulfill capabilities.

#![forbid(unsafe_code)]

/// Stable capability identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Chat,
    Search,
    Code,
    Vision,
    GenerateImages,
    BrowseTheWeb,
    ReadDocuments,
    OrganizeFiles,
    ExecuteTerminalCommands,
    AutomateTasks,
    FileManagement,
    Internet,
    Automation,
}

/// Capability Engine / manager skeleton.
#[derive(Debug, Default)]
pub struct CapabilityEngine;

impl CapabilityEngine {
    /// List capabilities known to the system.
    pub fn available(&self) -> Vec<Capability> {
        Vec::new()
    }
}
