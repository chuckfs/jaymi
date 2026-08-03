//! Deterministic application boot sequence.
//!
//! Startup order:
//! Configuration → Logging → Database → Policy Engine → Permission Engine →
//! Memory Engine → Context Engine → Capability Registry → Provider Registry →
//! Knowledge → Understanding → Search → Project Engine → Discovery → Tools →
//! Planner → Desktop UI

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jaymi_capabilities::{
    Capability, CapabilityDiscoveryReport, CapabilityEngine, CapabilityEngineApi,
    CapabilityInspectorReport, CapabilityState, CodingState, CreationState, ResearchState,
    WorkspaceKind,
};

use jaymi_config::Config;
use jaymi_context::ContextEngine;
use jaymi_core::{
    AppState, DiscoveryQueryKind, HealthReport, JaymiError, JaymiResult, Lifecycle,
    SearchRequest, ServiceContainer, UserRequest,
};
use jaymi_database::Database;
use jaymi_discovery::{DiscoveryEngine, FilesystemWatcher};
use jaymi_knowledge::{KnowledgeStore, SqliteKnowledgeStore};
use jaymi_logging::Logger;
use jaymi_memory::{
    AppendMessageRequest, ArchiveConversationRequest, AssembleContextRequest, AssembledMemoryContext,
    Conversation, ConversationMessage, ConversationMeta, CreateConversationRequest,
    CreatePersonalMemoryRequest, ListProjectDecisionsQuery, MemoryEngine, MemoryEngineApi,
    MemoryQuery, MemoryRecord, PersonalContext, ProjectDecision, PromoteMemoryRequest,
    PromotionAskDecision, PromotionSuggestQuery, PromotionSuggestion, SqliteMemoryStore,
    StoreMemoryRequest, StoreProjectDecisionRequest, StoreProjectMemoryRequest,
    UpdatePersonalMemoryRequest,
};
use jaymi_parsers::{default_registry, ParserRegistry};
use jaymi_permissions::PermissionEngine;
use jaymi_planner::{Planner, PlannerDeps, PlannerResponse};
use jaymi_policies::PolicyEngine;
use jaymi_project_engine::{
    CreateProjectRequest, Project, ProjectContext, ProjectEngine, ProjectEngineApi, ProjectHealth,
    SqliteProjectStore,
};
use jaymi_providers::{
    EmbeddingProvider, FilesystemProvider, LocalEmbeddingProvider, OcrProvider,
    PlaceholderOcrProvider, Provider, ProviderRegistry,
};
use jaymi_search::{EmbeddingQueue, SearchEngine, SearchEngineApi, SemanticDeps};
use jaymi_tools::{
    QueryInventoryTool, ReadFileTool, ScanFilesystemTool, SearchFilesTool, SearchKnowledgeTool,
    ToolOrchestrator, ToolRegistry,
};
use jaymi_understanding::{
    format_parser_usage, ContentIntelligence, ContentIntelligenceApi, SqliteContentStore,
    UnderstandingEngine,
};

use crate::diagnostics::DiagnosticsSnapshot;
use crate::experience::ExperienceSession;

/// Owns the process service container and application state.
pub struct Application {
    state: AppState,
    container: ServiceContainer,
    health_reports: Vec<HealthReport>,
    /// Conversation-first experience (workspaces expand without destroying chat).
    experience: Mutex<ExperienceSession>,
}

impl Application {
    /// Create an application in the `Starting` state.
    pub fn new() -> Self {
        Self {
            state: AppState::Starting,
            container: ServiceContainer::new(),
            health_reports: Vec::new(),
            experience: Mutex::new(ExperienceSession::new()),
        }
    }

    /// Current application state.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Immutable access to the service container.
    pub fn container(&self) -> &ServiceContainer {
        &self.container
    }

    /// Health reports collected during boot.
    pub fn health_reports(&self) -> &[HealthReport] {
        &self.health_reports
    }

    /// Run the deterministic boot sequence through Planner initialization.
    pub fn boot() -> JaymiResult<Self> {
        Self::boot_with_data_dir_override(None)
    }

    /// Boot using an explicit data directory (used by tests and isolated runs).
    pub fn boot_with_data_dir(data_dir: impl AsRef<Path>) -> JaymiResult<Self> {
        Self::boot_with_data_dir_override(Some(data_dir.as_ref().to_path_buf()))
    }

    fn boot_with_data_dir_override(data_dir: Option<PathBuf>) -> JaymiResult<Self> {
        let mut app = Self::new();

        if let Err(error) = app.boot_inner(data_dir) {
            app.state = AppState::Error {
                message: error.message().to_string(),
            };
            let _ = app.shutdown_initialized();
            return Err(error);
        }

        app.state = AppState::Ready;
        Ok(app)
    }

    fn boot_inner(&mut self, data_dir_override: Option<PathBuf>) -> JaymiResult<()> {
        let config = match data_dir_override {
            Some(data_dir) => Config::with_data_dir(data_dir),
            None => Config::new(),
        };
        self.boot_service(config)?;

        let (data_dir, log_level) = {
            let config = self.container.resolve::<Config>()?;
            (
                PathBuf::from(&config.data_dir),
                map_log_level(config.settings().log_level),
            )
        };
        self.boot_service(Logger::with_data_dir_and_level(&data_dir, log_level))?;
        {
            let logger = self.container.resolve::<Logger>()?;
            logger.info(
                "boot",
                format!("Jaymi startup data_dir={}", data_dir.display()),
            );
        }
        self.boot_service(Database::with_data_dir(&data_dir))?;
        // Share the database handle with Layer 1 discovery without changing open/migrate.
        let database = {
            let database = self
                .container
                .take::<Database>()
                .ok_or_else(|| JaymiError::new("database missing after boot"))?;
            let database = Arc::new(database);
            self.container.register(Arc::clone(&database));
            database
        };

        let mut policies = PolicyEngine::new();
        self.initialize_service(&mut policies)?;
        let policies = Arc::new(policies);
        self.container.register(Arc::clone(&policies));

        let mut permissions = PermissionEngine::new();
        self.initialize_service(&mut permissions)?;
        let permissions = Arc::new(permissions);
        self.container.register(Arc::clone(&permissions));

        // Memory Engine — centralized intentional memory (Planner never touches storage).
        let mut memory = MemoryEngine::new(Arc::new(SqliteMemoryStore::new(Arc::clone(&database))));
        self.initialize_service(&mut memory)?;
        let memory = Arc::new(memory);
        self.container.register(Arc::clone(&memory));

        // Context Engine — lifecycle first; sources bound after Project + Search.
        let mut context = ContextEngine::new();
        self.initialize_service(&mut context)?;
        let context = Arc::new(context);
        self.container.register(Arc::clone(&context));

        // Capability Engine — full catalog stays registered; availability
        // (Ready / Experimental / Planned / Unavailable) decides executability.
        let mut capabilities = CapabilityEngine::new();
        self.initialize_service(&mut capabilities)?;
        for capability in Capability::all() {
            capabilities.register(*capability)?;
        }
        let capabilities = Arc::new(capabilities);
        self.container.register(Arc::clone(&capabilities));

        // Provider registry + Filesystem Provider + Placeholder OCR Provider.
        let mut providers = ProviderRegistry::new();
        self.initialize_service(&mut providers)?;
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize()?;
        providers.register(&filesystem)?;
        let filesystem = Arc::new(filesystem);
        self.container.register(Arc::clone(&filesystem));

        let mut ocr = PlaceholderOcrProvider::new();
        ocr.initialize()?;
        providers.register(&ocr)?;
        let ocr = Arc::new(ocr);
        self.container.register(Arc::clone(&ocr));

        let mut embedding_impl = LocalEmbeddingProvider::new();
        embedding_impl.initialize()?;
        providers.register(&embedding_impl)?;
        let embedding_impl = Arc::new(embedding_impl);
        self.container.register(Arc::clone(&embedding_impl));
        let embedding: Arc<dyn EmbeddingProvider> = embedding_impl;

        let providers = Arc::new(providers);
        self.container.register(Arc::clone(&providers));

        // Parser registry with built-in TXT / Markdown / JSON parsers.
        let parsers = Arc::new(default_registry()?);
        self.container.register(Arc::clone(&parsers));

        // Knowledge API — single interface to indexed inventory (no direct SQLite for consumers).
        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&database));
        self.initialize_service(&mut knowledge)?;
        let knowledge = Arc::new(knowledge);
        self.container.register(Arc::clone(&knowledge));

        // Embedding queue (async generation) — separate from normalized content.
        let mut embedding_queue =
            EmbeddingQueue::new(Arc::clone(&database), Arc::clone(&embedding));
        self.initialize_service(&mut embedding_queue)?;
        let embedding_queue = Arc::new(embedding_queue);
        self.container.register(Arc::clone(&embedding_queue));

        // Content store + Understanding Engine (Layer 2).
        let content = Arc::new(SqliteContentStore::new(Arc::clone(&database)));
        self.container.register(Arc::clone(&content));
        let mut understanding = UnderstandingEngine::with_embedding_scheduler(
            Arc::clone(&knowledge),
            Arc::clone(&content),
            Arc::clone(&filesystem),
            Arc::clone(&parsers),
            Some(Arc::clone(&embedding_queue) as Arc<dyn jaymi_understanding::EmbeddingScheduler>),
        );
        self.initialize_service(&mut understanding)?;
        let understanding = Arc::new(understanding);
        self.container.register(Arc::clone(&understanding));

        // Content Intelligence API — stable consumer surface (hides parsers/SQLite).
        let content_api = Arc::new(ContentIntelligenceApi::new(Arc::clone(&understanding)));
        self.container.register(Arc::clone(&content_api));

        // Search Engine — single retrieval entry point (Planner tools use this, not SQLite).
        let mut search = SearchEngine::with_semantic(
            Arc::clone(&knowledge),
            Some(Arc::clone(&content_api)),
            Some(SemanticDeps {
                database: Arc::clone(&database),
                provider: Arc::clone(&embedding),
            }),
        );
        self.initialize_service(&mut search)?;
        let search = Arc::new(search);
        self.container.register(Arc::clone(&search));

        // Project Engine — first-class persistent projects (after Search so context sources exist).
        let mut projects =
            ProjectEngine::new(Arc::new(SqliteProjectStore::new(Arc::clone(&database))));
        self.initialize_service(&mut projects)?;
        let projects = Arc::new(projects);
        self.container.register(Arc::clone(&projects));

        // Bind Memory / Knowledge / Search / Content so Project Engine can assemble knowledge.
        projects
            .bind_sources(jaymi_project_engine::ProjectContextSources {
                memory: Arc::clone(&memory) as Arc<dyn jaymi_memory::MemoryEngineApi>,
                knowledge: Arc::clone(&knowledge) as Arc<dyn jaymi_knowledge::KnowledgeStore>,
                search: Arc::clone(&search) as Arc<dyn jaymi_search::SearchEngineApi>,
                content: Some(
                    Arc::clone(&content_api) as Arc<dyn jaymi_understanding::ContentIntelligence>
                ),
            })?;

        // Context Engine coordinates Memory + Project + Search for every request.
        context.bind_sources(jaymi_context::ContextSources {
            memory: Arc::clone(&memory) as Arc<dyn jaymi_memory::MemoryEngineApi>,
            projects: Arc::clone(&projects) as Arc<dyn ProjectEngineApi>,
            search: Arc::clone(&search) as Arc<dyn jaymi_search::SearchEngineApi>,
        })?;

        // Discovery engine (Layer 1) — explicit scans only; no boot-time crawl.
        let (discovery_roots, indexing_enabled) = {
            let config = self.container.resolve::<Config>()?;
            (
                config
                    .settings()
                    .discovery_roots
                    .iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>(),
                config.settings().indexing_enabled,
            )
        };
        let mut discovery = DiscoveryEngine::new(
            Arc::clone(&knowledge),
            discovery_roots,
            indexing_enabled,
        );
        self.initialize_service(&mut discovery)?;
        let discovery = Arc::new(discovery);
        self.container.register(Arc::clone(&discovery));

        // Filesystem watcher keeps the inventory synchronized with configured roots.
        let mut watcher = FilesystemWatcher::new(Arc::clone(&discovery));
        self.initialize_service(&mut watcher)?;
        let watcher = Arc::new(watcher);
        self.container.register(Arc::clone(&watcher));

        // Tool registry + Layer 0–3 tools.
        let mut tools = ToolRegistry::new();
        self.initialize_service(&mut tools)?;
        tools.register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))?;
        tools.register_tool(Arc::new(SearchKnowledgeTool::new(Arc::clone(&search))))?;
        tools.register_tool(Arc::new(ReadFileTool::new(Arc::clone(&content_api))))?;
        tools.register_tool(Arc::new(ScanFilesystemTool::new(Arc::clone(&discovery))))?;
        tools.register_tool(Arc::new(QueryInventoryTool::new(Arc::clone(&search))))?;
        let tools = Arc::new(tools);
        self.container.register(Arc::clone(&tools));

        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));
        let mut planner = Planner::new(PlannerDeps {
            capabilities: Arc::clone(&capabilities) as Arc<dyn CapabilityEngineApi>,
            providers,
            tools,
            orchestrator,
            policies,
            permissions,
            memory: Arc::clone(&memory) as Arc<dyn MemoryEngineApi>,
            projects: Arc::clone(&projects) as Arc<dyn ProjectEngineApi>,
            context: Arc::clone(&context),
        });
        self.initialize_service(&mut planner)?;
        self.container.register(planner);

        {
            let logger = self.container.resolve::<Logger>()?;
            logger.info("boot", "Jaymi startup complete");
        }

        Ok(())
    }

    fn boot_service<T>(&mut self, mut service: T) -> JaymiResult<()>
    where
        T: Lifecycle + 'static,
    {
        self.initialize_service(&mut service)?;
        self.container.register(service);
        Ok(())
    }

    fn initialize_service<T>(&mut self, service: &mut T) -> JaymiResult<()>
    where
        T: Lifecycle,
    {
        self.ensure_dependencies(service)?;
        service.initialize().map_err(|error| {
            JaymiError::new(format!(
                "failed to initialize {}: {}",
                service.name(),
                error.message()
            ))
        })?;

        let report = service.health_check();
        // Boot requires successful initialization. Operational readiness
        // (`healthy`) is surfaced honestly in diagnostics and may be false for
        // stub subsystems that are intentionally not feature-complete yet.
        if !report.initialized {
            return Err(JaymiError::new(format!(
                "subsystem {} failed to initialize",
                service.name()
            )));
        }

        self.health_reports.push(report);
        Ok(())
    }

    fn ensure_dependencies<T>(&self, service: &T) -> JaymiResult<()>
    where
        T: Lifecycle,
    {
        for dependency in service.dependencies() {
            let satisfied = self
                .health_reports
                .iter()
                .any(|report| report.name == *dependency && report.initialized);
            if !satisfied {
                return Err(JaymiError::new(format!(
                    "missing initialized dependency '{}' for {}",
                    dependency,
                    service.name()
                )));
            }
        }
        Ok(())
    }

    /// Sync experience session workspace into the Context Engine before handle.
    fn prepare_context_session(&self) -> JaymiResult<()> {
        let context = self.container.resolve::<Arc<ContextEngine>>()?;
        let workspace = self
            .experience()
            .ok()
            .and_then(|session| session.active_workspace_kind())
            .map(|kind| kind.id().to_string());
        context.set_session_workspace(workspace);
        Ok(())
    }

    /// Route a user request through the Planner (Context Engine assembles first).
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        self.prepare_context_session()?;
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(request)
    }

    /// Ask the Planner to list a single directory through the full architecture.
    pub fn list_directory(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::list_directory(path.as_ref()))
    }

    /// Ask the Planner to read a supported file into a unified document.
    pub fn read_file(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::read_file(path.as_ref()))
    }

    /// Ask the Planner to recursively index a root into the discovery inventory.
    pub fn index_root(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::index_root(path.as_ref()))
    }

    /// Ask the Planner what files exist using the Search Engine (inventory).
    pub fn discover_inventory(&self) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::discover_inventory())
    }

    /// Ask the Planner a structured discovery query against the knowledge database.
    pub fn discover_query(&self, kind: DiscoveryQueryKind) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::discover_query(kind))
    }

    /// Ask the Planner a structured Search Engine request.
    pub fn search(&self, request: SearchRequest) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::search(request))
    }

    /// Retrieve memories through the Memory Engine.
    pub fn retrieve_memory(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.retrieve(query)
    }

    /// Assemble relevant memory context through the Memory Engine.
    pub fn assemble_memory_context(
        &self,
        request: &AssembleContextRequest,
    ) -> JaymiResult<AssembledMemoryContext> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.assemble_context(request)
    }

    /// Store an intentional memory through the Memory Engine.
    pub fn store_memory(&self, request: &StoreMemoryRequest) -> JaymiResult<MemoryRecord> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.store(request)
    }

    /// Forget a memory through the Memory Engine.
    pub fn forget_memory(&self, memory_id: &str) -> JaymiResult<()> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.forget(memory_id)
    }

    /// Promote a memory up the durability ladder through the Memory Engine.
    pub fn promote_memory(
        &self,
        request: &PromoteMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.promote(request)
    }

    /// Ask the Memory Engine for promotion suggestions (never applies them).
    pub fn suggest_memory_promotions(
        &self,
        query: &PromotionSuggestQuery,
    ) -> JaymiResult<Vec<PromotionSuggestion>> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.suggest_promotions(query)
    }

    /// Decide whether to ask the user about promotion suggestions.
    pub fn decide_promotion_ask(
        &self,
        suggestions: &[PromotionSuggestion],
    ) -> JaymiResult<PromotionAskDecision> {
        Ok(PromotionAskDecision::from_suggestions(suggestions))
    }

    /// Archive a conversation through the Memory Engine.
    pub fn archive_conversation(
        &self,
        request: &ArchiveConversationRequest,
    ) -> JaymiResult<jaymi_memory::ConversationArchive> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.archive_conversation(request)
    }

    /// Create a persisted conversation through the Memory Engine.
    pub fn create_conversation(
        &self,
        request: &CreateConversationRequest,
    ) -> JaymiResult<ConversationMeta> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.create_conversation(request)
    }

    /// Append a message to a conversation through the Memory Engine.
    pub fn append_message(
        &self,
        request: &AppendMessageRequest,
    ) -> JaymiResult<ConversationMessage> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.append_message(request)
    }

    /// Load an entire conversation through the Memory Engine.
    pub fn load_conversation(
        &self,
        conversation_id: &str,
    ) -> JaymiResult<Option<Conversation>> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.load_conversation(conversation_id)
    }

    /// List conversations attached to a project through the Memory Engine.
    pub fn list_project_conversations(
        &self,
        project_id: &str,
    ) -> JaymiResult<Vec<ConversationMeta>> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.list_conversations_for_project(project_id)
    }

    /// Attach a conversation to exactly one project, or detach it (`None` = global).
    pub fn attach_conversation_to_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
    ) -> JaymiResult<ConversationMeta> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.attach_conversation_to_project(conversation_id, project_id)
    }

    /// Create a first-class project through the Project Engine.
    pub fn create_project(
        &self,
        request: &CreateProjectRequest,
    ) -> JaymiResult<Project> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.create(request)
    }

    /// Open a project: bind Memory session hints, open via Project Engine, resume conversation.
    pub fn open_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        memory.set_active_project(Some(project_id))?;
        let context = projects.open(project_id)?;
        if memory.active_conversation_id().is_none() {
            if let Some(latest) = memory
                .list_conversations_for_project(project_id)?
                .into_iter()
                .next()
            {
                memory.set_active_conversation(Some(latest.id.as_str()))?;
            }
        }
        Ok(context)
    }

    /// Switch the active workspace to another project by id.
    pub fn switch_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.open_project(project_id)
    }

    /// Close the session-open project.
    /// Does not clear the active conversation.
    pub fn close_project(&self) -> JaymiResult<Option<Project>> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        let closed = projects.close()?;
        let _ = memory.set_active_project(None);
        Ok(closed)
    }

    /// Current active workspace project id, when any.
    pub fn active_project_id(&self) -> Option<String> {
        self.container
            .resolve::<Arc<ProjectEngine>>()
            .ok()
            .and_then(|projects| projects.active_project_id())
    }

    /// Current active conversation id (persists across project switch).
    pub fn active_conversation_id(&self) -> Option<String> {
        self.container
            .resolve::<Arc<MemoryEngine>>()
            .ok()
            .and_then(|memory| memory.active_conversation_id())
    }

    /// Delete a project through the Project Engine.
    pub fn delete_project(&self, project_id: &str) -> JaymiResult<()> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.delete(project_id)
    }

    /// List active projects through the Project Engine.
    pub fn list_projects(&self) -> JaymiResult<Vec<Project>> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.list()
    }

    /// Request one assembled ProjectContext from the Project Engine.
    pub fn project_context(
        &self,
        project_id: Option<&str>,
    ) -> JaymiResult<Option<ProjectContext>> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.project_context(project_id)
    }

    /// Assemble project context for a known project id.
    pub fn assemble_project_context(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.assemble_context(project_id)
    }

    /// Search knowledge belonging to a project (Planner-mediated).
    ///
    /// Always enters [`Planner::handle`]; never calls the Project Engine
    /// directly from Application.
    pub fn search_project_knowledge(
        &self,
        project_id: &str,
        text: &str,
        limit: Option<usize>,
    ) -> JaymiResult<Vec<jaymi_project_engine::ProjectKnowledgeHit>> {
        let response = self.handle(UserRequest::search_project_knowledge(
            project_id, text, limit,
        ))?;
        Ok(response.project_knowledge)
    }

    /// Point Memory at a project id for context assembly (Project Engine owns identity).
    pub fn set_active_project(&self, project_id: Option<&str>) -> JaymiResult<()> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.set_active_project(project_id)
    }

    /// Activate a conversation for memory context assembly.
    pub fn set_active_conversation(&self, conversation_id: Option<&str>) -> JaymiResult<()> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.set_active_conversation(conversation_id)
    }

    /// Store categorized project memory through the Memory Engine.
    pub fn store_project_memory(
        &self,
        request: &StoreProjectMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.store_project_memory(request)
    }

    /// Persist an architectural decision through the Memory Engine.
    pub fn store_project_decision(
        &self,
        request: &StoreProjectDecisionRequest,
    ) -> JaymiResult<ProjectDecision> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.store_project_decision(request)
    }

    /// List a project's decision log through the Memory Engine.
    pub fn list_project_decisions(
        &self,
        query: &ListProjectDecisionsQuery,
    ) -> JaymiResult<Vec<ProjectDecision>> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.list_project_decisions(query)
    }

    /// Fetch one project decision by memory id through the Memory Engine.
    pub fn get_project_decision(&self, memory_id: &str) -> JaymiResult<Option<ProjectDecision>> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.get_project_decision(memory_id)
    }

    /// Restore / assemble project context through the Project Engine.
    pub fn restore_project_context(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.assemble_project_context(project_id)
    }

    /// Continue working on a named project (restores project memory automatically).
    pub fn continue_project(&self, name: &str) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::new(format!("Continue working on {name}.")))
    }

    /// Discover registered capabilities through the Planner / Capability Engine.
    pub fn discover_capabilities(&self) -> JaymiResult<Vec<Capability>> {
        let planner = self.container.resolve::<Planner>()?;
        Ok(planner.discover_capabilities())
    }

    /// Discover available vs unavailable capabilities (with tool/provider requirements).
    pub fn discover_capability_status(&self) -> JaymiResult<CapabilityDiscoveryReport> {
        let planner = self.container.resolve::<Planner>()?;
        planner.discover_capability_status()
    }

    /// Inspect the capability system for developers.
    ///
    /// Includes registered capabilities, active (runtime-available) capabilities,
    /// workspace associations, and required tools/providers. Optionally attaches
    /// the session's expanded workspace.
    pub fn inspect_capabilities(&self) -> JaymiResult<CapabilityInspectorReport> {
        let planner = self.container.resolve::<Planner>()?;
        let report = planner.inspect_capabilities()?;
        Ok(report.with_active_workspace(self.active_ui_workspace()?))
    }

    /// Describe a capability through the Capability Engine (catalog metadata).
    pub fn describe_capability(
        &self,
        capability: Capability,
    ) -> JaymiResult<jaymi_capabilities::CapabilityDescriptor> {
        let planner = self.container.resolve::<Planner>()?;
        Ok(planner.describe_capability(capability))
    }

    /// Resolve a registered capability by id through the Capability Engine.
    pub fn resolve_capability(
        &self,
        id: &str,
    ) -> JaymiResult<Option<jaymi_capabilities::CapabilityDescriptor>> {
        let planner = self.container.resolve::<Planner>()?;
        planner.resolve_capability(id)
    }

    /// Build a capability execution plan through the Planner (does not execute work).
    pub fn build_capability_plan(
        &self,
        capabilities: &[Capability],
    ) -> JaymiResult<jaymi_capabilities::ExecutionPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.build_capability_plan(capabilities)
    }

    /// Plan work for one capability and optional goal (does not execute tools).
    pub fn plan_capability(
        &self,
        capability: Capability,
        goal: Option<&str>,
    ) -> JaymiResult<jaymi_capabilities::ExecutionPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.plan_capability(capability, goal)
    }

    /// Compose independent capabilities into one execution plan (no execution).
    pub fn plan_capabilities(
        &self,
        capabilities: &[Capability],
        goal: Option<&str>,
    ) -> JaymiResult<jaymi_capabilities::ExecutionPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.plan_capabilities(capabilities, goal)
    }

    /// Compose from a [`jaymi_capabilities::CapabilityComposition`] value.
    pub fn compose_capability_plan(
        &self,
        composition: &jaymi_capabilities::CapabilityComposition,
    ) -> JaymiResult<jaymi_capabilities::ExecutionPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.compose_capability_plan(composition)
    }

    /// Snapshot of the conversation-first experience session.
    pub fn experience(&self) -> JaymiResult<ExperienceSession> {
        self.experience
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| JaymiError::new("experience session lock poisoned"))
    }

    /// Active UI workspace kind, when expanded beside the conversation.
    pub fn active_ui_workspace(&self) -> JaymiResult<Option<WorkspaceKind>> {
        Ok(self.experience()?.active_workspace_kind())
    }

    /// Expand a workspace from a Planner response without clearing conversation.
    pub fn apply_workspace_response(&self, response: &PlannerResponse) -> JaymiResult<()> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.apply_planner_response(response);
        Ok(())
    }

    /// Expand a capability workspace beside the conversation.
    ///
    /// Switching workspace kinds replaces temporary capability state so kinds
    /// stay isolated. Conversation turns are never cleared.
    pub fn expand_ui_workspace(
        &self,
        expansion: jaymi_capabilities::WorkspaceExpansion,
    ) -> JaymiResult<()> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.expand_workspace(expansion)
    }

    /// Close the expanded workspace; conversation turns remain intact.
    ///
    /// Capability runtime state is discarded with the workspace.
    pub fn close_ui_workspace(&self) -> JaymiResult<Option<jaymi_capabilities::WorkspaceExpansion>> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        let turns_before = experience.turn_count();
        let closed = experience.close_workspace();
        if experience.capability_state().is_some() {
            return Err(JaymiError::new(
                "closing a workspace must discard capability state",
            ));
        }
        if experience.turn_count() != turns_before {
            return Err(JaymiError::new(
                "closing a workspace must not destroy the conversation",
            ));
        }
        Ok(closed)
    }

    /// Temporary capability state for the active workspace, when any.
    pub fn capability_state(&self) -> JaymiResult<Option<CapabilityState>> {
        Ok(self.experience()?.capability_state().cloned())
    }

    /// Mutate coding workspace state (fails when Coding is not active).
    pub fn with_coding_state<R>(
        &self,
        update: impl FnOnce(&mut CodingState) -> R,
    ) -> JaymiResult<R> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.with_coding_state(update)
    }

    /// Mutate creation workspace state (fails when Creation is not active).
    pub fn with_creation_state<R>(
        &self,
        update: impl FnOnce(&mut CreationState) -> R,
    ) -> JaymiResult<R> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.with_creation_state(update)
    }

    /// Mutate research workspace state (fails when Research is not active).
    pub fn with_research_state<R>(
        &self,
        update: impl FnOnce(&mut ResearchState) -> R,
    ) -> JaymiResult<R> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.with_research_state(update)
    }

    /// Promote a capability-state entry into the durable conversation.
    ///
    /// The temporary workspace entry is summarized into conversation; the
    /// summary survives workspace close even though runtime state does not.
    pub fn promote_capability_entry(&self, entry_id: &str) -> JaymiResult<String> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.promote_capability_entry(entry_id)
    }

    /// Record a user message in the durable conversation transcript.
    pub fn record_user_message(&self, content: impl Into<String>) -> JaymiResult<()> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.record_user_message(content);
        Ok(())
    }

    /// Handle a user request and apply any capability workspace expansion.
    pub fn handle_with_workspace(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        let content = request.content.clone();
        if !content.trim().is_empty() {
            self.record_user_message(content)?;
        }
        let response = self.handle(request)?;
        self.apply_workspace_response(&response)?;
        Ok(response)
    }

    /// Create an intentional personal preference through the Memory Engine.
    pub fn create_personal_memory(
        &self,
        request: &CreatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.create_personal_memory(request)
    }

    /// Update a personal preference through the Memory Engine.
    pub fn update_personal_memory(
        &self,
        request: &UpdatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.update_personal_memory(request)
    }

    /// Delete a personal preference through the Memory Engine.
    pub fn delete_personal_memory(&self, memory_id: &str) -> JaymiResult<()> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.delete_personal_memory(memory_id)
    }

    /// Load active personal preferences through the Memory Engine.
    pub fn personal_context(&self) -> JaymiResult<PersonalContext> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.personal_context()
    }

    /// Ask the Planner to list active logical collections from the inventory.
    pub fn list_collections(&self) -> JaymiResult<PlannerResponse> {
        self.discover_query(DiscoveryQueryKind::Collections)
    }

    /// Build the diagnostics snapshot for the temporary UI.
    pub fn diagnostics(&self) -> JaymiResult<DiagnosticsSnapshot> {
        self.diagnostics_from_response(None)
    }

    /// Build diagnostics including an optional Planner response.
    pub fn diagnostics_from_response(
        &self,
        response: Option<PlannerResponse>,
    ) -> JaymiResult<DiagnosticsSnapshot> {
        use crate::diagnostics::{OperationalStatus, SubsystemStatus};

        let planner = self.container.resolve::<Planner>()?;
        let database = self.container.resolve::<Arc<Database>>()?;
        let logger = self.container.resolve::<Logger>()?;
        let config = self.container.resolve::<Config>()?;
        let discovery = self.container.resolve::<Arc<DiscoveryEngine>>()?;
        let knowledge = self.container.resolve::<Arc<SqliteKnowledgeStore>>()?;
        let understanding = self.container.resolve::<Arc<UnderstandingEngine>>()?;
        let content_api = self.container.resolve::<Arc<ContentIntelligenceApi>>()?;
        let search = self.container.resolve::<Arc<SearchEngine>>()?;
        let watcher = self.container.resolve::<Arc<FilesystemWatcher>>()?;
        let policies = self.container.resolve::<Arc<PolicyEngine>>()?;
        let permissions = self.container.resolve::<Arc<PermissionEngine>>()?;
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        let context_engine = self.container.resolve::<Arc<ContextEngine>>()?;
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        let capabilities = self.container.resolve::<Arc<CapabilityEngine>>()?;
        let providers = self.container.resolve::<Arc<ProviderRegistry>>()?;
        let ocr = self.container.resolve::<Arc<PlaceholderOcrProvider>>()?;
        let embedding = self.container.resolve::<Arc<LocalEmbeddingProvider>>()?;
        let embedding_queue = self.container.resolve::<Arc<EmbeddingQueue>>()?;
        let tools = self.container.resolve::<Arc<ToolRegistry>>()?;
        let parsers = self.container.resolve::<Arc<ParserRegistry>>()?;

        let planner_health = planner.health_check();
        let database_health = database.health_check();
        let logger_health = logger.health_check();
        let config_health = config.health_check();
        let policies_health = policies.health_check();
        let permissions_health = permissions.health_check();
        let memory_report = memory.health_check();
        let memory_status = memory.health().unwrap_or_else(|_| jaymi_memory::MemoryHealth {
            initialized: memory_report.initialized,
            healthy: false,
            version: memory_report.version.clone(),
            detail: "memory engine health unavailable".into(),
            statistics: jaymi_memory::MemoryStats::default(),
        });
        let context_health = context_engine.health_check();
        let project_report = projects.health_check();
        let project_status = projects.health().unwrap_or_else(|_| ProjectHealth {
            initialized: project_report.initialized,
            healthy: false,
            version: project_report.version.clone(),
            detail: "project engine health unavailable".into(),
            statistics: Default::default(),
        });
        let discovery_health = discovery.health_check();
        let knowledge_health = knowledge.health_check();
        let understanding_health = understanding.health_check();
        let understanding_stats = understanding.stats().ok();
        let content_health = content_api.retrieve_health().ok();
        let search_health = search.health().ok();
        let discovery_stats = knowledge.stats().ok();
        let collection_stats = knowledge.collection_stats().ok();
        let watcher_diagnostics = watcher.diagnostics();
        let ocr_status = ocr.ocr_status();
        let embedding_status = embedding.embedding_status();
        let embedding_diagnostics = embedding_queue.diagnostics().unwrap_or_default();

        let capability_ids: Vec<String> = capabilities
            .list()
            .into_iter()
            .map(|capability| capability.id().to_string())
            .collect();
        let discovery = planner
            .discover_capability_status()
            .unwrap_or_default();
        let available_capability_ids: Vec<String> = discovery
            .available
            .iter()
            .map(|status| status.descriptor.id.to_string())
            .collect();
        let unavailable_capability_ids: Vec<String> = discovery
            .unavailable
            .iter()
            .map(|status| status.descriptor.id.to_string())
            .collect();
        let capability_status_details: Vec<String> = discovery
            .all()
            .into_iter()
            .map(|status| status.detail())
            .collect();
        let capability_inspector = planner
            .inspect_capabilities()
            .ok()
            .map(|report| {
                report.with_active_workspace(
                    self.active_ui_workspace().ok().flatten(),
                )
            });
        let provider_ids: Vec<String> = providers
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|identity| identity.id)
            .collect();
        let tool_ids: Vec<String> = tools
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|metadata| metadata.id)
            .collect();
        let parser_ids = parsers.parser_ids();
        let active_policies: Vec<String> = policies
            .resolve()
            .unwrap_or_default()
            .into_iter()
            .map(|policy| policy.name)
            .collect();

        let logging_level = match logger.min_level() {
            jaymi_logging::LogLevel::Info => "info",
            jaymi_logging::LogLevel::Warn => "warn",
            jaymi_logging::LogLevel::Error => "error",
        }
        .to_string();

        let indexing_enabled = config.settings().indexing_enabled;
        let permission_mode = "filesystem read auto-allowed; write/internet denied".to_string();

        let subsystems = vec![
            SubsystemStatus::new(
                "Planner",
                if planner_health.healthy {
                    OperationalStatus::Operational
                } else if planner_health.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                format!(
                    "initialized={} tools={} providers={}",
                    planner_health.initialized,
                    planner.tool_count(),
                    planner.provider_count()
                ),
            ),
            SubsystemStatus::new(
                "Database",
                if database.is_connected() && database_health.healthy {
                    OperationalStatus::Operational
                } else if database_health.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                format!(
                    "schema v{} · {} · {}",
                    database.schema_version(),
                    database.migration_status().display(),
                    database.path().display()
                ),
            ),
            SubsystemStatus::new(
                "Configuration",
                if config_health.healthy {
                    OperationalStatus::Operational
                } else if config_health.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                format!(
                    "log_level={} theme={} indexing_enabled={} · {}",
                    config.settings().log_level.as_str(),
                    config.settings().theme.as_str(),
                    indexing_enabled,
                    config.config_path().display()
                ),
            ),
            SubsystemStatus::new(
                "Logging",
                if logger_health.healthy {
                    OperationalStatus::Operational
                } else if logger_health.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                format!(
                    "level={} · {}",
                    logging_level,
                    logger.log_path().display()
                ),
            ),
            SubsystemStatus::new(
                "Permissions",
                if permissions_health.healthy {
                    OperationalStatus::Operational
                } else if permissions_health.initialized {
                    OperationalStatus::Stub
                } else {
                    OperationalStatus::Unavailable
                },
                permission_mode.clone(),
            ),
            SubsystemStatus::new(
                "Policies",
                if policies_health.healthy {
                    OperationalStatus::Operational
                } else if policies_health.initialized {
                    OperationalStatus::Stub
                } else {
                    OperationalStatus::Unavailable
                },
                if active_policies.is_empty() {
                    "no active policies".to_string()
                } else {
                    format!("active: {}", active_policies.join(", "))
                },
            ),
            SubsystemStatus::new(
                "Providers",
                if providers.is_initialized() && !provider_ids.is_empty() {
                    OperationalStatus::Operational
                } else if providers.is_initialized() {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                if provider_ids.is_empty() {
                    "none registered".to_string()
                } else {
                    format!("{} · {}", provider_ids.len(), provider_ids.join(", "))
                },
            ),
            SubsystemStatus::new(
                "OCR Provider",
                if !ocr_status.initialized {
                    OperationalStatus::Unavailable
                } else if ocr_status.placeholder {
                    OperationalStatus::Stub
                } else if ocr_status.available {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Degraded
                },
                format!(
                    "id={} · engine={} · available={} · {}",
                    ocr_status.provider_id,
                    ocr_status.engine,
                    ocr_status.available,
                    ocr_status.detail
                ),
            ),
            SubsystemStatus::new(
                "Embedding Provider",
                if !embedding_status.initialized {
                    OperationalStatus::Unavailable
                } else if embedding_status.available {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Degraded
                },
                format!(
                    "id={} · model={} · dims={} · {}",
                    embedding_status.provider_id,
                    embedding_status.model_id,
                    embedding_status.dimensions,
                    embedding_status.detail
                ),
            ),
            SubsystemStatus::new(
                "Embedding Queue",
                if embedding_diagnostics.running {
                    OperationalStatus::Operational
                } else if embedding_queue.health_check().initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                format!(
                    "indexed={} · model={} · queue={} · processed={} · last={}",
                    embedding_diagnostics.indexed_embeddings,
                    embedding_diagnostics.model_id,
                    embedding_diagnostics.queue_depth,
                    embedding_diagnostics.processed,
                    embedding_diagnostics
                        .last_source_id
                        .clone()
                        .unwrap_or_else(|| "-".to_string())
                ),
            ),
            SubsystemStatus::new(
                "Capabilities",
                if capabilities.is_initialized() && !available_capability_ids.is_empty() {
                    OperationalStatus::Operational
                } else if capabilities.is_initialized() && !capability_ids.is_empty() {
                    OperationalStatus::Degraded
                } else if capabilities.is_initialized() {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                if capability_ids.is_empty() {
                    "none registered".to_string()
                } else if let Some(inspector) = &capability_inspector {
                    inspector.summary()
                } else {
                    format!(
                        "{} · available={} [{}] · unavailable={}",
                        discovery.summary(),
                        available_capability_ids.len(),
                        available_capability_ids.join(", "),
                        unavailable_capability_ids.len()
                    )
                },
            ),
            SubsystemStatus::new(
                "Tools",
                if tools.is_initialized() && !tool_ids.is_empty() {
                    OperationalStatus::Operational
                } else if tools.is_initialized() {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                if tool_ids.is_empty() {
                    "none registered".to_string()
                } else {
                    format!("{} · {}", tool_ids.len(), tool_ids.join(", "))
                },
            ),
            SubsystemStatus::new(
                "Parser Registry",
                if parsers.is_initialized() && !parser_ids.is_empty() {
                    OperationalStatus::Operational
                } else if parsers.is_initialized() {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                if parser_ids.is_empty() {
                    "none registered".to_string()
                } else {
                    let usage = understanding_stats
                        .as_ref()
                        .map(|stats| format_parser_usage(&stats.parser_usage))
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "registered={} · {} · usage={}",
                        parser_ids.len(),
                        parser_ids.join(", "),
                        usage
                    )
                },
            ),
            SubsystemStatus::new(
                "Index Status",
                if discovery_health.initialized {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Unavailable
                },
                {
                    let stats = discovery_stats.clone().unwrap_or_default();
                    format!(
                        "files={} folders={} last_scan={} duration_ms={} added={} updated={} removed={} unchanged={} db_bytes={} indexing_enabled={}",
                        stats.files,
                        stats.folders,
                        stats
                            .last_scan_at
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_scan_duration_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_added
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_updated
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_removed
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_unchanged
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats.database_size_bytes,
                        indexing_enabled
                    )
                },
            ),
            SubsystemStatus::new(
                "Discovery Queries",
                if knowledge_health.initialized {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Unavailable
                },
                {
                    let stats = discovery_stats.clone().unwrap_or_default();
                    format!(
                        "query_count={} last_query={} last_rows={} last_duration_ms={}",
                        stats.query_count,
                        stats
                            .last_query_label
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_query_rows
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_query_duration_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    )
                },
            ),
            SubsystemStatus::new(
                "Collections",
                if knowledge_health.initialized {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Unavailable
                },
                {
                    let stats = collection_stats.clone().unwrap_or_default();
                    let names = if stats.names.is_empty() {
                        "-".to_string()
                    } else {
                        stats.names.join(",")
                    };
                    format!(
                        "collections={} items={} names={}",
                        stats.collection_count, stats.total_items, names
                    )
                },
            ),
            SubsystemStatus::new(
                "Understanding",
                if understanding_health.initialized {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Unavailable
                },
                {
                    let stats = understanding_stats.clone().unwrap_or_default();
                    format!(
                        "parsed_documents={} enriched_documents={} parser_usage={} failed_parses={} unsupported_formats={} cache_hits={} last_failure={} last_unsupported={}",
                        stats.parsed_documents,
                        stats.enriched_documents,
                        format_parser_usage(&stats.parser_usage),
                        stats.failed_parses,
                        stats.unsupported_formats,
                        stats.cache_hits,
                        stats
                            .last_failure
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        stats
                            .last_unsupported
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                    )
                },
            ),
            SubsystemStatus::new(
                "Content Intelligence",
                match &content_health {
                    Some(health) if health.healthy => OperationalStatus::Operational,
                    Some(health) if health.initialized => OperationalStatus::Degraded,
                    Some(_) => OperationalStatus::Unavailable,
                    None => OperationalStatus::Unavailable,
                },
                content_health
                    .as_ref()
                    .map(|health| health.detail.clone())
                    .unwrap_or_else(|| "unavailable".to_string()),
            ),
            SubsystemStatus::new(
                "Search Engine",
                match &search_health {
                    Some(health) if health.healthy => OperationalStatus::Operational,
                    Some(health) if health.initialized => OperationalStatus::Degraded,
                    Some(_) => OperationalStatus::Unavailable,
                    None => OperationalStatus::Unavailable,
                },
                search_health
                    .as_ref()
                    .map(|health| health.detail.clone())
                    .unwrap_or_else(|| "unavailable".to_string()),
            ),
            SubsystemStatus::new(
                "Watcher Status",
                match &watcher_diagnostics.status {
                    jaymi_discovery::WatcherStatus::Watching
                    | jaymi_discovery::WatcherStatus::Idle
                    | jaymi_discovery::WatcherStatus::Disabled => OperationalStatus::Operational,
                    jaymi_discovery::WatcherStatus::Stopped => OperationalStatus::Degraded,
                    jaymi_discovery::WatcherStatus::Error(_) => OperationalStatus::Unavailable,
                },
                {
                    let watched = if watcher_diagnostics.watched_directories.is_empty() {
                        "-".to_string()
                    } else {
                        watcher_diagnostics
                            .watched_directories
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    format!(
                        "status={} watched={} queued={} last_event={}",
                        watcher_diagnostics.status.label(),
                        watched,
                        watcher_diagnostics.queued_updates,
                        watcher_diagnostics
                            .last_event
                            .clone()
                            .unwrap_or_else(|| "-".to_string())
                    )
                },
            ),
            SubsystemStatus::new(
                "Memory Status",
                if memory_status.healthy {
                    OperationalStatus::Operational
                } else if memory_status.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                memory_status.detail.clone(),
            ),
            SubsystemStatus::new(
                "Context Engine",
                if context_health.healthy {
                    OperationalStatus::Operational
                } else if context_health.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                format!(
                    "sources_bound={} · assembles={}",
                    context_engine.sources_bound(),
                    context_engine.assemble_count()
                ),
            ),
            SubsystemStatus::new(
                "Project Status",
                if project_status.healthy {
                    OperationalStatus::Operational
                } else if project_status.initialized {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                project_status.detail.clone(),
            ),
            SubsystemStatus::new(
                "Reasoning Status",
                if planner.reasoning_implemented() {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::NotImplemented
                },
                format!("backend={}", planner.reasoning_status()),
            ),
        ];

        let document = response.as_ref().and_then(|value| value.document.clone());
        Ok(DiagnosticsSnapshot {
            app_state: self.state.clone(),
            subsystems,
            planner_healthy: planner_health.healthy,
            provider_count: providers.len(),
            provider_ids,
            tool_count: tools.len(),
            tool_ids,
            capability_count: capabilities.len(),
            capability_ids,
            available_capability_ids,
            unavailable_capability_ids,
            capability_status_details,
            capability_inspector,
            parser_count: parsers.len(),
            parser_ids,
            database_connected: database.is_connected(),
            database_path: Some(database.path().display().to_string()),
            database_schema_version: Some(database.schema_version()),
            database_migration_status: Some(database.migration_status().display()),
            logging_healthy: logger_health.healthy,
            logging_path: Some(logger.log_path().display().to_string()),
            logging_dir: Some(logger.log_dir().display().to_string()),
            logging_level: Some(logging_level),
            config_path: Some(config.config_path().display().to_string()),
            config_log_level: Some(config.settings().log_level.as_str().to_string()),
            config_theme: Some(config.settings().theme.as_str().to_string()),
            config_indexing_enabled: Some(indexing_enabled),
            active_policies,
            permission_mode: Some(permission_mode),
            permission_decision: response.as_ref().and_then(|value| {
                value
                    .permission_result
                    .as_ref()
                    .map(|result| result.decision.as_str().to_string())
            }),
            permission_explanation: response.as_ref().and_then(|value| {
                value
                    .permission_result
                    .as_ref()
                    .map(|result| result.explanation.clone())
            }),
            policy_allowed: response
                .as_ref()
                .and_then(|value| value.policy_evaluation.as_ref().map(|evaluation| evaluation.allowed)),
            policy_summary: response.as_ref().and_then(|value| {
                value
                    .policy_evaluation
                    .as_ref()
                    .map(|evaluation| evaluation.summary())
            }),
            request_blocked: response.as_ref().map(|value| value.blocked).unwrap_or(false),
            listed_path: response
                .as_ref()
                .and_then(|value| value.listed_path.clone()),
            listing_summary: response.as_ref().and_then(|value| {
                if value.document.is_none() && !value.blocked {
                    Some(value.content.clone())
                } else if value.blocked {
                    Some(value.content.clone())
                } else {
                    None
                }
            }),
            entries: response
                .as_ref()
                .map(|value| value.entries.clone())
                .unwrap_or_default(),
            read_path: document.as_ref().map(|doc| doc.path.clone()),
            read_file_type: document.as_ref().map(|doc| doc.file_type.label()),
            read_parser: document.as_ref().map(|doc| doc.parser_id.clone()),
            read_success: document.is_some(),
            read_character_count: document.as_ref().map(|doc| doc.character_count()),
            read_summary: response.as_ref().and_then(|value| {
                if value.document.is_some() {
                    Some(value.content.clone())
                } else {
                    None
                }
            }),
            read_text: document.map(|doc| doc.text),
        })
    }

    /// Backward-compatible alias used by listing-focused call sites.
    pub fn diagnostics_with_listing(
        &self,
        listing: Option<PlannerResponse>,
    ) -> JaymiResult<DiagnosticsSnapshot> {
        self.diagnostics_from_response(listing)
    }

    /// Shut down subsystems in reverse boot order.
    pub fn shutdown(&mut self) -> JaymiResult<()> {
        self.state = AppState::ShuttingDown;
        self.shutdown_initialized()?;
        self.state = AppState::Starting;
        Ok(())
    }

    fn shutdown_initialized(&mut self) -> JaymiResult<()> {
        if let Ok(logger) = self.container.resolve::<Logger>() {
            logger.info("boot", "Jaymi shutdown beginning");
        }

        if let Some(mut planner) = self.container.take::<Planner>() {
            planner.shutdown()?;
        }

        let _ = self.container.take::<Arc<ToolRegistry>>();
        let _ = self.container.take::<Arc<ProviderRegistry>>();
        let _ = self.container.take::<Arc<CapabilityEngine>>();
        let _ = self.container.take::<Arc<FilesystemProvider>>();
        let _ = self.container.take::<Arc<PlaceholderOcrProvider>>();
        let _ = self.container.take::<Arc<ParserRegistry>>();
        if let Some(watcher) = self.container.take::<Arc<FilesystemWatcher>>() {
            if let Ok(mut watcher) = Arc::try_unwrap(watcher) {
                watcher.shutdown()?;
            }
        }
        let _ = self.container.take::<Arc<DiscoveryEngine>>();
        let _ = self.container.take::<Arc<ContentIntelligenceApi>>();
        if let Some(projects) = self.container.take::<Arc<ProjectEngine>>() {
            if let Ok(mut projects) = Arc::try_unwrap(projects) {
                projects.shutdown()?;
            }
        }
        let _ = self.container.take::<Arc<SearchEngine>>();
        let _ = self.container.take::<Arc<UnderstandingEngine>>();
        let _ = self.container.take::<Arc<SqliteContentStore>>();
        let _ = self.container.take::<Arc<EmbeddingQueue>>();
        let _ = self.container.take::<Arc<SqliteKnowledgeStore>>();

        if let Some(context) = self.container.take::<Arc<ContextEngine>>() {
            if let Ok(mut context) = Arc::try_unwrap(context) {
                context.shutdown()?;
            }
        }
        if let Some(memory) = self.container.take::<Arc<MemoryEngine>>() {
            if let Ok(mut memory) = Arc::try_unwrap(memory) {
                memory.shutdown()?;
            }
        }
        let _ = self.container.take::<Arc<PermissionEngine>>();
        let _ = self.container.take::<Arc<PolicyEngine>>();
        if let Some(database) = self.container.take::<Arc<Database>>() {
            if let Ok(mut database) = Arc::try_unwrap(database) {
                database.shutdown()?;
            }
        }
        shutdown_owned::<Logger>(&mut self.container)?;
        shutdown_owned::<Config>(&mut self.container)?;

        Ok(())
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

fn shutdown_owned<T>(container: &mut ServiceContainer) -> JaymiResult<()>
where
    T: Lifecycle + 'static,
{
    if let Some(mut service) = container.take::<T>() {
        service.shutdown()?;
    }
    Ok(())
}

fn map_log_level(level: jaymi_config::LogLevel) -> jaymi_logging::LogLevel {
    match level {
        jaymi_config::LogLevel::Error => jaymi_logging::LogLevel::Error,
        jaymi_config::LogLevel::Warn => jaymi_logging::LogLevel::Warn,
        jaymi_config::LogLevel::Info => jaymi_logging::LogLevel::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{EntryType, FileType};
    use jaymi_providers::OCR_PROVIDER_ID;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn boot_registers_search_and_read_stack() {
        let data_dir = temp_dir("boot-data");
        let app = Application::boot_with_data_dir(&data_dir).unwrap();
        assert!(app.state().is_ready());

        let diagnostics = app.diagnostics().unwrap();
        assert_eq!(diagnostics.app_state.label(), "Ready");
        assert!(diagnostics.planner_healthy);
        assert_eq!(diagnostics.provider_count, 3);
        assert!(diagnostics
            .provider_ids
            .iter()
            .any(|id| id == OCR_PROVIDER_ID));
        assert_eq!(diagnostics.tool_count, 5);
        assert_eq!(
            diagnostics.capability_count,
            jaymi_capabilities::Capability::all().len()
        );
        assert!(diagnostics.database_connected);
        assert_eq!(
            diagnostics.database_path.as_ref().map(std::path::PathBuf::from),
            Some(data_dir.join("jaymi.db"))
        );
        assert_eq!(
            diagnostics.database_schema_version,
            Some(jaymi_database::CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            diagnostics.database_migration_status.as_deref(),
            Some("applied")
        );
        assert!(data_dir.join("jaymi.db").exists());
        assert!(diagnostics.logging_healthy);
        assert_eq!(
            diagnostics.logging_path.as_ref().map(std::path::PathBuf::from),
            Some(data_dir.join("logs").join("jaymi.log"))
        );
        assert!(data_dir.join("logs").join("jaymi.log").exists());
        assert!(data_dir.join("config.json").exists());
        assert_eq!(
            diagnostics.config_path.as_ref().map(std::path::PathBuf::from),
            Some(data_dir.join("config.json"))
        );
        assert_eq!(diagnostics.config_log_level.as_deref(), Some("info"));
        assert_eq!(diagnostics.config_theme.as_deref(), Some("system"));
        assert_eq!(diagnostics.config_indexing_enabled, Some(true));

        use crate::diagnostics::OperationalStatus;
        assert_eq!(
            diagnostics.subsystem("Planner").unwrap().status,
            OperationalStatus::Operational
        );
        assert_eq!(
            diagnostics.subsystem("Index Status").unwrap().status,
            OperationalStatus::Operational
        );
        assert_eq!(
            diagnostics.subsystem("Memory Status").unwrap().status,
            OperationalStatus::Operational
        );
        assert_eq!(
            diagnostics.subsystem("Reasoning Status").unwrap().status,
            OperationalStatus::NotImplemented
        );
        assert!(!diagnostics.render_dashboard().contains("Healthy"));
    }

    #[test]
    fn list_directory_through_application() {
        let dir = temp_dir("list");
        let mut file = File::create(dir.join("hello.txt")).unwrap();
        write!(file, "hi").unwrap();

        let app = Application::boot_with_data_dir(temp_dir("list-data")).unwrap();
        let response = app.list_directory(&dir).unwrap();
        assert_eq!(response.entries.len(), 1);
        assert_eq!(response.entries[0].entry_type, EntryType::File);
    }

    #[test]
    fn read_file_through_application() {
        let dir = temp_dir("read");
        let path = dir.join("notes.txt");
        let mut file = File::create(&path).unwrap();
        write!(file, "universal reader").unwrap();

        let app = Application::boot_with_data_dir(temp_dir("read-data")).unwrap();
        let response = app.read_file(&path).unwrap();
        let document = response.document.as_ref().expect("document");
        assert_eq!(document.file_type, FileType::PlainText);
        assert_eq!(document.text, "universal reader");

        let snapshot = app.diagnostics_from_response(Some(response)).unwrap();
        assert!(snapshot.read_success);
        assert_eq!(snapshot.read_parser.as_deref(), Some("plain_text"));
        assert_eq!(snapshot.read_character_count, Some(16));
    }

    #[test]
    fn shutdown_returns_to_starting() {
        let mut app = Application::boot_with_data_dir(temp_dir("shutdown-data")).unwrap();
        app.shutdown().unwrap();
        assert_eq!(app.state().label(), "Starting");
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-app-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
