//! Centralized Memory Engine for Jaymi.
//!
//! Memory is intentional, editable, and provider-independent.
//! Architecture: Planner → Memory Engine → Memory Store (SQLite / knowledge layer).
//!
//! The Planner never accesses memory storage directly.
//! Conversation transcripts are persisted separately and remain isolated.

#![forbid(unsafe_code)]

mod conversation;
mod context;
mod engine;
mod personal;
mod project;
mod promotion;
mod store;
mod types;

pub use conversation::{
    AppendMessageRequest, Conversation, ConversationAttachment, ConversationAttachmentInput,
    ConversationMeta, ConversationMessage, ConversationReference, ConversationReferenceInput,
    ConversationStatus, CreateConversationRequest, MessageRole,
};
pub use context::{
    AssembleContextRequest, AssembledMemoryContext, MemoryRelevanceKind, RelevantMemory,
    DEFAULT_CONTEXT_LIMIT,
};
pub use engine::{MemoryEngine, MemoryEngineApi, MemoryHealth, MemoryStats};
pub use personal::{
    CreatePersonalMemoryRequest, PersonalContext, PersonalMemoryKind, UpdatePersonalMemoryRequest,
};
pub use project::{
    slugify_project_name, ProjectContext, ProjectMemoryKind, ProjectMeta, RegisterProjectRequest,
    StoreProjectMemoryRequest,
};
pub use promotion::{
    is_upward_promotion, next_scope, scope_rank, PromoteMemoryRequest, PromotionAskDecision,
    PromotionSuggestQuery, PromotionSuggestion,
};
pub use store::{InMemoryMemoryStore, MemoryStore, SqliteMemoryStore};
pub use types::{
    ArchiveConversationRequest, ConversationArchive, MemoryQuery, MemoryRecord, MemoryScope,
    MemoryStatus, StoreMemoryRequest,
};
