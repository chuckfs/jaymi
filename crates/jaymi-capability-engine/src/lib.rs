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
mod editor;
mod engine;
mod inspector;
mod plan;
mod problems;
mod registry;
mod state;
mod workspace;

pub use capability::Capability;
pub use composition::{
    compose_capabilities, is_multi_capability, research_coding_creation, CapabilityComposition,
};
pub use descriptor::{
    capability_descriptor, catalog_availability, effective_availability, CapabilityAvailability,
    CapabilityCategory, CapabilityDescriptor,
};
pub use discovery::{
    capability_requirements, CapabilityBlocker, CapabilityDiscoveryReport, CapabilityInventory,
    CapabilityRequirements, CapabilityStatus, DiscoveredProvider, DiscoveredTool,
};
pub use editor::{
    EditorBuffer, EditorCursor, EditorLayoutNode, EditorPane, EditorPaneId, EditorPaneTab,
    EditorSession, EditorSessionId, EditorSettings, EditorTab, EditorViewState,
    EditorWorkspaceSnapshot, FoldedRegion, OpenEditors, PersistedEditorPane, PersistedEditorTab,
    SplitDirection, EDITOR_WORKSPACE_SNAPSHOT_VERSION, RECENTLY_OPENED_CAP,
};
pub use engine::{CapabilityEngine, CapabilityEngineApi, CapabilityHealth, CapabilityStats};
pub use inspector::{
    build_inspector_report, inspect_requirements, CapabilityInspectorReport, InspectedCapability,
};
pub use plan::{
    build_plan_step, capability_permission_requirements, CapabilityPlanStep, ExecutionPlan,
    PermissionRequirement,
};
pub use problems::{
    ProblemIssue, ProblemSeverity, ProblemsCollectContext, ProblemsProvider, ProblemsRegistry,
};
pub use registry::CapabilityRegistry;
pub use state::{
    build_explorer_tree, is_editable_coding_extension, CanvasHistoryState, CapabilityState,
    CodingBottomTab, CodingState, CreationState, DiagnosticState, ExplorerNode, ExplorerPending,
    ExplorerState, ExplorerStatus, GeneratedAssetState, GitFileEntry, GitStatusState,
    OpenFileState, ResearchNoteState, ResearchSourceState, ResearchState, SearchPanelState,
    SearchResultEntry, TerminalSessionState, COLLAPSED_BOTTOM_TAB_HEIGHT,
    DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_CONVERSATION_FRACTION, DEFAULT_EXPLORER_WIDTH,
    DEFAULT_WORKSPACE_PANEL_WIDTH, MAX_BOTTOM_PANEL_HEIGHT, MAX_CONVERSATION_FRACTION,
    MAX_EXPLORER_WIDTH, MAX_WORKSPACE_PANEL_WIDTH, MIN_BOTTOM_PANEL_HEIGHT, MIN_CONVERSATION_WIDTH,
    MIN_EXPLORER_WIDTH, MIN_WORKSPACE_PANEL_WIDTH,
};
pub use workspace::{
    capability_workspace, workspace_expansion_for, workspace_panels, WorkspaceEdge,
    WorkspaceExpansion, WorkspaceKind, WorkspacePanel,
};
