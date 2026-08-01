//! Jaymi application library.
//!
//! Exposes the boot sequence and diagnostics types for the desktop binary and
//! unit tests.

#![forbid(unsafe_code)]

pub mod boot;
pub mod diagnostics;
pub mod ui;

pub use boot::Application;
pub use diagnostics::DiagnosticsSnapshot;
