//! Deterministic application boot sequence.
//!
//! Startup order:
//! Configuration → Logging → Database → Policy Engine → Permission Engine →
//! Memory Engine → Context Engine → Capability Registry → Provider Registry →
//! Tool Registry → Planner → Desktop UI

use std::path::Path;
use std::sync::Arc;

use jaymi_capabilities::{Capability, CapabilityRegistry};
use jaymi_config::Config;
use jaymi_context::ContextEngine;
use jaymi_core::{
    AppState, HealthReport, JaymiError, JaymiResult, Lifecycle, ServiceContainer, UserRequest,
};
use jaymi_database::Database;
use jaymi_logging::Logger;
use jaymi_memory::MemoryEngine;
use jaymi_parsers::{default_registry, ContentRegistry};
use jaymi_permissions::PermissionEngine;
use jaymi_planner::{Planner, PlannerDeps, PlannerResponse};
use jaymi_policies::PolicyEngine;
use jaymi_providers::{FilesystemProvider, Provider, ProviderRegistry};
use jaymi_tools::{ReadContentTool, SearchFilesTool, ToolOrchestrator, ToolRegistry};

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
        let mut app = Self::new();

        if let Err(error) = app.boot_inner() {
            app.state = AppState::Error {
                message: error.message().to_string(),
            };
            let _ = app.shutdown_initialized();
            return Err(error);
        }

        app.state = AppState::Ready;
        Ok(app)
    }

    fn boot_inner(&mut self) -> JaymiResult<()> {
        self.boot_service(Config::new())?;
        self.boot_service(Logger::new())?;
        self.boot_service(Database::new())?;
        self.boot_service(PolicyEngine::new())?;
        self.boot_service(PermissionEngine::new())?;
        self.boot_service(MemoryEngine::new())?;
        self.boot_service(ContextEngine::new())?;

        // Capability registry + Search / Read capabilities.
        let mut capabilities = CapabilityRegistry::new();
        self.initialize_service(&mut capabilities)?;
        capabilities.register(Capability::Search)?;
        capabilities.register(Capability::ReadContent)?;
        let capabilities = Arc::new(capabilities);
        self.container.register(Arc::clone(&capabilities));

        // Provider registry + Filesystem Provider.
        let mut providers = ProviderRegistry::new();
        self.initialize_service(&mut providers)?;
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize()?;
        providers.register(&filesystem)?;
        let filesystem = Arc::new(filesystem);
        self.container.register(Arc::clone(&filesystem));
        let providers = Arc::new(providers);
        self.container.register(Arc::clone(&providers));

        // Content registry with built-in TXT / Markdown / JSON parsers.
        let contents = Arc::new(default_registry()?);
        self.container.register(Arc::clone(&contents));

        // Tool registry + Search Files / Content tools.
        let mut tools = ToolRegistry::new();
        self.initialize_service(&mut tools)?;
        tools.register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))?;
        tools.register_tool(Arc::new(ReadContentTool::new(
            Arc::clone(&filesystem),
            Arc::clone(&contents),
        )))?;
        let tools = Arc::new(tools);
        self.container.register(Arc::clone(&tools));

        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));
        let mut planner = Planner::new(PlannerDeps {
            capabilities,
            providers,
            tools,
            orchestrator,
        });
        self.initialize_service(&mut planner)?;
        self.container.register(planner);

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
        if !report.initialized || !report.healthy {
            return Err(JaymiError::new(format!(
                "subsystem {} failed health check after initialize",
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
                .any(|report| report.name == *dependency && report.healthy);
            if !satisfied {
                return Err(JaymiError::new(format!(
                    "missing healthy dependency '{}' for {}",
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

    /// Ask the Planner to read a supported file into unified Content.
    pub fn read_file(&self, path: impl AsRef<Path>) -> JaymiResult<PlannerResponse> {
        let planner = self.container.resolve::<Planner>()?;
        planner.handle(UserRequest::read_file(path.as_ref()))
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
        let planner = self.container.resolve::<Planner>()?;
        let database = self.container.resolve::<Database>()?;
        let capabilities = self.container.resolve::<Arc<CapabilityRegistry>>()?;
        let providers = self.container.resolve::<Arc<ProviderRegistry>>()?;
        let tools = self.container.resolve::<Arc<ToolRegistry>>()?;

        let content = response.as_ref().and_then(|value| value.content.clone());
        Ok(DiagnosticsSnapshot {
            app_state: self.state.clone(),
            planner_healthy: planner.health_check().healthy,
            provider_count: providers.len(),
            tool_count: tools.len(),
            capability_count: capabilities.len(),
            database_connected: database.is_connected(),
            listed_path: response
                .as_ref()
                .and_then(|value| value.listed_path.clone()),
            listing_summary: response.as_ref().and_then(|value| {
                if value.content.is_none() {
                    Some(value.summary.clone())
                } else {
                    None
                }
            }),
            entries: response
                .as_ref()
                .map(|value| value.entries.clone())
                .unwrap_or_default(),
            read_path: content
                .as_ref()
                .and_then(|item| item.path.clone()),
            read_source: content.as_ref().map(|item| item.source.label().to_string()),
            read_file_type: content.as_ref().map(|item| item.content_type.label()),
            read_mime_type: content.as_ref().map(|item| item.mime_type.clone()),
            read_parser: content.as_ref().map(|item| item.parser_id.clone()),
            read_success: content.is_some(),
            read_character_count: content.as_ref().map(|item| item.character_count()),
            read_summary: response.as_ref().and_then(|value| {
                if value.content.is_some() {
                    Some(value.summary.clone())
                } else {
                    None
                }
            }),
            read_text: content.map(|item| item.text),
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
        if let Some(mut planner) = self.container.take::<Planner>() {
            planner.shutdown()?;
        }

        let _ = self.container.take::<Arc<ToolRegistry>>();
        let _ = self.container.take::<Arc<ProviderRegistry>>();
        let _ = self.container.take::<Arc<CapabilityRegistry>>();
        let _ = self.container.take::<Arc<FilesystemProvider>>();
        let _ = self.container.take::<Arc<ContentRegistry>>();

        shutdown_owned::<ContextEngine>(&mut self.container)?;
        shutdown_owned::<MemoryEngine>(&mut self.container)?;
        shutdown_owned::<PermissionEngine>(&mut self.container)?;
        shutdown_owned::<PolicyEngine>(&mut self.container)?;
        shutdown_owned::<Database>(&mut self.container)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{ContentSource, ContentType, EntryType};
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn boot_registers_search_and_read_stack() {
        let app = Application::boot().unwrap();
        assert!(app.state().is_ready());

        let diagnostics = app.diagnostics().unwrap();
        assert_eq!(diagnostics.app_state.label(), "Ready");
        assert!(diagnostics.planner_healthy);
        assert_eq!(diagnostics.provider_count, 1);
        assert_eq!(diagnostics.tool_count, 2);
        assert_eq!(diagnostics.capability_count, 2);
        assert!(diagnostics.database_connected);
    }

    #[test]
    fn list_directory_through_application() {
        let dir = temp_dir("list");
        let mut file = File::create(dir.join("hello.txt")).unwrap();
        write!(file, "hi").unwrap();

        let app = Application::boot().unwrap();
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

        let app = Application::boot().unwrap();
        let response = app.read_file(&path).unwrap();
        let content = response.content.as_ref().expect("content");
        assert_eq!(content.source, ContentSource::File);
        assert_eq!(content.content_type, ContentType::PlainText);
        assert_eq!(content.text, "universal reader");

        let snapshot = app.diagnostics_from_response(Some(response)).unwrap();
        assert!(snapshot.read_success);
        assert_eq!(snapshot.read_parser.as_deref(), Some("plain_text"));
        assert_eq!(snapshot.read_character_count, Some(16));
        assert_eq!(snapshot.read_source.as_deref(), Some("File"));
    }

    #[test]
    fn shutdown_returns_to_starting() {
        let mut app = Application::boot().unwrap();
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
