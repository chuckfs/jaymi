//! Jaymi application library.
//!
//! Exposes the boot sequence and diagnostics types for the desktop binary and
//! tests.

#![deny(unsafe_code)]

pub mod boot;
pub mod coding_breadcrumb;
pub mod coding_quick_actions;
pub mod coding_workspace;
pub mod command_dispatch;
pub mod command_palette;
pub mod context_maintenance;
pub mod context_session;
pub mod conversation_ux;
pub mod diagnostics;
pub mod editor_workspace;
pub mod execution_diagnostics;
pub mod experience;
pub mod monaco_host;
mod problems;
pub mod performance_diagnostics;
pub mod quick_open;
pub mod session_cache;
pub mod settings_workspace;
pub mod workspace_diagnostics;
#[allow(unsafe_code)] // AppKit / DWM bridges for OS accent color.
mod system_accent;
pub mod theme;
pub mod ui;

pub use conversation_ux::{
    action_accessibility_label, caret_blink_on, caret_glyph, display_content, loading_opacity,
    progress_accessibility_label, show_typing_indicator, smooth_streaming_text, turn_actions,
    ConversationTurnActions,
};
pub use boot::{Application, BeginGeneration, PumpGeneration};
pub use coding_breadcrumb::{
    apply_breadcrumb_reveal, breadcrumb_action, breadcrumbs_from_coding_state,
    truncate_breadcrumbs, BreadcrumbAction, BreadcrumbKind, BreadcrumbSegment,
};
pub use coding_quick_actions::{
    dispatch_quick_action, layout_quick_actions, QuickAction, QuickActionEffect, QuickActionLayout,
};
pub use coding_workspace::{
    build_coding_diagnostics_view, coding_panel_lines, coding_shell_summary,
    CodingDiagnosticsSection, CodingDiagnosticsView, CodingShellEvent, LastPlannerActivity,
};
pub use execution_diagnostics::{
    build_execution_inspection, ExecutionInspection, EXECUTION_INSPECTION_SECTION_TITLES,
};
pub use command_dispatch::{dispatch_command, CommandDispatchEffect};
pub use diagnostics::{
    DiagnosticsSnapshot, LastReasoningTurn, OperationalStatus, SubsystemStatus,
};
pub use experience::{ConversationTurn, ExperienceSession};
pub use context_maintenance::{
    ContextMaintenance, MaintenanceKind, MaintenanceUiUpdate,
};
pub use performance_diagnostics::{PerformanceDashboard, PerformanceTimelineRow};
pub use workspace_diagnostics::{
    MaintenanceStatusRow, SnapshotFreshnessRow, WorkspaceDiagnosticsInput,
    WorkspaceDiagnosticsReport,
};
pub use settings_workspace::{
    render_settings_workspace, ReasoningConnectionStatus, ReasoningSettingsModel,
    ReasoningSettingsProvider, ReasoningSettingsSnapshot, SettingsCategory, SettingsWorkspaceEvent,
    SettingsWorkspaceState,
};
