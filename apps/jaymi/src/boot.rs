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
    build_explorer_tree, is_editable_coding_extension, workspace_expansion_for, Capability,
    CapabilityDiscoveryReport, CapabilityEngine, CapabilityEngineApi, CapabilityInspectorReport,
    CapabilityState, CodingBottomTab, CodingState, CreationState, DiagnosticState, EditorPaneId,
    EditorSettings, ExplorerPending, ExplorerStatus, FoldedRegion, GitFileEntry, GitStatusState,
    ProblemIssue, ProblemSeverity, ProblemsCollectContext, ResearchState, SearchResultEntry,
    SplitDirection, WorkspaceKind,
};

use jaymi_config::Config;
use jaymi_context::ContextEngine;
use jaymi_core::{
    AppState, DiscoveryQueryKind, EntryType, GitOperation, HealthReport, JaymiError, JaymiResult,
    Lifecycle, SearchRequest, ServiceContainer, UserRequest,
};
use jaymi_database::Database;
use jaymi_discovery::{DiscoveryEngine, FilesystemWatcher};
use jaymi_knowledge::{KnowledgeStore, SqliteKnowledgeStore};
use jaymi_logging::Logger;
use jaymi_memory::{
    AppendMessageRequest, ArchiveConversationRequest, AssembleContextRequest,
    AssembledMemoryContext, Conversation, ConversationMessage, ConversationMeta,
    CreateConversationRequest, CreatePersonalMemoryRequest, ListProjectDecisionsQuery,
    MemoryEngine, MemoryEngineApi, MemoryQuery, MemoryRecord, PersonalContext, ProjectDecision,
    PromoteMemoryRequest, PromotionAskDecision, PromotionSuggestQuery, PromotionSuggestion,
    SqliteMemoryStore, StoreMemoryRequest, StoreProjectDecisionRequest, StoreProjectMemoryRequest,
    UpdatePersonalMemoryRequest,
};
use jaymi_parsers::{default_registry, ParserRegistry};
use jaymi_permissions::PermissionEngine;
use jaymi_planner::{Planner, PlannerDeps, PlannerResponse};
use jaymi_policies::PolicyEngine;
use jaymi_project_engine::{
    CreateProjectRequest, Project, ProjectContext, ProjectEngine, ProjectEngineApi, ProjectHealth,
    ProjectType, SqliteProjectStore,
};
use jaymi_providers::{
    EmbeddingProvider, FilesystemProvider, GitProvider, LocalEmbeddingProvider, LspProvider,
    OcrProvider, PlaceholderOcrProvider, Provider, ProviderRegistry, TerminalProvider,
    DEFAULT_TERMINAL_SESSION_ID,
};
use jaymi_search::{EmbeddingQueue, SearchEngine, SearchEngineApi, SemanticDeps};
use jaymi_tools::{
    GitTool, LanguageServerTool, ListProjectTreeTool, ManagePathTool, QueryInventoryTool,
    ReadFileTool, ScanFilesystemTool, SearchFilesTool, SearchKnowledgeTool,
    SearchProjectKnowledgeTool, TerminalTool, ToolOrchestrator, ToolRegistry, WriteFileTool,
};
use jaymi_understanding::{
    format_parser_usage, ContentIntelligence, ContentIntelligenceApi, SqliteContentStore,
    UnderstandingEngine,
};

use crate::coding_workspace::{
    build_coding_diagnostics_view, CodingDiagnosticsView, LastPlannerActivity,
};
use crate::diagnostics::DiagnosticsSnapshot;
use crate::editor_workspace::{load_editor_workspace, save_editor_workspace};
use crate::experience::{ConversationTurn, ExperienceSession};

/// Owns the process service container and application state.
pub struct Application {
    state: AppState,
    container: ServiceContainer,
    health_reports: Vec<HealthReport>,
    /// Conversation-first experience (workspaces expand without destroying chat).
    experience: Mutex<ExperienceSession>,
    /// Last Planner turn for Coding Diagnostics (activity / timing).
    last_planner_activity: Mutex<Option<LastPlannerActivity>>,
}

impl Application {
    /// Create an application in the `Starting` state.
    pub fn new() -> Self {
        Self {
            state: AppState::Starting,
            container: ServiceContainer::new(),
            health_reports: Vec::new(),
            experience: Mutex::new(ExperienceSession::new()),
            last_planner_activity: Mutex::new(None),
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

        // Command registry — plugin-ready palette catalog (metadata only).
        let mut commands = jaymi_commands::CommandRegistry::new();
        self.initialize_service(&mut commands)?;
        commands.register_all(jaymi_commands::builtin_descriptors())?;
        self.container.register(Arc::new(commands));

        // Problems registry — aggregates the Problems panel from every registered source.
        let mut problems_registry = jaymi_capabilities::ProblemsRegistry::new();
        self.initialize_service(&mut problems_registry)?;
        problems_registry.register_all(crate::problems::builtin_problem_providers())?;
        self.container.register(Arc::new(problems_registry));

        // Provider registry + Filesystem Provider + Placeholder OCR Provider.
        let mut providers = ProviderRegistry::new();
        self.initialize_service(&mut providers)?;
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize()?;
        providers.register(&filesystem)?;
        let filesystem = Arc::new(filesystem);
        self.container.register(Arc::clone(&filesystem));

        let mut terminal = TerminalProvider::new();
        terminal.initialize()?;
        providers.register(&terminal)?;
        let terminal = Arc::new(terminal);
        self.container.register(Arc::clone(&terminal));

        let mut git = GitProvider::new();
        git.initialize()?;
        providers.register(&git)?;
        let git = Arc::new(git);
        self.container.register(Arc::clone(&git));

        let mut lsp = if cfg!(test) {
            LspProvider::mock()
        } else {
            LspProvider::new()
        };
        lsp.initialize()?;
        providers.register(&lsp)?;
        let lsp = Arc::new(lsp);
        self.container.register(Arc::clone(&lsp));

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
        projects.bind_sources(jaymi_project_engine::ProjectContextSources {
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
        let mut discovery =
            DiscoveryEngine::new(Arc::clone(&knowledge), discovery_roots, indexing_enabled);
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
        tools.register_tool(Arc::new(ListProjectTreeTool::new(Arc::clone(&filesystem))))?;
        tools.register_tool(Arc::new(SearchKnowledgeTool::new(Arc::clone(&search))))?;
        tools.register_tool(Arc::new(SearchProjectKnowledgeTool::new(
            Arc::clone(&projects) as Arc<dyn ProjectEngineApi>,
        )))?;
        tools.register_tool(Arc::new(ReadFileTool::new(Arc::clone(&content_api))))?;
        tools.register_tool(Arc::new(WriteFileTool::new(Arc::clone(&filesystem))))?;
        tools.register_tool(Arc::new(ManagePathTool::new(Arc::clone(&filesystem))))?;
        tools.register_tool(Arc::new(TerminalTool::new(Arc::clone(&terminal))))?;
        tools.register_tool(Arc::new(GitTool::new(Arc::clone(&git))))?;
        tools.register_tool(Arc::new(LanguageServerTool::new(Arc::clone(&lsp))))?;
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
        let started = std::time::Instant::now();
        let response = planner.handle(request)?;
        self.record_planner_activity(&response, started.elapsed().as_millis() as u64);
        Ok(response)
    }

    /// Ask the Planner to list a single directory through the full architecture.
    pub fn list_directory(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::list_directory(path.as_ref()))
    }

    /// Ask the Planner to recursively list a project tree for Coding Explorer.
    pub fn list_project_tree(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::list_project_tree(path.as_ref()))
    }

    /// Ask the Planner to read a supported file into a unified document.
    pub fn read_file(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::read_file(path.as_ref()))
    }

    /// Ask the Planner to write text content to a file.
    pub fn write_file(
        &self,
        path: impl AsRef<Path>,
        content: impl Into<String>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::write_file(path.as_ref(), content))
    }

    /// Ask the Planner to create a directory.
    pub fn manage_mkdir(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::manage_mkdir(path.as_ref()))
    }

    /// Ask the Planner to rename/move a path.
    pub fn manage_rename(
        &self,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::manage_rename(from.as_ref(), to.as_ref()))
    }

    /// Ask the Planner to delete a file or directory.
    pub fn manage_delete(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::manage_delete(path.as_ref()))
    }

    /// Ask the Planner to ensure a terminal PTY session exists.
    pub fn ensure_terminal(
        &self,
        session_id: impl Into<String>,
        cwd: impl AsRef<Path>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::ensure_terminal(session_id, cwd.as_ref()))
    }

    /// Ask the Planner to run a command in a terminal PTY session.
    pub fn run_terminal(
        &self,
        session_id: impl Into<String>,
        cwd: impl AsRef<Path>,
        command: impl Into<String>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::run_terminal(session_id, cwd.as_ref(), command))
    }

    /// Ask the Planner to spawn a new terminal PTY session.
    pub fn create_terminal(
        &self,
        cwd: impl AsRef<Path>,
        title: Option<String>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::create_terminal(cwd.as_ref(), title))
    }

    /// Ask the Planner to rename a terminal PTY session's display title.
    pub fn rename_terminal(
        &self,
        session_id: impl Into<String>,
        cwd: impl AsRef<Path>,
        title: impl Into<String>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::rename_terminal(
            session_id,
            cwd.as_ref(),
            title,
        ))
    }

    /// Ask the Planner to kill / close a terminal PTY session.
    pub fn kill_terminal(
        &self,
        session_id: impl Into<String>,
        cwd: impl AsRef<Path>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::kill_terminal(session_id, cwd.as_ref()))
    }

    /// Ask the Planner for Git repository status.
    pub fn git_status(&self, repo_root: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::git_status(repo_root.as_ref()))
    }

    /// Ask the Planner to stage paths in a Git repository.
    pub fn git_stage(
        &self,
        repo_root: impl AsRef<Path>,
        paths: Vec<PathBuf>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::git_stage(repo_root.as_ref(), paths))
    }

    /// Ask the Planner to unstage paths in a Git repository.
    pub fn git_unstage(
        &self,
        repo_root: impl AsRef<Path>,
        paths: Vec<PathBuf>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::git_unstage(repo_root.as_ref(), paths))
    }

    /// Ask the Planner to discard path changes in a Git repository.
    pub fn git_discard(
        &self,
        repo_root: impl AsRef<Path>,
        paths: Vec<PathBuf>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::git_discard(repo_root.as_ref(), paths))
    }

    /// Ask the Planner to create a Git commit.
    pub fn git_commit(
        &self,
        repo_root: impl AsRef<Path>,
        message: impl Into<String>,
    ) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::git_commit(repo_root.as_ref(), message))
    }

    /// Ask the Planner to run a Language Server operation.
    pub fn lsp(&self, request: jaymi_core::LspRequest) -> JaymiResult<PlannerResponse> {
        self.handle(UserRequest::lsp(request))
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

    /// Product search entry point for Quick Open / Find in Files.
    ///
    /// Always enters through [`Self::search`] (Planner → `search_knowledge` →
    /// Search Engine). Never resolves the Search Engine directly — there is
    /// exactly one retrieval index shared by every surface.
    pub fn project_search(&self, request: SearchRequest) -> JaymiResult<Vec<SearchResultEntry>> {
        let response = self.search(request)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        Ok(response
            .citations
            .into_iter()
            .map(|citation| SearchResultEntry {
                path: citation.location.to_string_lossy().into_owned(),
                title: citation.title,
                line: citation.line,
                column: citation.column,
                end_line: citation.end_line,
                end_column: citation.end_column,
                preview: citation.preview,
                why_matched: citation.why_matched,
            })
            .collect())
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
    pub fn promote_memory(&self, request: &PromoteMemoryRequest) -> JaymiResult<MemoryRecord> {
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
    pub fn load_conversation(&self, conversation_id: &str) -> JaymiResult<Option<Conversation>> {
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

    /// Command registry (palette catalog). Plugins register additional commands here.
    pub fn command_registry(&self) -> JaymiResult<Arc<jaymi_commands::CommandRegistry>> {
        let registry = self
            .container
            .resolve::<Arc<jaymi_commands::CommandRegistry>>()?;
        Ok(Arc::clone(registry))
    }

    /// Problems registry (Problems panel aggregation). Plugins register additional sources here.
    pub fn problems_registry(&self) -> JaymiResult<Arc<jaymi_capabilities::ProblemsRegistry>> {
        let registry = self
            .container
            .resolve::<Arc<jaymi_capabilities::ProblemsRegistry>>()?;
        Ok(Arc::clone(registry))
    }

    /// Create a first-class project through the Project Engine.
    pub fn create_project(&self, request: &CreateProjectRequest) -> JaymiResult<Project> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.create(request)
    }

    /// Open an existing project for `root`, or create one then open it.
    ///
    /// Reuses a project that already points at the same canonical directory.
    pub fn open_project_from_path(&self, root: impl AsRef<Path>) -> JaymiResult<ProjectContext> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(JaymiError::new(format!(
                "project path is not a directory: {}",
                root.display()
            )));
        }
        let canonical = root
            .canonicalize()
            .map_err(|error| JaymiError::new(format!("resolve project path: {error}")))?;

        if let Some(existing) = self.find_project_by_root(&canonical)? {
            return self.open_project(existing.id.as_str());
        }

        let name = canonical
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Project".to_string());
        let project = self.create_project(&CreateProjectRequest {
            project_id: None,
            name,
            description: None,
            root_directory: Some(canonical),
            project_type: Some(ProjectType::Code),
        })?;
        self.open_project(project.id.as_str())
    }

    fn find_project_by_root(&self, root: &Path) -> JaymiResult<Option<Project>> {
        let projects = self.list_projects()?;
        Ok(projects.into_iter().find(|project| {
            project
                .root_directory
                .as_ref()
                .and_then(|path| path.canonicalize().ok())
                .as_deref()
                == Some(root)
        }))
    }

    /// Open a project through the Planner (sole session open path).
    ///
    /// Always enters [`Planner::handle`]. Project Engine owns open state;
    /// Planner syncs Memory and resumes conversation. Does not mutate engines
    /// from Application.
    pub fn open_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        let response = self.handle(UserRequest::open_project(project_id))?;
        response.project_context.ok_or_else(|| {
            JaymiError::new(format!(
                "open project did not return context: {}",
                response.content
            ))
        })
    }

    /// Switch the active workspace to another project by id.
    ///
    /// Same lifecycle as [`Self::open_project`] (Planner-orchestrated).
    pub fn switch_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.open_project(project_id)
    }

    /// Close the session-open project through the Planner (sole session close path).
    /// Does not clear the active conversation.
    pub fn close_project(&self) -> JaymiResult<Option<Project>> {
        let response = self.handle(UserRequest::close_project())?;
        Ok(response.closed_project)
    }

    /// Current open workspace project id (Project Engine is source of truth).
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
    pub fn project_context(&self, project_id: Option<&str>) -> JaymiResult<Option<ProjectContext>> {
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

    /// Activate or clear the project workspace session.
    ///
    /// Delegates to [`Self::open_project`] / [`Self::close_project`] so session
    /// open/close has one Planner-orchestrated path (Project Engine owns state).
    /// Does not mutate Memory or Project Engine directly.
    pub fn set_active_project(&self, project_id: Option<&str>) -> JaymiResult<()> {
        match project_id {
            Some(id) => {
                let _ = self.open_project(id)?;
                Ok(())
            }
            None => {
                let _ = self.close_project()?;
                Ok(())
            }
        }
    }

    /// Activate a conversation for memory context assembly.
    pub fn set_active_conversation(&self, conversation_id: Option<&str>) -> JaymiResult<()> {
        let memory = self.container.resolve::<Arc<MemoryEngine>>()?;
        memory.set_active_conversation(conversation_id)
    }

    /// Load a persisted conversation into the live experience session.
    ///
    /// Replaces the in-memory transcript and binds `conversation_id`. Does not
    /// close or clear an expanded Coding workspace.
    pub fn switch_to_conversation(&self, conversation_id: &str) -> JaymiResult<()> {
        let loaded = self
            .load_conversation(conversation_id)?
            .ok_or_else(|| JaymiError::new(format!("conversation not found: {conversation_id}")))?;
        self.set_active_conversation(Some(conversation_id))?;
        let turns = loaded
            .messages
            .into_iter()
            .map(|message| ConversationTurn {
                role: message.role,
                content: message.content,
                created_at: message.created_at,
            })
            .collect();
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        experience.replace_transcript(Some(conversation_id.to_string()), turns);
        Ok(())
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
        let coding_open = experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
        drop(experience);
        if coding_open {
            let _ = self.refresh_coding_explorer();
        }
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
        experience.expand_workspace(expansion)?;
        drop(experience);
        self.prepare_context_session()?;
        Ok(())
    }

    /// Open the Coding Workspace from the conversation action menu.
    ///
    /// Reuses the existing Coding shell. Does not create a new conversation or
    /// clear turns. Loads the Project Explorer from the active project root when
    /// one is open. When editors are empty (fresh expand or project change),
    /// restores open tabs / view state from `.jaymi/workspace.json`.
    pub fn start_coding_project(&self) -> JaymiResult<()> {
        let previous_root = self
            .with_coding_state(|coding| coding.explorer.project_root.clone())
            .ok()
            .flatten();
        let new_root = self.active_project_root_path();

        let expansion = workspace_expansion_for(
            Capability::Code,
            "Started Coding Project from conversation menu",
        )
        .ok_or_else(|| JaymiError::new("code capability has no coding workspace mapping"))?;
        self.expand_ui_workspace(expansion)?;

        let project_changed = match (&previous_root, &new_root) {
            (Some(previous), Some(next)) => previous != next,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        if project_changed && previous_root.is_some() {
            // Caller should have persisted the previous project; drop in-memory tabs.
            self.with_coding_state(|coding| coding.clear_editors())?;
        }

        self.refresh_coding_explorer()?;
        self.ensure_coding_terminal()?;
        let _ = self.refresh_coding_git();

        let editors_empty = self.with_coding_state(|coding| coding.editors.is_empty())?;
        if editors_empty {
            let _ = self.restore_coding_editor_workspace();
        }
        let _ = self.refresh_coding_problems();
        Ok(())
    }

    /// Absolute root of the active project, when open.
    pub fn active_project_root_path(&self) -> Option<PathBuf> {
        self.active_project_id().and_then(|id| {
            self.container
                .resolve::<Arc<ProjectEngine>>()
                .ok()
                .and_then(|projects| projects.get(&id).ok().flatten())
                .and_then(|project| project.root_directory)
        })
    }

    /// Persist Coding editor UI state to the current project `.jaymi/workspace.json`.
    ///
    /// Never writes buffer contents — only paths, view state, and settings.
    pub fn persist_coding_editor_workspace(&self) -> JaymiResult<()> {
        let Some((root, snapshot)) = self
            .with_coding_state(|coding| {
                coding
                    .explorer
                    .project_root
                    .as_ref()
                    .map(|root| (PathBuf::from(root), coding.editor_workspace_snapshot()))
            })
            .ok()
            .flatten()
        else {
            return Ok(());
        };
        save_editor_workspace(&root, &snapshot)
    }

    /// Reload editor panes / layout / view / settings from `.jaymi/workspace.json`.
    ///
    /// Rebuilds the pane/split tree structure directly from the snapshot (so
    /// multi-pane layouts restore intact), then re-reads file contents through
    /// Planner → read_file to seed each unique buffer. Missing files are
    /// skipped without disturbing the restored layout. Never opens files
    /// through `open_coding_file` in a loop — that API always targets the
    /// focused pane and would collapse a multi-pane layout back to one pane.
    pub fn restore_coding_editor_workspace(&self) -> JaymiResult<()> {
        let Some(root) = self
            .with_coding_state(|coding| coding.explorer.project_root.clone())
            .ok()
            .flatten()
            .map(PathBuf::from)
            .or_else(|| self.active_project_root_path())
        else {
            return Ok(());
        };

        let Some(snapshot) = load_editor_workspace(&root)? else {
            return Ok(());
        };

        self.with_coding_state(|coding| {
            coding.clear_editors();
            coding.apply_editor_workspace_structure(&snapshot);
        })?;

        // Unique paths across every restored pane (v2) — `apply_editor_workspace_structure`
        // already normalizes legacy v1 single-pane snapshots into one pane, so this
        // covers both schema versions.
        let paths: Vec<String> = self.with_coding_state(|coding| {
            let mut paths: Vec<String> = coding
                .editors
                .panes
                .values()
                .flat_map(|pane| pane.tabs.iter().map(|tab| tab.path.clone()))
                .collect();
            paths.sort();
            paths.dedup();
            paths
        })?;

        for path in paths {
            let Ok(text) = self.read_coding_file_text(&path) else {
                continue;
            };
            self.with_coding_state(|coding| {
                coding.seed_editor_buffer(&path, text.clone());
            })?;
            let _ = self.coding_lsp_did_open(&path, &text);
        }

        Ok(())
    }

    /// Refresh Project Explorer from the active project via Planner → Tool → Provider.
    pub fn refresh_coding_explorer(&self) -> JaymiResult<()> {
        let root = self.active_project_id().and_then(|id| {
            self.container
                .resolve::<Arc<ProjectEngine>>()
                .ok()
                .and_then(|projects| projects.get(&id).ok().flatten())
                .and_then(|project| project.root_directory)
        });

        let Some(root) = root else {
            return self.with_coding_state(|coding| {
                coding.explorer.clear_no_project();
            });
        };

        let response = self.list_project_tree(&root)?;
        if response.blocked {
            let message = response.content.clone();
            let root_display = root.to_string_lossy().into_owned();
            return self.with_coding_state(|coding| {
                coding.explorer.project_root = Some(root_display);
                coding.explorer.nodes.clear();
                coding.explorer.status = ExplorerStatus::Error(message);
            });
        }

        let root_display = response
            .listed_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let flat: Vec<(String, String, bool)> = response
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.path.to_string_lossy().into_owned(),
                    entry.name.clone(),
                    entry.entry_type == EntryType::Directory,
                )
            })
            .collect();
        let nodes = build_explorer_tree(&root_display, &flat);
        self.with_coding_state(|coding| {
            coding.explorer.project_root = Some(root_display);
            // Expand top-level folders so files under src/ etc. are one click away.
            for node in &nodes {
                if node.is_dir {
                    coding.explorer.expanded_paths.insert(node.path.clone());
                }
            }
            coding.explorer.nodes = nodes;
            coding.explorer.status = ExplorerStatus::Ready;
        })?;
        Ok(())
    }

    /// Select a path in Project Explorer without opening files.
    pub fn select_coding_path(&self, path: &str, _is_dir: bool) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.explorer.selected_path = Some(path.to_string());
        })
    }

    /// Toggle folder expansion in Project Explorer.
    pub fn toggle_coding_expand(&self, path: &str) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.toggle_expanded(path);
        })
    }

    /// Begin an inline new-file draft under `parent`.
    pub fn begin_coding_new_file(&self, parent: &str) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.explorer.begin_new_file(parent);
        })
    }

    /// Begin an inline new-folder draft under `parent`.
    pub fn begin_coding_new_folder(&self, parent: &str) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.explorer.begin_new_folder(parent);
        })
    }

    /// Begin an inline rename draft for `path`.
    pub fn begin_coding_rename(&self, path: &str, name: &str) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.explorer.begin_rename(path, name);
        })
    }

    /// Update the pending create/rename draft name.
    pub fn set_coding_explorer_pending_name(&self, draft_name: String) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.explorer.set_pending_draft(draft_name);
        })
    }

    /// Cancel any pending create/rename draft.
    pub fn cancel_coding_explorer_pending(&self) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.explorer.clear_pending();
        })
    }

    /// Confirm the pending create/rename through Planner → write_file / manage_path.
    pub fn confirm_coding_explorer_pending(&self) -> JaymiResult<()> {
        let pending = self.with_coding_state(|coding| coding.explorer.pending.clone())?;
        match pending {
            ExplorerPending::None => Ok(()),
            ExplorerPending::NewFile { parent, draft_name } => {
                let name = draft_name.trim();
                if name.is_empty() {
                    return Err(JaymiError::new("new file name must not be empty"));
                }
                let path = Path::new(&parent).join(name);
                let path_str = path.to_string_lossy().into_owned();
                let response = self.write_file(&path, "")?;
                if response.blocked {
                    return Err(JaymiError::new(response.content));
                }
                self.with_coding_state(|coding| coding.explorer.clear_pending())?;
                self.refresh_coding_explorer()?;
                self.select_coding_path(&path_str, false)?;
                if is_editable_coding_extension(&path_str) {
                    let _ = self.open_coding_file(&path_str);
                }
                Ok(())
            }
            ExplorerPending::NewFolder { parent, draft_name } => {
                let name = draft_name.trim();
                if name.is_empty() {
                    return Err(JaymiError::new("new folder name must not be empty"));
                }
                let path = Path::new(&parent).join(name);
                let path_str = path.to_string_lossy().into_owned();
                let response = self.manage_mkdir(&path)?;
                if response.blocked {
                    return Err(JaymiError::new(response.content));
                }
                self.with_coding_state(|coding| {
                    coding.explorer.clear_pending();
                    coding.explorer.expanded_paths.insert(parent);
                })?;
                self.refresh_coding_explorer()?;
                self.select_coding_path(&path_str, true)
            }
            ExplorerPending::Rename { path, draft_name } => {
                let name = draft_name.trim();
                if name.is_empty() {
                    return Err(JaymiError::new("rename target must not be empty"));
                }
                let from = PathBuf::from(&path);
                let to = from
                    .parent()
                    .map(|parent| parent.join(name))
                    .ok_or_else(|| JaymiError::new("cannot rename path without a parent"))?;
                let to_str = to.to_string_lossy().into_owned();
                let response = self.manage_rename(&from, &to)?;
                if response.blocked {
                    return Err(JaymiError::new(response.content));
                }
                self.with_coding_state(|coding| {
                    coding.explorer.clear_pending();
                    coding.editors.remap_path(&path, &to_str, name);
                    coding.explorer.selected_path = Some(to_str.clone());
                })?;
                self.refresh_coding_explorer()?;
                Ok(())
            }
        }
    }

    /// Delete a path through Planner → manage_path and refresh the explorer.
    pub fn delete_coding_path(&self, path: &str) -> JaymiResult<()> {
        let response = self.manage_delete(path)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.with_coding_state(|coding| {
            let _ = coding.close_tab(path);
            if coding.explorer.selected_path.as_deref() == Some(path) {
                coding.explorer.selected_path = None;
            }
            coding.explorer.expanded_paths.remove(path);
        })?;
        self.refresh_coding_explorer()
    }

    /// Reveal a path in the OS file manager (Finder on macOS).
    #[allow(clippy::needless_return)]
    pub fn reveal_in_file_manager(&self, path: &str) -> JaymiResult<()> {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .args(["-R", path])
                .spawn()
                .map_err(|error| JaymiError::new(format!("failed to reveal in Finder: {error}")))?;
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(format!("/select,{path}"))
                .spawn()
                .map_err(|error| {
                    JaymiError::new(format!("failed to reveal in Explorer: {error}"))
                })?;
            return Ok(());
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let parent = Path::new(path).parent().unwrap_or_else(|| Path::new(path));
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|error| {
                    JaymiError::new(format!("failed to reveal in file manager: {error}"))
                })?;
            return Ok(());
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
        {
            let _ = path;
            Err(JaymiError::new(
                "reveal in file manager is not supported on this platform",
            ))
        }
    }

    /// Open a file in the Coding Editor through Planner → read_file.
    ///
    /// Reopening an already-open path focuses that tab (and promotes preview → permanent)
    /// without re-reading. Opens as a permanent (non-preview) session.
    pub fn open_coding_file(&self, path: &str) -> JaymiResult<()> {
        let existed = self.with_coding_state(|coding| promote_and_focus_existing(coding, path))?;
        if existed {
            return Ok(());
        }

        let text = self.read_coding_file_text(path)?;
        self.with_coding_state(|coding| {
            // Race: another opener may have created the session since the existence check.
            if promote_and_focus_existing(coding, path) {
                return;
            }
            coding.open_permanent(path, text.clone());
        })?;
        let _ = self.coding_lsp_did_open(path, &text);
        Ok(())
    }

    /// Open a file as a VS Code-style preview tab through Planner → read_file.
    ///
    /// Reopening an already-open path focuses that session. A new preview replaces
    /// any existing preview tab.
    pub fn open_coding_file_preview(&self, path: &str) -> JaymiResult<()> {
        let focused = self.with_coding_state(|coding| coding.focus_tab(path))?;
        if focused {
            return Ok(());
        }

        let text = self.read_coding_file_text(path)?;
        self.with_coding_state(|coding| {
            if coding.focus_tab(path) {
                return;
            }
            coding.open_preview(path, text.clone());
        })?;
        let _ = self.coding_lsp_did_open(path, &text);
        Ok(())
    }

    fn read_coding_file_text(&self, path: &str) -> JaymiResult<String> {
        if !is_editable_coding_extension(path) {
            return Err(JaymiError::new(format!(
                "unsupported editor file type: {path}"
            )));
        }
        let response = self.read_file(path)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        let document = response
            .document
            .ok_or_else(|| JaymiError::new(format!("read_file returned no document for {path}")))?;
        Ok(document.text.clone())
    }

    /// Activate an already-open editor tab.
    pub fn activate_coding_tab(&self, path: &str) -> JaymiResult<()> {
        let focused = self.with_coding_state(|coding| coding.focus_tab(path))?;
        if !focused {
            return Err(JaymiError::new(format!("no open tab for {path}")));
        }
        Ok(())
    }

    /// Close an editor tab.
    pub fn close_coding_tab(&self, path: &str) -> JaymiResult<()> {
        let closed = self.with_coding_state(|coding| coding.close_tab(path))?;
        if !closed {
            return Err(JaymiError::new(format!("no open tab for {path}")));
        }
        Ok(())
    }

    /// Update editor buffer content for a tab.
    pub fn set_coding_tab_content(&self, path: &str, content: String) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.set_tab_content(path, content.clone());
        })?;
        let _ = self.coding_lsp_did_change(path, &content);
        Ok(())
    }

    /// Persist scroll offset for a tab.
    pub fn set_coding_tab_scroll(&self, path: &str, offset: f32) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.set_scroll_offset(path, offset);
        })
    }

    /// Persist cursor position for a tab.
    pub fn set_coding_tab_cursor(&self, path: &str, line: u32, column: u32) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.set_cursor(path, line, column);
        })
    }

    /// Open a Find in Files / Quick Open result in the Coding Editor.
    ///
    /// Expands the Coding workspace when it is not already active, opens the
    /// file through the normal Planner-mediated path, then places the cursor
    /// at the located match when a line/column is known.
    pub fn open_search_result(
        &self,
        path: &str,
        line: Option<u32>,
        column: Option<u32>,
    ) -> JaymiResult<()> {
        if self.with_coding_state(|_| ()).is_err() {
            self.start_coding_project()?;
        }
        self.open_coding_file(path)?;
        if let (Some(line), Some(column)) = (line, column) {
            self.set_coding_tab_cursor(path, line, column)?;
        }
        Ok(())
    }

    /// Replace every located match of `request.free_text` across search results.
    ///
    /// Reuses the same match semantics as retrieval ([`jaymi_search::replace_matches`])
    /// so Replace All never disagrees with what Find in Files reported. Open
    /// editor buffers are updated in place (leaving them dirty for review);
    /// files with no open buffer are written directly through the Planner.
    /// Never builds a second index.
    pub fn replace_in_search_results(
        &self,
        mut request: SearchRequest,
        replace_text: &str,
    ) -> JaymiResult<usize> {
        let query = request
            .free_text
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| JaymiError::new("replace requires a non-empty search query"))?;
        request.filename_only = false;

        let hits = self.project_search(request.clone())?;
        let mut paths: Vec<String> = hits.into_iter().map(|hit| hit.path).collect();
        paths.sort();
        paths.dedup();

        let mut total_replacements = 0usize;
        for path in paths {
            let open_content = self
                .with_coding_state(|coding| {
                    coding
                        .editors
                        .buffer_by_path(&path)
                        .map(|buffer| buffer.content.clone())
                })
                .ok()
                .flatten();

            let current_text = match &open_content {
                Some(content) => content.clone(),
                None => {
                    let response = self.read_file(&path)?;
                    let Some(document) = response.document else {
                        continue;
                    };
                    document.text
                }
            };

            let (new_text, count) =
                jaymi_search::replace_matches(&current_text, &query, replace_text, &request);
            if count == 0 {
                continue;
            }
            total_replacements += count;

            if open_content.is_some() {
                self.set_coding_tab_content(&path, new_text)?;
            } else {
                let response = self.write_file(&path, new_text)?;
                if response.blocked {
                    return Err(JaymiError::new(response.content));
                }
            }
        }
        Ok(total_replacements)
    }

    /// Run a search from the Coding Search panel and store results on the panel.
    ///
    /// Builds a [`SearchRequest`] scoped to the active project root from the
    /// panel's current query/toggles, runs it through [`Self::project_search`]
    /// (Planner → Search Engine), and switches the bottom tab to Search.
    pub fn run_coding_search_from_panel(&self) -> JaymiResult<()> {
        let panel = self.with_coding_state(|coding| coding.search.clone())?;
        let query = panel.query.trim().to_string();
        if query.is_empty() {
            return self.with_coding_state(|coding| {
                coding.search.results.clear();
                coding.search.status = "Type to search".to_string();
                coding.show_bottom_tab(CodingBottomTab::Search);
            });
        }

        self.with_coding_state(|coding| {
            coding.search.searching = true;
            coding.show_bottom_tab(CodingBottomTab::Search);
        })?;

        let root = self
            .with_coding_state(|coding| coding.explorer.project_root.clone())
            .ok()
            .flatten();

        let mut request = if panel.filename_only {
            SearchRequest::filename(query)
        } else {
            SearchRequest::free_text(query)
        }
        .with_case_sensitive(panel.case_sensitive)
        .with_whole_word(panel.whole_word)
        .with_regex(panel.use_regex)
        .with_filename_only(panel.filename_only);
        request.limit = Some(500);
        if let Some(root) = root {
            request.folder = Some(PathBuf::from(root));
        }

        let outcome = self.project_search(request);
        self.with_coding_state(|coding| {
            coding.search.searching = false;
            match outcome {
                Ok(results) => {
                    coding.search.status = format!("{} result(s)", results.len());
                    coding.search.results = results;
                }
                Err(error) => {
                    coding.search.results.clear();
                    coding.search.status = format!("Search failed: {error}");
                }
            }
        })
    }

    /// Persist folded regions for a tab (workspace-owned; not Monaco).
    pub fn set_coding_tab_folds(
        &self,
        path: &str,
        folded_regions: Vec<FoldedRegion>,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.set_folded_regions(path, folded_regions);
        })
    }

    /// Activate an already-open editor tab in a specific pane.
    pub fn activate_coding_tab_in_pane(&self, pane_id: &str, path: &str) -> JaymiResult<()> {
        let activated = self.with_coding_state(|coding| {
            coding
                .editors
                .activate_path_in_pane(&EditorPaneId(pane_id.to_string()), path)
        })?;
        if !activated {
            return Err(JaymiError::new(format!(
                "no open tab for {path} in pane {pane_id}"
            )));
        }
        Ok(())
    }

    /// Close an editor tab in a specific pane.
    pub fn close_coding_tab_in_pane(&self, pane_id: &str, path: &str) -> JaymiResult<()> {
        let closed = self.with_coding_state(|coding| {
            coding
                .editors
                .close_path_in_pane(&EditorPaneId(pane_id.to_string()), path)
        })?;
        if !closed {
            return Err(JaymiError::new(format!(
                "no open tab for {path} in pane {pane_id}"
            )));
        }
        Ok(())
    }

    /// Update editor buffer content for a tab, focusing the pane it was edited from.
    pub fn set_coding_tab_content_in_pane(
        &self,
        pane_id: &str,
        path: &str,
        content: String,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            let _ = coding
                .editors
                .focus_pane(&EditorPaneId(pane_id.to_string()));
            coding.set_tab_content(path, content.clone());
        })?;
        let _ = self.coding_lsp_did_change(path, &content);
        Ok(())
    }

    /// Persist scroll offset for a tab in a specific pane.
    pub fn set_coding_tab_scroll_in_pane(
        &self,
        pane_id: &str,
        path: &str,
        offset: f32,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding
                .editors
                .set_scroll_top_in_pane(&EditorPaneId(pane_id.to_string()), path, offset);
        })
    }

    /// Persist cursor position for a tab in a specific pane.
    pub fn set_coding_tab_cursor_in_pane(
        &self,
        pane_id: &str,
        path: &str,
        line: u32,
        column: u32,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.editors.set_cursor_in_pane(
                &EditorPaneId(pane_id.to_string()),
                path,
                line,
                column,
            );
        })
    }

    /// Persist folded regions for a tab in a specific pane (workspace-owned; not Monaco).
    pub fn set_coding_tab_folds_in_pane(
        &self,
        pane_id: &str,
        path: &str,
        folded_regions: Vec<FoldedRegion>,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.editors.set_folded_regions_in_pane(
                &EditorPaneId(pane_id.to_string()),
                path,
                folded_regions,
            );
        })
    }

    /// Split the focused editor pane (VS Code "Split Right" / "Split Down").
    pub fn split_coding_editor(&self, direction: SplitDirection) -> JaymiResult<String> {
        let new_pane = self.with_coding_state(|coding| coding.split_editor(direction))?;
        new_pane
            .map(|id| id.as_str().to_string())
            .ok_or_else(|| JaymiError::new("failed to split editor pane"))
    }

    /// Close an entire editor pane (no-op error when it is the only pane).
    pub fn close_coding_editor_pane(&self, pane_id: &str) -> JaymiResult<()> {
        let closed = self.with_coding_state(|coding| {
            coding.close_editor_pane(&EditorPaneId(pane_id.to_string()))
        })?;
        if !closed {
            return Err(JaymiError::new(format!(
                "cannot close pane {pane_id} (missing or the only pane)"
            )));
        }
        Ok(())
    }

    /// Give keyboard / Monaco focus to a pane.
    pub fn focus_coding_editor_pane(&self, pane_id: &str) -> JaymiResult<()> {
        let focused = self.with_coding_state(|coding| {
            coding
                .editors
                .focus_pane(&EditorPaneId(pane_id.to_string()))
        })?;
        if !focused {
            return Err(JaymiError::new(format!("no such editor pane {pane_id}")));
        }
        Ok(())
    }

    /// Move a tab from one pane to another (drag and drop between splits).
    pub fn move_coding_editor_tab(
        &self,
        from_pane: &str,
        path: &str,
        to_pane: &str,
        index: Option<usize>,
    ) -> JaymiResult<()> {
        let moved = self.with_coding_state(|coding| {
            coding.editors.move_tab(
                &EditorPaneId(from_pane.to_string()),
                path,
                &EditorPaneId(to_pane.to_string()),
                index,
            )
        })?;
        if !moved {
            return Err(JaymiError::new(format!(
                "cannot move {path} from pane {from_pane} to {to_pane}"
            )));
        }
        Ok(())
    }

    /// Resize a split node's relative child sizes, addressed by child-index path from the layout root.
    pub fn resize_coding_editor_split(
        &self,
        node_path: &[usize],
        sizes: Vec<f32>,
    ) -> JaymiResult<()> {
        let resized =
            self.with_coding_state(|coding| coding.editors.resize_split(node_path, sizes))?;
        if !resized {
            return Err(JaymiError::new("invalid editor split resize"));
        }
        Ok(())
    }

    /// Update workspace-owned editor chrome preferences.
    pub fn set_coding_editor_settings(&self, settings: EditorSettings) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.editor_settings = settings;
        })?;
        let _ = self.persist_coding_editor_workspace();
        Ok(())
    }

    /// Save an open editor tab through Planner → write_file.
    pub fn save_coding_file(&self, path: &str) -> JaymiResult<()> {
        let content = self.with_coding_state(|coding| {
            coding
                .editors
                .session_by_path(path)
                .map(|session| session.content.clone())
        })?;
        let Some(content) = content else {
            return Err(JaymiError::new(format!("no open tab for {path}")));
        };

        let response = self.write_file(path, content.clone())?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.with_coding_state(|coding| {
            coding.mark_tab_clean(path);
        })?;
        let _ = self.coding_lsp_did_change(path, &content);
        Ok(())
    }

    /// Notify the language server that a coding tab was opened.
    pub fn coding_lsp_did_open(&self, path: &str, content: &str) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::DidOpen,
            Some(path),
            Some(content),
            None,
            None,
            None,
            Some(1),
        )?;
        let response = self.lsp(request)?;
        self.apply_lsp_diagnostics(&response);
        Ok(response)
    }

    /// Notify the language server that a coding tab changed.
    pub fn coding_lsp_did_change(&self, path: &str, content: &str) -> JaymiResult<PlannerResponse> {
        let version = (content.len() as i32).saturating_add(1);
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::DidChange,
            Some(path),
            Some(content),
            None,
            None,
            None,
            Some(version),
        )?;
        let response = self.lsp(request)?;
        self.apply_lsp_diagnostics(&response);
        Ok(response)
    }

    /// Request hover information for the active coding buffer.
    pub fn coding_lsp_hover(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::Hover,
            Some(path),
            None,
            Some(line),
            Some(character),
            None,
            None,
        )?;
        self.lsp(request)
    }

    /// Request completions for the active coding buffer.
    pub fn coding_lsp_completion(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::Completion,
            Some(path),
            None,
            Some(line),
            Some(character),
            None,
            None,
        )?;
        self.lsp(request)
    }

    /// Refresh diagnostics into CodingState through Planner → language_server.
    pub fn coding_lsp_diagnostics(&self, path: Option<&str>) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::Diagnostics,
            path,
            None,
            None,
            None,
            None,
            None,
        )?;
        let response = self.lsp(request)?;
        self.apply_lsp_diagnostics(&response);
        Ok(response)
    }

    /// Go to definition at a coding buffer position.
    pub fn coding_lsp_definition(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::Definition,
            Some(path),
            None,
            Some(line),
            Some(character),
            None,
            None,
        )?;
        self.lsp(request)
    }

    /// Rename the symbol under the cursor.
    pub fn coding_lsp_rename(
        &self,
        path: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::Rename,
            Some(path),
            None,
            Some(line),
            Some(character),
            Some(new_name),
            None,
        )?;
        self.lsp(request)
    }

    /// Find references for the symbol under the cursor.
    pub fn coding_lsp_references(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> JaymiResult<PlannerResponse> {
        let request = self.coding_lsp_request(
            jaymi_core::LspOperation::References,
            Some(path),
            None,
            Some(line),
            Some(character),
            None,
            None,
        )?;
        self.lsp(request)
    }

    #[allow(clippy::too_many_arguments)]
    fn coding_lsp_request(
        &self,
        operation: jaymi_core::LspOperation,
        path: Option<&str>,
        content: Option<&str>,
        line: Option<u32>,
        character: Option<u32>,
        new_name: Option<&str>,
        version: Option<i32>,
    ) -> JaymiResult<jaymi_core::LspRequest> {
        let workspace_root = self
            .with_coding_state(|coding| coding.explorer.project_root.clone())?
            .map(PathBuf::from)
            .or_else(|| {
                path.and_then(|value| Path::new(value).parent().map(|parent| parent.to_path_buf()))
            })
            .ok_or_else(|| JaymiError::new("coding lsp has no workspace root"))?;
        let language = path.map(|value| {
            if Path::new(value)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
            {
                "rust".to_string()
            } else {
                "plaintext".to_string()
            }
        });
        Ok(jaymi_core::LspRequest {
            workspace_root,
            operation,
            path: path.map(PathBuf::from),
            content: content.map(str::to_string),
            language,
            version,
            line,
            character,
            new_name: new_name.map(str::to_string),
        })
    }

    fn apply_lsp_diagnostics(&self, response: &PlannerResponse) {
        let diagnostics = response
            .lsp_diagnostics
            .iter()
            .map(|diag| DiagnosticState {
                message: diag.message.clone(),
                path: Some(diag.path.clone()),
                severity: diag.severity.clone(),
                source: diag
                    .source
                    .clone()
                    .unwrap_or_else(|| "rust-analyzer".into()),
                line: Some(diag.range.start.line),
                character: Some(diag.range.start.character),
                end_line: Some(diag.range.end.line),
                end_character: Some(diag.range.end.character),
            })
            .collect::<Vec<_>>();
        let _ = self.with_coding_state(|coding| {
            if response
                .lsp_diagnostics
                .first()
                .map(|diag| diag.path.as_str())
                .is_some()
            {
                // Replace diagnostics for touched paths; keep others.
                let touched: std::collections::BTreeSet<_> = response
                    .lsp_diagnostics
                    .iter()
                    .map(|diag| diag.path.clone())
                    .collect();
                coding.diagnostics.retain(|item| {
                    item.path
                        .as_ref()
                        .is_none_or(|path| !touched.contains(path))
                });
                coding.diagnostics.extend(diagnostics);
            } else if !diagnostics.is_empty() {
                coding.diagnostics = diagnostics;
            }
        });
        let _ = self.refresh_coding_problems();
    }

    /// Save the active editor tab, when any.
    pub fn save_active_coding_file(&self) -> JaymiResult<()> {
        let path = self.with_coding_state(|coding| coding.active_tab_path().map(str::to_string))?;
        let Some(path) = path else {
            return Err(JaymiError::new("no active editor tab to save"));
        };
        self.save_coding_file(&path)
    }

    /// Ensure the Coding Workspace has a persistent PTY session.
    pub fn ensure_coding_terminal(&self) -> JaymiResult<()> {
        let cwd = self.with_coding_state(|coding| coding.explorer.project_root.clone())?;
        let Some(cwd) = cwd else {
            return self.with_coding_state(|coding| {
                if coding.terminal_sessions.is_empty() {
                    coding.push_terminal_session(jaymi_capabilities::TerminalSessionState::new(
                        DEFAULT_TERMINAL_SESSION_ID,
                        None,
                    ));
                }
            });
        };

        let response = self.ensure_terminal(DEFAULT_TERMINAL_SESSION_ID, &cwd)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.apply_terminal_response(&response)?;
        self.with_coding_state(|coding| {
            if coding.active_terminal_id.is_none() {
                coding.active_terminal_id = Some(DEFAULT_TERMINAL_SESSION_ID.to_string());
            }
        })?;
        Ok(())
    }

    /// Spawn a new terminal tab in the Coding Workspace (cwd = project root)
    /// and make it the active tab.
    pub fn create_coding_terminal(&self, title: Option<String>) -> JaymiResult<()> {
        let cwd = self
            .with_coding_state(|coding| coding.explorer.project_root.clone())?
            .ok_or_else(|| JaymiError::new("cannot create terminal — open a project first"))?;

        let response = self.create_terminal(&cwd, title)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        let session_id = response
            .terminal_session_id
            .clone()
            .ok_or_else(|| JaymiError::new("terminal create did not return a session id"))?;
        self.apply_terminal_response(&response)?;
        self.with_coding_state(|coding| {
            coding.active_terminal_id = Some(session_id.clone());
            coding.show_bottom_tab(CodingBottomTab::Terminal);
        })?;
        Ok(())
    }

    /// Rename a Coding Workspace terminal tab's display title.
    pub fn rename_coding_terminal(&self, session_id: &str, title: &str) -> JaymiResult<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err(JaymiError::new("terminal title must not be empty"));
        }
        let cwd = self.with_coding_state(|coding| {
            coding
                .terminal_sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| session.cwd.clone())
                .or_else(|| coding.explorer.project_root.clone())
        })?;
        let Some(cwd) = cwd else {
            return Err(JaymiError::new(
                "coding terminal has no working directory — open a project first",
            ));
        };

        let response = self.rename_terminal(session_id, &cwd, title)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.apply_terminal_response(&response)?;
        Ok(())
    }

    /// Kill a Coding Workspace terminal tab and pick a new active tab.
    pub fn kill_coding_terminal(&self, session_id: &str) -> JaymiResult<()> {
        let cwd = self.with_coding_state(|coding| {
            coding
                .terminal_sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| session.cwd.clone())
                .or_else(|| coding.explorer.project_root.clone())
        })?;
        let cwd = cwd.unwrap_or_default();

        let response = self.kill_terminal(session_id, &cwd)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.with_coding_state(|coding| {
            coding.remove_terminal_session(session_id);
        })?;
        Ok(())
    }

    /// Select an existing Coding Workspace terminal tab as active.
    pub fn select_coding_terminal(&self, session_id: &str) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.select_terminal(session_id);
        })
    }

    /// Run a command in the Coding Workspace terminal through Planner → Tool → PTY.
    pub fn run_coding_terminal_command(&self, session_id: &str, command: &str) -> JaymiResult<()> {
        let command = command.trim();
        if command.is_empty() {
            return Err(JaymiError::new("terminal command must not be empty"));
        }
        let cwd = self.with_coding_state(|coding| {
            coding
                .terminal_sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| session.cwd.clone())
                .or_else(|| coding.explorer.project_root.clone())
        })?;
        let Some(cwd) = cwd else {
            return Err(JaymiError::new(
                "coding terminal has no working directory — open a project first",
            ));
        };

        let response = self.run_terminal(session_id, &cwd, command)?;
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.apply_terminal_response(&response)?;
        Ok(())
    }

    /// Update the draft input line for a terminal session.
    pub fn set_coding_terminal_input(&self, session_id: &str, input: String) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            if let Some(session) = coding
                .terminal_sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                session.input = input;
                session.history_index = None;
            }
        })
    }

    /// Persist terminal output scroll offset.
    pub fn set_coding_terminal_scroll(&self, session_id: &str, offset: f32) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            if let Some(session) = coding
                .terminal_sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                session.scroll_offset = offset;
            }
        })
    }

    /// Navigate terminal command history (negative = older, positive = newer).
    pub fn navigate_coding_terminal_history(
        &self,
        session_id: &str,
        direction: i8,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            if let Some(session) = coding
                .terminal_sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                if direction < 0 {
                    session.history_up();
                } else if direction > 0 {
                    session.history_down();
                }
            }
        })
    }

    fn apply_terminal_response(&self, response: &PlannerResponse) -> JaymiResult<()> {
        let session_id = response
            .terminal_session_id
            .clone()
            .unwrap_or_else(|| DEFAULT_TERMINAL_SESSION_ID.to_string());

        if response.terminal_alive == Some(false) {
            return self.with_coding_state(|coding| {
                coding.remove_terminal_session(&session_id);
            });
        }

        let cwd = response
            .listed_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let scrollback = response
            .terminal_scrollback
            .clone()
            .unwrap_or_else(|| response.terminal_output.clone().unwrap_or_default());
        let history = response.terminal_history.clone();
        let last_command = history.last().cloned();
        let title = response.terminal_title.clone();

        self.with_coding_state(|coding| {
            let session = if let Some(existing) = coding
                .terminal_sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                existing
            } else {
                coding.push_terminal_session(jaymi_capabilities::TerminalSessionState::with_title(
                    session_id.clone(),
                    title.clone(),
                    cwd.clone(),
                ));
                coding
                    .terminal_sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                    .expect("just pushed")
            };
            if let Some(title) = title {
                session.title = title;
            }
            session.apply_result(cwd, last_command, scrollback, history);
        })
    }

    /// Refresh Coding Workspace Git status through Planner → git → Git Provider.
    pub fn refresh_coding_git(&self) -> JaymiResult<()> {
        let root = self.with_coding_state(|coding| coding.explorer.project_root.clone())?;
        let Some(root) = root else {
            return self.with_coding_state(|coding| {
                coding.git = Some(GitStatusState {
                    is_repository: false,
                    summary: "No open project".into(),
                    last_error: Some("open a project to use Git".into()),
                    ..GitStatusState::default()
                });
            });
        };

        match self.git_status(&root) {
            Ok(response) if !response.blocked => self.apply_git_response(&response),
            Ok(response) => self.with_coding_state(|coding| {
                coding.git = Some(GitStatusState {
                    is_repository: false,
                    summary: "unavailable".into(),
                    last_error: Some(response.content),
                    ..GitStatusState::default()
                });
            }),
            Err(error) => self.with_coding_state(|coding| {
                coding.git = Some(GitStatusState {
                    is_repository: false,
                    summary: "unavailable".into(),
                    last_error: Some(error.message().to_string()),
                    ..GitStatusState::default()
                });
            }),
        }
    }

    /// Stage paths from the Coding Git panel.
    pub fn coding_git_stage(&self, paths: &[String]) -> JaymiResult<()> {
        self.coding_git_mutate(GitOperation::Stage, paths, None)
    }

    /// Unstage paths from the Coding Git panel.
    pub fn coding_git_unstage(&self, paths: &[String]) -> JaymiResult<()> {
        self.coding_git_mutate(GitOperation::Unstage, paths, None)
    }

    /// Request discard confirmation for paths (does not mutate the repository yet).
    pub fn coding_git_request_discard(&self, paths: &[String]) -> JaymiResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        self.with_coding_state(|coding| {
            let git = coding.git.get_or_insert_with(GitStatusState::default);
            git.pending_discard = Some(paths.to_vec());
            git.last_error = None;
        })
    }

    /// Cancel a pending discard confirmation.
    pub fn coding_git_cancel_discard(&self) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            if let Some(git) = coding.git.as_mut() {
                git.pending_discard = None;
            }
        })
    }

    /// Confirm and execute a pending discard (or discard `paths` when provided).
    pub fn coding_git_confirm_discard(&self, paths: Option<&[String]>) -> JaymiResult<()> {
        let pending = self.with_coding_state(|coding| {
            coding
                .git
                .as_ref()
                .and_then(|git| git.pending_discard.clone())
        })?;
        let targets = match paths {
            Some(paths) if !paths.is_empty() => paths.to_vec(),
            _ => pending.unwrap_or_default(),
        };
        if targets.is_empty() {
            return self.coding_git_cancel_discard();
        }
        self.coding_git_discard(&targets)
    }

    /// Discard path changes from the Coding Git panel (no confirmation).
    pub fn coding_git_discard(&self, paths: &[String]) -> JaymiResult<()> {
        self.coding_git_mutate(GitOperation::Discard, paths, None)
    }

    /// Commit staged changes using the draft commit message.
    pub fn coding_git_commit_active(&self) -> JaymiResult<()> {
        let message = self.with_coding_state(|coding| {
            coding
                .git
                .as_ref()
                .map(|git| git.commit_message.clone())
                .unwrap_or_default()
        })?;
        self.coding_git_commit(&message)
    }

    /// Commit staged changes from the Coding Git panel.
    pub fn coding_git_commit(&self, message: &str) -> JaymiResult<()> {
        self.coding_git_mutate(GitOperation::Commit, &[], Some(message))
    }

    /// Update the draft commit message in Coding Git state.
    pub fn set_coding_git_commit_message(&self, message: String) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            let git = coding.git.get_or_insert_with(GitStatusState::default);
            git.commit_message = message;
        })
    }

    fn coding_git_mutate(
        &self,
        operation: GitOperation,
        paths: &[String],
        message: Option<&str>,
    ) -> JaymiResult<()> {
        let root = self
            .with_coding_state(|coding| coding.explorer.project_root.clone())?
            .ok_or_else(|| JaymiError::new("coding git has no project root"))?;
        let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let response = match operation {
            GitOperation::Status => self.git_status(&root)?,
            GitOperation::Stage => self.git_stage(&root, path_bufs)?,
            GitOperation::Unstage => self.git_unstage(&root, path_bufs)?,
            GitOperation::Discard => self.git_discard(&root, path_bufs)?,
            GitOperation::Commit => {
                let message = message
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| JaymiError::new("commit requires a message"))?;
                self.git_commit(&root, message)?
            }
        };
        if response.blocked {
            return Err(JaymiError::new(response.content));
        }
        self.apply_git_response(&response)?;
        if matches!(operation, GitOperation::Commit) {
            self.with_coding_state(|coding| {
                if let Some(git) = coding.git.as_mut() {
                    git.commit_message.clear();
                }
            })?;
        }
        Ok(())
    }

    fn apply_git_response(&self, response: &PlannerResponse) -> JaymiResult<()> {
        let commit_message = self.with_coding_state(|coding| {
            coding
                .git
                .as_ref()
                .map(|git| git.commit_message.clone())
                .unwrap_or_default()
        })?;
        let to_entries = |items: &[jaymi_core::GitPathStatus]| -> Vec<GitFileEntry> {
            items
                .iter()
                .map(|item| GitFileEntry {
                    path: item.path.clone(),
                    status: item.status.clone(),
                })
                .collect()
        };
        let repo_root = response
            .listed_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let is_repository = response.git_is_repository.unwrap_or(true);
        self.with_coding_state(|coding| {
            let mut state = GitStatusState {
                commit_message,
                ..GitStatusState::default()
            };
            state.apply_snapshot(
                is_repository,
                repo_root,
                response.git_branch.clone(),
                response.git_summary.clone().unwrap_or_else(|| {
                    if is_repository {
                        "clean".into()
                    } else {
                        "not a git repository".into()
                    }
                }),
                to_entries(&response.git_modified),
                to_entries(&response.git_added),
                to_entries(&response.git_deleted),
                to_entries(&response.git_staged),
                to_entries(&response.git_untracked),
            );
            coding.git = Some(state);
        })
    }

    /// Open the Research Workspace from the conversation action menu.
    pub fn start_research_workspace(&self) -> JaymiResult<()> {
        let expansion = workspace_expansion_for(
            Capability::Search,
            "Started Research from conversation menu",
        )
        .ok_or_else(|| JaymiError::new("search capability has no research workspace mapping"))?;
        self.expand_ui_workspace(expansion)
    }

    /// Open the Creation Workspace from the conversation action menu.
    pub fn start_creation_workspace(&self) -> JaymiResult<()> {
        let expansion = workspace_expansion_for(
            Capability::GenerateImages,
            "Started Creation from conversation menu",
        )
        .ok_or_else(|| {
            JaymiError::new("generate_images capability has no creation workspace mapping")
        })?;
        self.expand_ui_workspace(expansion)
    }

    /// Close the expanded workspace; conversation turns remain intact.
    ///
    /// Persists Coding editor UI state to the project, then discards in-memory
    /// capability runtime state with the workspace.
    pub fn close_ui_workspace(
        &self,
    ) -> JaymiResult<Option<jaymi_capabilities::WorkspaceExpansion>> {
        let _ = self.persist_coding_editor_workspace();
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
        drop(experience);
        if let Ok(terminal) = self.container.resolve::<Arc<TerminalProvider>>() {
            let _ = terminal.close_all_sessions();
        }
        self.prepare_context_session()?;
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

    /// Last Planner activity recorded for Coding Diagnostics.
    pub fn last_planner_activity(&self) -> Option<LastPlannerActivity> {
        self.last_planner_activity
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Build the read-only Coding Workspace diagnostics view.
    pub fn coding_diagnostics_view(&self) -> JaymiResult<CodingDiagnosticsView> {
        let snapshot = self.diagnostics()?;
        let experience = self.experience().unwrap_or_default();
        let coding = experience
            .capability_state()
            .and_then(|state| state.coding())
            .cloned();
        let activity = self.last_planner_activity();
        let project = self
            .project_context(None)
            .ok()
            .flatten()
            .map(|context| context.project);

        Ok(build_coding_diagnostics_view(
            &snapshot,
            &experience,
            coding.as_ref(),
            project.as_ref(),
            activity.as_ref(),
        ))
    }

    /// Build the [`ProblemsCollectContext`] snapshot from live Application state.
    ///
    /// Read-only: providers never reach into UI, Planner, or storage directly —
    /// they only interpret this context, rebuilt on every call to
    /// [`Self::refresh_coding_problems`].
    pub fn build_problems_context(&self) -> ProblemsCollectContext {
        let coding = self.experience().ok().and_then(|experience| {
            experience
                .capability_state()
                .and_then(|state| state.coding())
                .cloned()
        });

        let project_root = coding
            .as_ref()
            .and_then(|state| state.explorer.project_root.clone());

        let lsp_issues = coding
            .as_ref()
            .map(|state| {
                state
                    .diagnostics
                    .iter()
                    .enumerate()
                    .map(|(index, diag)| ProblemIssue {
                        id: format!(
                            "lsp:{}:{}",
                            diag.path.as_deref().unwrap_or("-"),
                            diag.line.unwrap_or(index as u32)
                        ),
                        severity: ProblemSeverity::parse(&diag.severity),
                        source: "lsp".to_string(),
                        source_label: if diag.source.trim().is_empty() {
                            "rust-analyzer".to_string()
                        } else {
                            diag.source.clone()
                        },
                        path: diag.path.clone(),
                        line: diag.line,
                        column: diag.character,
                        end_line: diag.end_line,
                        end_column: diag.end_character,
                        message: diag.message.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let workspace_error = coding
            .as_ref()
            .and_then(|state| match &state.explorer.status {
                ExplorerStatus::Error(message) => Some(message.clone()),
                _ => None,
            });
        let git_error = coding
            .as_ref()
            .and_then(|state| state.git.as_ref())
            .and_then(|git| git.last_error.clone());

        let activity = self.last_planner_activity();
        let planner_blocked = activity.as_ref().is_some_and(|activity| activity.blocked);
        let planner_summary = activity
            .as_ref()
            .filter(|activity| activity.blocked)
            .map(|activity| activity.summary.clone())
            .filter(|summary| !summary.trim().is_empty());
        let permission_decision = activity
            .as_ref()
            .and_then(|activity| activity.permission_decision.clone());
        let permission_denied = permission_decision
            .as_deref()
            .map(|decision| decision.eq_ignore_ascii_case("denied"))
            .unwrap_or(false);

        let snapshot = self.diagnostics().ok();
        let (index_status, index_detail) = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.subsystem("Index Status"))
            .map(|row| {
                (
                    Some(row.status.label().to_string()),
                    Some(row.detail.clone()),
                )
            })
            .unwrap_or((None, None));
        let search_unhealthy = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.subsystem("Search Engine"))
            .filter(|row| !row.status.is_operational())
            .map(|row| row.detail.clone());
        let understanding_failure = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.subsystem("Understanding"))
            .and_then(|row| extract_labeled_value(&row.detail, "last_failure="));
        let memory_row = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.subsystem("Memory Status"));
        let memory_status = memory_row.map(|row| row.status.label().to_string());
        let memory_detail = memory_row.map(|row| row.detail.clone());
        let memory_unhealthy = memory_row.is_some_and(|row| !row.status.is_operational());

        ProblemsCollectContext {
            project_root,
            lsp_issues,
            workspace_error,
            git_error,
            planner_blocked,
            planner_summary,
            permission_decision,
            permission_denied,
            index_status,
            index_detail,
            search_unhealthy,
            understanding_failure,
            memory_status,
            memory_detail,
            memory_unhealthy,
        }
    }

    /// Recompute `CodingState.problems` from every registered Problems provider.
    ///
    /// No-op (returns `Ok`) when there is no Coding capability state yet.
    pub fn refresh_coding_problems(&self) -> JaymiResult<()> {
        if self.with_coding_state(|_| ()).is_err() {
            return Ok(());
        }
        let registry = self.problems_registry()?;
        let ctx = self.build_problems_context();
        let issues = registry.collect_all(&ctx)?;
        self.with_coding_state(|coding| {
            coding.problems = issues;
        })
    }

    fn record_planner_activity(&self, response: &PlannerResponse, duration_ms: u64) {
        let activity = LastPlannerActivity {
            summary: response.content.clone(),
            capability_id: response
                .capability
                .map(|capability| capability.id().to_string()),
            tool_id: response.tool_id.clone(),
            provider_id: response.provider_id.clone(),
            blocked: response.blocked,
            duration_ms,
            permission_decision: response
                .permission_result
                .as_ref()
                .map(|result| result.decision.as_str().to_string()),
            policy_summary: response
                .policy_evaluation
                .as_ref()
                .map(|evaluation| evaluation.summary()),
            memory_hits: response
                .memory_context
                .as_ref()
                .map(|context| context.memories.len())
                .unwrap_or(0),
        };
        if let Ok(mut guard) = self.last_planner_activity.lock() {
            *guard = Some(activity);
        }
        // Opportunistic refresh so a blocked / denied turn shows up in the
        // Problems panel without waiting for the next explicit refresh.
        let _ = self.refresh_coding_problems();
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
        let memory_status = memory
            .health()
            .unwrap_or_else(|_| jaymi_memory::MemoryHealth {
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
        let discovery = planner.discover_capability_status().unwrap_or_default();
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
            .map(|report| report.with_active_workspace(self.active_ui_workspace().ok().flatten()));
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

        let stub_provider_ids: Vec<&str> = provider_ids
            .iter()
            .filter(|id| id.contains("placeholder"))
            .map(String::as_str)
            .collect();
        let ready_provider_ids: Vec<&str> = provider_ids
            .iter()
            .filter(|id| !id.contains("placeholder"))
            .map(String::as_str)
            .collect();

        let subsystems = vec![
            SubsystemStatus::new(
                "Planner",
                OperationalStatus::from_health(planner_health.healthy, planner_health.initialized),
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
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Disabled
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
                OperationalStatus::from_health(config_health.healthy, config_health.initialized),
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
                OperationalStatus::from_health(logger_health.healthy, logger_health.initialized),
                format!("level={} · {}", logging_level, logger.log_path().display()),
            ),
            SubsystemStatus::new(
                "Permissions",
                OperationalStatus::from_health(
                    permissions_health.healthy,
                    permissions_health.initialized,
                ),
                permission_mode.clone(),
            ),
            SubsystemStatus::new(
                "Policies",
                if !policies_health.initialized {
                    OperationalStatus::Disabled
                } else {
                    // Only Offline First is boot-active and enforced; other builtins
                    // are declared without constraint logic.
                    OperationalStatus::Experimental
                },
                {
                    let enforced: Vec<&str> = active_policies
                        .iter()
                        .filter(|name| *name == "Offline First" || *name == "Privacy Maximum")
                        .map(String::as_str)
                        .collect();
                    if active_policies.is_empty() {
                        "no active policies · other builtins declared".to_string()
                    } else {
                        format!(
                            "enforced: {} · active: {} · other builtins declared",
                            if enforced.is_empty() {
                                "none".to_string()
                            } else {
                                enforced.join(", ")
                            },
                            active_policies.join(", ")
                        )
                    }
                },
            ),
            SubsystemStatus::new(
                "Providers",
                if !providers.is_initialized() {
                    OperationalStatus::Disabled
                } else if ready_provider_ids.is_empty() && !stub_provider_ids.is_empty() {
                    OperationalStatus::Stub
                } else if ready_provider_ids.is_empty() {
                    OperationalStatus::Disabled
                } else if !stub_provider_ids.is_empty() {
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Operational
                },
                if provider_ids.is_empty() {
                    "none registered".to_string()
                } else {
                    format!(
                        "{} ready · {} stub · {}",
                        ready_provider_ids.len(),
                        stub_provider_ids.len(),
                        provider_ids.join(", ")
                    )
                },
            ),
            SubsystemStatus::new(
                "OCR Provider",
                if !ocr_status.initialized {
                    OperationalStatus::Disabled
                } else if ocr_status.placeholder {
                    OperationalStatus::Stub
                } else if ocr_status.available {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Experimental
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
                    OperationalStatus::Disabled
                } else if embedding_status.available {
                    // Local lexical embeddings — usable, not a neural model.
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Disabled
                },
                format!(
                    "id={} · model={} · dims={} · lexical · {}",
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
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Disabled
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
                } else if capabilities.is_initialized() {
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Disabled
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
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Disabled
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
                    OperationalStatus::Experimental
                } else {
                    OperationalStatus::Disabled
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
                if !indexing_enabled {
                    OperationalStatus::Disabled
                } else if discovery_health.initialized {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Disabled
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
                    OperationalStatus::Disabled
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
                    OperationalStatus::Disabled
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
                    OperationalStatus::Disabled
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
                    Some(health) if health.initialized => OperationalStatus::Experimental,
                    Some(_) => OperationalStatus::Disabled,
                    None => OperationalStatus::Disabled,
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
                    Some(health) if health.initialized => OperationalStatus::Experimental,
                    Some(_) => OperationalStatus::Disabled,
                    None => OperationalStatus::Disabled,
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
                    | jaymi_discovery::WatcherStatus::Idle => OperationalStatus::Operational,
                    jaymi_discovery::WatcherStatus::Disabled => OperationalStatus::Disabled,
                    jaymi_discovery::WatcherStatus::Stopped => OperationalStatus::Disabled,
                    jaymi_discovery::WatcherStatus::Error(_) => OperationalStatus::Disabled,
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
                OperationalStatus::from_health(memory_status.healthy, memory_status.initialized),
                memory_status.detail.clone(),
            ),
            SubsystemStatus::new(
                "Context Engine",
                OperationalStatus::from_health(context_health.healthy, context_health.initialized),
                format!(
                    "sources_bound={} · assembles={}",
                    context_engine.sources_bound(),
                    context_engine.assemble_count()
                ),
            ),
            SubsystemStatus::new(
                "Project Status",
                OperationalStatus::from_health(project_status.healthy, project_status.initialized),
                project_status.detail.clone(),
            ),
            SubsystemStatus::new(
                "Reasoning Status",
                if planner.reasoning_implemented() {
                    OperationalStatus::Operational
                } else {
                    OperationalStatus::Stub
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
            policy_allowed: response.as_ref().and_then(|value| {
                value
                    .policy_evaluation
                    .as_ref()
                    .map(|evaluation| evaluation.allowed)
            }),
            policy_summary: response.as_ref().and_then(|value| {
                value
                    .policy_evaluation
                    .as_ref()
                    .map(|evaluation| evaluation.summary())
            }),
            request_blocked: response
                .as_ref()
                .map(|value| value.blocked)
                .unwrap_or(false),
            listed_path: response
                .as_ref()
                .and_then(|value| value.listed_path.clone()),
            listing_summary: response.as_ref().and_then(|value| {
                if value.document.is_none() || value.blocked {
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

/// Promote an already-open tab to permanent (clear `preview`) in whichever
/// pane it lives, then focus that pane/tab. Returns `false` when the path
/// isn't open in any pane. Never touches buffer content.
fn promote_and_focus_existing(coding: &mut CodingState, path: &str) -> bool {
    if coding.editors.session_by_path(path).is_none() {
        return false;
    }
    for pane in coding.editors.panes.values_mut() {
        if let Some(tab) = pane.tabs.iter_mut().find(|tab| tab.path == path) {
            tab.preview = false;
        }
    }
    let _ = coding.focus_tab(path);
    true
}

/// Extract a `key=value` token from a subsystem detail string (space-delimited).
///
/// Returns `None` when the key is absent or the value is empty / `-`.
fn extract_labeled_value(detail: &str, key: &str) -> Option<String> {
    let start = detail.find(key)? + key.len();
    let rest = &detail[start..];
    let end = rest.find(' ').unwrap_or(rest.len());
    let value = rest[..end].trim();
    if value.is_empty() || value == "-" {
        None
    } else {
        Some(value.to_string())
    }
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
        assert_eq!(diagnostics.provider_count, 6);
        assert!(diagnostics
            .provider_ids
            .iter()
            .any(|id| id == OCR_PROVIDER_ID));
        assert_eq!(diagnostics.tool_count, 12);
        assert_eq!(
            diagnostics.capability_count,
            jaymi_capabilities::Capability::all().len()
        );
        assert!(diagnostics.database_connected);
        assert_eq!(
            diagnostics
                .database_path
                .as_ref()
                .map(std::path::PathBuf::from),
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
            diagnostics
                .logging_path
                .as_ref()
                .map(std::path::PathBuf::from),
            Some(data_dir.join("logs").join("jaymi.log"))
        );
        assert!(data_dir.join("logs").join("jaymi.log").exists());
        assert!(data_dir.join("config.json").exists());
        assert_eq!(
            diagnostics
                .config_path
                .as_ref()
                .map(std::path::PathBuf::from),
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
            OperationalStatus::Stub
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
