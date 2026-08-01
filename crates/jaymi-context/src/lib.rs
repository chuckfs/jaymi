//! Context Engine for Jaymi.
//!
//! Determines what Jaymi should know before responding. Context is assembled
//! dynamically and never assumed.

#![forbid(unsafe_code)]

use jaymi_core::{JaymiResult, UserRequest};

/// Sources that may contribute to assembled context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    ActiveProject,
    PreviousConversation,
    Files,
    SearchResults,
    GitStatus,
    TerminalOutput,
    Notes,
    Messages,
    BrowserHistory,
    RetrievedMemories,
}

/// Assembled context for a single request.
#[derive(Debug, Default, Clone)]
pub struct ContextBundle {
    pub sources: Vec<ContextSource>,
}

/// Context Engine skeleton.
#[derive(Debug, Default)]
pub struct ContextEngine;

impl ContextEngine {
    /// Build only the context required for the current request.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn assemble(&self, _request: &UserRequest) -> JaymiResult<ContextBundle> {
        Ok(ContextBundle::default())
    }
}
