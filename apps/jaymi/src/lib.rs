//! Jaymi application library.
//!
//! Exposes the boot sequence, conversation model, and conversation-first UI.

#![forbid(unsafe_code)]

pub mod boot;
pub mod conversation;
pub mod diagnostics;
pub mod ui;

pub use boot::Application;
pub use conversation::{ChatMessage, Conversation, MessageRole};
pub use diagnostics::DiagnosticsSnapshot;
