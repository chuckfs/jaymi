//! Capability Engine — register, resolve, validate, and describe abilities.

use std::sync::Mutex;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

use crate::composition::{compose_capabilities, CapabilityComposition};
use crate::descriptor::{
    capability_descriptor, catalog_availability, CapabilityAvailability, CapabilityDescriptor,
};
use crate::discovery::{
    assess_capability, CapabilityDiscoveryReport, CapabilityInventory, CapabilityStatus,
};
use crate::inspector::{build_inspector_report, CapabilityInspectorReport};
use crate::plan::{build_plan_step, ExecutionPlan};
use crate::registry::CapabilityRegistry;
use crate::Capability;

const NAME: &str = "capability_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
    "context_engine",
];

/// Health snapshot for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityHealth {
    /// Lifecycle initialized.
    pub initialized: bool,
    /// Number of registered capabilities.
    pub registered: usize,
}

/// Registry statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityStats {
    /// Registered capability count.
    pub registered: usize,
    /// Currently available after last discovery (when known).
    pub available: Option<usize>,
    /// Currently unavailable after last discovery (when known).
    pub unavailable: Option<usize>,
}

/// Planner-facing Capability Engine API.
///
/// Capabilities describe what Jaymi knows how to do. They never execute work.
pub trait CapabilityEngineApi: Send + Sync {
    /// Register a capability. Idempotent for duplicates.
    fn register(&self, capability: Capability) -> JaymiResult<()>;

    /// Resolve a capability by stable id when it is registered.
    fn resolve(&self, id: &str) -> JaymiResult<Option<CapabilityDescriptor>>;

    /// Resolve metadata for a known capability enum value when registered.
    fn resolve_capability(
        &self,
        capability: Capability,
    ) -> JaymiResult<Option<CapabilityDescriptor>>;

    /// Describe a capability's metadata (catalog description; registration optional).
    fn describe(&self, capability: Capability) -> CapabilityDescriptor;

    /// Validate catalog availability for planning (without live inventory).
    ///
    /// Returns Ready / Experimental / Planned when registered, Unavailable when
    /// the engine is down or the capability is not registered, and Unknown for
    /// unrecognized ids. Prefer [`Self::assess`] / [`Self::plan`] when inventory
    /// is known — those apply effective availability.
    fn validate(&self, capability: Capability) -> CapabilityAvailability;

    /// Validate a stable id (unknown ids → [`CapabilityAvailability::Unknown`]).
    fn validate_id(&self, id: &str) -> CapabilityAvailability;

    /// Returns true when the capability is registered.
    fn contains(&self, capability: Capability) -> bool;

    /// List registered capabilities in registration order.
    fn list(&self) -> Vec<Capability>;

    /// List descriptors for registered capabilities.
    fn list_descriptors(&self) -> Vec<CapabilityDescriptor>;

    /// Build an execution plan for the requested capabilities (no execution).
    ///
    /// Uses an empty inventory (preferred tools/providers only). Prefer
    /// [`Self::plan`] when tools and providers are known.
    fn build_execution_plan(&self, capabilities: &[Capability]) -> JaymiResult<ExecutionPlan> {
        self.plan(capabilities, &CapabilityInventory::default(), None)
    }

    /// Build a structured execution plan from capabilities and inventory.
    ///
    /// Plans include capability, required tools, required providers, and
    /// required permissions. Nothing is executed.
    fn plan(
        &self,
        capabilities: &[Capability],
        inventory: &CapabilityInventory,
        goal: Option<&str>,
    ) -> JaymiResult<ExecutionPlan>;

    /// Compose independent capabilities into one execution plan.
    ///
    /// Capabilities are never merged — each becomes its own plan step with
    /// its own tools, providers, and permissions. Duplicates are dropped
    /// while preserving order.
    fn compose(
        &self,
        capabilities: &[Capability],
        inventory: &CapabilityInventory,
        goal: Option<&str>,
    ) -> JaymiResult<ExecutionPlan> {
        let ordered = compose_capabilities(capabilities)?;
        self.plan(&ordered, inventory, goal)
    }

    /// Compose from a [`CapabilityComposition`] value.
    fn compose_plan(
        &self,
        composition: &CapabilityComposition,
        inventory: &CapabilityInventory,
    ) -> JaymiResult<ExecutionPlan> {
        self.compose(
            composition.as_slice(),
            inventory,
            composition.goal.as_deref(),
        )
    }

    /// Inspect registered vs active capabilities for developers.
    ///
    /// Includes workspace associations and required tools/providers so
    /// diagnostics accurately reflect runtime capability state.
    fn inspect(&self, inventory: &CapabilityInventory) -> JaymiResult<CapabilityInspectorReport> {
        let discovery = self.discover(inventory)?;
        let registered = self.list();
        Ok(build_inspector_report(&registered, &discovery))
    }

    /// Discover what Jaymi can currently execute given tools and providers.
    ///
    /// Assesses the full catalog. Ready / Experimental capabilities with
    /// fulfilled requirements become executable; Planned capabilities stay
    /// visible but not executable; blocked ones are Unavailable.
    fn discover(&self, inventory: &CapabilityInventory) -> JaymiResult<CapabilityDiscoveryReport>;

    /// Assess one capability against registration and a runtime inventory.
    fn assess(
        &self,
        capability: Capability,
        inventory: &CapabilityInventory,
    ) -> JaymiResult<CapabilityStatus>;

    /// Number of registered capabilities.
    fn len(&self) -> usize;

    /// True when no capabilities are registered.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True after successful initialization.
    fn is_initialized(&self) -> bool;

    /// Health snapshot.
    fn health(&self) -> CapabilityHealth;

    /// Stats snapshot.
    fn stats(&self) -> CapabilityStats;
}

/// Capability Engine — first-class subsystem for abstract abilities.
#[derive(Debug)]
pub struct CapabilityEngine {
    inner: Mutex<CapabilityRegistry>,
}

impl CapabilityEngine {
    /// Create an empty, uninitialized engine.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CapabilityRegistry::new()),
        }
    }

    fn with_registry<R>(&self, f: impl FnOnce(&CapabilityRegistry) -> R) -> JaymiResult<R> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("capability engine lock poisoned"))?;
        Ok(f(&guard))
    }

    fn with_registry_mut<R>(
        &self,
        f: impl FnOnce(&mut CapabilityRegistry) -> JaymiResult<R>,
    ) -> JaymiResult<R> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("capability engine lock poisoned"))?;
        f(&mut guard)
    }

    fn ensure_ready(&self) -> JaymiResult<()> {
        if self.is_initialized() {
            Ok(())
        } else {
            Err(JaymiError::new("capability engine is not initialized"))
        }
    }
}

impl Default for CapabilityEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityEngineApi for CapabilityEngine {
    fn register(&self, capability: Capability) -> JaymiResult<()> {
        self.with_registry_mut(|registry| registry.register(capability))?;
        jaymi_logging::info(
            "capability",
            format!("registered capability id={}", capability.id()),
        );
        Ok(())
    }

    fn resolve(&self, id: &str) -> JaymiResult<Option<CapabilityDescriptor>> {
        self.ensure_ready()?;
        self.with_registry(|registry| {
            registry
                .resolve(id)
                .map(capability_descriptor)
        })
    }

    fn resolve_capability(
        &self,
        capability: Capability,
    ) -> JaymiResult<Option<CapabilityDescriptor>> {
        self.ensure_ready()?;
        self.with_registry(|registry| {
            if registry.contains(capability) {
                Some(capability_descriptor(capability))
            } else {
                None
            }
        })
    }

    fn describe(&self, capability: Capability) -> CapabilityDescriptor {
        capability_descriptor(capability)
    }

    fn validate(&self, capability: Capability) -> CapabilityAvailability {
        match self.with_registry(|registry| {
            if !registry.is_initialized() {
                CapabilityAvailability::Unavailable
            } else if registry.contains(capability) {
                catalog_availability(capability)
            } else {
                CapabilityAvailability::Unavailable
            }
        }) {
            Ok(availability) => availability,
            Err(_) => CapabilityAvailability::Unavailable,
        }
    }

    fn validate_id(&self, id: &str) -> CapabilityAvailability {
        let trimmed = id.trim();
        let Some(capability) = Capability::from_id(trimmed) else {
            return CapabilityAvailability::Unknown;
        };
        self.validate(capability)
    }

    fn contains(&self, capability: Capability) -> bool {
        self.with_registry(|registry| registry.contains(capability))
            .unwrap_or(false)
    }

    fn list(&self) -> Vec<Capability> {
        self.with_registry(|registry| registry.list())
            .unwrap_or_default()
    }

    fn list_descriptors(&self) -> Vec<CapabilityDescriptor> {
        self.with_registry(|registry| registry.list_descriptors())
            .unwrap_or_default()
    }

    fn build_execution_plan(&self, capabilities: &[Capability]) -> JaymiResult<ExecutionPlan> {
        self.plan(capabilities, &CapabilityInventory::default(), None)
    }

    fn plan(
        &self,
        capabilities: &[Capability],
        inventory: &CapabilityInventory,
        goal: Option<&str>,
    ) -> JaymiResult<ExecutionPlan> {
        self.ensure_ready()?;
        if capabilities.is_empty() {
            return Err(JaymiError::new(
                "plan requires at least one capability",
            ));
        }
        let registered = self.list();
        let engine_ready = true;
        let mut steps = Vec::with_capacity(capabilities.len());
        for capability in capabilities {
            let status = assess_capability(
                *capability,
                registered.contains(capability),
                engine_ready,
                inventory,
            );
            steps.push(build_plan_step(
                *capability,
                status.availability,
                inventory,
            ));
        }
        let plan = ExecutionPlan {
            goal: goal.map(str::to_string),
            steps,
        };
        jaymi_logging::info("capability", plan.summary());
        Ok(plan)
    }

    fn discover(&self, inventory: &CapabilityInventory) -> JaymiResult<CapabilityDiscoveryReport> {
        self.ensure_ready()?;
        let registered = self.list();
        let engine_ready = true;
        let mut available = Vec::new();
        let mut unavailable = Vec::new();

        for capability in Capability::all() {
            let is_registered = registered.contains(capability);
            let status =
                assess_capability(*capability, is_registered, engine_ready, inventory);
            if status.is_available() {
                available.push(status);
            } else {
                unavailable.push(status);
            }
        }

        let report = CapabilityDiscoveryReport {
            available,
            unavailable,
        };
        jaymi_logging::info("capability", format!("discovery {}", report.summary()));
        Ok(report)
    }

    fn assess(
        &self,
        capability: Capability,
        inventory: &CapabilityInventory,
    ) -> JaymiResult<CapabilityStatus> {
        let engine_ready = self.is_initialized();
        if !engine_ready {
            return Ok(assess_capability(capability, false, false, inventory));
        }
        let registered = self.contains(capability);
        Ok(assess_capability(
            capability,
            registered,
            true,
            inventory,
        ))
    }

    fn len(&self) -> usize {
        self.with_registry(|registry| registry.len()).unwrap_or(0)
    }

    fn is_initialized(&self) -> bool {
        self.with_registry(|registry| registry.is_initialized())
            .unwrap_or(false)
    }

    fn health(&self) -> CapabilityHealth {
        CapabilityHealth {
            initialized: self.is_initialized(),
            registered: self.len(),
        }
    }

    fn stats(&self) -> CapabilityStats {
        CapabilityStats {
            registered: self.len(),
            available: None,
            unavailable: None,
        }
    }
}

impl Lifecycle for CapabilityEngine {
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
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("capability engine lock poisoned"))?;
        guard.mark_initialized();
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.is_initialized(),
            self.is_initialized(),
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("capability engine lock poisoned"))?;
        guard.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CapabilityBlocker, DiscoveredProvider, DiscoveredTool,
    };

    #[test]
    fn register_resolve_validate_and_plan() {
        let mut engine = CapabilityEngine::new();
        assert_eq!(
            engine.validate(Capability::Search),
            CapabilityAvailability::Unavailable
        );
        engine.initialize().unwrap();

        engine.register(Capability::Search).unwrap();
        engine.register(Capability::ReadDocuments).unwrap();
        engine.register(Capability::Search).unwrap();

        assert_eq!(engine.len(), 2);
        assert!(engine.contains(Capability::Search));
        assert!(!engine.contains(Capability::Index));

        let search = engine.resolve("search").unwrap().expect("search");
        assert_eq!(search.id, "search");
        assert_eq!(search.name, "Search");
        assert!(search.description.contains("knowledge"));
        assert!(!search.requires_internet);
        assert!(search.offline_capable);
        assert_eq!(search.availability, CapabilityAvailability::Ready);

        assert_eq!(
            engine.validate(Capability::Search),
            CapabilityAvailability::Ready
        );
        assert_eq!(
            engine.validate(Capability::Index),
            CapabilityAvailability::Unavailable
        );
        assert_eq!(
            engine.validate(Capability::Chat),
            CapabilityAvailability::Unavailable
        );
        assert_eq!(
            engine.validate_id("not-a-capability"),
            CapabilityAvailability::Unknown
        );

        let catalog = engine.describe(Capability::Internet);
        assert_eq!(catalog.id, "internet");
        assert!(catalog.requires_internet);
        assert_eq!(catalog.availability, CapabilityAvailability::Planned);

        let plan = engine
            .build_execution_plan(&[Capability::Search, Capability::Index])
            .unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert!(!plan.is_ready());
        assert_eq!(plan.unavailable().len(), 2); // Search missing inventory → Unavailable; Index unregistered
        assert_eq!(
            plan.steps[0].availability,
            CapabilityAvailability::Unavailable
        );
        assert_eq!(
            plan.steps[1].availability,
            CapabilityAvailability::Unavailable
        );
        assert_eq!(
            plan.steps[0].required_tools,
            vec![
                "search_files".to_string(),
                "search_knowledge".to_string(),
                "search_project_knowledge".to_string(),
            ]
        );
        assert!(plan
            .required_permissions()
            .iter()
            .any(|permission| permission.category == "filesystem"
                && permission.action == "read"));

        let inventory = CapabilityInventory {
            tools: vec![
                DiscoveredTool {
                    id: "search_files".into(),
                    capabilities: vec![Capability::Search],
                },
                DiscoveredTool {
                    id: "read_file".into(),
                    capabilities: vec![Capability::ReadDocuments],
                },
            ],
            providers: vec![DiscoveredProvider {
                id: "filesystem".into(),
                capabilities: vec![Capability::Search, Capability::ReadDocuments],
            }],
        };
        let ready = engine
            .plan(
                &[Capability::Search, Capability::ReadDocuments],
                &inventory,
                None,
            )
            .unwrap();
        assert!(ready.is_ready());
        assert!(ready.is_executable());
        assert!(ready.summary().contains("executable"));
        assert_eq!(ready.steps[0].availability, CapabilityAvailability::Ready);
    }

    #[test]
    fn plan_is_deterministic_for_coding_capability() {
        let mut engine = CapabilityEngine::new();
        engine.initialize().unwrap();
        engine.register(Capability::Code).unwrap();

        let inventory = CapabilityInventory {
            tools: vec![],
            providers: vec![DiscoveredProvider {
                id: "filesystem".into(),
                capabilities: vec![Capability::Search, Capability::ReadDocuments],
            }],
        };

        let first = engine
            .plan(
                &[Capability::Code],
                &inventory,
                Some("Help me build an app."),
            )
            .unwrap();
        let second = engine
            .plan(
                &[Capability::Code],
                &inventory,
                Some("Help me build an app."),
            )
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.goal.as_deref(), Some("Help me build an app."));
        assert_eq!(first.steps.len(), 1);
        let step = &first.steps[0];
        assert_eq!(step.capability, Capability::Code);
        assert_eq!(
            step.availability,
            CapabilityAvailability::Unavailable
        );
        assert_eq!(
            step.required_tools,
            vec![
                "editor".to_string(),
                "language_server".to_string(),
                "terminal".to_string(),
                "git".to_string()
            ]
        );
        assert_eq!(
            step.required_providers,
            vec!["filesystem".to_string(), "git".to_string()]
        );
        assert_eq!(step.required_permissions.len(), 3);
        assert!(step
            .required_permissions
            .iter()
            .any(|permission| permission.label() == "filesystem:write"));
        assert!(step
            .required_permissions
            .iter()
            .any(|permission| permission.label() == "terminal:execute"));
        assert!(!first.is_executable());
        assert!(!first.is_ready());
        assert!(first.render().contains("Goal: Help me build an app."));
    }

    #[test]
    fn discover_separates_executable_planned_and_unavailable() {
        let mut engine = CapabilityEngine::new();
        engine.initialize().unwrap();
        for capability in Capability::all() {
            engine.register(*capability).unwrap();
        }

        let inventory = CapabilityInventory {
            tools: vec![
                DiscoveredTool {
                    id: "search_files".into(),
                    capabilities: vec![Capability::Search],
                },
                DiscoveredTool {
                    id: "read_file".into(),
                    capabilities: vec![Capability::ReadDocuments],
                },
            ],
            providers: vec![
                DiscoveredProvider {
                    id: "filesystem".into(),
                    capabilities: vec![Capability::Search, Capability::ReadDocuments],
                },
                DiscoveredProvider {
                    id: "ocr.placeholder".into(),
                    capabilities: vec![Capability::Ocr, Capability::Vision],
                },
            ],
        };

        let report = engine.discover(&inventory).unwrap();
        assert!(report
            .available
            .iter()
            .any(|status| status.descriptor.id == "search"));
        assert!(report
            .available
            .iter()
            .any(|status| status.descriptor.id == "read_documents"));
        // OCR remains Planned — placeholder provider must not claim executability.
        let ocr = report.get("ocr").expect("ocr status");
        assert!(!ocr.is_available());
        assert_eq!(ocr.availability, CapabilityAvailability::Planned);
        assert!(ocr.registered);

        let vision = report.get("vision").expect("vision status");
        assert!(!vision.is_available());
        assert_eq!(vision.availability, CapabilityAvailability::Unavailable);
        assert!(vision.blockers.contains(&CapabilityBlocker::MissingTool));
        assert!(vision.requirements.requires_tool);
        assert!(vision.fulfilling_tools.is_empty());
        assert!(vision
            .fulfilling_providers
            .iter()
            .any(|id| id == "ocr.placeholder"));

        let search = report.get("search").expect("search");
        assert!(search.is_available());
        assert_eq!(search.availability, CapabilityAvailability::Ready);
        assert_eq!(search.fulfilling_tools, vec!["search_files".to_string()]);
        assert!(search
            .fulfilling_providers
            .iter()
            .any(|id| id == "filesystem"));

        let chat = report.get("chat").expect("chat");
        assert!(!chat.is_available());
        assert_eq!(chat.availability, CapabilityAvailability::Planned);
        assert!(chat.blockers.is_empty());
        assert!(chat.registered);

        let code = report.get("code").expect("code");
        assert!(!code.is_available());
        assert_eq!(code.availability, CapabilityAvailability::Unavailable);
        assert!(code.blockers.contains(&CapabilityBlocker::MissingTool));
        assert!(report.planned_count() > 0);
    }

    #[test]
    fn planned_capabilities_remain_registered_and_plannable() {
        let mut engine = CapabilityEngine::new();
        engine.initialize().unwrap();
        engine.register(Capability::Internet).unwrap();
        engine.register(Capability::GenerateImages).unwrap();

        assert_eq!(
            engine.validate(Capability::Internet),
            CapabilityAvailability::Planned
        );
        let plan = engine
            .build_execution_plan(&[Capability::Internet, Capability::GenerateImages])
            .unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].availability, CapabilityAvailability::Planned);
        assert_eq!(plan.steps[1].availability, CapabilityAvailability::Planned);
        assert!(!plan.is_ready());
        assert!(!plan.is_executable());
    }
}
