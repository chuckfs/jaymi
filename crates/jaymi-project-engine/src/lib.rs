//! Project Engine for Jaymi.
//!
//! Projects are first-class persistent workspaces.
//! Architecture: Planner → Project Engine → Project Store.
//!
//! The Project Engine is the sole owner of project identity (create, delete,
//! list, lookup). Memory, Search, and Knowledge reference projects only by
//! `project_id`. The Planner requests one assembled [`ProjectContext`].

#![forbid(unsafe_code)]

mod context;
mod engine;
mod knowledge;
mod store;
mod types;

pub use context::{
    ProjectArchitectureItem, ProjectContext, ProjectContextSources, ProjectConversationEntry,
    ProjectDecisionEntry, ProjectFileEntry, ProjectParsedContent, ProjectRecentWorkItem,
    ProjectSearchIndex, ProjectTaskEntry, DEFAULT_ARCHITECTURE_LIMIT, DEFAULT_CONVERSATION_LIMIT,
    DEFAULT_CONVERSATION_MESSAGE_LIMIT, DEFAULT_IMPORTANT_DOC_LIMIT, DEFAULT_INDEXED_FILE_LIMIT,
    DEFAULT_PARSED_CONTENT_LIMIT, DEFAULT_RECENT_LIMIT,
};
pub use engine::{ProjectEngine, ProjectEngineApi};
pub use knowledge::{ProjectKnowledgeHit, ProjectKnowledgeKind, ProjectKnowledgeQuery};
pub use store::{InMemoryProjectStore, ProjectStore, SqliteProjectStore};
pub use types::{
    slugify_project_name, CreateProjectRequest, Project, ProjectHealth, ProjectStats,
    ProjectStatus, ProjectType,
};
