//! Tool Orchestrator — selects and runs tools on behalf of the Planner.

use std::sync::Arc;

use crate::registry::ToolRegistry;
use crate::tool::{ToolInput, ToolOutput};
use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};

/// Coordinates tool selection and execution.
///
/// The Planner never executes providers directly. Every interaction passes
/// through a Tool managed by this orchestrator.
#[derive(Debug, Clone)]
pub struct ToolOrchestrator {
    registry: Arc<ToolRegistry>,
}

impl ToolOrchestrator {
    /// Create an orchestrator backed by a tool registry.
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    /// Select a tool that satisfies the requested capability.
    pub fn select(&self, capability: Capability) -> JaymiResult<Option<String>> {
        Ok(self
            .registry
            .find_for_capability(capability)?
            .map(|tool| tool.metadata().id.clone()))
    }

    /// Execute a selected tool with structured input.
    pub fn execute(&self, tool_id: &str, input: ToolInput) -> JaymiResult<ToolOutput> {
        let tool = self.registry.get(tool_id)?;
        tool.validate(&input)?;
        tool.execute(&input)
    }

    /// Select and execute a tool for a capability in one step.
    pub fn execute_for_capability(
        &self,
        capability: Capability,
        input: ToolInput,
    ) -> JaymiResult<(String, ToolOutput)> {
        let tool_id = self.select(capability)?.ok_or_else(|| {
            JaymiError::new(format!(
                "no tool registered for capability {}",
                capability.id()
            ))
        })?;
        let output = self.execute(&tool_id, input)?;
        Ok((tool_id, output))
    }
}
