//! Shared types and core architecture primitives for Jaymi.
//!
//! Jaymi is an intelligent environment that coordinates models, tools, and
//! providers through a single conversational interface. This crate holds the
//! foundational types shared across every subsystem.

#![forbid(unsafe_code)]

pub mod citation;
pub mod coding_action;
pub mod collection_names;
pub mod container;
pub mod document;
pub mod error;
pub mod file_entry;
pub mod health;
pub mod id;
pub mod intent;
pub mod lifecycle;
pub mod lsp;
pub mod preview;
pub mod request;
pub mod result;
pub mod search;
pub mod state;

pub use citation::{format_citations, Citation};
pub use coding_action::CodingAction;
pub use collection_names::{is_known_collection_name, parse_collection_slug, COLLECTION_SLUGS};
pub use container::ServiceContainer;
pub use document::{Document, DocumentMetadata, FileType};
pub use error::JaymiError;
pub use file_entry::{EntryType, FileEntry};
pub use health::HealthReport;
pub use id::EntityId;
pub use intent::IntentId;
pub use lifecycle::Lifecycle;
pub use lsp::{
    LspCompletionItem, LspDiagnostic, LspHover, LspLocation, LspOperation, LspPosition, LspRange,
    LspRequest, LspTextEdit,
};
pub use preview::{
    truncate_preview_body, ActionPreview, PreviewKind, PREVIEW_MAX_BODY_CHARS,
    PREVIEW_MAX_BODY_LINES,
};
pub use request::{
    DeletionMethod, DiscoveryQueryKind, GitCommitSummary, GitOperation, GitPathStatus, GitRequest,
    ManagePathRequest, ProjectKnowledgeRequest, TerminalOperation, TerminalRequest, UserRequest,
    WriteFileRequest,
};
pub use result::JaymiResult;
pub use search::{MetadataFilters, SearchRequest};
pub use state::AppState;
