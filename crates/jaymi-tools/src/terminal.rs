//! Terminal Tool — spawn / run commands through the PTY Provider.
//!
//! Architecture path:
//! Planner → ExecuteTerminalCommands → Terminal Tool → Terminal Provider → PTY

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_providers::{TerminalProvider, DEFAULT_TERMINAL_SESSION_ID, TERMINAL_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const TERMINAL_TOOL_ID: &str = "terminal";

/// Tool that ensures a PTY session and optionally runs a shell command.
#[derive(Debug)]
pub struct TerminalTool {
    metadata: ToolMetadata,
    terminal: Arc<TerminalProvider>,
}

impl TerminalTool {
    /// Create a Terminal tool bound to a terminal provider instance.
    pub fn new(terminal: Arc<TerminalProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: TERMINAL_TOOL_ID.to_string(),
                name: "Terminal".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Run commands in a persistent local PTY session".to_string(),
                provider: TERMINAL_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::ExecuteTerminalCommands, Capability::Code],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Medium,
                resource_cost: ResourceCost::Low,
                memory_usage: MemoryUsage::Small,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::StructuredData,
            },
            terminal,
        }
    }
}

impl Tool for TerminalTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.session_id {
            Some(id) if !id.trim().is_empty() => {}
            Some(_) => return Err(JaymiError::new("terminal session id must not be empty")),
            None => return Err(JaymiError::new("terminal tool requires a session id")),
        }
        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => {}
            Some(_) => return Err(JaymiError::new("terminal cwd must not be empty")),
            None => return Err(JaymiError::new("terminal tool requires a working directory")),
        }
        if let Some(command) = &input.command {
            if command.trim().is_empty() {
                return Err(JaymiError::new("terminal command must not be empty"));
            }
        }
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let session_id = input
            .session_id
            .as_deref()
            .unwrap_or(DEFAULT_TERMINAL_SESSION_ID);
        let cwd = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("terminal cwd is required"))?;

        let result = match &input.command {
            Some(command) => self.terminal.run_command(session_id, cwd, command)?,
            None => self.terminal.ensure_session(session_id, cwd)?,
        };

        let message = match &result.command {
            Some(command) => format!(
                "Ran `{command}` in session {} at {}",
                result.session_id,
                result.cwd.display()
            ),
            None => format!(
                "Ensured terminal session {} at {}",
                result.session_id,
                result.cwd.display()
            ),
        };

        Ok(ToolOutput::terminal(
            result.session_id,
            result.cwd,
            result.output,
            result.scrollback,
            result.history,
            message,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_providers::Provider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn runs_command_through_terminal_provider() {
        let dir = temp_dir();
        fs::write(dir.join("note.txt"), "x").unwrap();

        let mut provider = TerminalProvider::new();
        provider.initialize().unwrap();
        let tool = TerminalTool::new(Arc::new(provider));

        let output = tool
            .execute(&ToolInput::run_terminal(
                DEFAULT_TERMINAL_SESSION_ID,
                &dir,
                "ls",
            ))
            .unwrap();
        assert!(output.success);
        assert_eq!(output.session_id.as_deref(), Some(DEFAULT_TERMINAL_SESSION_ID));
        let text = format!(
            "{}{}",
            output.terminal_output.as_deref().unwrap_or(""),
            output.terminal_scrollback.as_deref().unwrap_or("")
        );
        assert!(text.contains("note.txt"), "missing note.txt in {text}");
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-terminal-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
