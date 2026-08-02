//! Context Engine for Jaymi.
//!
//! Seventh subsystem in the deterministic boot sequence.
//! Context is assembled dynamically and never assumed.

#![forbid(unsafe_code)]

use jaymi_core::{HealthReport, JaymiResult, Lifecycle, UserRequest};

const NAME: &str = "context_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
];

/// Sources that may contribute to assembled context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    /// Currently open project.
    ActiveProject,
    /// Prior turns in the active conversation.
    PreviousConversation,
    /// Local files.
    Files,
    /// Search hits.
    SearchResults,
    /// Repository status.
    GitStatus,
    /// Terminal output.
    TerminalOutput,
    /// User notes.
    Notes,
    /// Messaging sources.
    Messages,
    /// Browser history.
    BrowserHistory,
    /// Memories selected by the Memory Engine.
    RetrievedMemories,
}

/// Assembled context for a single request.
#[derive(Debug, Default, Clone)]
pub struct ContextBundle {
    /// Sources included in this bundle.
    pub sources: Vec<ContextSource>,
}

/// Context Engine lifecycle.
#[derive(Debug, Default)]
pub struct ContextEngine {
    initialized: bool,
}

impl ContextEngine {
    /// Create an uninitialized context engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build only the context required for the current request.
    ///
    /// Not implemented in the boot-sequence milestone.
    pub fn assemble(&self, _request: &UserRequest) -> JaymiResult<ContextBundle> {
        Ok(ContextBundle::default())
    }
}

impl Lifecycle for ContextEngine {
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
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        // Lifecycle may be initialized, but assemble() is a stub.
        HealthReport::new(
            NAME,
            self.initialized,
            false,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            ("status".to_string(), "stub".to_string()),
            (
                "note".to_string(),
                "context assembly not implemented".to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_reports_dependencies() {
        let engine = ContextEngine::new();
        let health = engine.health_check();
        assert!(health.dependencies.contains(&"memory_engine".to_string()));
    }
}
