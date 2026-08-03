//! Memory scopes, records, and request types.
//!
//! Memory is intentional and structured. Scopes remain independent.

use jaymi_core::EntityId;

/// Four independent memory scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryScope {
    /// Ephemeral scratch for the active turn / session.
    Working,
    /// Temporary memory for one conversation.
    Conversation,
    /// Long-term memory attached to a project.
    Project,
    /// Persistent user preferences and important facts.
    Personal,
}

impl MemoryScope {
    /// Stable string identity for persistence and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Conversation => "conversation",
            Self::Project => "project",
            Self::Personal => "personal",
        }
    }

    /// Parse a persisted scope label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "working" => Some(Self::Working),
            "conversation" => Some(Self::Conversation),
            "project" => Some(Self::Project),
            "personal" => Some(Self::Personal),
            _ => None,
        }
    }
}

impl std::fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Lifecycle status of a memory record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryStatus {
    /// Available for retrieval.
    Active,
    /// Soft-archived; excluded from default retrieval.
    Archived,
    /// Forgotten / deleted from active use.
    Forgotten,
}

impl MemoryStatus {
    /// Stable string identity.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Forgotten => "forgotten",
        }
    }

    /// Parse a persisted status label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            "forgotten" => Some(Self::Forgotten),
            _ => None,
        }
    }
}

/// Structured memory record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    /// Stable memory identity.
    pub id: EntityId,
    /// Owning scope.
    pub scope: MemoryScope,
    /// Short summary.
    pub summary: String,
    /// Detailed content.
    pub content: String,
    /// Associated conversation, when any.
    pub conversation_id: Option<String>,
    /// Associated project, when any.
    pub project_id: Option<String>,
    /// Importance score `0..=100`.
    pub importance: u32,
    /// Confidence score `0..=100`.
    pub confidence: u32,
    /// Free-form tags.
    pub tags: Vec<String>,
    /// Provenance / source label.
    pub source: Option<String>,
    /// Structured kind (project memory categories, etc.).
    pub kind: Option<String>,
    /// Free-form JSON metadata (decision reasoning / relations, etc.).
    pub metadata_json: String,
    /// Lifecycle status.
    pub status: MemoryStatus,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds last updated.
    pub updated_at: i64,
    /// Unix seconds archived, when archived.
    pub archived_at: Option<i64>,
}

/// Request to store a new memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMemoryRequest {
    /// Target scope.
    pub scope: MemoryScope,
    /// Short summary.
    pub summary: String,
    /// Detailed content.
    pub content: String,
    /// Optional conversation association.
    pub conversation_id: Option<String>,
    /// Optional project association.
    pub project_id: Option<String>,
    /// Importance `0..=100` (default 50).
    pub importance: Option<u32>,
    /// Confidence `0..=100` (default 50).
    pub confidence: Option<u32>,
    /// Tags.
    pub tags: Vec<String>,
    /// Provenance label.
    pub source: Option<String>,
    /// Structured kind (optional; project memories set this).
    pub kind: Option<String>,
    /// Optional JSON metadata blob.
    pub metadata_json: Option<String>,
}

/// Query for retrieving memories. The Memory Engine decides relevance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryQuery {
    /// Free-text relevance filter (substring match for foundation).
    pub text: Option<String>,
    /// Restrict to one scope.
    pub scope: Option<MemoryScope>,
    /// Restrict to a project.
    pub project_id: Option<String>,
    /// Restrict to a conversation.
    pub conversation_id: Option<String>,
    /// Restrict to a structured kind.
    pub kind: Option<String>,
    /// Include archived memories.
    pub include_archived: bool,
    /// Maximum records to return.
    pub limit: Option<usize>,
}

/// Archived conversation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationArchive {
    /// Archive identity.
    pub archive_id: String,
    /// Conversation identity.
    pub conversation_id: String,
    /// Optional title.
    pub title: Option<String>,
    /// Serialized conversation content (JSON or plain text).
    pub content: String,
    /// Unix seconds archived.
    pub archived_at: i64,
    /// Memory created by promotion, when any.
    pub promoted_memory_id: Option<String>,
}

/// Request to archive a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveConversationRequest {
    /// Conversation identity.
    pub conversation_id: String,
    /// Optional title.
    pub title: Option<String>,
    /// Conversation body to archive.
    pub content: String,
    /// When true, also promote a conversation-scoped summary memory.
    pub promote_summary: bool,
    /// Summary used when `promote_summary` is true.
    pub summary: Option<String>,
}
