//! Jaymi application library.
//!
//! Exposes the boot sequence and diagnostics types for the desktop binary and
//! tests.

// wry / raw-window-handle paths may use `unsafe` internally; keep app logic safe.
#![deny(unsafe_code)]

pub mod boot;
pub mod coding_workspace;
pub mod diagnostics;
pub mod experience;
pub mod monaco_host;
pub mod ui;

pub use boot::Application;
pub use coding_workspace::{
    build_coding_diagnostics_view, coding_panel_lines, coding_shell_summary,
    CodingDiagnosticsSection, CodingDiagnosticsView, CodingShellEvent, LastPlannerActivity,
};
pub use diagnostics::{DiagnosticsSnapshot, OperationalStatus, SubsystemStatus};
pub use experience::{ConversationTurn, ExperienceSession};
