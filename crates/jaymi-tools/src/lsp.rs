//! Language Server Tool — LSP operations through the LSP Provider.
//!
//! Architecture path:
//! Planner → Code → language_server Tool → LSP Provider → rust-analyzer

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult, LspOperation};
use jaymi_providers::{LspProvider, LSP_PROVIDER_ID};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
///
/// Matches Capability Engine preferred tools for [`Capability::Code`].
pub const LANGUAGE_SERVER_TOOL_ID: &str = "language_server";

/// Tool that forwards structured LSP requests to the language server provider.
#[derive(Debug)]
pub struct LanguageServerTool {
    metadata: ToolMetadata,
    lsp: Arc<LspProvider>,
}

impl LanguageServerTool {
    /// Create a language_server tool bound to an LSP provider instance.
    pub fn new(lsp: Arc<LspProvider>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: LANGUAGE_SERVER_TOOL_ID.to_string(),
                name: "Language Server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Hover, completion, diagnostics, and navigation via Rust Analyzer"
                    .to_string(),
                provider: LSP_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Code],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Medium,
                resource_cost: ResourceCost::Medium,
                memory_usage: MemoryUsage::Moderate,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::StructuredData,
            },
            lsp,
        }
    }
}

impl Tool for LanguageServerTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        let request = input
            .lsp
            .as_ref()
            .ok_or_else(|| JaymiError::new("language_server tool requires an lsp request"))?;
        if request.workspace_root.as_os_str().is_empty() {
            return Err(JaymiError::new("language_server workspace root must not be empty"));
        }
        match request.operation {
            LspOperation::Ensure | LspOperation::Diagnostics => Ok(()),
            LspOperation::DidOpen | LspOperation::DidChange | LspOperation::DidClose => {
                if request.path.as_ref().is_none_or(|path| path.as_os_str().is_empty()) {
                    return Err(JaymiError::new("language_server document ops require a path"));
                }
                Ok(())
            }
            LspOperation::Hover
            | LspOperation::Completion
            | LspOperation::Definition
            | LspOperation::References => {
                if request.path.as_ref().is_none_or(|path| path.as_os_str().is_empty()) {
                    return Err(JaymiError::new("language_server position ops require a path"));
                }
                if request.line.is_none() || request.character.is_none() {
                    return Err(JaymiError::new(
                        "language_server position ops require line and character",
                    ));
                }
                Ok(())
            }
            LspOperation::Rename => {
                if request.path.as_ref().is_none_or(|path| path.as_os_str().is_empty()) {
                    return Err(JaymiError::new("language_server rename requires a path"));
                }
                if request.line.is_none() || request.character.is_none() {
                    return Err(JaymiError::new("language_server rename requires line and character"));
                }
                if request
                    .new_name
                    .as_ref()
                    .is_none_or(|name| name.trim().is_empty())
                {
                    return Err(JaymiError::new("language_server rename requires new_name"));
                }
                Ok(())
            }
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let request = input
            .lsp
            .as_ref()
            .ok_or_else(|| JaymiError::new("language_server tool requires an lsp request"))?;
        let result = self.lsp.execute(request)?;
        Ok(ToolOutput::lsp(result, request.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{LspOperation, LspRequest};
    use jaymi_providers::{Provider, LspProvider};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn hover_flows_through_language_server_tool() {
        let dir = temp_dir();
        let file = dir.join("main.rs");
        let content = "fn greet() {}\nfn main() { greet(); }\n";
        fs::write(&file, content).unwrap();

        let mut provider = LspProvider::mock();
        provider.initialize().unwrap();
        let tool = LanguageServerTool::new(Arc::new(provider));

        tool.execute(&ToolInput::lsp(LspRequest {
            workspace_root: dir.clone(),
            operation: LspOperation::DidOpen,
            path: Some(file.clone()),
            content: Some(content.into()),
            language: Some("rust".into()),
            version: Some(1),
            line: None,
            character: None,
            new_name: None,
        }))
        .unwrap();

        let output = tool
            .execute(&ToolInput::lsp(LspRequest {
                workspace_root: dir,
                operation: LspOperation::Hover,
                path: Some(file),
                content: None,
                language: None,
                version: None,
                line: Some(1),
                character: Some(13),
                new_name: None,
            }))
            .unwrap();
        assert!(output.success);
        assert!(output
            .lsp_hover
            .as_ref()
            .map(|hover| hover.contents.contains("greet"))
            .unwrap_or(false));
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-lsp-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
