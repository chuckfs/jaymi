//! Tool interface and structured I/O.

use crate::metadata::ToolMetadata;
use jaymi_core::JaymiResult;

/// Structured input supplied by the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolInput;

/// Structured result returned to the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolOutput {
    pub success: bool,
}

/// Tool trait — validate, execute, return structured output.
///
/// A Tool is not responsible for planning, choosing providers, memory,
/// permissions, or user interaction.
pub trait Tool: Send + Sync {
    /// Describe this tool for Planner selection.
    fn metadata(&self) -> &ToolMetadata;

    /// Validate input before execution.
    fn validate(&self, _input: &ToolInput) -> JaymiResult<()>;

    /// Execute the operation through the appropriate provider.
    fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput>;
}
