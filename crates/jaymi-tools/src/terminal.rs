//! Terminal Tool — spawn / run / manage commands through the PTY Provider.
//!
//! Architecture path:
//! Planner → ExecuteTerminalCommands → Terminal Tool → Terminal Provider → PTY

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult, TerminalOperation};
use jaymi_providers::{
    TerminalCommandResult, TerminalProvider, DEFAULT_TERMINAL_SESSION_ID, TERMINAL_PROVIDER_ID,
};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata, ToolRisk,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const TERMINAL_TOOL_ID: &str = "terminal";

/// Tool that ensures, creates, renames, kills, or runs a command in a PTY session.
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
                risk: ToolRisk::Destructive,
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

    fn operation(input: &ToolInput) -> TerminalOperation {
        input.terminal_operation.unwrap_or({
            if input.command.is_some() {
                TerminalOperation::Run
            } else {
                TerminalOperation::Ensure
            }
        })
    }
}

impl Tool for TerminalTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        let operation = Self::operation(input);

        match &input.path {
            Some(path) if !path.as_os_str().is_empty() => {}
            Some(_) => return Err(JaymiError::new("terminal cwd must not be empty")),
            None => {
                return Err(JaymiError::new(
                    "terminal tool requires a working directory",
                ))
            }
        }

        let session_id_present = matches!(&input.session_id, Some(id) if !id.trim().is_empty());
        match operation {
            TerminalOperation::Create => {
                // Session id may be empty — the provider assigns one.
            }
            TerminalOperation::Rename | TerminalOperation::Kill => {
                if !session_id_present {
                    return Err(JaymiError::new(format!(
                        "terminal {} requires a session id",
                        operation.as_str()
                    )));
                }
            }
            TerminalOperation::Ensure | TerminalOperation::Run => match &input.session_id {
                Some(id) if !id.trim().is_empty() => {}
                Some(_) => return Err(JaymiError::new("terminal session id must not be empty")),
                None => return Err(JaymiError::new("terminal tool requires a session id")),
            },
        }

        if matches!(operation, TerminalOperation::Rename) {
            match &input.title {
                Some(title) if !title.trim().is_empty() => {}
                _ => {
                    return Err(JaymiError::new(
                        "terminal rename requires a non-empty title",
                    ))
                }
            }
        }

        if matches!(operation, TerminalOperation::Run) {
            match &input.command {
                Some(command) if !command.trim().is_empty() => {}
                _ => return Err(JaymiError::new("terminal command must not be empty")),
            }
        } else if let Some(command) = &input.command {
            if command.trim().is_empty() {
                return Err(JaymiError::new("terminal command must not be empty"));
            }
        }

        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let operation = Self::operation(input);
        let cwd = input
            .path
            .as_ref()
            .ok_or_else(|| JaymiError::new("terminal cwd is required"))?;
        let session_id = input
            .session_id
            .as_deref()
            .unwrap_or(DEFAULT_TERMINAL_SESSION_ID);

        let result: TerminalCommandResult = match operation {
            TerminalOperation::Ensure => self.terminal.ensure_session(session_id, cwd)?,
            TerminalOperation::Run => {
                let command = input
                    .command
                    .as_deref()
                    .ok_or_else(|| JaymiError::new("terminal command is required"))?;
                self.terminal.run_command(session_id, cwd, command)?
            }
            TerminalOperation::Create => {
                self.terminal.create_session(cwd, input.title.as_deref())?
            }
            TerminalOperation::Rename => {
                let title = input
                    .title
                    .as_deref()
                    .ok_or_else(|| JaymiError::new("terminal rename requires a title"))?;
                self.terminal.rename_session(session_id, title)?
            }
            TerminalOperation::Kill => self.terminal.kill_session(session_id)?,
        };

        let message = match operation {
            TerminalOperation::Run => format!(
                "Ran `{}` in session {} at {}",
                result.command.as_deref().unwrap_or(""),
                result.session_id,
                result.cwd.display()
            ),
            TerminalOperation::Ensure => format!(
                "Ensured terminal session {} at {}",
                result.session_id,
                result.cwd.display()
            ),
            TerminalOperation::Create => format!(
                "Created terminal session {} ({}) at {}",
                result.session_id,
                result.title,
                result.cwd.display()
            ),
            TerminalOperation::Rename => format!(
                "Renamed terminal session {} to {}",
                result.session_id, result.title
            ),
            TerminalOperation::Kill => format!("Killed terminal session {}", result.session_id),
        };

        Ok(ToolOutput::terminal(
            result.session_id,
            result.cwd,
            result.output,
            result.scrollback,
            result.history,
            result.title,
            result.alive,
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
        assert_eq!(
            output.session_id.as_deref(),
            Some(DEFAULT_TERMINAL_SESSION_ID)
        );
        assert_eq!(output.terminal_alive, Some(true));
        assert_eq!(output.terminal_title.as_deref(), Some("Terminal"));
        let text = format!(
            "{}{}",
            output.terminal_output.as_deref().unwrap_or(""),
            output.terminal_scrollback.as_deref().unwrap_or("")
        );
        assert!(text.contains("note.txt"), "missing note.txt in {text}");
    }

    #[test]
    fn create_rename_and_kill_sessions_through_tool() {
        let dir = temp_dir();
        let mut provider = TerminalProvider::new();
        provider.initialize().unwrap();
        let tool = TerminalTool::new(Arc::new(provider));

        let created = tool
            .execute(&ToolInput::create_terminal(&dir, Some("Build".into())))
            .unwrap();
        assert!(created.success);
        assert_eq!(created.terminal_title.as_deref(), Some("Build"));
        let session_id = created.session_id.clone().expect("session id");

        let renamed = tool
            .execute(&ToolInput::rename_terminal(&session_id, &dir, "Tests"))
            .unwrap();
        assert_eq!(renamed.terminal_title.as_deref(), Some("Tests"));

        let killed = tool
            .execute(&ToolInput::kill_terminal(&session_id, &dir))
            .unwrap();
        assert_eq!(killed.terminal_alive, Some(false));
    }

    #[test]
    fn rename_requires_non_empty_title() {
        let dir = temp_dir();
        let mut provider = TerminalProvider::new();
        provider.initialize().unwrap();
        let tool = TerminalTool::new(Arc::new(provider));

        let created = tool
            .execute(&ToolInput::create_terminal(&dir, None))
            .unwrap();
        let session_id = created.session_id.unwrap();

        let mut input = ToolInput::rename_terminal(&session_id, &dir, "  ");
        input.title = Some(String::new());
        let error = tool.validate(&input).unwrap_err();
        assert!(error.message().contains("title"));
    }

    #[test]
    fn kill_requires_session_id() {
        let dir = temp_dir();
        let mut provider = TerminalProvider::new();
        provider.initialize().unwrap();
        let tool = TerminalTool::new(Arc::new(provider));

        let mut input = ToolInput::kill_terminal("", &dir);
        input.session_id = None;
        let error = tool.validate(&input).unwrap_err();
        assert!(error.message().contains("session id"));
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
