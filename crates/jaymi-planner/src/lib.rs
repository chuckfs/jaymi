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

use decision::DecisionEngine;
use jaymi_capabilities::{Capability, CapabilityRegistry};
use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle, UserRequest};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::ToolRegistry;
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
    /// Natural-language response content.
    pub content: String,
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

    /// Process a user request through the full Planner lifecycle.
    ///
    /// Not implemented in the boot-sequence milestone.
    pub fn handle(&self, _request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            return Err(JaymiError::new("planner is not initialized"));
        }
        Ok(PlannerResponse::default())
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

        // Touch decision/reasoning placeholders so the kernel is coherent.
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
    use jaymi_core::Lifecycle;

    fn registries() -> PlannerDeps {
        let mut capabilities = CapabilityRegistry::new();
        capabilities.initialize().unwrap();
        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        PlannerDeps {
            capabilities: Arc::new(capabilities),
            providers: Arc::new(providers),
            tools: Arc::new(tools),
        }
    }

    #[test]
    fn planner_initializes_from_registries() {
        let mut planner = Planner::new(registries());
        planner.initialize().unwrap();
        assert!(planner.health_check().healthy);
        assert!(planner.discover_capabilities().is_empty());
        assert_eq!(planner.provider_count(), 0);
        assert_eq!(planner.tool_count(), 0);
    }

    #[test]
    fn planner_rejects_uninitialized_registries() {
        let deps = PlannerDeps {
            capabilities: Arc::new(CapabilityRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            tools: Arc::new(ToolRegistry::new()),
        };
        let mut planner = Planner::new(deps);
        assert!(planner.initialize().is_err());
    }
}
