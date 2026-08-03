//! Capability Engine for Jaymi.
//!
//! Capabilities are abstract abilities. They describe what Jaymi knows how to
//! do — never how work is performed.
//!
//! Architecture: Planner → Capability Engine → Execution Plan
//!
//! Tools and providers implement capabilities. The Capability Engine does not
//! execute work.

#![forbid(unsafe_code)]

mod capability;
mod composition;
mod descriptor;
mod discovery;
mod engine;
mod inspector;
mod plan;
mod registry;
mod state;
mod workspace;

pub use capability::Capability;
pub use composition::{
    compose_capabilities, is_multi_capability, research_coding_creation, CapabilityComposition,
};
pub use descriptor::{
    capability_descriptor, CapabilityAvailability, CapabilityCategory, CapabilityDescriptor,
};
pub use discovery::{
    capability_requirements, CapabilityBlocker, CapabilityDiscoveryReport, CapabilityInventory,
    CapabilityRequirements, CapabilityStatus, DiscoveredProvider, DiscoveredTool,
};
pub use engine::{CapabilityEngine, CapabilityEngineApi, CapabilityHealth, CapabilityStats};
pub use inspector::{
    build_inspector_report, inspect_requirements, CapabilityInspectorReport, InspectedCapability,
};
pub use plan::{
    build_plan_step, capability_permission_requirements, CapabilityPlanStep, ExecutionPlan,
    PermissionRequirement,
};
pub use registry::CapabilityRegistry;
pub use state::{
    CanvasHistoryState, CapabilityState, CodingState, CreationState, DiagnosticState,
    GeneratedAssetState, OpenFileState, ResearchNoteState, ResearchSourceState, ResearchState,
    TerminalSessionState,
};
pub use workspace::{
    capability_workspace, workspace_expansion_for, workspace_panels, WorkspaceEdge,
    WorkspaceExpansion, WorkspaceKind, WorkspacePanel,
};
