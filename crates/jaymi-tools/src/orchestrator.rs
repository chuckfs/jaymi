//! Tool Orchestrator — selects and runs tools on behalf of the Planner.

use crate::tool::{ToolInput, ToolOutput};
use jaymi_capabilities::Capability;
use jaymi_core::JaymiResult;

/// Coordinates tool selection and execution.
///
/// The Planner never executes providers directly. Every interaction passes
/// through a Tool managed by this orchestrator.
#[derive(Debug, Default)]
pub struct ToolOrchestrator;

impl ToolOrchestrator {
    /// Select a tool that satisfies the requested capability.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn select(&self, _capability: Capability) -> JaymiResult<Option<String>> {
        Ok(None)
    }

    /// Execute a selected tool with structured input.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn execute(&self, _tool_id: &str, _input: ToolInput) -> JaymiResult<ToolOutput> {
        Ok(ToolOutput { success: false })
    }
}
