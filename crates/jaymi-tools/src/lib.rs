//! Tool framework and Tool Orchestrator for Jaymi.
//!
//! Tools are executable building blocks. They do not make decisions, reason,
//! or remember. The Planner decides; the Tool performs.

#![forbid(unsafe_code)]

pub mod categories;
pub mod metadata;
pub mod orchestrator;
pub mod registry;
pub mod tool;

pub use metadata::ToolMetadata;
pub use orchestrator::ToolOrchestrator;
pub use registry::ToolRegistry;
pub use tool::{Tool, ToolInput, ToolOutput};

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
    "capability_registry",
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
