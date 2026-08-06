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
pub mod diagnostics;
pub mod editor_workspace;
pub mod experience;
pub mod monaco_host;
mod problems;
pub mod quick_open;
#[allow(unsafe_code)] // AppKit / DWM bridges for OS accent color.
mod system_accent;
pub mod theme;
pub mod ui;

pub use boot::Application;
pub use coding_breadcrumb::{
    apply_breadcrumb_reveal, breadcrumb_action, breadcrumbs_from_coding_state,
    truncate_breadcrumbs, BreadcrumbAction, BreadcrumbKind, BreadcrumbSegment,
};
pub use coding_quick_actions::{
    dispatch_quick_action, layout_quick_actions, QuickAction, QuickActionIntent, QuickActionLayout,
};
pub use coding_workspace::{
    build_coding_diagnostics_view, coding_panel_lines, coding_shell_summary,
    CodingDiagnosticsSection, CodingDiagnosticsView, CodingShellEvent, LastPlannerActivity,
};
pub use command_dispatch::{dispatch_command, CommandDispatchEffect};
pub use diagnostics::{DiagnosticsSnapshot, OperationalStatus, SubsystemStatus};
pub use experience::{ConversationTurn, ExperienceSession};
pub use theme::{inset, radius, space, stroke, type_size, Theme, ThemeMode};
