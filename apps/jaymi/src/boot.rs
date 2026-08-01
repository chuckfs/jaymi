//! Deterministic application boot sequence.
//!
//! Startup order:
//! Configuration → Logging → Database → Policy Engine → Permission Engine →
//! Memory Engine → Context Engine → Capability Registry → Provider Registry →
//! Tool Registry → Planner → Desktop UI

use std::sync::Arc;

use jaymi_capabilities::CapabilityRegistry;
use jaymi_config::Config;
use jaymi_context::ContextEngine;
use jaymi_core::{AppState, HealthReport, JaymiError, JaymiResult, Lifecycle, ServiceContainer};
use jaymi_database::Database;
use jaymi_logging::Logger;
use jaymi_memory::MemoryEngine;
use jaymi_permissions::PermissionEngine;
use jaymi_planner::{Planner, PlannerDeps};
use jaymi_policies::PolicyEngine;
use jaymi_providers::ProviderRegistry;
use jaymi_tools::ToolRegistry;

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

        let capabilities = self.boot_shared(CapabilityRegistry::new())?;
        let providers = self.boot_shared(ProviderRegistry::new())?;
        let tools = self.boot_shared(ToolRegistry::new())?;

        let mut planner = Planner::new(PlannerDeps {
            capabilities,
            providers,
            tools,
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

    fn boot_shared<T>(&mut self, mut service: T) -> JaymiResult<Arc<T>>
    where
        T: Lifecycle + 'static,
    {
        self.initialize_service(&mut service)?;
        let shared = Arc::new(service);
        self.container.register(Arc::clone(&shared));
        Ok(shared)
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

    /// Build the diagnostics snapshot for the temporary UI.
    pub fn diagnostics(&self) -> JaymiResult<DiagnosticsSnapshot> {
        let planner = self.container.resolve::<Planner>()?;
        let database = self.container.resolve::<Database>()?;
        let capabilities = self.container.resolve::<Arc<CapabilityRegistry>>()?;
        let providers = self.container.resolve::<Arc<ProviderRegistry>>()?;
        let tools = self.container.resolve::<Arc<ToolRegistry>>()?;

        Ok(DiagnosticsSnapshot {
            app_state: self.state.clone(),
            planner_healthy: planner.health_check().healthy,
            provider_count: providers.len(),
            tool_count: tools.len(),
            capability_count: capabilities.len(),
            database_connected: database.is_connected(),
        })
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

        shutdown_arc::<ToolRegistry>(&mut self.container)?;
        shutdown_arc::<ProviderRegistry>(&mut self.container)?;
        shutdown_arc::<CapabilityRegistry>(&mut self.container)?;

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

fn shutdown_arc<T>(container: &mut ServiceContainer) -> JaymiResult<()>
where
    T: Lifecycle + 'static,
{
    if let Some(service) = container.take::<Arc<T>>() {
        match Arc::try_unwrap(service) {
            Ok(mut service) => service.shutdown()?,
            Err(_) => {
                // Another handle remains; skip mutable shutdown.
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_reaches_ready_with_empty_registries() {
        let app = Application::boot().unwrap();
        assert!(app.state().is_ready());

        let diagnostics = app.diagnostics().unwrap();
        assert_eq!(diagnostics.app_state.label(), "Ready");
        assert!(diagnostics.planner_healthy);
        assert_eq!(diagnostics.provider_count, 0);
        assert_eq!(diagnostics.tool_count, 0);
        assert_eq!(diagnostics.capability_count, 0);
        assert!(diagnostics.database_connected);

        let names: Vec<_> = app
            .health_reports()
            .iter()
            .map(|report| report.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
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
                "planner",
            ]
        );
    }

    #[test]
    fn boot_registers_services_in_container() {
        let app = Application::boot().unwrap();
        assert!(app.container().contains::<Config>());
        assert!(app.container().contains::<Logger>());
        assert!(app.container().contains::<Database>());
        assert!(app.container().contains::<Planner>());
        assert!(app.container().contains::<Arc<CapabilityRegistry>>());
        assert!(app.container().contains::<Arc<ProviderRegistry>>());
        assert!(app.container().contains::<Arc<ToolRegistry>>());
    }

    #[test]
    fn shutdown_returns_to_starting() {
        let mut app = Application::boot().unwrap();
        app.shutdown().unwrap();
        assert_eq!(app.state().label(), "Starting");
    }
}
