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
        let path = input
            .path
            .as_ref()
            .map(|value| value.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        jaymi_logging::info(
            "tools",
            format!("execute tool={tool_id} path={path}"),
        );

        let tool = self.registry.get(tool_id)?;
        if let Err(error) = tool.validate(&input) {
            jaymi_logging::warn(
                "tools",
                format!("tool={tool_id} validation failed: {}", error.message()),
            );
            return Err(error);
        }

        match tool.execute(&input) {
            Ok(output) => {
                if output.success {
                    jaymi_logging::info(
                        "tools",
                        format!(
                            "tool={tool_id} completed success=true entries={} has_document={}",
                            output.entries.len(),
                            output.document.is_some()
                        ),
                    );
                } else {
                    jaymi_logging::warn(
                        "tools",
                        format!(
                            "tool={tool_id} completed success=false message={:?}",
                            output.message
                        ),
                    );
                }
                Ok(output)
            }
            Err(error) => {
                jaymi_logging::error(
                    "tools",
                    format!("tool={tool_id} execution failed: {}", error.message()),
                );
                Err(error)
            }
        }
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
