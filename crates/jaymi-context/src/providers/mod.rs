//! Built-in [`crate::ContextProvider`] implementations.
//!
//! Each provider owns its subsystem dependency (or reads session inputs) and
//! may decline when it has nothing relevant for the request.

mod conversation;
mod diagnostics;
mod editor;
mod memory;
mod permission;
mod project;
mod search;
mod workspace;

pub use conversation::ConversationProvider;
pub use diagnostics::DiagnosticsProvider;
pub use editor::EditorProvider;
pub use memory::MemoryProvider;
pub use permission::PermissionProvider;
pub use project::ProjectProvider;
pub use search::SearchProvider;
pub use workspace::WorkspaceProvider;

use std::sync::Arc;

use jaymi_memory_engine::MemoryEngineApi;
use jaymi_project_engine::ProjectEngineApi;
use jaymi_search::SearchEngineApi;

use crate::ContextProvider;

/// Dependencies used to construct the default provider set.
#[derive(Clone)]
pub struct ProviderDeps {
    /// Memory Engine.
    pub memory: Arc<dyn MemoryEngineApi>,
    /// Project Engine.
    pub projects: Arc<dyn ProjectEngineApi>,
    /// Search Engine (wired for future scoped use; providers never execute search tools).
    pub search: Arc<dyn SearchEngineApi>,
}

/// Default provider set registered by [`crate::ContextEngine::bind_sources`].
pub fn default_providers(deps: ProviderDeps) -> Vec<Arc<dyn ContextProvider>> {
    vec![
        Arc::new(ConversationProvider::new(Arc::clone(&deps.memory))),
        Arc::new(ProjectProvider::new(Arc::clone(&deps.projects))),
        Arc::new(WorkspaceProvider),
        Arc::new(EditorProvider),
        Arc::new(SearchProvider::new(deps.search)),
        Arc::new(MemoryProvider::new(deps.memory)),
        Arc::new(DiagnosticsProvider),
        Arc::new(PermissionProvider),
    ]
}
