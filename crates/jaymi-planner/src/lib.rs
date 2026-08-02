//! Planner — the orchestration kernel of Jaymi.
//!
//! Every request passes through the Planner. It understands goals, gathers
//! context, delegates work, enforces permissions, and manages execution.
//! The Planner does not perform the work itself.

#![forbid(unsafe_code)]

pub mod decision;
pub mod request_lifecycle;
pub mod reasoning;

use std::sync::Arc;

use decision::{DecisionEngine, Intent};
use jaymi_capabilities::{Capability, CapabilityRegistry};
use jaymi_core::{
    Document, FileEntry, HealthReport, JaymiError, JaymiResult, Lifecycle, UserRequest,
};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionCheckResult, PermissionEngine,
    PermissionRequest, PermissionScope,
};
use jaymi_policies::{ExecutionCandidate, PolicyEngine, PolicyEvaluation};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::{
    InternetRequirement, PrivacyMode, ToolInput, ToolOrchestrator, ToolRegistry,
};
use reasoning::ReasoningEngine;

const NAME: &str = "planner";
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
    "tool_registry",
];

/// Final response produced after the request lifecycle completes.
#[derive(Debug, Default, Clone)]
pub struct PlannerResponse {
    /// Human-readable summary of the result.
    pub content: String,
    /// Capability selected for the request, when any.
    pub capability: Option<Capability>,
    /// Tool selected for execution, when any.
    pub tool_id: Option<String>,
    /// Provider that fulfilled the tool, when known.
    pub provider_id: Option<String>,
    /// Directory that was listed, when applicable.
    pub listed_path: Option<std::path::PathBuf>,
    /// Structured directory listing entries.
    pub entries: Vec<FileEntry>,
    /// Unified document produced by the Read pipeline.
    pub document: Option<Document>,
    /// Policy evaluation for the selected tool, when evaluated.
    pub policy_evaluation: Option<PolicyEvaluation>,
    /// Permission check result for the selected tool, when evaluated.
    pub permission_result: Option<PermissionCheckResult>,
    /// True when policy or permission blocked tool execution.
    pub blocked: bool,
}

/// Dependencies required to construct the Planner from registries.
#[derive(Clone)]
pub struct PlannerDeps {
    /// Capability registry used for discovery.
    pub capabilities: Arc<CapabilityRegistry>,
    /// Provider registry used for discovery.
    pub providers: Arc<ProviderRegistry>,
    /// Tool registry used for discovery.
    pub tools: Arc<ToolRegistry>,
    /// Orchestrator used to select and execute tools.
    pub orchestrator: ToolOrchestrator,
    /// Policy engine consulted before permissions.
    pub policies: Arc<PolicyEngine>,
    /// Permission engine consulted before tool execution.
    pub permissions: Arc<PermissionEngine>,
}

/// Planner kernel.
///
/// The Planner remains deterministic. Reasoning is delegated. Execution is
/// delegated. Nothing bypasses this component.
pub struct Planner {
    initialized: bool,
    decision: DecisionEngine,
    reasoning: ReasoningEngine,
    capabilities: Arc<CapabilityRegistry>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    orchestrator: ToolOrchestrator,
    policies: Arc<PolicyEngine>,
    permissions: Arc<PermissionEngine>,
}

impl Planner {
    /// Construct a Planner that discovers capabilities through registries.
    pub fn new(deps: PlannerDeps) -> Self {
        Self {
            initialized: false,
            decision: DecisionEngine,
            reasoning: ReasoningEngine,
            capabilities: deps.capabilities,
            providers: deps.providers,
            tools: deps.tools,
            orchestrator: deps.orchestrator,
            policies: deps.policies,
            permissions: deps.permissions,
        }
    }

    /// Discover registered capabilities through the capability registry.
    pub fn discover_capabilities(&self) -> Vec<Capability> {
        self.capabilities.list()
    }

    /// Number of registered providers visible to the Planner.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Number of registered tools visible to the Planner.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Returns true when the Planner completed initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Honest reasoning-backend status for diagnostics.
    pub fn reasoning_status(&self) -> &'static str {
        self.reasoning.status_label()
    }

    /// Whether a reasoning backend is wired.
    pub fn reasoning_implemented(&self) -> bool {
        self.reasoning.is_implemented()
    }

    /// Process a user request through the architectural pipeline.
    ///
    /// Flow for tool-backed intents:
    /// Planner → Policy Engine → Permission Engine → Tool
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            jaymi_logging::error("planner", "request rejected: planner is not initialized");
            return Err(JaymiError::new("planner is not initialized"));
        }

        jaymi_logging::info(
            "planner",
            format!(
                "request received content={:?} directory={:?} file={:?}",
                truncate_for_log(&request.content),
                request
                    .directory
                    .as_ref()
                    .map(|path| path.display().to_string()),
                request
                    .file
                    .as_ref()
                    .map(|path| path.display().to_string())
            ),
        );

        let intent = self.decision.determine_intent(&request);
        let Some(capability) = self.decision.required_capability(&intent) else {
            jaymi_logging::warn(
                "planner",
                "unsupported request; no capability mapped for intent",
            );
            return Ok(PlannerResponse {
                content: "Unsupported request. Try: list <directory> or read <file>".to_string(),
                ..PlannerResponse::default()
            });
        };

        jaymi_logging::info(
            "planner",
            format!("intent resolved capability={}", capability.id()),
        );

        if !self.capabilities.contains(capability) {
            let message = format!("capability {} is not registered", capability.id());
            jaymi_logging::error("planner", &message);
            return Err(JaymiError::new(message));
        }

        // Reasoning Engine is intentionally unused for these deterministic paths.
        let _ = &self.reasoning;

        let result = match intent {
            Intent::ListDirectory { path } => self.handle_list_directory(capability, path),
            Intent::ReadFile { path } => self.handle_read_file(capability, path),
            Intent::Unknown => {
                jaymi_logging::warn("planner", "unknown intent after capability mapping");
                Ok(PlannerResponse {
                    content: "Unsupported request.".to_string(),
                    ..PlannerResponse::default()
                })
            }
        };

        match &result {
            Ok(response) => jaymi_logging::info(
                "planner",
                format!(
                    "request completed tool={:?} provider={:?} blocked={} permission={:?}",
                    response.tool_id,
                    response.provider_id,
                    response.blocked,
                    response
                        .permission_result
                        .as_ref()
                        .map(|result| result.decision.as_str())
                ),
            ),
            Err(error) => jaymi_logging::error(
                "planner",
                format!("request failed: {}", error.message()),
            ),
        }

        result
    }

    fn handle_list_directory(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::list_directory(path.clone());
        let prepared = self.prepare_execution(capability, &input, &path, "List directory")?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let content = format!(
            "Listed {} entries in {} via {} → {} → {}",
            output.entries.len(),
            path.display(),
            capability.id(),
            tool_id,
            provider_id.as_deref().unwrap_or("unknown")
        );

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(path),
            entries: output.entries,
            document: None,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            blocked: false,
        })
    }

    fn handle_read_file(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::read_file(path.clone());
        let prepared = self.prepare_execution(capability, &input, &path, "Read file")?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let document = output.document.ok_or_else(|| {
            JaymiError::new("read tool succeeded without returning a document")
        })?;
        let provider_id = prepared.provider_id.clone();
        let content = format!(
            "Read {} ({}) via {} → {} → {} → {}",
            path.display(),
            document.file_type,
            capability.id(),
            tool_id,
            provider_id.as_deref().unwrap_or("unknown"),
            document.parser_id
        );

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: None,
            entries: Vec::new(),
            document: Some(document),
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            blocked: false,
        })
    }

    /// Planner → Policy Engine → Permission Engine (tool selected, not yet run).
    fn prepare_execution(
        &self,
        capability: Capability,
        _input: &ToolInput,
        path: &std::path::Path,
        action_label: &str,
    ) -> JaymiResult<PreparedExecution> {
        let tool_id = self.orchestrator.select(capability)?.ok_or_else(|| {
            JaymiError::new(format!(
                "no tool registered for capability {}",
                capability.id()
            ))
        })?;

        let tool = self.tools.get(&tool_id)?;
        let metadata = tool.metadata();
        let provider_id = metadata.provider.clone();
        let candidate = ExecutionCandidate {
            tool_id: tool_id.clone(),
            provider_id: provider_id.clone(),
            requires_internet: matches!(metadata.internet, InternetRequirement::Required),
            local_only: matches!(metadata.privacy, PrivacyMode::LocalOnly),
            cloud_only: matches!(metadata.privacy, PrivacyMode::CloudOnly),
        };

        jaymi_logging::info(
            "planner",
            format!(
                "policy evaluation tool={} provider={} internet={} local_only={}",
                candidate.tool_id,
                candidate.provider_id,
                candidate.requires_internet,
                candidate.local_only
            ),
        );
        let policy_evaluation = self.policies.evaluate(&candidate)?;
        if !policy_evaluation.allowed {
            jaymi_logging::warn(
                "planner",
                format!(
                    "policy blocked tool={}: {}",
                    tool_id,
                    policy_evaluation.summary()
                ),
            );
            return Ok(PreparedExecution {
                tool_id: tool_id.clone(),
                provider_id: Some(provider_id),
                policy_evaluation: Some(policy_evaluation.clone()),
                permission_result: None,
                blocked_response: Some(PlannerResponse {
                    content: format!(
                        "Blocked by policy before executing '{}': {}",
                        tool_id,
                        policy_evaluation.summary()
                    ),
                    capability: Some(capability),
                    tool_id: Some(tool_id),
                    provider_id: Some(candidate.provider_id),
                    policy_evaluation: Some(policy_evaluation),
                    permission_result: None,
                    blocked: true,
                    ..PlannerResponse::default()
                }),
            });
        }

        let permission_request = PermissionRequest {
            category: PermissionCategory::Filesystem,
            action: PermissionAction::Read,
            scope: PermissionScope::Once,
            explanation: format!("{action_label} at {}", path.display()),
            resource: Some(path.display().to_string()),
        };
        jaymi_logging::info(
            "planner",
            format!(
                "permission check category=filesystem action=read resource={}",
                path.display()
            ),
        );
        let permission_result = self.permissions.check(&permission_request)?;
        if !permission_result.allows_execution() {
            jaymi_logging::warn(
                "planner",
                format!(
                    "permission {} for tool={} resource={}",
                    permission_result.decision.as_str(),
                    tool_id,
                    path.display()
                ),
            );
            return Ok(PreparedExecution {
                tool_id: tool_id.clone(),
                provider_id: Some(provider_id),
                policy_evaluation: Some(policy_evaluation.clone()),
                permission_result: Some(permission_result.clone()),
                blocked_response: Some(PlannerResponse {
                    content: format!(
                        "Blocked by permission ({}) before executing '{}': {}",
                        permission_result.decision.as_str(),
                        tool_id,
                        permission_result.explanation
                    ),
                    capability: Some(capability),
                    tool_id: Some(tool_id),
                    provider_id: Some(candidate.provider_id),
                    policy_evaluation: Some(policy_evaluation),
                    permission_result: Some(permission_result),
                    blocked: true,
                    ..PlannerResponse::default()
                }),
            });
        }

        Ok(PreparedExecution {
            tool_id,
            provider_id: Some(provider_id),
            policy_evaluation: Some(policy_evaluation),
            permission_result: Some(permission_result),
            blocked_response: None,
        })
    }

    fn ensure_success(&self, output: &jaymi_tools::ToolOutput) -> JaymiResult<()> {
        if output.success {
            Ok(())
        } else {
            Err(JaymiError::new(
                output
                    .message
                    .clone()
                    .unwrap_or_else(|| "tool execution failed".to_string()),
            ))
        }
    }
}

struct PreparedExecution {
    tool_id: String,
    provider_id: Option<String>,
    policy_evaluation: Option<PolicyEvaluation>,
    permission_result: Option<PermissionCheckResult>,
    blocked_response: Option<PlannerResponse>,
}

fn truncate_for_log(value: &str) -> String {
    const MAX: usize = 120;
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let shortened: String = trimmed.chars().take(MAX).collect();
        format!("{shortened}…")
    }
}

impl Lifecycle for Planner {
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
        if !self.capabilities.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: capability registry is not ready",
            ));
        }
        if !self.providers.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: provider registry is not ready",
            ));
        }
        if !self.tools.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: tool registry is not ready",
            ));
        }
        if !self.policies.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: policy engine is not ready",
            ));
        }
        if !self.permissions.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: permission engine is not ready",
            ));
        }

        let _ = &self.decision;
        let _ = &self.reasoning;
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let registries_ready = self.capabilities.is_initialized()
            && self.providers.is_initialized()
            && self.tools.is_initialized()
            && self.policies.is_initialized()
            && self.permissions.is_initialized();
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized && registries_ready,
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{EntryType, FileType, Lifecycle};
    use jaymi_parsers::default_registry;
    use jaymi_permissions::PermissionDecision;
    use jaymi_providers::{FilesystemProvider, Provider, FILESYSTEM_PROVIDER_ID};
    use jaymi_tools::{
        EstimatedRuntime, ExecutionMode, GpuRequirements, MemoryUsage, Reliability, ResourceCost,
        ResultType, Tool, ToolMetadata, ToolOutput, ReadFileTool, SearchFilesTool,
    };
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn planner_with_search_and_read() -> Planner {
        planner_with_tools(|tools, filesystem, parsers| {
            tools
                .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                .unwrap();
            tools
                .register_tool(Arc::new(ReadFileTool::new(filesystem, parsers)))
                .unwrap();
        })
    }

    fn planner_with_tools<F>(register: F) -> Planner
    where
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<jaymi_parsers::ParserRegistry>),
    {
        let mut capabilities = CapabilityRegistry::new();
        capabilities.initialize().unwrap();
        capabilities.register(Capability::Search).unwrap();
        capabilities.register(Capability::ReadDocuments).unwrap();

        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        providers.register(&filesystem).unwrap();
        let filesystem = Arc::new(filesystem);

        let parsers = Arc::new(default_registry().unwrap());

        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        register(&mut tools, Arc::clone(&filesystem), Arc::clone(&parsers));
        let tools = Arc::new(tools);
        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));

        let mut policies = PolicyEngine::new();
        policies.initialize().unwrap();
        let mut permissions = PermissionEngine::new();
        permissions.initialize().unwrap();

        let mut planner = Planner::new(PlannerDeps {
            capabilities: Arc::new(capabilities),
            providers: Arc::new(providers),
            tools,
            orchestrator,
            policies: Arc::new(policies),
            permissions: Arc::new(permissions),
        });
        planner.initialize().unwrap();
        planner
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-planner-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct CloudSearchTool {
        metadata: ToolMetadata,
    }

    impl CloudSearchTool {
        fn new() -> Self {
            Self {
                metadata: ToolMetadata {
                    id: "cloud_search".into(),
                    name: "Cloud Search".into(),
                    version: "0.1.0".into(),
                    description: "Requires internet".into(),
                    provider: "cloud".into(),
                    capabilities: vec![Capability::Search],
                    execution_mode: ExecutionMode::Synchronous,
                    estimated_runtime: EstimatedRuntime::Fast,
                    resource_cost: ResourceCost::Low,
                    memory_usage: MemoryUsage::Small,
                    gpu_requirements: GpuRequirements::None,
                    privacy: PrivacyMode::CloudOnly,
                    internet: InternetRequirement::Required,
                    reliability: Reliability::Experimental,
                    result_type: ResultType::SearchResults,
                },
            }
        }
    }

    impl Tool for CloudSearchTool {
        fn metadata(&self) -> &ToolMetadata {
            &self.metadata
        }

        fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
            Ok(())
        }

        fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput> {
            Ok(ToolOutput::directory_listing(Vec::new()))
        }
    }

    #[test]
    fn planner_initializes_from_registries() {
        let planner = planner_with_search_and_read();
        assert!(planner.health_check().healthy);
        assert!(planner.discover_capabilities().contains(&Capability::Search));
        assert!(planner
            .discover_capabilities()
            .contains(&Capability::ReadDocuments));
        assert_eq!(planner.provider_count(), 1);
        assert_eq!(planner.tool_count(), 2);
    }

    #[test]
    fn list_directory_flows_through_architecture() {
        let dir = temp_dir();
        let mut file = File::create(dir.join("readme.md")).unwrap();
        write!(file, "jaymi").unwrap();
        fs::create_dir(dir.join("src")).unwrap();

        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::list_directory(&dir))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert_eq!(response.tool_id.as_deref(), Some("search_files"));
        assert_eq!(response.provider_id.as_deref(), Some(FILESYSTEM_PROVIDER_ID));
        assert!(!response.blocked);
        assert!(response.policy_evaluation.as_ref().unwrap().allowed);
        assert_eq!(
            response.permission_result.as_ref().unwrap().decision,
            PermissionDecision::Allowed
        );
        assert_eq!(response.entries.len(), 2);
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "readme.md" && entry.entry_type == EntryType::File));
    }

    #[test]
    fn read_file_returns_unified_document() {
        let dir = temp_dir();
        let path = dir.join("spec.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "# Spec\n\nDetails.").unwrap();

        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::read_file(&path)).unwrap();

        assert_eq!(response.capability, Some(Capability::ReadDocuments));
        assert_eq!(response.tool_id.as_deref(), Some("read_file"));
        assert!(!response.blocked);
        let document = response.document.expect("document");
        assert_eq!(document.file_type, FileType::Markdown);
        assert_eq!(document.title.as_deref(), Some("Spec"));
        assert_eq!(document.parser_id, "markdown");
        assert!(document.text.contains("Details."));
        assert!(response.content.contains("markdown"));
    }

    #[test]
    fn planner_does_not_call_filesystem_for_unknown_intent() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("sing a song")).unwrap();
        assert!(response.entries.is_empty());
        assert!(response.document.is_none());
        assert!(response.capability.is_none());
        assert!(!response.blocked);
    }

    #[test]
    fn offline_first_blocks_cloud_only_tool() {
        let planner = planner_with_tools(|tools, _, _| {
            tools
                .register_tool(Arc::new(CloudSearchTool::new()))
                .unwrap();
        });
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(response.blocked);
        assert!(response.entries.is_empty());
        assert_eq!(response.tool_id.as_deref(), Some("cloud_search"));
        assert!(!response.policy_evaluation.as_ref().unwrap().allowed);
        assert!(response.permission_result.is_none());
    }
}
