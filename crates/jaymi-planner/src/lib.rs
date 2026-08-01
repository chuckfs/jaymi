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
    Content, FileEntry, HealthReport, JaymiError, JaymiResult, Lifecycle, UserRequest,
};
use jaymi_providers::{ProviderRegistry, FILESYSTEM_PROVIDER_ID};
use jaymi_tools::{
    ToolInput, ToolOrchestrator, ToolRegistry, INDEX_FILES_TOOL_ID, READ_CONTENT_TOOL_ID,
    SEARCH_FILES_TOOL_ID, SEARCH_INDEX_TOOL_ID,
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
    pub summary: String,
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
    /// Unified Content produced by the Read pipeline.
    ///
    /// The Planner reasons over Content without depending on parsers or formats.
    pub content: Option<Content>,
}

impl PlannerResponse {
    /// Backward-compatible accessor for the human-readable summary string.
    pub fn message(&self) -> &str {
        &self.summary
    }

    /// Conversation-facing text derived from this response.
    ///
    /// The UI should display this rather than inventing provider/tool details.
    pub fn assistant_text(&self) -> String {
        if let Some(content) = &self.content {
            let mut body = String::new();
            if let Some(title) = &content.title {
                body.push_str(&format!("I read “{title}”.\n\n"));
            } else if let Some(path) = &content.path {
                body.push_str(&format!("I read {}.\n\n", path.display()));
            }
            body.push_str(&content.preview(900));
            return body;
        }

        if !self.entries.is_empty() {
            let path = self
                .listed_path
                .as_ref()
                .map(|value| value.display().to_string());
            let mut body = if let Some(path) = path {
                format!(
                    "I found {} item{} in {}:\n",
                    self.entries.len(),
                    if self.entries.len() == 1 { "" } else { "s" },
                    path
                )
            } else {
                format!(
                    "{}\n\n",
                    if self.summary.is_empty() {
                        format!(
                            "I found {} item{}:",
                            self.entries.len(),
                            if self.entries.len() == 1 { "" } else { "s" }
                        )
                    } else {
                        self.summary.clone()
                    }
                )
            };
            for entry in self.entries.iter().take(40) {
                body.push_str(&format!("• {} ({})\n", entry.name, entry.entry_type.label()));
            }
            if self.entries.len() > 40 {
                body.push_str(&format!("…and {} more\n", self.entries.len() - 40));
            }
            return body.trim_end().to_string();
        }

        self.summary.clone()
    }
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
    /// Supported flows:
    /// - chat: Chat capability (no tools yet — conversational acknowledgment)
    /// - list-directory: Search → Search Files Tool → Filesystem Provider
    /// - query-index: Search → Search Index Tool → Database
    /// - index-knowledge: Search → Index Files Tool → Filesystem + Database
    /// - read-file: ReadContent → Content Tool → Provider → Content Registry → Content
    ///
    /// The Planner never accesses the filesystem or parser implementations directly.
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            return Err(JaymiError::new("planner is not initialized"));
        }

        let intent = self.decision.determine_intent(&request);
        let Some(capability) = self.decision.required_capability(&intent) else {
            return Ok(PlannerResponse {
                summary: "Say hello, ask what exists, or try `list <directory>` / `read <file>`."
                    .to_string(),
                ..PlannerResponse::default()
            });
        };

        if !self.capabilities.contains(capability) {
            return Err(JaymiError::new(format!(
                "capability {} is not registered",
                capability.id()
            )));
        }

        // Reasoning Engine is intentionally unused for these deterministic paths.
        let _ = &self.reasoning;

        match intent {
            Intent::ListDirectory { path } => self.handle_list_directory(capability, path),
            Intent::ReadFile { path } => self.handle_read_file(capability, path),
            Intent::QueryIndex {
                query,
                source_root,
            } => self.handle_query_index(capability, query, source_root),
            Intent::IndexKnowledge => self.handle_index_knowledge(capability),
            Intent::Chat { message } => self.handle_chat(capability, message),
            Intent::Unknown => Ok(PlannerResponse {
                summary: "Say hello, ask what exists, or try `list <directory>` / `read <file>`."
                    .to_string(),
                ..PlannerResponse::default()
            }),
        }
    }

    fn handle_chat(
        &self,
        capability: Capability,
        message: String,
    ) -> JaymiResult<PlannerResponse> {
        let summary = format!(
            "Thanks — I heard you.\n\n\
I’m still learning how to talk freely, but I can already help with your files.\n\
Ask “what exists?”, or try `list <directory>` / `read <file>` / `index my files`.\n\n\
You said: “{message}”"
        );

        Ok(PlannerResponse {
            summary,
            capability: Some(capability),
            tool_id: None,
            provider_id: None,
            listed_path: None,
            entries: Vec::new(),
            content: None,
        })
    }

    fn handle_query_index(
        &self,
        capability: Capability,
        query: Option<String>,
        source_root: Option<String>,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::search_index(query, source_root.clone(), Some(40));
        let output = self
            .orchestrator
            .execute(SEARCH_INDEX_TOOL_ID, input)?;
        self.ensure_success(&output)?;

        let summary = output
            .message
            .clone()
            .unwrap_or_else(|| "Searched the knowledge index.".to_string());

        Ok(PlannerResponse {
            summary,
            capability: Some(capability),
            tool_id: Some(SEARCH_INDEX_TOOL_ID.to_string()),
            provider_id: Some(FILESYSTEM_PROVIDER_ID.to_string()),
            listed_path: None,
            entries: output.entries,
            content: None,
        })
    }

    fn handle_index_knowledge(&self, capability: Capability) -> JaymiResult<PlannerResponse> {
        let output = self
            .orchestrator
            .execute(INDEX_FILES_TOOL_ID, ToolInput::index_roots())?;
        self.ensure_success(&output)?;

        let summary = output
            .message
            .clone()
            .unwrap_or_else(|| "Updated the knowledge index.".to_string());

        Ok(PlannerResponse {
            summary,
            capability: Some(capability),
            tool_id: Some(INDEX_FILES_TOOL_ID.to_string()),
            provider_id: Some(FILESYSTEM_PROVIDER_ID.to_string()),
            listed_path: None,
            entries: Vec::new(),
            content: None,
        })
    }

    fn handle_list_directory(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::list_directory(path.clone());
        let output = self.orchestrator.execute(SEARCH_FILES_TOOL_ID, input)?;
        self.ensure_success(&output)?;
        let tool_id = SEARCH_FILES_TOOL_ID.to_string();

        let provider_id = self.provider_for_tool(&tool_id);
        let summary = format!(
            "Listed {} entries in {}.",
            output.entries.len(),
            path.display()
        );

        Ok(PlannerResponse {
            summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(path),
            entries: output.entries,
            content: None,
        })
    }

    fn handle_read_file(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::read_file(path.clone());
        let output = self
            .orchestrator
            .execute(READ_CONTENT_TOOL_ID, input)?;
        self.ensure_success(&output)?;
        let tool_id = READ_CONTENT_TOOL_ID.to_string();

        let content = output.content.ok_or_else(|| {
            JaymiError::new("content tool succeeded without returning Content")
        })?;
        let provider_id = self.provider_for_tool(&tool_id);
        let summary = format!(
            "Read {} ({}) with {}.",
            path.display(),
            content.content_type,
            content.parser_id
        );

        Ok(PlannerResponse {
            summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: None,
            entries: Vec::new(),
            content: Some(content),
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

    fn provider_for_tool(&self, tool_id: &str) -> Option<String> {
        self.tools
            .get(tool_id)
            .ok()
            .map(|tool| tool.metadata().provider.clone())
            .or_else(|| Some(FILESYSTEM_PROVIDER_ID.to_string()))
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
    use jaymi_core::{ContentSource, ContentType, EntryType, Lifecycle};
    use jaymi_database::{Database, IndexRoot};
    use jaymi_parsers::default_registry;
    use jaymi_providers::{FilesystemProvider, Provider};
    use jaymi_tools::{IndexFilesTool, ReadContentTool, SearchFilesTool, SearchIndexTool};
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn planner_with_search_and_read() -> Planner {
        let mut capabilities = CapabilityRegistry::new();
        capabilities.initialize().unwrap();
        capabilities.register(Capability::Chat).unwrap();
        capabilities.register(Capability::Search).unwrap();
        capabilities.register(Capability::ReadContent).unwrap();

        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        providers.register(&filesystem).unwrap();
        let filesystem = Arc::new(filesystem);

        let mut database = Database::new();
        database.initialize().unwrap();
        let database = Arc::new(database);

        let contents = Arc::new(default_registry().unwrap());
        let roots = vec![IndexRoot::new("workspace", temp_dir())];

        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        tools
            .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
            .unwrap();
        tools
            .register_tool(Arc::new(SearchIndexTool::new(Arc::clone(&database))))
            .unwrap();
        tools
            .register_tool(Arc::new(IndexFilesTool::new(
                Arc::clone(&filesystem),
                Arc::clone(&database),
                roots,
            )))
            .unwrap();
        tools
            .register_tool(Arc::new(ReadContentTool::new(
                Arc::clone(&filesystem),
                contents,
            )))
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
        let planner = planner_with_search_and_read();
        assert!(planner.health_check().healthy);
        assert!(planner.discover_capabilities().contains(&Capability::Chat));
        assert!(planner.discover_capabilities().contains(&Capability::Search));
        assert!(planner
            .discover_capabilities()
            .contains(&Capability::ReadContent));
        assert_eq!(planner.provider_count(), 1);
        assert_eq!(planner.tool_count(), 4);
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
        assert_eq!(response.entries.len(), 2);
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "readme.md" && entry.entry_type == EntryType::File));
    }

    #[test]
    fn read_file_returns_unified_content() {
        let dir = temp_dir();
        let path = dir.join("spec.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "# Spec\n\nDetails.").unwrap();

        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::read_file(&path)).unwrap();

        assert_eq!(response.capability, Some(Capability::ReadContent));
        assert_eq!(response.tool_id.as_deref(), Some("read_content"));
        let content = response.content.expect("content");
        assert_eq!(content.source, ContentSource::File);
        assert_eq!(content.content_type, ContentType::Markdown);
        assert_eq!(content.mime_type, "text/markdown");
        assert_eq!(content.title.as_deref(), Some("Spec"));
        assert_eq!(content.parser_id, "markdown");
        assert!(content.text.contains("Details."));
        assert!(response.summary.contains("markdown"));
    }

    #[test]
    fn chat_message_flows_through_chat_capability() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("What can you help with?"))
            .unwrap();
        assert_eq!(response.capability, Some(Capability::Chat));
        assert!(response.tool_id.is_none());
        assert!(response.assistant_text().contains("What can you help with?"));
    }

    #[test]
    fn what_exists_queries_knowledge_index() {
        let dir = temp_dir();
        write!(File::create(dir.join("invoice.txt")).unwrap(), "alpha").unwrap();

        let mut capabilities = CapabilityRegistry::new();
        capabilities.initialize().unwrap();
        capabilities.register(Capability::Chat).unwrap();
        capabilities.register(Capability::Search).unwrap();
        capabilities.register(Capability::ReadContent).unwrap();

        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        providers.register(&filesystem).unwrap();
        let filesystem = Arc::new(filesystem);

        let mut database = Database::new();
        database.initialize().unwrap();
        let database = Arc::new(database);
        let contents = Arc::new(default_registry().unwrap());

        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        tools
            .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
            .unwrap();
        tools
            .register_tool(Arc::new(SearchIndexTool::new(Arc::clone(&database))))
            .unwrap();
        tools
            .register_tool(Arc::new(IndexFilesTool::new(
                Arc::clone(&filesystem),
                Arc::clone(&database),
                vec![IndexRoot::new("workspace", dir.clone())],
            )))
            .unwrap();
        tools
            .register_tool(Arc::new(ReadContentTool::new(filesystem, contents)))
            .unwrap();
        let tools = Arc::new(tools);
        let mut planner = Planner::new(PlannerDeps {
            capabilities: Arc::new(capabilities),
            providers: Arc::new(providers),
            tools: Arc::clone(&tools),
            orchestrator: ToolOrchestrator::new(tools),
        });
        planner.initialize().unwrap();

        planner
            .handle(UserRequest::new("index my files"))
            .unwrap();
        let response = planner
            .handle(UserRequest::new("What exists?"))
            .unwrap();
        assert_eq!(response.capability, Some(Capability::Search));
        assert_eq!(response.tool_id.as_deref(), Some("search_index"));
        assert!(response.entries.iter().any(|entry| entry.name == "invoice.txt"));
    }

    #[test]
    fn planner_does_not_call_filesystem_for_chat_intent() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("sing a song")).unwrap();
        assert!(response.entries.is_empty());
        assert!(response.content.is_none());
        assert_eq!(response.capability, Some(Capability::Chat));
    }
}
