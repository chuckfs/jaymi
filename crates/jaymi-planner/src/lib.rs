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
use jaymi_core::{FileEntry, HealthReport, JaymiError, JaymiResult, Lifecycle, UserRequest};
use jaymi_providers::{ProviderRegistry, FILESYSTEM_PROVIDER_ID};
use jaymi_tools::{ToolInput, ToolOrchestrator, ToolRegistry};
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

    /// Process a user request through the architectural pipeline.
    ///
    /// Flow for list-directory:
    /// Planner → Search capability → Search Files Tool → Filesystem Provider
    ///
    /// The Planner never accesses the filesystem directly.
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            return Err(JaymiError::new("planner is not initialized"));
        }

        // Decision Engine selects intent without language-model reasoning.
        let intent = self.decision.determine_intent(&request);
        let Some(capability) = self.decision.required_capability(&intent) else {
            return Ok(PlannerResponse {
                content: "Unsupported request. Try: list <directory>".to_string(),
                ..PlannerResponse::default()
            });
        };

        if !self.capabilities.contains(capability) {
            return Err(JaymiError::new(format!(
                "capability {} is not registered",
                capability.id()
            )));
        }

        let Intent::ListDirectory { path } = intent else {
            return Ok(PlannerResponse {
                content: "Unsupported request.".to_string(),
                ..PlannerResponse::default()
            });
        };

        // Reasoning Engine is intentionally unused for this deterministic path.
        let _ = &self.reasoning;

        let input = ToolInput::list_directory(path.clone());
        let (tool_id, output) = self
            .orchestrator
            .execute_for_capability(capability, input)?;

        if !output.success {
            return Err(JaymiError::new(
                output
                    .message
                    .unwrap_or_else(|| "tool execution failed".to_string()),
            ));
        }

        let provider_id = self
            .tools
            .get(&tool_id)
            .ok()
            .map(|tool| tool.metadata().provider.clone())
            .or_else(|| Some(FILESYSTEM_PROVIDER_ID.to_string()));

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
        })
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

        let _ = &self.decision;
        let _ = &self.reasoning;
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let registries_ready = self.capabilities.is_initialized()
            && self.providers.is_initialized()
            && self.tools.is_initialized();
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
    use jaymi_core::{EntryType, Lifecycle};
    use jaymi_providers::{FilesystemProvider, Provider};
    use jaymi_tools::SearchFilesTool;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn planner_with_search() -> Planner {
        let mut capabilities = CapabilityRegistry::new();
        capabilities.initialize().unwrap();
        capabilities.register(Capability::Search).unwrap();

        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        providers.register(&filesystem).unwrap();
        let filesystem = Arc::new(filesystem);

        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        tools
            .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
            .unwrap();
        let tools = Arc::new(tools);
        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));

        let mut planner = Planner::new(PlannerDeps {
            capabilities: Arc::new(capabilities),
            providers: Arc::new(providers),
            tools,
            orchestrator,
        });
        planner.initialize().unwrap();
        planner
    }

    fn temp_dir() -> std::path::PathBuf {
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

    #[test]
    fn planner_initializes_from_registries() {
        let planner = planner_with_search();
        assert!(planner.health_check().healthy);
        assert!(planner.discover_capabilities().contains(&Capability::Search));
        assert_eq!(planner.provider_count(), 1);
        assert_eq!(planner.tool_count(), 1);
    }

    #[test]
    fn planner_rejects_uninitialized_registries() {
        let tools = Arc::new(ToolRegistry::new());
        let deps = PlannerDeps {
            capabilities: Arc::new(CapabilityRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            tools: Arc::clone(&tools),
            orchestrator: ToolOrchestrator::new(tools),
        };
        let mut planner = Planner::new(deps);
        assert!(planner.initialize().is_err());
    }

    #[test]
    fn list_directory_flows_through_architecture() {
        let dir = temp_dir();
        let mut file = File::create(dir.join("readme.md")).unwrap();
        write!(file, "jaymi").unwrap();
        fs::create_dir(dir.join("src")).unwrap();

        let planner = planner_with_search();
        let response = planner
            .handle(UserRequest::list_directory(&dir))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert_eq!(response.tool_id.as_deref(), Some("search_files"));
        assert_eq!(response.provider_id.as_deref(), Some(FILESYSTEM_PROVIDER_ID));
        assert_eq!(response.entries.len(), 2);
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "readme.md" && entry.entry_type == EntryType::File));
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "src" && entry.entry_type == EntryType::Directory));
        assert!(!response.content.is_empty());
    }

    #[test]
    fn planner_does_not_call_filesystem_for_unknown_intent() {
        let planner = planner_with_search();
        let response = planner.handle(UserRequest::new("sing a song")).unwrap();
        assert!(response.entries.is_empty());
        assert!(response.capability.is_none());
    }
}
