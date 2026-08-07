//! Deterministic application boot sequence.
//!
//! Startup order:
//! Configuration → Logging → Database → Policy Engine → Permission Engine →
//! Memory Engine → Context Engine → Capability Registry → Provider Registry →
//! Knowledge → Understanding → Search → Project Engine → Discovery → Tools →
//! Planner → Desktop UI

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use jaymi_capabilities::{
    build_explorer_tree, is_editable_coding_extension, workspace_expansion_for, Capability,
    CapabilityDiscoveryReport, CapabilityEngine, CapabilityEngineApi, CapabilityInspectorReport,
    CapabilityState, CodingBottomTab, CodingState, CreationState, DiagnosticState, EditorPaneId,
    EditorSelection, EditorSettings, ExplorerPending, ExplorerStatus, FoldedRegion, GitFileEntry,
    GitStatusState, ProblemIssue, ProblemSeverity, ProblemsCollectContext, ResearchState,
    SearchResultEntry, SplitDirection, WorkspaceKind,
};

use jaymi_config::{Config, ReasoningPreferences};
use jaymi_context::{ContextBundle, ContextEngine, ContextHistoryEntry, ContextInspectorReport};
use jaymi_core::{
    AppState, CodingAction, DiscoveryQueryKind, EntryType, GitOperation, HealthReport, JaymiError,
    JaymiResult, Lifecycle, SearchRequest, ServiceContainer, TerminalOperation, TerminalRequest,
    UserRequest,
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
use jaymi_planner::{Planner, PlannerDeps, PlannerResponse, ReviewIntent, ToolRouteTable};
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
use jaymi_reasoning::{
    ConversationStream, ConversationStreamEvent, ModelRegistry, ReasoningDiagnosticsInput,
    ReasoningDiagnosticsReport, ReasoningProvider,
};
use jaymi_reasoning_ollama::OllamaReasoningProvider;
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
use crate::diagnostics::{DiagnosticsSnapshot, LastReasoningTurn};
use crate::editor_workspace::{load_editor_workspace, save_editor_workspace};
use crate::experience::{ConversationTurn, ExperienceSession};
use crate::session_cache::SessionCache;
use crate::context_maintenance::{
    merge_completed_into_session, ContextMaintenance, MaintenanceJobRequest, MaintenanceKind,
    MaintenanceUiUpdate, WorkspaceSnapshotCaptureInput,
};

/// In-flight pumpable conversational generation (Sprint B1.11).
struct ActiveGeneration {
    stream: ConversationStream,
    context: ContextBundle,
    turn_index: usize,
    prompt_diagnostics: jaymi_reasoning::PromptDiagnostics,
    /// Wall clock from request receipt (diagnostics only).
    request_started: std::time::Instant,
    /// Planner / Context / PromptBuilder timings captured at stream start.
    early_pipeline: jaymi_reasoning::PipelineTiming,
    #[allow(dead_code)] // retained for regenerate / diagnostics context
    user_text: String,
}

/// Background start in progress — UI already acknowledged Thinking.
struct PendingGeneration {
    rx: Receiver<GenerationStartOutcome>,
    cancel: Arc<AtomicBool>,
    turn_index: usize,
    request_started: std::time::Instant,
    user_text: String,
}

/// Generation slot: ack/start pending, or stream ready to pump.
enum GenerationSlot {
    /// Host prep + assemble + stream open running off the UI thread.
    Starting(PendingGeneration),
    /// Provider stream open; tokens via [`ConversationStream::try_pump`].
    Active(ActiveGeneration),
}

/// Result of background generation start (delivered via [`Application::pump_generation`]).
enum GenerationStartOutcome {
    /// Conversational stream opened successfully.
    Ready {
        stream: ConversationStream,
        context: ContextBundle,
        prompt_diagnostics: jaymi_reasoning::PromptDiagnostics,
        early_pipeline: jaymi_reasoning::PipelineTiming,
    },
    /// Soft-fail / tool-backed path finished on the worker (no stream to pump).
    Completed(PlannerResponse),
    /// Worker aborted because the user cancelled before start finished.
    Cancelled,
    /// Start failed after ack (Planner should already be Failed when applicable).
    Failed(String),
}

/// Outcome of starting a prompt (always pumpable after UI-thread ack).
#[derive(Debug)]
pub enum BeginGeneration {
    /// Send acknowledged — call [`Application::pump_generation`] each frame.
    /// Expensive prep / assemble / stream-open run on a background task.
    Started,
    /// Non-conversational / soft path completed (legacy sync callers only).
    /// Interactive UI receives soft/tool completions via [`PumpGeneration::Finished`].
    Completed(PlannerResponse),
}

/// Result of pumping an active generation.
#[derive(Debug)]
pub enum PumpGeneration {
    /// Still active; Experience was updated with incremental events.
    Active {
        /// Number of stream events applied this pump.
        events: usize,
    },
    /// Terminal event applied; generation cleared.
    Finished(PlannerResponse),
    /// No active generation.
    Idle,
}

/// Owns the process service container and application state.
pub struct Application {
    state: AppState,
    container: ServiceContainer,
    health_reports: Vec<HealthReport>,
    /// Conversation-first experience (workspaces expand without destroying chat).
    experience: Mutex<ExperienceSession>,
    /// Last Planner turn for Coding Diagnostics (activity / timing).
    last_planner_activity: Mutex<Option<LastPlannerActivity>>,
    /// Last conversational reasoning turn for Reasoning Diagnostics (B1.10).
    last_reasoning: Mutex<Option<LastReasoningTurn>>,
    /// Active or starting pumpable conversational generation (B1.11 / UI-thread ack).
    active_generation: Mutex<Option<GenerationSlot>>,
    /// Session-scoped cache for inexpensive immutable snapshots (not conversation).
    session_cache: Mutex<SessionCache>,
    /// Background context maintenance (git / inventory / diagnostics / file
    /// summaries / ambient WorkspaceSnapshot).
    context_maintenance: Arc<ContextMaintenance>,
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
            last_reasoning: Mutex::new(None),
            active_generation: Mutex::new(None),
            session_cache: Mutex::new(SessionCache::new()),
            context_maintenance: Arc::new(ContextMaintenance::new()),
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
        // Wrap for runtime preference updates from Settings (&self Application APIs).
        let config = {
            let config = self
                .container
                .take::<Config>()
                .ok_or_else(|| JaymiError::new("configuration missing after boot"))?;
            let config = Arc::new(Mutex::new(config));
            self.container.register(Arc::clone(&config));
            config
        };

        let (data_dir, log_level) = {
            let config = config.lock().map_err(|_| JaymiError::new("config lock poisoned"))?;
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
            let config = self.container.resolve::<Arc<Mutex<Config>>>()?;
            let config = config
                .lock()
                .map_err(|_| JaymiError::new("config lock poisoned"))?;
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

        // Invalidate ContextBundle cache when discovery mutates the inventory
        // (explicit index scans and filesystem watcher flushes).
        let context_for_cache = Arc::clone(&context);
        discovery.add_change_hook(Arc::new(move || {
            context_for_cache.request_fresh_context("search_index_updated");
        }));

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

        let ollama = Arc::new(OllamaReasoningProvider::local());
        self.container.register(Arc::clone(&ollama));

        let registry =
            ModelRegistry::with_provider(Arc::clone(&ollama) as Arc<dyn ReasoningProvider>);
        let _ = registry.refresh();
        let registry = Arc::new(registry);
        self.container.register(Arc::clone(&registry));

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
            routes: ToolRouteTable::builtin(),
            reasoning: Some(Arc::clone(&ollama) as Arc<dyn ReasoningProvider>),
            model_registry: Some(Arc::clone(&registry)),
        });
        self.initialize_service(&mut planner)?;
        // Restore persisted Reasoning preferences into registry + Planner.
        Self::apply_reasoning_preferences_locked(&planner, &registry, &self.container)?;
        self.container.register(planner);

        {
            let logger = self.container.resolve::<Logger>()?;
            logger.info("boot", "Jaymi startup complete");
        }

        // Seed session cache with inexpensive immutable snapshots (not conversation).
        self.seed_session_cache()?;

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

    /// Sync live UI / engine state into the Context Engine before handle.
    ///
    /// **Required before every Planner path that assembles context** — including
    /// conversational generation (`begin_generation` / streaming). This is the
    /// sole Application preparation entrypoint.
    ///
    /// Merges the latest **completed** ambient maintenance snapshots (git /
    /// inventory / diagnostics / file summaries / WorkspaceSnapshot /
    /// EditorSnapshot / ProjectSnapshot / RuntimeSnapshot). Never waits on
    /// in-flight maintenance. Never rebuilds a WorkspaceSnapshot or probes
    /// toolchain marker files here (Sprint B2.13.2) — if none completed yet,
    /// schedules ambient refresh. Request-selected capabilities are **not**
    /// pushed here; the Planner supplies them via
    /// [`jaymi_context::AssembleHints`].
    ///
    /// Future Workspace Intelligence enrichments land here so conversation and
    /// tool-backed requests share one preparation path.
    fn prepare_context_session(&self) -> JaymiResult<()> {
        // Apply finished UI updates without blocking; conversation uses snapshots only.
        let _ = self.pump_context_maintenance();

        let context = self.container.resolve::<Arc<ContextEngine>>()?;
        let workspace_kind = self
            .experience()
            .ok()
            .and_then(|session| session.active_workspace_kind())
            .map(|kind| kind.id().to_string());

        let coding = self
            .with_coding_state(|coding| coding.clone())
            .ok();

        let (project_open, project_indexed_documents, project_facts) =
            self.workspace_snapshot_host_facts(coding.as_ref());

        let permissions = self.container.resolve::<Arc<PermissionEngine>>()?;
        let mut inputs = crate::context_session::build_context_session_inputs(
            workspace_kind,
            coding.as_ref(),
            permissions.as_ref(),
            project_open,
            project_indexed_documents,
            project_facts,
        );
        let completed = self.context_maintenance.latest_completed();
        // Prefer the latest **completed** ambient snapshots. WorkspaceSnapshot is
        // never rebuilt here — ambient maintenance owns observation (including
        // observe_toolchain). EditorSnapshot may still bootstrap from in-memory
        // CodingState when no completed editor exists.
        let bootstrapped_editor =
            completed.editor_snapshot.is_none() && inputs.editor_snapshot.is_some();
        let needs_workspace_refresh = completed.workspace_snapshot.is_none();
        merge_completed_into_session(&mut inputs, &completed);
        if bootstrapped_editor {
            if let Some(snapshot) = inputs.editor_snapshot.clone() {
                self.context_maintenance.publish_editor_snapshot(snapshot);
            }
        }
        if needs_workspace_refresh {
            // Non-blocking: first conversation may assemble without a snapshot
            // until the ambient job completes; subsequent prepares merge it.
            let _ = self.schedule_workspace_snapshot_refresh();
        }
        // Keep snapshot branch aligned with the latest completed git contribution.
        if let Some(snapshot) = inputs.workspace_snapshot.as_mut() {
            if snapshot.active_branch.is_none() {
                snapshot.active_branch = inputs.git_status.branch.clone();
            }
        }
        context.set_session_inputs(inputs);
        Ok(())
    }

    /// Project / branch facts for WorkspaceSnapshot capture (shared by prepare + ambient).
    fn workspace_snapshot_host_facts(
        &self,
        coding: Option<&CodingState>,
    ) -> (bool, Option<u64>, crate::context_session::WorkspaceSnapshotHostFacts) {
        self.container
            .resolve::<Arc<ProjectEngine>>()
            .map(|projects| {
                let open_id = projects.open_project_id();
                let open = open_id.is_some();
                let ctx = projects.project_context(None).ok().flatten();
                let indexed = ctx
                    .as_ref()
                    .map(|ctx| ctx.search_index.indexed_file_count);
                let (name, root) = ctx
                    .map(|ctx| {
                        (
                            Some(ctx.project.name.clone()),
                            ctx.project
                                .root_directory
                                .as_ref()
                                .map(|path| path.display().to_string()),
                        )
                    })
                    .unwrap_or((None, None));
                (
                    open,
                    indexed,
                    crate::context_session::WorkspaceSnapshotHostFacts {
                        project_id: open_id,
                        project_name: name,
                        project_root: root.or_else(|| {
                            coding.and_then(|state| state.explorer.project_root.clone())
                        }),
                        active_branch: coding
                            .and_then(|state| state.git.as_ref())
                            .and_then(|git| git.branch.clone())
                            .or_else(|| {
                                self.context_maintenance
                                    .latest_completed()
                                    .git_status
                                    .as_ref()
                                    .and_then(|git| git.branch.clone())
                            }),
                    },
                )
            })
            .unwrap_or((
                false,
                None,
                crate::context_session::WorkspaceSnapshotHostFacts {
                    project_root: coding.and_then(|state| state.explorer.project_root.clone()),
                    active_branch: coding
                        .and_then(|state| state.git.as_ref())
                        .and_then(|git| git.branch.clone()),
                    ..Default::default()
                },
            ))
    }

    /// Drain completed background maintenance into Coding UI + snapshot store.
    ///
    /// Non-blocking. Call from the UI frame and before conversational prepare.
    /// Also drains coalesced WorkspaceSnapshot reschedules after inflight jobs.
    pub fn pump_context_maintenance(&self) -> JaymiResult<usize> {
        let updates = self.context_maintenance.pump();
        let count = updates.len();
        let mut refresh_workspace_snapshot = false;
        for update in updates {
            match update {
                MaintenanceUiUpdate::Git(git) => {
                    let _ = self.with_coding_state(|coding| {
                        coding.git = Some(git.clone());
                    });
                    refresh_workspace_snapshot = true;
                }
                MaintenanceUiUpdate::Explorer {
                    root,
                    nodes,
                    status,
                } => {
                    let _ = self.with_coding_state(|coding| {
                        coding.explorer.project_root = Some(root.clone());
                        if matches!(status, ExplorerStatus::Ready) {
                            for node in &nodes {
                                if node.is_dir {
                                    coding.explorer.expanded_paths.insert(node.path.clone());
                                }
                            }
                        }
                        coding.explorer.nodes = nodes;
                        coding.explorer.status = status;
                    });
                }
                MaintenanceUiUpdate::Problems(issues) => {
                    let _ = self.with_coding_state(|coding| {
                        coding.problems = issues;
                    });
                    refresh_workspace_snapshot = true;
                }
            }
        }
        if refresh_workspace_snapshot
            || self
                .context_maintenance
                .take_workspace_snapshot_reschedule()
            || self.context_maintenance.take_editor_snapshot_reschedule()
        {
            self.schedule_coding_observation_refresh();
        }
        if self.context_maintenance.take_project_snapshot_reschedule() {
            let _ = self.schedule_project_snapshot_refresh();
        }
        if self.context_maintenance.take_runtime_snapshot_reschedule() {
            let _ = self.schedule_runtime_snapshot_refresh();
        }
        Ok(count)
    }

    /// Schedule background maintenance without blocking conversation.
    pub fn schedule_context_maintenance(&self, kind: MaintenanceKind) -> bool {
        let request = self.maintenance_job_request(kind);
        self.context_maintenance.schedule(request)
    }

    /// Schedule ambient [`jaymi_context::WorkspaceSnapshot`] refresh (Sprint B2.2).
    ///
    /// Observation only — never rebuilds a ContextBundle, never reasons, never
    /// calls LLMs, never executes tools. Conversation prepare merges the latest
    /// completed snapshot without waiting.
    pub fn schedule_workspace_snapshot_refresh(&self) -> bool {
        self.schedule_context_maintenance(MaintenanceKind::WorkspaceSnapshot)
    }

    /// Schedule ambient [`jaymi_context::EditorSnapshot`] refresh (Sprint B2.3).
    ///
    /// Read-only observation (± optional LspProvider enrichment). Never rebuilds
    /// a ContextBundle, never reasons, never calls LLMs, never executes the
    /// language_server tool. Planner remains the sole interactive LSP owner.
    pub fn schedule_editor_snapshot_refresh(&self) -> bool {
        self.schedule_context_maintenance(MaintenanceKind::EditorSnapshot)
    }

    /// Schedule ambient [`jaymi_context::ProjectSnapshot`] refresh (Sprint B2.4).
    ///
    /// Marker / shallow FS observation only — never rebuilds a ContextBundle,
    /// never reasons, never calls LLMs, never executes tools. Planner never
    /// scans projects; Context providers read the completed session snapshot.
    pub fn schedule_project_snapshot_refresh(&self) -> bool {
        self.schedule_context_maintenance(MaintenanceKind::ProjectSnapshot)
    }

    /// Schedule ambient [`jaymi_context::RuntimeSnapshot`] refresh (Sprint B2.6).
    ///
    /// Observes Coding terminal sessions (+ TerminalProvider alive list). Never
    /// re-runs cargo / tests, never rebuilds a ContextBundle, never reasons,
    /// never calls LLMs. Conversation never waits for runtime.
    pub fn schedule_runtime_snapshot_refresh(&self) -> bool {
        self.schedule_context_maintenance(MaintenanceKind::RuntimeSnapshot)
    }

    /// Schedule ambient Coding observation refreshes (workspace + editor).
    ///
    /// Project intelligence is scheduled separately on project / Coding open so
    /// cursor thrash does not re-walk marker files. Runtime intelligence is
    /// scheduled on terminal activity / Coding open.
    pub fn schedule_coding_observation_refresh(&self) {
        let _ = self.schedule_workspace_snapshot_refresh();
        let _ = self.schedule_editor_snapshot_refresh();
    }

    /// Schedule the coding-open maintenance set (inventory / git / diagnostics /
    /// summaries / WorkspaceSnapshot / EditorSnapshot / ProjectSnapshot /
    /// RuntimeSnapshot).
    pub fn schedule_coding_context_maintenance(&self) {
        let request = self.maintenance_job_request(MaintenanceKind::WorkspaceInventory);
        self.context_maintenance.schedule_coding_open(request);
    }

    fn maintenance_job_request(&self, kind: MaintenanceKind) -> MaintenanceJobRequest {
        let project_root = self.active_project_root_path().or_else(|| {
            self.with_coding_state(|coding| {
                coding
                    .explorer
                    .project_root
                    .as_ref()
                    .map(PathBuf::from)
            })
            .ok()
            .flatten()
        });
        let open_file_paths = self
            .with_coding_state(|coding| {
                coding
                    .editors
                    .open_files()
                    .into_iter()
                    .map(|file| file.path)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // Capture problems inputs for every request so `schedule_coding_open`
        // can clone one payload into the Diagnostics job without a second round-trip.
        let problems_context = Some(self.build_problems_context());
        let problems_registry = self.problems_registry().ok();
        let workspace_snapshot_input = Some(self.workspace_snapshot_capture_input());
        let editor_snapshot_input = Some(self.editor_snapshot_capture_input());
        let project_snapshot_input = Some(self.project_snapshot_capture_input());
        let runtime_snapshot_input = Some(self.runtime_snapshot_capture_input());
        MaintenanceJobRequest {
            kind,
            project_root,
            open_file_paths,
            problems_context,
            problems_registry,
            workspace_snapshot_input,
            editor_snapshot_input,
            project_snapshot_input,
            runtime_snapshot_input,
        }
    }

    fn workspace_snapshot_capture_input(&self) -> WorkspaceSnapshotCaptureInput {
        let workspace_kind = self
            .experience()
            .ok()
            .and_then(|session| session.active_workspace_kind())
            .map(|kind| kind.id().to_string());
        let coding = self.with_coding_state(|coding| coding.clone()).ok();
        let (_, _, facts) = self.workspace_snapshot_host_facts(coding.as_ref());
        WorkspaceSnapshotCaptureInput {
            workspace_kind,
            coding,
            facts,
        }
    }

    fn editor_snapshot_capture_input(&self) -> crate::context_maintenance::EditorSnapshotCaptureInput {
        let coding = self.with_coding_state(|coding| coding.clone()).ok();
        let project_root = self.active_project_root_path().or_else(|| {
            coding
                .as_ref()
                .and_then(|state| state.explorer.project_root.as_ref().map(PathBuf::from))
        });
        let lsp = self
            .container
            .resolve::<Arc<LspProvider>>()
            .ok()
            .map(|provider| Arc::clone(&provider));
        crate::context_maintenance::EditorSnapshotCaptureInput {
            coding,
            lsp,
            project_root,
        }
    }

    fn project_snapshot_capture_input(
        &self,
    ) -> crate::context_maintenance::ProjectSnapshotCaptureInput {
        let project_root = self.active_project_root_path().or_else(|| {
            self.with_coding_state(|coding| {
                coding
                    .explorer
                    .project_root
                    .as_ref()
                    .map(PathBuf::from)
            })
            .ok()
            .flatten()
        });
        let facts = self.project_snapshot_host_facts(project_root.as_deref());
        crate::context_maintenance::ProjectSnapshotCaptureInput {
            project_root,
            facts,
        }
    }

    /// Host-observed Coding + TerminalProvider facts for ambient RuntimeSnapshot.
    ///
    /// Never re-runs cargo / tests. TerminalProvider owns live session updates;
    /// this only clones observed state for the maintenance worker.
    fn runtime_snapshot_capture_input(
        &self,
    ) -> crate::context_maintenance::RuntimeSnapshotCaptureInput {
        let coding = self.with_coding_state(|coding| coding.clone()).ok();
        let alive_ids: std::collections::HashSet<String> = self
            .container
            .resolve::<Arc<TerminalProvider>>()
            .ok()
            .and_then(|terminal| terminal.list_sessions().ok())
            .map(|sessions| {
                sessions
                    .into_iter()
                    .filter(|session| session.alive)
                    .map(|session| session.id)
                    .collect()
            })
            .unwrap_or_default();

        let (active_session_id, sessions) = match coding.as_ref() {
            Some(coding) => {
                let sessions = coding
                    .terminal_sessions
                    .iter()
                    .map(|session| jaymi_context::RuntimeTerminalSessionFact {
                        id: session.id.clone(),
                        title: session.title.clone(),
                        cwd: session.cwd.clone(),
                        last_command: session.last_command.clone(),
                        output: session.output.clone(),
                        history: session.history.clone(),
                        alive: alive_ids.contains(&session.id),
                    })
                    .collect();
                (coding.active_terminal_id.clone(), sessions)
            }
            None => (None, Vec::new()),
        };

        crate::context_maintenance::RuntimeSnapshotCaptureInput {
            facts: jaymi_context::RuntimeSnapshotHostFacts {
                active_session_id,
                sessions,
            },
        }
    }

    /// Lightweight Project Engine identity for ambient ProjectSnapshot (no FS).
    fn project_snapshot_host_facts(
        &self,
        project_root: Option<&Path>,
    ) -> jaymi_context::ProjectSnapshotHostFacts {
        self.container
            .resolve::<Arc<ProjectEngine>>()
            .ok()
            .and_then(|projects| {
                let open_id = projects.open_project_id()?;
                let project = projects.get(&open_id).ok().flatten()?;
                Some(jaymi_context::ProjectSnapshotHostFacts {
                    project_id: Some(open_id),
                    name: Some(project.name),
                    description: {
                        let trimmed = project.description.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(project.description)
                        }
                    },
                    root_directory: project
                        .root_directory
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .or_else(|| project_root.map(|path| path.display().to_string())),
                    project_type: Some(project.project_type.as_str().to_string()),
                })
            })
            .unwrap_or_else(|| jaymi_context::ProjectSnapshotHostFacts {
                root_directory: project_root.map(|path| path.display().to_string()),
                ..Default::default()
            })
    }

    /// Route a user request through the Planner (Intent → Capability → Context assemble).
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

    /// Resume a ToolRisk-paused plan after an explicit user UI gesture.
    ///
    /// Coding / Git / Terminal / Explorer / LSP rename gestures already express
    /// user intent, so they auto-submit [`ReviewIntent::Approve`] through the
    /// **same** review lifecycle as conversation Review Cards
    /// ([`Self::submit_review`]). Tools never execute directly.
    pub fn complete_user_initiated(
        &self,
        response: PlannerResponse,
    ) -> JaymiResult<PlannerResponse> {
        if !response.awaiting_review {
            if response.blocked {
                return Err(JaymiError::new(response.content));
            }
            return Ok(response);
        }
        let plan_id = response
            .execution_plan
            .as_ref()
            .ok_or_else(|| JaymiError::new("awaiting review without an execution plan"))?
            .id()
            .clone();
        let resumed = self.submit_review(ReviewIntent::Approve { plan_id })?;
        if resumed.blocked {
            return Err(JaymiError::new(resumed.content));
        }
        Ok(resumed)
    }

    /// Single approval implementation for every entry point.
    ///
    /// Lifecycle: ExecutionPlan → Review → Planner → Approved → Execution.
    ///
    /// Conversation Review Cards, Coding Save/Delete/Run, Git Commit, Terminal,
    /// Explorer, and LSP rename all emit [`ReviewIntent`] here. Review UI may
    /// differ (card vs gesture auto-submit); approval semantics never do.
    pub fn submit_review(
        &self,
        intent: jaymi_planner::ReviewIntent,
    ) -> JaymiResult<jaymi_planner::PlannerResponse> {
        let conversation_id = {
            let mut experience = self
                .experience
                .lock()
                .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
            experience.record_review_intent(intent.clone());
            experience.conversation_id().map(str::to_string)
        };
        let planner = self.container.resolve::<Planner>()?;
        let response = planner.resolve_review(intent)?;
        if let Some(entry) = planner.approval_history()?.last().cloned() {
            let _ = self.store_approval_history_memory(&entry, conversation_id.as_deref());
        }
        self.apply_workspace_response(&response)?;
        Ok(response)
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
    ///
    /// **Administrative / transitional** — not the request-context path.
    /// Request handling uses `PlannerResponse.context()` (`ContextBundle`) only.
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
        self.schedule_coding_observation_refresh();
        let _ = self.schedule_project_snapshot_refresh();
        let _ = self.schedule_runtime_snapshot_refresh();
        if let Some(context) = response.project().cloned() {
            return Ok(context);
        }
        // B2.4: ordinary assemble omits heavy ProjectContext; project-session
        // intents still attach it. Fall back to Project Engine if the bundle
        // accessor is empty so the Application open API stays honest.
        self.container
            .resolve::<Arc<ProjectEngine>>()?
            .project_context(Some(project_id))?
            .ok_or_else(|| {
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
        self.schedule_coding_observation_refresh();
        self.context_maintenance
            .publish_project_snapshot(jaymi_context::ProjectSnapshot::empty());
        self.context_maintenance
            .publish_runtime_snapshot(jaymi_context::RuntimeSnapshot::empty());
        let _ = self.with_coding_state(|coding| coding.clear_workspace_activity());
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
    ///
    /// **Administrative / transitional** — not the request-context path.
    /// Request handling exposes project detail only via `PlannerResponse.project()`
    /// on the ContextBundle.
    pub fn project_context(&self, project_id: Option<&str>) -> JaymiResult<Option<ProjectContext>> {
        let projects = self.container.resolve::<Arc<ProjectEngine>>()?;
        projects.project_context(project_id)
    }

    /// Assemble project context for a known project id.
    ///
    /// **Administrative / transitional** — prefer `PlannerResponse.context()` for requests.
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
        let previous = memory.active_conversation_id();
        let next = conversation_id.map(str::to_string);
        if previous == next {
            return Ok(());
        }
        memory.set_active_conversation(conversation_id)?;
        if let Ok(context) = self.container.resolve::<Arc<ContextEngine>>() {
            context.request_fresh_context("conversation_changed");
        }
        Ok(())
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
                review: None,
                execution_summary: None,
                stream_lifecycle: None,
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

    /// Developer-facing Context Inspector for the latest assembled ContextBundle.
    ///
    /// Read-only diagnostics — never re-assembles and never affects execution.
    pub fn inspect_context(&self) -> JaymiResult<Option<ContextInspectorReport>> {
        let context = self.container.resolve::<Arc<ContextEngine>>()?;
        Ok(context.inspect_last())
    }

    /// Recent Context History entries (newest first) for debugging / transparency.
    ///
    /// Read-only — never re-assembles and never affects execution.
    pub fn context_history(&self) -> JaymiResult<Vec<ContextHistoryEntry>> {
        let context = self.container.resolve::<Arc<ContextEngine>>()?;
        Ok(context.history())
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
    ) -> JaymiResult<jaymi_capabilities::CapabilityPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.build_capability_plan(capabilities)
    }

    /// Plan work for one capability and optional goal (does not execute tools).
    pub fn plan_capability(
        &self,
        capability: Capability,
        goal: Option<&str>,
    ) -> JaymiResult<jaymi_capabilities::CapabilityPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.plan_capability(capability, goal)
    }

    /// Compose independent capabilities into one capability plan (no execution).
    pub fn plan_capabilities(
        &self,
        capabilities: &[Capability],
        goal: Option<&str>,
    ) -> JaymiResult<jaymi_capabilities::CapabilityPlan> {
        let planner = self.container.resolve::<Planner>()?;
        planner.plan_capabilities(capabilities, goal)
    }

    /// Compose from a [`jaymi_capabilities::CapabilityComposition`] value.
    pub fn compose_capability_plan(
        &self,
        composition: &jaymi_capabilities::CapabilityComposition,
    ) -> JaymiResult<jaymi_capabilities::CapabilityPlan> {
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
        let conversation_id = experience.conversation_id().map(str::to_string);
        let coding_open = experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
        drop(experience);
        if let Some(summary) = response
            .execution_summary
            .as_ref()
            .filter(|summary| summary.should_surface_in_conversation())
        {
            let _ = self.store_execution_summary_memory(summary, conversation_id.as_deref());
        }
        if coding_open {
            let _ = self.refresh_coding_explorer();
        }
        Ok(())
    }

    /// Persist an Execution Summary for future Memory retrieval.
    fn store_execution_summary_memory(
        &self,
        summary: &jaymi_planner::ExecutionSummary,
        conversation_id: Option<&str>,
    ) -> JaymiResult<MemoryRecord> {
        let project_id = self.active_project_id();
        let scope = if conversation_id.is_some() {
            jaymi_memory::MemoryScope::Conversation
        } else {
            jaymi_memory::MemoryScope::Working
        };
        self.store_memory(&StoreMemoryRequest {
            scope,
            summary: summary.memory_summary_line(),
            content: summary.memory_content(),
            conversation_id: conversation_id.map(str::to_string),
            project_id,
            importance: Some(60),
            confidence: Some(90),
            tags: vec![
                "execution_summary".into(),
                summary.status.as_str().into(),
            ],
            source: Some("planner".into()),
            kind: Some("execution_summary".into()),
            metadata_json: Some(summary.memory_metadata_json()),
        })
    }

    /// Persist an Approval History entry for transparency and future retrieval.
    ///
    /// Stored as `kind = approval_history` with Private sensitivity metadata.
    /// Callers that surface history must use [`Self::search_approval_history`]
    /// so Restricted access redacts reasons and resource paths.
    fn store_approval_history_memory(
        &self,
        entry: &jaymi_planner::ApprovalHistoryEntry,
        conversation_id: Option<&str>,
    ) -> JaymiResult<MemoryRecord> {
        let project_id = self.active_project_id();
        let mut entry = entry.clone();
        if entry.conversation_id.is_none() {
            entry.conversation_id = conversation_id.map(str::to_string);
        }
        if entry.project_id.is_none() {
            entry.project_id = project_id.clone();
        }
        let scope = if entry.conversation_id.is_some() {
            jaymi_memory::MemoryScope::Conversation
        } else {
            jaymi_memory::MemoryScope::Working
        };
        self.store_memory(&StoreMemoryRequest {
            scope,
            summary: entry.memory_summary_line(),
            content: entry.memory_content(),
            conversation_id: entry.conversation_id.clone(),
            project_id: entry.project_id.clone(),
            importance: Some(70),
            confidence: Some(95),
            tags: vec![
                "approval_history".into(),
                entry.decision.as_str().into(),
            ],
            source: Some("planner".into()),
            kind: Some("approval_history".into()),
            metadata_json: Some(entry.memory_metadata_json()),
        })
    }

    /// Search Approval History (in-session Planner store + durable Memory).
    ///
    /// `access` controls whether reasons, goals, and resource paths are visible.
    /// Use [`ApprovalHistoryAccess::Full`] for local user UI / diagnostics and
    /// [`ApprovalHistoryAccess::Restricted`] for Context / Planner exports.
    pub fn search_approval_history(
        &self,
        query: &jaymi_planner::ApprovalHistoryQuery,
        access: jaymi_planner::ApprovalHistoryAccess,
    ) -> JaymiResult<Vec<jaymi_planner::ApprovalHistoryView>> {
        let planner = self.container.resolve::<Planner>()?;
        let mut entries = planner.search_approval_history(query)?;

        let conversation_id = query.conversation_id.clone().or_else(|| {
            self.experience()
                .ok()
                .and_then(|session| session.conversation_id().map(str::to_string))
        });
        let project_id = query.project_id.clone().or_else(|| self.active_project_id());

        let memory_query = MemoryQuery {
            text: query.text.clone(),
            kind: Some("approval_history".into()),
            conversation_id: conversation_id.clone(),
            project_id: project_id.clone(),
            limit: query.limit.or(Some(100)),
            ..MemoryQuery::default()
        };
        if let Ok(records) = self.retrieve_memory(&memory_query) {
            for record in records {
                if let Some(entry) = jaymi_planner::ApprovalHistoryEntry::from_memory_record(
                    &record.summary,
                    &record.content,
                    &record.metadata_json,
                    record.conversation_id.clone(),
                    record.project_id.clone(),
                    record.created_at,
                ) {
                    if entry.matches(query)
                        && !entries.iter().any(|existing| {
                            existing.plan_id == entry.plan_id
                                && existing.decision == entry.decision
                                && existing.timestamp == entry.timestamp
                        })
                    {
                        entries.push(entry);
                    }
                }
            }
        }

        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        Ok(entries
            .into_iter()
            .map(|entry| entry.view_for(access))
            .collect())
    }

    /// Record a Review Card intent and resolve the paused Execution Plan.
    ///
    /// Delegates to [`Self::submit_review`] — the single approval implementation.
    /// Conversation cards, Coding gestures, Git, Terminal, Explorer, and LSP
    /// rename all share that path.
    pub fn communicate_review_intent(
        &self,
        intent: jaymi_planner::ReviewIntent,
    ) -> JaymiResult<jaymi_planner::PlannerResponse> {
        self.submit_review(intent)
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

        // Seed explorer.project_root before ensuring a PTY so the terminal is
        // not stuck as a UI-only tab with no working directory.
        if let Some(root) = self.active_project_root_path() {
            let root_display = root.to_string_lossy().into_owned();
            self.with_coding_state(|coding| {
                if coding.explorer.project_root.as_deref() != Some(root_display.as_str()) {
                    coding.explorer.project_root = Some(root_display);
                }
            })?;
        }

        // Soft-fail: Coding must open even when PTY is unavailable (restricted
        // environments). First Run / Create will surface the error.
        if let Err(error) = self.ensure_coding_terminal() {
            jaymi_logging::warn(
                "application",
                format!("coding terminal ensure deferred: {error}"),
            );
            let _ = self.with_coding_state(|coding| {
                if coding.terminal_sessions.is_empty() {
                    let cwd = coding.explorer.project_root.clone();
                    coding.push_terminal_session(
                        jaymi_capabilities::TerminalSessionState::new(
                            DEFAULT_TERMINAL_SESSION_ID,
                            cwd,
                        ),
                    );
                }
                if coding.active_terminal_id.is_none() {
                    coding.active_terminal_id = Some(DEFAULT_TERMINAL_SESSION_ID.to_string());
                }
            });
        }

        // Slow refreshes (explorer / git / problems / file summaries) run in
        // background maintenance so opening Coding never blocks conversation.
        self.schedule_coding_context_maintenance();
        // Keep a synchronous explorer seed when the panel is empty so first paint
        // is not blank; background inventory will replace it when complete.
        let explorer_empty = self.with_coding_state(|coding| coding.explorer.nodes.is_empty())?;
        if explorer_empty {
            let _ = self.refresh_coding_explorer_now();
        }

        let editors_empty = self.with_coding_state(|coding| coding.editors.is_empty())?;
        if editors_empty {
            let _ = self.restore_coding_editor_workspace();
            // Open tabs may have changed — refresh file summaries without blocking.
            let _ = self.schedule_context_maintenance(MaintenanceKind::FileSummaries);
        }
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

    /// Refresh Project Explorer via background inventory maintenance (non-blocking).
    pub fn refresh_coding_explorer(&self) -> JaymiResult<()> {
        let root = self.active_project_id().and_then(|id| {
            self.container
                .resolve::<Arc<ProjectEngine>>()
                .ok()
                .and_then(|projects| projects.get(&id).ok().flatten())
                .and_then(|project| project.root_directory)
        });

        if root.is_none() {
            return self.with_coding_state(|coding| {
                coding.explorer.clear_no_project();
            });
        }

        let _ = self.schedule_context_maintenance(MaintenanceKind::WorkspaceInventory);
        Ok(())
    }

    /// Synchronously refresh Project Explorer through Planner → Tool → Provider.
    pub fn refresh_coding_explorer_now(&self) -> JaymiResult<()> {
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
                self.complete_user_initiated(self.write_file(&path, "")?)?;
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
                self.complete_user_initiated(self.manage_mkdir(&path)?)?;
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
                self.complete_user_initiated(self.manage_rename(&from, &to)?)?;
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
        self.complete_user_initiated(self.manage_delete(path)?)?;
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
            self.schedule_coding_observation_refresh();
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
        self.schedule_coding_observation_refresh();
        Ok(())
    }

    /// Open a file as a VS Code-style preview tab through Planner → read_file.
    ///
    /// Reopening an already-open path focuses that session. A new preview replaces
    /// any existing preview tab.
    pub fn open_coding_file_preview(&self, path: &str) -> JaymiResult<()> {
        let focused = self.with_coding_state(|coding| coding.focus_tab(path))?;
        if focused {
            self.schedule_coding_observation_refresh();
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
        self.schedule_coding_observation_refresh();
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
        self.schedule_coding_observation_refresh();
        Ok(())
    }

    /// Close an editor tab.
    pub fn close_coding_tab(&self, path: &str) -> JaymiResult<()> {
        let closed = self.with_coding_state(|coding| coding.close_tab(path))?;
        if !closed {
            return Err(JaymiError::new(format!("no open tab for {path}")));
        }
        self.schedule_coding_observation_refresh();
        Ok(())
    }

    /// Update editor buffer content for a tab.
    pub fn set_coding_tab_content(&self, path: &str, content: String) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.set_tab_content(path, content.clone());
        })?;
        let _ = self.coding_lsp_did_change(path, &content);
        self.schedule_coding_observation_refresh();
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
        })?;
        self.schedule_coding_observation_refresh();
        Ok(())
    }

    /// Persist text selection for a tab (Monaco IPC → CodingState → ambient snapshots).
    pub fn set_coding_tab_selection(
        &self,
        path: &str,
        selection: EditorSelection,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.set_selection(path, selection);
        })?;
        self.schedule_coding_observation_refresh();
        Ok(())
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
            // Clear any prior span so Environmental Resolution does not bind
            // stale selected text after a jump-to-match.
            self.set_coding_tab_selection(path, EditorSelection::caret(line, column))?;
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
                self.complete_user_initiated(self.write_file(&path, new_text)?)?;
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
        self.schedule_coding_observation_refresh();
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
        self.schedule_coding_observation_refresh();
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
        self.schedule_coding_observation_refresh();
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
        })?;
        self.schedule_coding_observation_refresh();
        Ok(())
    }

    /// Persist text selection for a tab in a specific pane.
    pub fn set_coding_tab_selection_in_pane(
        &self,
        pane_id: &str,
        path: &str,
        selection: EditorSelection,
    ) -> JaymiResult<()> {
        self.with_coding_state(|coding| {
            coding.editors.set_selection_in_pane(
                &EditorPaneId(pane_id.to_string()),
                path,
                selection,
            );
        })?;
        self.schedule_coding_observation_refresh();
        Ok(())
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
        self.schedule_coding_observation_refresh();
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
        self.schedule_coding_observation_refresh();
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

        self.complete_user_initiated(self.write_file(path, content.clone())?)?;
        self.with_coding_state(|coding| {
            coding.mark_tab_clean(path);
        })?;
        let _ = self.coding_lsp_did_change(path, &content);
        self.schedule_coding_observation_refresh();
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
    ///
    /// Uses the same review lifecycle as Save / Git / Terminal: Planner pause
    /// then [`Self::complete_user_initiated`] auto-submits
    /// [`ReviewIntent::Approve`]. Tools never run outside an Approved plan.
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
        self.complete_user_initiated(self.lsp(request)?)
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
        self.schedule_coding_observation_refresh();
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
        let cwd = self.coding_terminal_cwd(None)?;
        let Some(cwd) = cwd else {
            return self.with_coding_state(|coding| {
                if coding.terminal_sessions.is_empty() {
                    coding.push_terminal_session(jaymi_capabilities::TerminalSessionState::new(
                        DEFAULT_TERMINAL_SESSION_ID,
                        None,
                    ));
                }
                if coding.active_terminal_id.is_none() {
                    coding.active_terminal_id = Some(DEFAULT_TERMINAL_SESSION_ID.to_string());
                }
            });
        };

        let response = self.complete_user_initiated(
            self.ensure_terminal(DEFAULT_TERMINAL_SESSION_ID, &cwd)?,
        )?;
        self.apply_terminal_response(&response)?;
        self.with_coding_state(|coding| {
            if coding.active_terminal_id.is_none() {
                coding.active_terminal_id = Some(DEFAULT_TERMINAL_SESSION_ID.to_string());
            }
        })?;
        Ok(())
    }

    /// Resolve a terminal working directory: session cwd → explorer root →
    /// active project root.
    fn coding_terminal_cwd(&self, session_id: Option<&str>) -> JaymiResult<Option<PathBuf>> {
        let from_coding = self.with_coding_state(|coding| {
            if let Some(session_id) = session_id {
                if let Some(cwd) = coding
                    .terminal_sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .and_then(|session| session.cwd.clone())
                {
                    return Some(PathBuf::from(cwd));
                }
            }
            coding
                .explorer
                .project_root
                .as_ref()
                .map(PathBuf::from)
        })?;
        Ok(from_coding.or_else(|| self.active_project_root_path()))
    }

    /// Spawn a new terminal tab in the Coding Workspace (cwd = project root)
    /// and make it the active tab.
    pub fn create_coding_terminal(&self, title: Option<String>) -> JaymiResult<()> {
        let cwd = self.coding_terminal_cwd(None)?.ok_or_else(|| {
            JaymiError::new("cannot create terminal — open a project first")
        })?;

        let response = self.complete_user_initiated(self.create_terminal(&cwd, title)?)?;
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
        let cwd = self.coding_terminal_cwd(Some(session_id))?.ok_or_else(|| {
            JaymiError::new("coding terminal has no working directory — open a project first")
        })?;

        let response =
            self.complete_user_initiated(self.rename_terminal(session_id, &cwd, title)?)?;
        self.apply_terminal_response(&response)?;
        Ok(())
    }

    /// Kill a Coding Workspace terminal tab and pick a new active tab.
    pub fn kill_coding_terminal(&self, session_id: &str) -> JaymiResult<()> {
        let cwd = self.coding_terminal_cwd(Some(session_id))?;
        if let Some(cwd) = cwd {
            // Best-effort PTY teardown — still remove the UI tab if the tool fails
            // (e.g. phantom UI-only session that never opened a PTY).
            if let Ok(response) = self.kill_terminal(session_id, &cwd) {
                let _ = self.complete_user_initiated(response);
            }
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
        let cwd = self.coding_terminal_cwd(Some(session_id))?.ok_or_else(|| {
            JaymiError::new("coding terminal has no working directory — open a project first")
        })?;

        // Ensure the dock is visible when the user runs a command.
        let _ = self.with_coding_state(|coding| {
            coding.show_bottom_tab(CodingBottomTab::Terminal);
            coding.active_terminal_id = Some(session_id.to_string());
        });

        let response =
            self.complete_user_initiated(self.run_terminal(session_id, &cwd, command)?)?;
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
            self.with_coding_state(|coding| {
                coding.remove_terminal_session(&session_id);
            })?;
            let _ = self.schedule_runtime_snapshot_refresh();
            return Ok(());
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
            // Workspace Memory: remember build / check / test outcomes.
            if let Some(cmd) = session.last_command.clone() {
                let lower = cmd.to_ascii_lowercase();
                let is_buildish = lower.contains("cargo check")
                    || lower.contains("cargo build")
                    || lower.contains("cargo test")
                    || lower.contains("npm test")
                    || lower.contains("npm run build")
                    || lower.contains("pnpm test")
                    || lower.contains("yarn test");
                if is_buildish {
                    let scroll = session.output.to_ascii_lowercase();
                    let ok = !(scroll.contains("error:")
                        || scroll.contains("error[")
                        || scroll.contains("failed")
                        || scroll.contains("panicked"));
                    let summary = if ok {
                        "ok".to_string()
                    } else {
                        "failed".to_string()
                    };
                    coding.record_workspace_build(&cmd, &summary, ok);
                }
            }
        })?;
        // Workspace/editor observation + ambient runtime intelligence. Terminal
        // Provider owns the PTY; Application only observes — never blocks chat.
        self.schedule_coding_observation_refresh();
        let _ = self.schedule_runtime_snapshot_refresh();
        Ok(())
    }

    /// Refresh Coding Workspace Git status through background maintenance.
    ///
    /// Non-blocking: schedules a read-only Git status job. Mutating Git ops still
    /// go Planner → Tool → Provider. Conversation consumes the latest completed
    /// snapshot via ContextEngine — never waits here.
    pub fn refresh_coding_git(&self) -> JaymiResult<()> {
        let _ = self.schedule_context_maintenance(MaintenanceKind::GitStatus);
        Ok(())
    }

    /// Synchronously refresh Git status (tests / rare explicit sync paths).
    pub fn refresh_coding_git_now(&self) -> JaymiResult<()> {
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
        let response = self.complete_user_initiated(response)?;
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
        let section = {
            let summary = response.git_summary.clone().unwrap_or_else(|| {
                if is_repository {
                    "clean".into()
                } else {
                    "not a git repository".into()
                }
            });
            let mut sample_paths = Vec::new();
            for path in response
                .git_modified
                .iter()
                .chain(response.git_staged.iter())
                .chain(response.git_untracked.iter())
                .map(|item| item.path.clone())
            {
                if sample_paths.len() >= 8 {
                    break;
                }
                if !sample_paths.contains(&path) {
                    sample_paths.push(path);
                }
            }
            jaymi_context::GitStatusSection {
                is_repository,
                branch: response.git_branch.clone(),
                summary: summary.clone(),
                modified_count: response.git_modified.len(),
                staged_count: response.git_staged.len(),
                untracked_count: response.git_untracked.len(),
                conflict_count: 0,
                dirty_paths: response
                    .git_modified
                    .iter()
                    .map(|item| item.path.clone())
                    .take(16)
                    .collect(),
                staged_paths: response
                    .git_staged
                    .iter()
                    .map(|item| item.path.clone())
                    .take(16)
                    .collect(),
                untracked_paths: response
                    .git_untracked
                    .iter()
                    .map(|item| item.path.clone())
                    .take(16)
                    .collect(),
                sample_paths,
                ..jaymi_context::GitStatusSection::default()
            }
        };
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
        })?;
        self.context_maintenance.publish_git_section(section);
        // Ambient GitSnapshot refresh fills HEAD / conflicts / recent commits.
        let _ = self.schedule_context_maintenance(MaintenanceKind::GitStatus);
        self.schedule_coding_observation_refresh();
        Ok(())
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
        // Clear ambient Coding observation so conversation does not keep a stale snapshot.
        self.context_maintenance
            .publish_workspace_snapshot(jaymi_context::WorkspaceSnapshot::empty());
        self.context_maintenance
            .publish_editor_snapshot(jaymi_context::EditorSnapshot::empty());
        self.context_maintenance
            .publish_project_snapshot(jaymi_context::ProjectSnapshot::empty());
        self.context_maintenance
            .publish_git_snapshot(jaymi_context::GitSnapshot::empty());
        self.context_maintenance
            .publish_runtime_snapshot(jaymi_context::RuntimeSnapshot::empty());
        let _ = self.with_coding_state(|coding| coding.clear_workspace_activity());
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

    /// Snapshot Experience turns for ReasoningRequest.history (request-scoped).
    fn collect_reasoning_history(
        &self,
        exclude_goal: Option<&str>,
    ) -> JaymiResult<Vec<jaymi_reasoning::ConversationTurn>> {
        let experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        Ok(experience.to_reasoning_history(exclude_goal))
    }

    /// Handle a user request and apply any capability workspace expansion.
    pub fn handle_with_workspace(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        let content = request.content.clone();
        // Snapshot prior turns before recording the current user message.
        let history = self.prepare_conversational_host()?;
        if !content.trim().is_empty() {
            self.record_user_message(content)?;
        }
        let planner = self.container.resolve::<Planner>()?;
        let started = std::time::Instant::now();
        // Conversational path consumes history; tool-backed intents fall through
        // to Planner::handle inside handle_conversational_with_observer.
        let response =
            planner.handle_conversational_with_observer(request, history, |_| {})?;
        self.record_planner_activity(&response, started.elapsed().as_millis() as u64);
        self.apply_workspace_response(&response)?;
        Ok(response)
    }

    /// Handle a request with incremental conversation streaming when conversational.
    ///
    /// Blocking delivery with live Experience updates (one-shot collect). Prefer
    /// [`Self::begin_generation`] / [`Self::pump_generation`] for the interactive UI.
    /// Tool / plan / review paths behave like [`Self::handle_with_workspace`].
    pub fn handle_streaming_with_workspace(
        &self,
        request: UserRequest,
    ) -> JaymiResult<PlannerResponse> {
        let content = request.content.clone();
        let history = self.prepare_conversational_host()?;
        if !content.trim().is_empty() {
            self.record_user_message(content)?;
        }
        let planner = self.container.resolve::<Planner>()?;
        let started = std::time::Instant::now();
        let turn_slot = std::sync::Mutex::new(None::<usize>);
        let response = planner.handle_conversational_with_observer(request, history, |event| {
            let Ok(mut experience) = self.experience.lock() else {
                return;
            };
            // Mirror Planner only — Experience never invents ConversationState.
            experience.mirror_conversation_state(planner.conversation_state());
            let Ok(mut slot) = turn_slot.lock() else {
                return;
            };
            if slot.is_none() {
                *slot = Some(experience.begin_streaming_assistant());
            }
            if let Some(index) = *slot {
                let _ = experience.apply_stream_event(index, &event);
            }
        })?;
        self.record_planner_activity(&response, started.elapsed().as_millis() as u64);
        let had_stream = turn_slot
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .is_some();
        if response.reasoning_used && had_stream {
            self.apply_workspace_expansion_only(&response)?;
            let mut experience = self
                .experience
                .lock()
                .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
            experience.mirror_conversation_state(response.conversation_state);
        } else {
            self.apply_workspace_response(&response)?;
        }
        Ok(response)
    }

    /// True when a pumpable conversational generation is in flight.
    pub fn generation_active(&self) -> bool {
        self.active_generation
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Shared Application host prep for both conversational delivery modes.
    ///
    /// Pushes Context session inputs then snapshots Experience history. User
    /// message recording is mode-specific: blocking records before Planner
    /// entry; pumpable records on UI-thread ack (before background start).
    fn prepare_conversational_host(
        &self,
    ) -> JaymiResult<Vec<jaymi_reasoning::ConversationTurn>> {
        self.prepare_conversational_host_excluding(None)
    }

    /// Like [`Self::prepare_conversational_host`], excluding the current goal
    /// from history when it was already recorded into Experience.
    fn prepare_conversational_host_excluding(
        &self,
        exclude_goal: Option<&str>,
    ) -> JaymiResult<Vec<jaymi_reasoning::ConversationTurn>> {
        // Same host prep as Application::handle — conversational reasoning must
        // not assemble with a stale or empty Context session.
        self.prepare_context_session()?;
        self.collect_reasoning_history(exclude_goal)
    }

    /// Begin a conversational generation without blocking the UI thread.
    ///
    /// On the calling thread (UI): records the user turn, Planner-acks Thinking
    /// (`PreparingContext → Reasoning`), opens an empty assistant turn, and
    /// returns [`BeginGeneration::Started`]. Host prep, Context assemble, prompt
    /// build, and provider stream-open run on a background task; drive
    /// [`Self::pump_generation`] each frame.
    ///
    /// Soft-fail / tool-backed completions surface as [`PumpGeneration::Finished`]
    /// after the worker finishes — never as a blocking return from this method.
    pub fn begin_generation(
        self: &Arc<Self>,
        content: impl Into<String>,
    ) -> JaymiResult<BeginGeneration> {
        self.begin_user_request(UserRequest::new(content))
    }

    /// Submit a typed Coding Action as a normal conversation turn (Sprint C0.1).
    ///
    /// UI emits the action only; Application binds Workspace Intelligence
    /// (selection / file / run hint) into the [`UserRequest`], then the Planner
    /// owns routing. No direct editor, terminal, or provider bypass.
    pub fn begin_coding_action(
        self: &Arc<Self>,
        action: CodingAction,
    ) -> JaymiResult<BeginGeneration> {
        let request = self.build_coding_action_request(action)?;
        self.begin_user_request(request)
    }

    /// Resolve Explain to selection vs file from CodingState, then submit.
    pub fn begin_explain_coding_action(self: &Arc<Self>) -> JaymiResult<BeginGeneration> {
        let action = self.with_coding_state(|coding| {
            let has_selection = coding
                .editors
                .active_session()
                .map(|session| {
                    let selection = &session.view.selection;
                    !selection.is_empty()
                        && selection
                            .text
                            .as_ref()
                            .map(|text| !text.trim().is_empty())
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if has_selection {
                CodingAction::ExplainSelection
            } else {
                CodingAction::ExplainFile
            }
        })?;
        self.begin_coding_action(action)
    }

    /// Build a Planner [`UserRequest`] for a Coding Action (WI-enriched).
    pub fn build_coding_action_request(&self, action: CodingAction) -> JaymiResult<UserRequest> {
        let mut request = UserRequest::coding_action(action);
        match action {
            CodingAction::SearchWorkspace => {
                if let Ok(Some(query)) = self.with_coding_state(|coding| {
                    coding.editors.active_session().and_then(|session| {
                        session
                            .view
                            .selection
                            .text
                            .as_ref()
                            .map(|text| text.trim().to_string())
                            .filter(|text| !text.is_empty())
                    })
                }) {
                    let preview: String = query.chars().take(80).collect();
                    request.content = format!("Search the workspace for: {preview}");
                    request.search = Some(SearchRequest::free_text(query));
                }
            }
            CodingAction::RunProject => {
                if let Some((session_id, cwd, command)) = self.suggest_project_run()? {
                    request.content = format!("Run the project (`{command}`).");
                    request.terminal = Some(TerminalRequest {
                        operation: TerminalOperation::Run,
                        session_id,
                        cwd,
                        command: Some(command),
                        title: None,
                    });
                }
            }
            CodingAction::ExplainSelection
            | CodingAction::ExplainFile
            | CodingAction::EditSelection
            | CodingAction::RefactorSelection
            | CodingAction::OpenCodingActions => {}
        }
        Ok(request)
    }

    /// Suggest a reviewed run command from the open project (observation only).
    fn suggest_project_run(&self) -> JaymiResult<Option<(String, PathBuf, String)>> {
        let cwd = match self.coding_terminal_cwd(None)? {
            Some(cwd) => cwd,
            None => return Ok(None),
        };
        let command = if cwd.join("Cargo.toml").is_file() {
            "cargo test".to_string()
        } else if cwd.join("package.json").is_file() {
            "npm test".to_string()
        } else if cwd.join("pyproject.toml").is_file() || cwd.join("pytest.ini").is_file() {
            "pytest".to_string()
        } else if cwd.join("go.mod").is_file() {
            "go test ./...".to_string()
        } else {
            return Ok(None);
        };
        let session_id = self
            .with_coding_state(|coding| {
                coding
                    .active_terminal_id
                    .clone()
                    .or_else(|| coding.terminal_sessions.first().map(|s| s.id.clone()))
            })?
            .unwrap_or_else(|| DEFAULT_TERMINAL_SESSION_ID.to_string());
        Ok(Some((session_id, cwd, command)))
    }

    /// Begin generation from a fully formed [`UserRequest`] (Conversation First).
    pub fn begin_user_request(
        self: &Arc<Self>,
        request: UserRequest,
    ) -> JaymiResult<BeginGeneration> {
        if self.generation_active() {
            return Err(JaymiError::new(
                "a generation is already in progress — cancel it first",
            ));
        }
        let trimmed = request.content.trim();
        if trimmed.is_empty() {
            return Err(JaymiError::new("empty prompt"));
        }
        let request_started = std::time::Instant::now();
        let planner = self.container.resolve::<Planner>()?;

        // --- UI thread: acknowledge send + Thinking, return to event loop ---
        self.record_user_message(trimmed)?;
        planner.acknowledge_conversational_send();
        let turn_index = {
            let mut experience = self
                .experience
                .lock()
                .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
            experience.mirror_conversation_state(planner.conversation_state());
            experience.begin_streaming_assistant()
        };

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut guard = self
                .active_generation
                .lock()
                .map_err(|_| JaymiError::new("generation lock poisoned"))?;
            *guard = Some(GenerationSlot::Starting(PendingGeneration {
                rx,
                cancel: Arc::clone(&cancel),
                turn_index,
                request_started,
                user_text: trimmed.to_string(),
            }));
        }

        let app = Arc::clone(self);
        let worker_request = request;
        thread::spawn(move || {
            let outcome = app.run_generation_start(worker_request, &cancel);
            let _ = tx.send(outcome);
        });

        Ok(BeginGeneration::Started)
    }

    /// Background: prepare session, assemble, open stream (or soft/tool fallback).
    fn run_generation_start(
        &self,
        request: UserRequest,
        cancel: &AtomicBool,
    ) -> GenerationStartOutcome {
        if cancel.load(Ordering::Relaxed) {
            return GenerationStartOutcome::Cancelled;
        }
        let content = request.content.clone();
        // Soft-update Workspace Memory coding objective before prepare/assemble.
        let _ = self.with_coding_state(|coding| {
            let lower = content.to_ascii_lowercase();
            let codingish = lower.contains("compile")
                || lower.contains("error")
                || lower.contains("fix")
                || lower.contains("refactor")
                || lower.contains("implement")
                || lower.contains("build")
                || lower.contains("test ")
                || lower.contains("bug")
                || request.coding_action.is_some()
                || coding.workspace_activity.has_activity();
            if codingish {
                let objective = content.chars().take(160).collect::<String>();
                coding.set_coding_objective(Some(objective));
            }
        });
        let history = match self.prepare_conversational_host_excluding(Some(content.as_str())) {
            Ok(history) => history,
            Err(error) => {
                if let Ok(planner) = self.container.resolve::<Planner>() {
                    planner.transition_conversation(jaymi_planner::ConversationState::Failed);
                }
                return GenerationStartOutcome::Failed(error.message().to_string());
            }
        };
        if cancel.load(Ordering::Relaxed) {
            return GenerationStartOutcome::Cancelled;
        }
        let planner = match self.container.resolve::<Planner>() {
            Ok(planner) => planner,
            Err(error) => return GenerationStartOutcome::Failed(error.message().to_string()),
        };
        match planner.start_conversation_stream(&request, history) {
            Ok((context, stream, prompt_diagnostics, mut early_pipeline)) => {
                if cancel.load(Ordering::Relaxed) {
                    return GenerationStartOutcome::Cancelled;
                }
                early_pipeline.stages.insert(
                    0,
                    jaymi_reasoning::PipelineStageTiming::new("request_received", 0),
                );
                GenerationStartOutcome::Ready {
                    stream,
                    context,
                    prompt_diagnostics,
                    early_pipeline,
                }
            }
            Err(error) => {
                if cancel.load(Ordering::Relaxed) {
                    return GenerationStartOutcome::Cancelled;
                }
                // Tool-backed or soft conversational — user turn already recorded;
                // empty assistant turn is finalized when pump installs Completed.
                match self.soft_or_tool_generation_fallback(request) {
                    Ok(response) => GenerationStartOutcome::Completed(response),
                    Err(fallback_error) => {
                        planner.transition_conversation(jaymi_planner::ConversationState::Failed);
                        GenerationStartOutcome::Failed(format!(
                            "{}; fallback: {}",
                            error.message(),
                            fallback_error.message()
                        ))
                    }
                }
            }
        }
    }

    /// Soft-fail / tool path after UI ack (no second user-message record).
    fn soft_or_tool_generation_fallback(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        let history = self.prepare_conversational_host_excluding(Some(request.content.as_str()))?;
        let planner = self.container.resolve::<Planner>()?;
        let started = std::time::Instant::now();
        // Empty observer: Experience already has the Thinking assistant turn.
        let response =
            planner.handle_conversational_with_observer(request, history, |_| {})?;
        self.record_planner_activity(&response, started.elapsed().as_millis() as u64);
        Ok(response)
    }

    /// Pump pending start outcomes and/or active stream events onto Experience.
    ///
    /// Non-blocking: pending start uses `try_recv`; active streams use
    /// [`ConversationStream::try_pump`] so the UI never waits on provider I/O,
    /// diagnostics, metrics, or the final response object.
    pub fn pump_generation(&self, max_events: usize) -> JaymiResult<PumpGeneration> {
        let max_events = max_events.max(1);
        let mut guard = self
            .active_generation
            .lock()
            .map_err(|_| JaymiError::new("generation lock poisoned"))?;

        // Promote Starting → Active / Finished without holding work on the UI thread.
        let promote = if let Some(GenerationSlot::Starting(pending)) = guard.as_ref() {
            match pending.rx.try_recv() {
                Ok(outcome) => Some(Ok(outcome)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(JaymiError::new(
                    "generation start worker disconnected",
                ))),
            }
        } else {
            None
        };

        if let Some(Err(error)) = promote {
            let turn_index = match guard.as_ref() {
                Some(GenerationSlot::Starting(pending)) => pending.turn_index,
                _ => 0,
            };
            *guard = None;
            drop(guard);
            self.install_failed_generation(turn_index, error.message())?;
            return Err(error);
        }

        if let Some(Ok(outcome)) = promote {
            match outcome {
                GenerationStartOutcome::Ready {
                    stream,
                    context,
                    prompt_diagnostics,
                    early_pipeline,
                } => {
                    let (turn_index, request_started, user_text) = match guard.as_ref() {
                        Some(GenerationSlot::Starting(pending)) => (
                            pending.turn_index,
                            pending.request_started,
                            pending.user_text.clone(),
                        ),
                        _ => unreachable!("promote only from Starting"),
                    };
                    *guard = Some(GenerationSlot::Active(ActiveGeneration {
                        stream,
                        context,
                        turn_index,
                        prompt_diagnostics,
                        request_started,
                        early_pipeline,
                        user_text,
                    }));
                }
                GenerationStartOutcome::Completed(response) => {
                    let turn_index = match guard.as_ref() {
                        Some(GenerationSlot::Starting(pending)) => pending.turn_index,
                        _ => unreachable!("promote only from Starting"),
                    };
                    *guard = None;
                    drop(guard);
                    return self.install_completed_generation(turn_index, response);
                }
                GenerationStartOutcome::Cancelled => {
                    *guard = None;
                    return Ok(PumpGeneration::Idle);
                }
                GenerationStartOutcome::Failed(message) => {
                    let turn_index = match guard.as_ref() {
                        Some(GenerationSlot::Starting(pending)) => pending.turn_index,
                        _ => unreachable!("promote only from Starting"),
                    };
                    *guard = None;
                    drop(guard);
                    self.install_failed_generation(turn_index, &message)?;
                    return Err(JaymiError::new(message));
                }
            }
        }

        let Some(GenerationSlot::Active(active)) = guard.as_mut() else {
            // Still Starting (Empty) or cleared.
            if guard.as_ref().is_some() {
                return Ok(PumpGeneration::Active { events: 0 });
            }
            return Ok(PumpGeneration::Idle);
        };

        let planner = self.container.resolve::<Planner>()?;
        let mut applied = 0usize;
        let mut terminal: Option<ConversationStreamEvent> = None;
        for _ in 0..max_events {
            match active.stream.try_pump() {
                Ok(jaymi_reasoning::StreamPumpPoll::Pending) => break,
                Ok(jaymi_reasoning::StreamPumpPoll::Idle) => break,
                Ok(jaymi_reasoning::StreamPumpPoll::Event(event)) => {
                    let is_terminal = event.is_terminal();
                    match &event {
                        ConversationStreamEvent::Lifecycle(lifecycle) => {
                            planner.mirror_stream_lifecycle(*lifecycle);
                        }
                        ConversationStreamEvent::Token(_) => {
                            planner.mirror_stream_lifecycle(
                                jaymi_reasoning::StreamingLifecycle::Streaming,
                            );
                        }
                        _ => {}
                    }
                    {
                        let mut experience = self
                            .experience
                            .lock()
                            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
                        experience.mirror_conversation_state(planner.conversation_state());
                        let _ = experience.apply_stream_event(active.turn_index, &event);
                    }
                    applied += 1;
                    if is_terminal {
                        terminal = Some(event);
                        break;
                    }
                }
                Err(error) => {
                    let failed = ConversationStreamEvent::Failed {
                        partial: active.stream.accumulated_text().to_string(),
                        error,
                        metrics: active.stream.diagnostics().into_metrics(),
                    };
                    {
                        let mut experience = self
                            .experience
                            .lock()
                            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
                        let _ = experience.apply_stream_event(active.turn_index, &failed);
                    }
                    terminal = Some(failed);
                    break;
                }
            }
        }
        if let Some(event) = terminal {
            let context = active.context.clone();
            let prompt_diagnostics = Some(active.prompt_diagnostics.clone());
            let mut early_pipeline = active.early_pipeline.clone();
            let total_ms = active.request_started.elapsed().as_millis() as u64;
            early_pipeline.total_ms = Some(total_ms);
            *guard = None;
            drop(guard);
            let response = planner.complete_conversation_stream(
                context,
                event,
                prompt_diagnostics,
                Some(early_pipeline),
            )?;
            self.apply_workspace_expansion_only(&response)?;
            {
                let mut experience = self
                    .experience
                    .lock()
                    .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
                experience.mirror_conversation_state(response.conversation_state);
            }
            self.record_planner_activity(
                &response,
                response
                    .reasoning_metrics
                    .as_ref()
                    .map(|m| m.latency_ms)
                    .unwrap_or(0),
            );
            Ok(PumpGeneration::Finished(response))
        } else {
            Ok(PumpGeneration::Active { events: applied })
        }
    }

    /// Install a background soft/tool completion onto the pre-acked assistant turn.
    fn install_completed_generation(
        &self,
        turn_index: usize,
        response: PlannerResponse,
    ) -> JaymiResult<PumpGeneration> {
        {
            let mut experience = self
                .experience
                .lock()
                .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
            // Drop the Thinking placeholder so apply_planner_response owns the turn.
            if experience.conversation().len() > turn_index {
                let placeholder = experience.conversation().get(turn_index).map(|turn| {
                    matches!(turn.role, jaymi_memory::MessageRole::Assistant)
                        && turn.content.is_empty()
                });
                if placeholder == Some(true) {
                    let _ = experience.remove_turn_at(turn_index);
                }
            }
        }
        // Placeholder removed — full Experience + workspace apply (no double user record).
        self.apply_workspace_response(&response)?;
        Ok(PumpGeneration::Finished(response))
    }

    /// Mirror a failed background start onto Experience + Planner.
    fn install_failed_generation(&self, turn_index: usize, message: &str) -> JaymiResult<()> {
        if let Ok(planner) = self.container.resolve::<Planner>() {
            planner.transition_conversation(jaymi_planner::ConversationState::Failed);
            let mut experience = self
                .experience
                .lock()
                .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
            let _ = experience.finalize_streaming_turn(
                turn_index,
                format!("I couldn't start that reply ({message})"),
                jaymi_reasoning::StreamingLifecycle::Failed,
            );
            experience.mirror_conversation_state(planner.conversation_state());
        }
        Ok(())
    }

    /// Cancel the active generation (cooperative).
    pub fn cancel_generation(&self) -> JaymiResult<()> {
        let mut guard = self
            .active_generation
            .lock()
            .map_err(|_| JaymiError::new("generation lock poisoned"))?;
        match guard.as_mut() {
            Some(GenerationSlot::Starting(pending)) => {
                pending.cancel.store(true, Ordering::Relaxed);
                let turn_index = pending.turn_index;
                *guard = None;
                drop(guard);
                if let Ok(planner) = self.container.resolve::<Planner>() {
                    planner.transition_conversation(jaymi_planner::ConversationState::Cancelled);
                    let mut experience = self
                        .experience
                        .lock()
                        .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
                    let _ = experience.finalize_streaming_turn(
                        turn_index,
                        "Generation cancelled (user)",
                        jaymi_reasoning::StreamingLifecycle::Cancelled,
                    );
                    experience.mirror_conversation_state(planner.conversation_state());
                }
                Ok(())
            }
            Some(GenerationSlot::Active(active)) => {
                active.stream.cancel();
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Retry the active generation, or regenerate when no stream remains.
    pub fn retry_generation(
        self: &Arc<Self>,
        keep_partial: bool,
    ) -> JaymiResult<BeginGeneration> {
        let mut guard = self
            .active_generation
            .lock()
            .map_err(|_| JaymiError::new("generation lock poisoned"))?;
        match guard.as_mut() {
            Some(GenerationSlot::Active(active)) => {
                active.stream.retry(keep_partial).map_err(|err| {
                    JaymiError::new(format!("retry failed: {}", err.message()))
                })?;
                let planner = self.container.resolve::<Planner>()?;
                planner.resume_reasoning_after_retry();
                let mut experience = self
                    .experience
                    .lock()
                    .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
                if keep_partial {
                    experience.set_stream_lifecycle(
                        active.turn_index,
                        jaymi_reasoning::StreamingLifecycle::Thinking,
                    )?;
                } else {
                    experience.reset_assistant_for_retry(active.turn_index)?;
                }
                experience.mirror_conversation_state(planner.conversation_state());
                Ok(BeginGeneration::Started)
            }
            Some(GenerationSlot::Starting(_)) => Err(JaymiError::new(
                "generation is still starting — wait or cancel before retry",
            )),
            None => {
                drop(guard);
                self.regenerate_response()
            }
        }
    }

    /// Regenerate the last assistant reply from the preceding user message.
    pub fn regenerate_response(self: &Arc<Self>) -> JaymiResult<BeginGeneration> {
        if self.generation_active() {
            return Err(JaymiError::new(
                "cannot regenerate while a generation is active — cancel first",
            ));
        }
        let user_text = {
            let mut experience = self
                .experience
                .lock()
                .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
            let text = experience
                .last_user_content()
                .ok_or_else(|| JaymiError::new("no user turn to regenerate from"))?
                .to_string();
            let _ = experience.remove_last_assistant_turn();
            if let Some(index) = experience
                .conversation()
                .iter()
                .rposition(|turn| matches!(turn.role, jaymi_memory::MessageRole::User))
            {
                experience.remove_turn_at(index)?;
            }
            text
        };
        self.begin_generation(user_text)
    }

    /// Copy helper — returns assistant turn text for clipboard (UI owns pasteboard).
    pub fn assistant_turn_text(&self, turn_index: usize) -> JaymiResult<String> {
        let experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        let turn = experience
            .conversation()
            .get(turn_index)
            .ok_or_else(|| JaymiError::new("turn index out of range"))?;
        if !matches!(turn.role, jaymi_memory::MessageRole::Assistant) {
            return Err(JaymiError::new("turn is not an assistant response"));
        }
        Ok(turn.content.clone())
    }

    /// Apply workspace expansion / summary side effects without appending assistant text.
    fn apply_workspace_expansion_only(&self, response: &PlannerResponse) -> JaymiResult<()> {
        let mut experience = self
            .experience
            .lock()
            .map_err(|_| JaymiError::new("experience session lock poisoned"))?;
        if let Some(workspace) = &response.workspace {
            if workspace.expands() {
                let _ = experience.expand_workspace(workspace.clone());
            }
        }
        let conversation_id = experience.conversation_id().map(str::to_string);
        let coding_open = experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
        drop(experience);
        if let Some(summary) = response
            .execution_summary
            .as_ref()
            .filter(|summary| summary.should_surface_in_conversation())
        {
            let _ = self.store_execution_summary_memory(summary, conversation_id.as_deref());
        }
        if coding_open {
            let _ = self.refresh_coding_explorer();
        }
        Ok(())
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

        let approval_history = self
            .search_approval_history(
                &jaymi_planner::ApprovalHistoryQuery {
                    limit: Some(20),
                    ..Default::default()
                },
                jaymi_planner::ApprovalHistoryAccess::Full,
            )
            .unwrap_or_default();
        let paused = self
            .container
            .resolve::<Planner>()
            .ok()
            .and_then(|planner| planner.paused_snapshots().ok())
            .unwrap_or_default();
        let inspection = crate::execution_diagnostics::build_execution_inspection(
            paused,
            &experience,
            approval_history,
        );

        Ok(build_coding_diagnostics_view(
            &snapshot,
            &experience,
            coding.as_ref(),
            project.as_ref(),
            activity.as_ref(),
            &inspection,
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

    /// Recompute `CodingState.problems` via background maintenance (non-blocking).
    ///
    /// No-op (returns `Ok`) when there is no Coding capability state yet.
    pub fn refresh_coding_problems(&self) -> JaymiResult<()> {
        if self.with_coding_state(|_| ()).is_err() {
            return Ok(());
        }
        let _ = self.schedule_context_maintenance(MaintenanceKind::Diagnostics);
        Ok(())
    }

    /// Synchronously recompute `CodingState.problems` from every registered Problems provider.
    pub fn refresh_coding_problems_now(&self) -> JaymiResult<()> {
        if self.with_coding_state(|_| ()).is_err() {
            return Ok(());
        }
        let registry = self.problems_registry()?;
        let ctx = self.build_problems_context();
        let issues = registry.collect_all(&ctx)?;
        self.with_coding_state(|coding| {
            coding.problems = issues.clone();
        })?;
        let diagnostics = issues
            .iter()
            .map(|issue| jaymi_context::BundleDiagnostic {
                path: issue.path.clone(),
                severity: issue.severity.as_str().to_string(),
                message: issue.message.clone(),
                line: issue.line,
                column: issue.column,
                source: Some(if issue.source_label.is_empty() {
                    issue.source.clone()
                } else {
                    issue.source_label.clone()
                }),
            })
            .collect();
        self.context_maintenance
            .publish_diagnostics_section(jaymi_context::DiagnosticsSection { diagnostics });
        Ok(())
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
            awaiting_review: response.awaiting_review,
            plan_id: response
                .execution_plan
                .as_ref()
                .map(|plan| plan.id().as_str().to_string()),
            plan_status: response
                .execution_plan
                .as_ref()
                .map(|plan| plan.status().as_str().to_string()),
            risk: response
                .execution_plan
                .as_ref()
                .map(|plan| plan.estimated_risk().as_str().to_string()),
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
                .memory()
                .map(|context| context.memories.len())
                .unwrap_or(0),
        };
        if let Ok(mut guard) = self.last_planner_activity.lock() {
            *guard = Some(activity);
        }
        let mut pipeline_timing = response.pipeline_timing.clone();
        if let Some(timing) = pipeline_timing.as_mut() {
            if timing.total_ms.is_none() {
                timing.total_ms = Some(duration_ms);
            }
            if !timing
                .stages
                .iter()
                .any(|stage| stage.stage == "request_received")
            {
                timing.stages.insert(
                    0,
                    jaymi_reasoning::PipelineStageTiming::new("request_received", 0),
                );
            }
        } else if response.reasoning_used {
            let mut timing = jaymi_reasoning::PipelineTiming::new();
            timing.push(jaymi_reasoning::PipelineStageTiming::new(
                "request_received",
                0,
            ));
            timing.total_ms = Some(duration_ms);
            pipeline_timing = Some(timing);
        }
        let turn = LastReasoningTurn {
            reasoning_used: response.reasoning_used,
            reasoning_provider_id: response.reasoning_provider_id.clone(),
            stream_lifecycle: response.stream_lifecycle,
            metrics: response.reasoning_metrics.clone(),
            prompt_diagnostics: response.prompt_diagnostics.clone(),
            configured_model: response.configured_model.clone(),
            provider_model: response.provider_model.clone(),
            conversation_state: response.conversation_state,
            pipeline_timing,
        };
        if let Ok(mut guard) = self.last_reasoning.lock() {
            *guard = Some(turn);
        }
        // Opportunistic background refresh so a blocked / denied turn shows up in
        // the Problems panel without blocking the conversational path.
        let _ = self.refresh_coding_problems();
    }

    /// Last conversational reasoning turn retained for diagnostics.
    pub fn last_reasoning_turn(&self) -> Option<LastReasoningTurn> {
        self.last_reasoning
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Prefer an explicit reasoning model for conversational turns (B1.13.6).
    pub fn set_preferred_model(
        &self,
        model: Option<jaymi_reasoning::ModelIdentifier>,
    ) -> JaymiResult<()> {
        let planner = self.container.resolve::<Planner>()?;
        planner.set_preferred_model(model)
    }

    /// Current explicit preferred reasoning model, when set.
    pub fn preferred_model(&self) -> JaymiResult<Option<jaymi_reasoning::ModelIdentifier>> {
        let planner = self.container.resolve::<Planner>()?;
        Ok(planner.preferred_model())
    }

    /// Session cache generation (bumped on every invalidation).
    pub fn session_cache_generation(&self) -> u64 {
        self.session_cache
            .lock()
            .map(|cache| cache.generation())
            .unwrap_or(0)
    }

    /// Diagnostic summary of the session cache (Developer Diagnostics / tests).
    pub fn session_cache_summary(&self) -> String {
        self.session_cache
            .lock()
            .map(|cache| cache.summary())
            .unwrap_or_else(|_| "session cache lock poisoned".into())
    }

    /// Theme preference from the session settings cache (falls back to Config).
    pub fn theme_preference(&self) -> JaymiResult<jaymi_config::Theme> {
        if let Ok(cache) = self.session_cache.lock() {
            if let Some(theme) = cache.theme() {
                return Ok(theme);
            }
        }
        Ok(self.load_settings_uncached()?.theme)
    }

    /// Immutable settings snapshot from the session cache (falls back to Config).
    pub fn settings_snapshot(&self) -> JaymiResult<jaymi_config::Settings> {
        self.cached_settings()
    }

    /// Invalidate Model Registry / installed-models / provider-health cache.
    ///
    /// Call after Refresh Models, connection tests, or any live rediscovery.
    pub fn invalidate_session_cache_models(&self) {
        if let Ok(mut cache) = self.session_cache.lock() {
            cache.invalidate_models();
        }
    }

    /// Notify that persisted settings changed (theme, reasoning prefs, …).
    ///
    /// Invalidates the settings + theme session cache slots.
    pub fn notify_settings_changed(&self) {
        if let Ok(mut cache) = self.session_cache.lock() {
            cache.invalidate_settings();
        }
        // Eagerly re-warm so theme preference reads stay cheap.
        let _ = self.cached_settings();
    }

    /// Notify that provider registration changed (tool or reasoning providers).
    ///
    /// Invalidates Model Registry and capability-availability cache slots.
    pub fn notify_providers_changed(&self) {
        if let Ok(mut cache) = self.session_cache.lock() {
            cache.invalidate_providers();
        }
    }

    /// Register an additional reasoning provider and invalidate related cache slots.
    pub fn register_reasoning_provider(
        &self,
        provider: Arc<dyn ReasoningProvider>,
    ) -> JaymiResult<()> {
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        registry.register_provider(provider);
        self.notify_providers_changed();
        Ok(())
    }

    fn store_model_registry_snapshot(&self, snapshot: jaymi_reasoning::ModelRegistrySnapshot) {
        if let Ok(mut cache) = self.session_cache.lock() {
            cache.set_model_registry(snapshot);
        }
    }

    fn cached_model_registry_snapshot(
        &self,
    ) -> JaymiResult<jaymi_reasoning::ModelRegistrySnapshot> {
        if let Ok(cache) = self.session_cache.lock() {
            if let Some(snapshot) = cache.model_registry().cloned() {
                return Ok(snapshot);
            }
        }
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        let snapshot = registry.snapshot();
        self.store_model_registry_snapshot(snapshot.clone());
        Ok(snapshot)
    }

    fn cached_capability_discovery(&self) -> JaymiResult<CapabilityDiscoveryReport> {
        if let Ok(cache) = self.session_cache.lock() {
            if let Some(report) = cache.capability_availability().cloned() {
                return Ok(report);
            }
        }
        let planner = self.container.resolve::<Planner>()?;
        let report = planner.discover_capability_status().unwrap_or_default();
        if let Ok(mut cache) = self.session_cache.lock() {
            cache.set_capability_availability(report.clone());
        }
        Ok(report)
    }

    fn load_settings_uncached(&self) -> JaymiResult<jaymi_config::Settings> {
        let config = self.container.resolve::<Arc<Mutex<Config>>>()?;
        let config = config
            .lock()
            .map_err(|_| JaymiError::new("config lock poisoned"))?;
        Ok(config.settings().clone())
    }

    fn cached_settings(&self) -> JaymiResult<jaymi_config::Settings> {
        if let Ok(cache) = self.session_cache.lock() {
            if let Some(settings) = cache.settings().cloned() {
                return Ok(settings);
            }
        }
        let settings = self.load_settings_uncached()?;
        if let Ok(mut cache) = self.session_cache.lock() {
            cache.set_settings(settings.clone());
        }
        Ok(settings)
    }

    fn seed_session_cache(&self) -> JaymiResult<()> {
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        self.store_model_registry_snapshot(registry.snapshot());
        let _ = self.cached_capability_discovery()?;
        let _ = self.cached_settings()?;
        jaymi_logging::info(
            "boot",
            format!("session cache seeded · {}", self.session_cache_summary()),
        );
        Ok(())
    }

    /// Immutable Reasoning Settings snapshot (Settings Workspace paints this only).
    pub fn reasoning_settings_snapshot(
        &self,
    ) -> JaymiResult<crate::settings_workspace::ReasoningSettingsSnapshot> {
        self.build_reasoning_settings_snapshot(false)
    }

    /// Refresh ModelRegistry and return an updated Settings snapshot.
    ///
    /// Invalidates the session Model Registry cache (installed models + provider health).
    pub fn refresh_reasoning_models(
        &self,
    ) -> JaymiResult<crate::settings_workspace::ReasoningSettingsSnapshot> {
        self.invalidate_session_cache_models();
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        let _ = registry.refresh();
        self.reapply_persisted_reasoning_preference()?;
        self.store_model_registry_snapshot(registry.snapshot());
        self.build_reasoning_settings_snapshot(false)
    }

    /// Persist and apply a user-selected default reasoning model.
    pub fn set_default_reasoning_model(
        &self,
        provider_id: impl Into<String>,
        model_name: impl Into<String>,
    ) -> JaymiResult<crate::settings_workspace::ReasoningSettingsSnapshot> {
        let provider_id = provider_id.into();
        let model_name = model_name.into();
        let id = jaymi_reasoning::ModelIdentifier::new(provider_id.clone(), model_name.clone());
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        registry.set_default(Some(id.clone())).map_err(|err| {
            JaymiError::new(format!("could not set default model: {}", err.message()))
        })?;
        let planner = self.container.resolve::<Planner>()?;
        planner.set_preferred_model(Some(id))?;
        self.persist_reasoning_preferences(ReasoningPreferences {
            preferred_provider_id: Some(provider_id),
            preferred_model: Some(model_name),
        })?;
        // Default selection changed the registry snapshot + settings.
        self.invalidate_session_cache_models();
        self.store_model_registry_snapshot(registry.snapshot());
        self.build_reasoning_settings_snapshot(false)
    }

    /// Probe reasoning provider connectivity through the registry path.
    ///
    /// Invalidates and refreshes the session Model Registry cache.
    pub fn test_reasoning_connection(
        &self,
    ) -> JaymiResult<crate::settings_workspace::ReasoningSettingsSnapshot> {
        self.invalidate_session_cache_models();
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        let _ = registry.refresh();
        self.reapply_persisted_reasoning_preference()?;
        self.store_model_registry_snapshot(registry.snapshot());
        self.build_reasoning_settings_snapshot(true)
    }

    fn persist_reasoning_preferences(&self, prefs: ReasoningPreferences) -> JaymiResult<()> {
        let config = self.container.resolve::<Arc<Mutex<Config>>>()?;
        let mut config = config
            .lock()
            .map_err(|_| JaymiError::new("config lock poisoned"))?;
        config.settings_mut().reasoning = prefs;
        config.settings_mut().version = jaymi_config::CURRENT_SETTINGS_VERSION;
        config.save()?;
        drop(config);
        self.notify_settings_changed();
        Ok(())
    }

    fn reapply_persisted_reasoning_preference(&self) -> JaymiResult<()> {
        let planner = self.container.resolve::<Planner>()?;
        let registry = self.container.resolve::<Arc<ModelRegistry>>()?;
        Self::apply_reasoning_preferences_locked(&planner, &registry, &self.container)
    }

    fn apply_reasoning_preferences_locked(
        planner: &Planner,
        registry: &ModelRegistry,
        container: &ServiceContainer,
    ) -> JaymiResult<()> {
        let config = container.resolve::<Arc<Mutex<Config>>>()?;
        let prefs = config
            .lock()
            .map_err(|_| JaymiError::new("config lock poisoned"))?
            .settings()
            .reasoning
            .clone();
        if !prefs.is_set() {
            return Ok(());
        }
        let id = jaymi_reasoning::ModelIdentifier::new(
            prefs.preferred_provider_id.clone().unwrap_or_default(),
            prefs.preferred_model.clone().unwrap_or_default(),
        );
        if registry.get(&id).is_some() {
            let _ = registry.set_default(Some(id.clone()));
            let _ = planner.set_preferred_model(Some(id));
        } else if let Some(fallback) = registry.default_model() {
            // Preference removed from disk catalog — keep registry default; clear Planner override
            // so prepare_reasoning_model uses the live default.
            let _ = planner.set_preferred_model(Some(fallback));
        }
        Ok(())
    }

    fn build_reasoning_settings_snapshot(
        &self,
        after_test: bool,
    ) -> JaymiResult<crate::settings_workspace::ReasoningSettingsSnapshot> {
        use crate::settings_workspace::{
            ReasoningConnectionStatus, ReasoningSettingsModel, ReasoningSettingsProvider,
            ReasoningSettingsSnapshot,
        };

        let snapshot = self.cached_model_registry_snapshot()?;
        let models: Vec<ReasoningSettingsModel> = snapshot
            .models
            .iter()
            .map(|model| ReasoningSettingsModel {
                provider_id: model.provider_id.clone(),
                model_name: model.info.id.name.clone(),
                display_name: model.info.display_name.clone(),
                parameter_size: model.info.parameter_count.clone(),
                context_length: model.info.context_tokens,
                quantization: model.info.quantization.clone(),
                local: model.info.local,
                capability_labels: model
                    .info
                    .capabilities
                    .labels()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                is_default: model.is_default,
                available: model.available,
            })
            .collect();

        let providers: Vec<ReasoningSettingsProvider> = snapshot
            .providers
            .iter()
            .map(|entry| {
                let (status, detail) = map_provider_connection(&entry.health);
                ReasoningSettingsProvider {
                    id: entry.provider_id.clone(),
                    display_name: entry.display_name.clone(),
                    status,
                    detail,
                    model_count: entry.model_count,
                }
            })
            .collect();

        let primary = providers.first();
        let status = if after_test {
            primary
                .map(|provider| provider.status)
                .unwrap_or(ReasoningConnectionStatus::Offline)
        } else {
            primary
                .map(|provider| provider.status)
                .unwrap_or(ReasoningConnectionStatus::Offline)
        };
        let message = primary
            .map(|provider| provider.detail.clone())
            .unwrap_or_else(|| "No reasoning providers are registered.".into());
        if matches!(status, ReasoningConnectionStatus::Connected) && models.is_empty() {
            // Connected but empty catalog — guide the user.
        }
        let message = if matches!(status, ReasoningConnectionStatus::Connected) && models.is_empty()
        {
            "Connected, but no models are installed yet. Pull a model in Ollama, then refresh."
                .into()
        } else if matches!(status, ReasoningConnectionStatus::Connected) && after_test {
            format!("{message} Connection check succeeded.")
        } else {
            message
        };

        let default_model_key = snapshot
            .default_model
            .as_ref()
            .map(|id| format!("{}/{}", id.provider, id.name));
        let active_provider_id = snapshot
            .default_model
            .as_ref()
            .map(|id| id.provider.clone())
            .or_else(|| primary.map(|provider| provider.id.clone()));
        let active_provider_name = active_provider_id.as_ref().and_then(|id| {
            providers
                .iter()
                .find(|provider| &provider.id == id)
                .map(|provider| provider.display_name.clone())
        });

        Ok(ReasoningSettingsSnapshot {
            status,
            message,
            active_provider_id,
            active_provider_name,
            default_model_key,
            providers,
            models,
        })
    }

    /// Assemble the Conversational Reasoning diagnostics report.
    pub fn reasoning_diagnostics(&self) -> JaymiResult<ReasoningDiagnosticsReport> {
        let planner = self.container.resolve::<Planner>()?;
        // Session-cached registry snapshot — do not refresh on every paint.
        // Refresh Models / Test Connection own rediscovery + cache invalidation.
        let registry_snapshot = self.cached_model_registry_snapshot().ok();
        let last = self.last_reasoning_turn();
        let live_state = planner.conversation_state();
        let conversation_runtime_state = if live_state.is_active() {
            live_state.as_str().to_string()
        } else {
            last.as_ref()
                .map(|turn| turn.conversation_state.as_str().to_string())
                .unwrap_or_else(|| live_state.as_str().to_string())
        };
        let streaming = last
            .as_ref()
            .and_then(|turn| turn.stream_lifecycle)
            .or_else(|| {
                use jaymi_reasoning::StreamingLifecycle;
                match live_state {
                    jaymi_planner::ConversationState::Reasoning => {
                        Some(StreamingLifecycle::Thinking)
                    }
                    jaymi_planner::ConversationState::Streaming => {
                        Some(StreamingLifecycle::Streaming)
                    }
                    jaymi_planner::ConversationState::Cancelled => {
                        Some(StreamingLifecycle::Cancelled)
                    }
                    jaymi_planner::ConversationState::Failed => Some(StreamingLifecycle::Failed),
                    jaymi_planner::ConversationState::Completed => {
                        Some(StreamingLifecycle::Completed)
                    }
                    _ => None,
                }
            });
        Ok(ReasoningDiagnosticsReport::assemble(
            ReasoningDiagnosticsInput {
                health: Some(planner.reasoning().health()),
                capabilities: Some(planner.reasoning().capabilities()),
                provider_id: planner
                    .reasoning()
                    .provider_id()
                    .map(str::to_string)
                    .or_else(|| {
                        last.as_ref()
                            .and_then(|turn| turn.reasoning_provider_id.clone())
                    }),
                registry: registry_snapshot.clone(),
                metrics: last.as_ref().and_then(|turn| turn.metrics.clone()),
                prompt: last
                    .as_ref()
                    .and_then(|turn| turn.prompt_diagnostics.clone()),
                streaming,
                conversation_runtime_state: Some(conversation_runtime_state),
                reasoning_used: last
                    .as_ref()
                    .map(|turn| turn.reasoning_used)
                    .unwrap_or(false),
                configured_model: last
                    .as_ref()
                    .and_then(|turn| turn.configured_model.as_ref().map(|m| m.display()))
                    .or_else(|| {
                        registry_snapshot
                            .as_ref()
                            .and_then(|snap| snap.default_model.as_ref().map(|m| m.display()))
                    }),
                provider_model: last
                    .as_ref()
                    .and_then(|turn| turn.provider_model.as_ref().map(|m| m.display())),
                loaded_model: self
                    .container
                    .resolve::<Arc<OllamaReasoningProvider>>()
                    .ok()
                    .and_then(|ollama| ollama.diagnostics_cached().loaded_model),
                pipeline_timing: last.as_ref().and_then(|turn| turn.pipeline_timing.clone()),
            },
        ))
    }

    /// Assemble Workspace Intelligence diagnostics (Sprint B2.11).
    ///
    /// Developer Diagnostics only — never writes transcript / Memory / Planner state.
    /// Observation only: does not schedule maintenance or re-assemble Context.
    pub fn workspace_diagnostics(
        &self,
    ) -> JaymiResult<crate::workspace_diagnostics::WorkspaceDiagnosticsReport> {
        let context_inspector = self
            .container
            .resolve::<Arc<ContextEngine>>()
            .ok()
            .and_then(|engine| engine.inspect_last());
        Ok(
            crate::workspace_diagnostics::WorkspaceDiagnosticsReport::from_maintenance(
                &self.context_maintenance,
                context_inspector,
            ),
        )
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
        let (
            config_health,
            indexing_enabled,
            config_log_level,
            config_theme,
            config_path_display,
            config_detail_line,
        ) = {
            let config = self.container.resolve::<Arc<Mutex<Config>>>()?;
            let config = config
                .lock()
                .map_err(|_| JaymiError::new("config lock poisoned"))?;
            let health = config.health_check();
            let path = config.config_path().display().to_string();
            // Settings values come from the session cache when warm.
            let settings = self.cached_settings().unwrap_or_else(|_| config.settings().clone());
            let indexing_enabled = settings.indexing_enabled;
            let log_level = settings.log_level.as_str().to_string();
            let theme = settings.theme.as_str().to_string();
            let detail = format!(
                "log_level={} theme={} indexing={} path={}",
                settings.log_level.as_str(),
                settings.theme.as_str(),
                indexing_enabled,
                config.config_path().display()
            );
            (health, indexing_enabled, log_level, theme, path, detail)
        };
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
        let discovery = self.cached_capability_discovery().unwrap_or_default();
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
        let capability_inspector = {
            let registered = capabilities.list();
            Some(
                CapabilityInspectorReport::from_discovery(&registered, &discovery)
                    .with_active_workspace(self.active_ui_workspace().ok().flatten()),
            )
        };
        let context_inspector = self
            .container
            .resolve::<Arc<ContextEngine>>()
            .ok()
            .and_then(|engine| engine.inspect_last());
        let context_history = self
            .container
            .resolve::<Arc<ContextEngine>>()
            .ok()
            .map(|engine| engine.history())
            .unwrap_or_default();
        let reasoning_inspector = self.reasoning_diagnostics().ok();
        let workspace_inspector = self.workspace_diagnostics().ok();
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
            .map(|metadata| format!("{} ({})", metadata.id, metadata.risk.as_str()))
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

        let permission_mode =
            "read: allowed · write/delete/terminal: requires_approval · internet: denied · review from policy+permission+ToolRisk"
                .to_string();

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
                config_detail_line,
            ),
            SubsystemStatus::new(
                "Session Cache",
                OperationalStatus::Operational,
                self.session_cache_summary(),
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
                    "sources_bound={} · assembles={} · history={} · cache_hits={} · policies=[{}]",
                    context_engine.sources_bound(),
                    context_engine.assemble_count(),
                    context_engine.history_len(),
                    context_engine.cache_stats().hits,
                    context_engine.active_context_policies().join(",")
                ),
            ),
            SubsystemStatus::new(
                "Project Status",
                OperationalStatus::from_health(project_status.healthy, project_status.initialized),
                project_status.detail.clone(),
            ),
            SubsystemStatus::new(
                "Reasoning Status",
                {
                    match reasoning_inspector
                        .as_ref()
                        .map(|report| report.reasoning_health.as_str())
                    {
                        Some("ready") | Some("degraded") => OperationalStatus::Operational,
                        Some("unavailable") => OperationalStatus::Disabled,
                        Some(_) if planner.reasoning_implemented() => {
                            OperationalStatus::Operational
                        }
                        Some(_) => OperationalStatus::Disabled,
                        None if planner.reasoning_implemented() => OperationalStatus::Operational,
                        None => OperationalStatus::Stub,
                    }
                },
                {
                    match &reasoning_inspector {
                        Some(report) => {
                            format!(
                                "backend={} · {}",
                                planner.reasoning_status(),
                                report.summary_line()
                            )
                        }
                        None => format!("backend={}", planner.reasoning_status()),
                    }
                },
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
            context_inspector,
            reasoning_inspector,
            workspace_inspector,
            context_history,
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
            config_path: Some(config_path_display),
            config_log_level: Some(config_log_level),
            config_theme: Some(config_theme),
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
        if let Some(config) = self.container.take::<Arc<Mutex<Config>>>() {
            if let Ok(mutex) = Arc::try_unwrap(config) {
                if let Ok(mut config) = mutex.into_inner() {
                    config.shutdown()?;
                }
            }
        }

        Ok(())
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

fn map_provider_connection(
    health: &jaymi_reasoning::ReasoningHealth,
) -> (
    crate::settings_workspace::ReasoningConnectionStatus,
    String,
) {
    use crate::settings_workspace::ReasoningConnectionStatus;
    match health {
        jaymi_reasoning::ReasoningHealth::Ready => {
            (ReasoningConnectionStatus::Connected, "Connected".into())
        }
        jaymi_reasoning::ReasoningHealth::Degraded { reason } => (
            ReasoningConnectionStatus::Connected,
            format!("Connected with issues: {reason}"),
        ),
        jaymi_reasoning::ReasoningHealth::Unavailable { reason } => {
            let lower = reason.to_ascii_lowercase();
            if lower.contains("isn't running")
                || lower.contains("unreachable")
                || lower.contains("connection refused")
                || lower.contains("can't reach")
                || lower.contains("can’t reach")
            {
                (ReasoningConnectionStatus::Offline, reason.clone())
            } else {
                (ReasoningConnectionStatus::Error, reason.clone())
            }
        }
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
        assert!(
            diagnostics
                .tool_ids
                .iter()
                .any(|id| id == "write_file (modify)"),
            "diagnostics must show ToolRisk: {:?}",
            diagnostics.tool_ids
        );
        assert!(
            diagnostics
                .tool_ids
                .iter()
                .any(|id| id == "terminal (destructive)"),
            "diagnostics must show ToolRisk: {:?}",
            diagnostics.tool_ids
        );
        assert!(
            diagnostics
                .permission_mode
                .as_deref()
                .is_some_and(|mode| mode.contains("requires_approval")),
            "permission mode should describe RequiresApproval decisions"
        );
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
        assert_ne!(
            diagnostics.subsystem("Reasoning Status").unwrap().status,
            OperationalStatus::Stub
        );
        assert!(matches!(
            diagnostics.subsystem("Reasoning Status").unwrap().status,
            OperationalStatus::Disabled | OperationalStatus::Operational
        ));
        assert!(diagnostics
            .subsystem("Reasoning Status")
            .unwrap()
            .detail
            .contains("health="));
        assert!(diagnostics.reasoning_inspector.is_some());
        assert!(diagnostics
            .render_dashboard()
            .contains("Conversational Reasoning"));
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
