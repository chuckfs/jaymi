//! Jaymi application library.
//!
//! Exposes the boot sequence and diagnostics types for the desktop binary and
//! tests.

#![forbid(unsafe_code)]

pub mod boot;
pub mod coding_workspace;
pub mod diagnostics;
pub mod experience;
pub mod ui;

pub use boot::Application;
pub use coding_workspace::{coding_panel_lines, coding_shell_summary};
pub use diagnostics::{DiagnosticsSnapshot, OperationalStatus, SubsystemStatus};
pub use experience::{ConversationTurn, ExperienceSession};
