//! Jaymi application library.
//!
//! Exposes the boot sequence and diagnostics types for the desktop binary and
//! tests.

// wry / raw-window-handle paths may use `unsafe` internally; keep app logic safe.
#![deny(unsafe_code)]

pub mod boot;
pub mod coding_workspace;
pub mod command_dispatch;
pub mod command_palette;
pub mod diagnostics;
pub mod editor_workspace;
pub mod experience;
pub mod monaco_host;
mod problems;
pub mod quick_open;
pub mod ui;

pub use boot::Application;
pub use coding_workspace::{
    build_coding_diagnostics_view, coding_panel_lines, coding_shell_summary,
    CodingDiagnosticsSection, CodingDiagnosticsView, CodingShellEvent, LastPlannerActivity,
};
pub use command_dispatch::{dispatch_command, CommandDispatchEffect};
pub use diagnostics::{DiagnosticsSnapshot, OperationalStatus, SubsystemStatus};
pub use experience::{ConversationTurn, ExperienceSession};
