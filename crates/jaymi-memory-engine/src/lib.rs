//! Centralized Memory Engine for Jaymi.
//!
//! Memory is intentional, editable, and provider-independent.
//! Architecture: Planner → Memory Engine → Memory Store (SQLite / knowledge layer).
//!
//! The Planner never accesses memory storage directly.
//! Conversation transcripts are persisted separately and remain isolated.
//!
//! Project identity is owned exclusively by the Project Engine. Memory
//! references projects only by `project_id` (see [`ProjectMemoryBundle`]).

#![forbid(unsafe_code)]

mod context;
mod conversation;
mod engine;
mod personal;
mod project;
mod promotion;
mod store;
mod types;

pub use context::{
    AssembleContextRequest, AssembledMemoryContext, MemoryRelevanceKind, RelevantMemory,
    DEFAULT_CONTEXT_LIMIT,
};
pub use conversation::{
    AppendMessageRequest, Conversation, ConversationAttachment, ConversationAttachmentInput,
    ConversationMessage, ConversationMeta, ConversationReference, ConversationReferenceInput,
    ConversationStatus, CreateConversationRequest, MessageRole,
};
pub use engine::{MemoryEngine, MemoryEngineApi, MemoryHealth, MemoryStats};
pub use personal::{
    CreatePersonalMemoryRequest, PersonalContext, PersonalMemoryKind, UpdatePersonalMemoryRequest,
};
pub use project::{
    encode_decision_metadata, project_decision_from_record, slugify_project_name,
    ListProjectDecisionsQuery, ProjectDecision, ProjectMemoryBundle, ProjectMemoryKind,
    StoreProjectDecisionRequest, StoreProjectMemoryRequest,
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
