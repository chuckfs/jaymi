//! Deterministic application boot sequence.
//!
//! Startup order:
//! Configuration → Logging → Database → Policy Engine → Permission Engine →
//! Memory Engine → Context Engine → Capability Registry → Provider Registry →
//! Knowledge → Understanding → Discovery → Tools → Planner → Desktop UI

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jaymi_capabilities::{Capability, CapabilityRegistry};
use jaymi_config::Config;
use jaymi_context::ContextEngine;
use jaymi_core::{
    AppState, DiscoveryQueryKind, HealthReport, JaymiError, JaymiResult, Lifecycle,
    ServiceContainer, UserRequest,
};
use jaymi_database::Database;
use jaymi_discovery::{DiscoveryEngine, FilesystemWatcher};
use jaymi_knowledge::{KnowledgeStore, SqliteKnowledgeStore};
use jaymi_logging::Logger;
use jaymi_memory::MemoryEngine;
use jaymi_parsers::{default_registry, ParserRegistry};
use jaymi_permissions::PermissionEngine;
use jaymi_planner::{Planner, PlannerDeps, PlannerResponse};
use jaymi_policies::PolicyEngine;
use jaymi_providers::{
    FilesystemProvider, OcrProvider, PlaceholderOcrProvider, Provider, ProviderRegistry,
};
use jaymi_tools::{
    QueryInventoryTool, ReadFileTool, ScanFilesystemTool, SearchFilesTool, ToolOrchestrator,
    ToolRegistry,
};
use jaymi_understanding::{
    format_parser_usage, ContentIntelligence, ContentIntelligenceApi, SqliteContentStore,
    UnderstandingEngine,
};

use crate::diagnostics::DiagnosticsSnapshot;

/// Owns the process service container and application state.
pub struct Application {
    state: AppState,
    container: ServiceContainer,
    health_reports: Vec<HealthReport>,
}

impl Application {
    /// Create an application in the `Starting` state.
    pub fn new() -> Self {
        Self {
            state: AppState::Starting,
            container: ServiceContainer::new(),
            health_reports: Vec::new(),
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

        self.boot_service(MemoryEngine::new())?;
        self.boot_service(ContextEngine::new())?;

        // Capability registry + Layer 0 and Layer 1 capabilities.
        let mut capabilities = CapabilityRegistry::new();
        self.initialize_service(&mut capabilities)?;
        capabilities.register(Capability::Search)?;
        capabilities.register(Capability::ReadDocuments)?;
        capabilities.register(Capability::Discover)?;
        capabilities.register(Capability::Index)?;
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

        // Content store + Understanding Engine (Layer 2).
        let content = Arc::new(SqliteContentStore::new(Arc::clone(&database)));
        self.container.register(Arc::clone(&content));
        let mut understanding = UnderstandingEngine::new(
            Arc::clone(&knowledge),
            Arc::clone(&content),
            Arc::clone(&filesystem),
            Arc::clone(&parsers),
        );
        self.initialize_service(&mut understanding)?;
        let understanding = Arc::new(understanding);
        self.container.register(Arc::clone(&understanding));

        // Content Intelligence API — stable consumer surface (hides parsers/SQLite).
        let content_api = Arc::new(ContentIntelligenceApi::new(Arc::clone(&understanding)));
        self.container.register(Arc::clone(&content_api));

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

        // Tool registry + Layer 0 and Layer 1 tools.
        let mut tools = ToolRegistry::new();
        self.initialize_service(&mut tools)?;
        tools.register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))?;
        tools.register_tool(Arc::new(ReadFileTool::new(Arc::clone(&content_api))))?;
        tools.register_tool(Arc::new(ScanFilesystemTool::new(Arc::clone(&discovery))))?;
        tools.register_tool(Arc::new(QueryInventoryTool::new(Arc::clone(&knowledge))))?;
        let tools = Arc::new(tools);
        self.container.register(Arc::clone(&tools));

        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));
        let mut planner = Planner::new(PlannerDeps {
            capabilities,
            providers,
            tools,
            orchestrator,
            policies,
            permissions,
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

    /// Ask the Planner to list a single directory through the full architecture.
    pub fn list_directory(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(UserRequest::list_directory(path.as_ref()))
    }

    /// Ask the Planner to read a supported file into a unified document.
    pub fn read_file(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(UserRequest::read_file(path.as_ref()))
    }

    /// Ask the Planner to recursively index a root into the discovery inventory.
    pub fn index_root(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(UserRequest::index_root(path.as_ref()))
    }

    /// Ask the Planner what files exist using the knowledge database only.
    pub fn discover_inventory(&self) -> JaymiResult<PlannerResponse> {
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(UserRequest::discover_inventory())
    }

    /// Ask the Planner a structured discovery query against the knowledge database.
    pub fn discover_query(&self, kind: DiscoveryQueryKind) -> JaymiResult<PlannerResponse> {
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(UserRequest::discover_query(kind))
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
        let watcher = self.container.resolve::<Arc<FilesystemWatcher>>()?;
        let policies = self.container.resolve::<Arc<PolicyEngine>>()?;
        let permissions = self.container.resolve::<Arc<PermissionEngine>>()?;
        let memory = self.container.resolve::<MemoryEngine>()?;
        let capabilities = self.container.resolve::<Arc<CapabilityRegistry>>()?;
        let providers = self.container.resolve::<Arc<ProviderRegistry>>()?;
        let ocr = self.container.resolve::<Arc<PlaceholderOcrProvider>>()?;
        let tools = self.container.resolve::<Arc<ToolRegistry>>()?;
        let parsers = self.container.resolve::<Arc<ParserRegistry>>()?;

        let planner_health = planner.health_check();
        let database_health = database.health_check();
        let logger_health = logger.health_check();
        let config_health = config.health_check();
        let policies_health = policies.health_check();
        let permissions_health = permissions.health_check();
        let memory_health = memory.health_check();
        let discovery_health = discovery.health_check();
        let knowledge_health = knowledge.health_check();
        let understanding_health = understanding.health_check();
        let understanding_stats = understanding.stats().ok();
        let content_health = content_api.retrieve_health().ok();
        let discovery_stats = knowledge.stats().ok();
        let collection_stats = knowledge.collection_stats().ok();
        let watcher_diagnostics = watcher.diagnostics();
        let ocr_status = ocr.ocr_status();

        let capability_ids: Vec<String> = capabilities
            .list()
            .into_iter()
            .map(|capability| capability.id().to_string())
            .collect();
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
                "Capabilities",
                if capabilities.is_initialized() && !capability_ids.is_empty() {
                    OperationalStatus::Operational
                } else if capabilities.is_initialized() {
                    OperationalStatus::Degraded
                } else {
                    OperationalStatus::Unavailable
                },
                if capability_ids.is_empty() {
                    "none registered".to_string()
                } else {
                    format!("{} · {}", capability_ids.len(), capability_ids.join(", "))
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
                if memory_health.initialized {
                    OperationalStatus::Stub
                } else {
                    OperationalStatus::Unavailable
                },
                memory_health
                    .details
                    .iter()
                    .find(|(key, _)| key == "note")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| "memory engine not operational".to_string()),
            ),
            SubsystemStatus::new(
                "Project Status",
                OperationalStatus::NotImplemented,
                "jaymi-projects not wired into boot".to_string(),
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
        let _ = self.container.take::<Arc<CapabilityRegistry>>();
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
        let _ = self.container.take::<Arc<UnderstandingEngine>>();
        let _ = self.container.take::<Arc<SqliteContentStore>>();
        let _ = self.container.take::<Arc<SqliteKnowledgeStore>>();

        shutdown_owned::<ContextEngine>(&mut self.container)?;
        shutdown_owned::<MemoryEngine>(&mut self.container)?;
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
        assert_eq!(diagnostics.provider_count, 2);
        assert!(diagnostics
            .provider_ids
            .iter()
            .any(|id| id == OCR_PROVIDER_ID));
        assert_eq!(diagnostics.tool_count, 4);
        assert_eq!(diagnostics.capability_count, 4);
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
            OperationalStatus::Stub
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
