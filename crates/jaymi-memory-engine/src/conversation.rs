//! Conversation transcript types (history, isolated per conversation).

use jaymi_core::EntityId;

/// Role of a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageRole {
    /// Message authored by the user.
    User,
    /// Message authored by the assistant.
    Assistant,
    /// System / control message.
    System,
}

impl MessageRole {
    /// Stable string identity.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }

    /// Parse a persisted role label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Lifecycle status of a persisted conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationStatus {
    /// Open / active conversation.
    Active,
    /// Soft-archived conversation (still loadable).
    Archived,
    /// Explicitly closed conversation (still loadable).
    Closed,
}

impl ConversationStatus {
    /// Stable string identity.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Closed => "closed",
        }
    }

    /// Parse a persisted status label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// Attachment on a conversation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationAttachment {
    /// Stable attachment identity.
    pub id: EntityId,
    /// Kind (`file` / `image` / `url` / `other`).
    pub kind: String,
    /// Display name.
    pub name: Option<String>,
    /// Local path or URI.
    pub uri: Option<String>,
    /// MIME type when known.
    pub mime_type: Option<String>,
    /// Size in bytes when known.
    pub size_bytes: Option<u64>,
    /// Free-form metadata (JSON object string).
    pub metadata_json: String,
}

/// Reference attached to a conversation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationReference {
    /// Stable reference identity.
    pub id: EntityId,
    /// Kind (`memory` / `document` / `citation` / `tool` / `other`).
    pub kind: String,
    /// Target identity when known.
    pub target_id: Option<String>,
    /// Human-readable label.
    pub label: Option<String>,
    /// URI when known.
    pub uri: Option<String>,
    /// Free-form metadata (JSON object string).
    pub metadata_json: String,
}

/// One message in a conversation transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    /// Stable message identity.
    pub id: EntityId,
    /// Owning conversation.
    pub conversation_id: String,
    /// Author role.
    pub role: MessageRole,
    /// Message body.
    pub content: String,
    /// Unix seconds created.
    pub created_at: i64,
    /// Deterministic order within the conversation.
    pub sequence_no: u64,
    /// Attachments on this message.
    pub attachments: Vec<ConversationAttachment>,
    /// References on this message.
    pub references: Vec<ConversationReference>,
}

/// Conversation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMeta {
    /// Stable conversation identity.
    pub id: EntityId,
    /// Optional title.
    pub title: Option<String>,
    /// Optional owning project.
    pub project_id: Option<String>,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds last updated.
    pub updated_at: i64,
    /// Lifecycle status.
    pub status: ConversationStatus,
}

/// Fully loaded conversation — exact reopen payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    /// Conversation metadata.
    pub meta: ConversationMeta,
    /// Ordered transcript.
    pub messages: Vec<ConversationMessage>,
}

/// Request to create a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CreateConversationRequest {
    /// Optional explicit identity; generated when omitted.
    pub conversation_id: Option<String>,
    /// Optional title.
    pub title: Option<String>,
    /// Optional owning project.
    pub project_id: Option<String>,
}

/// Request to append a message to a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendMessageRequest {
    /// Target conversation.
    pub conversation_id: String,
    /// Author role.
    pub role: MessageRole,
    /// Message body.
    pub content: String,
    /// Optional explicit timestamp (unix seconds); defaults to now.
    pub created_at: Option<i64>,
    /// Attachments for this message.
    pub attachments: Vec<ConversationAttachmentInput>,
    /// References for this message.
    pub references: Vec<ConversationReferenceInput>,
}

/// Attachment input when appending a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationAttachmentInput {
    /// Kind (`file` / `image` / `url` / `other`).
    pub kind: String,
    /// Display name.
    pub name: Option<String>,
    /// Local path or URI.
    pub uri: Option<String>,
    /// MIME type when known.
    pub mime_type: Option<String>,
    /// Size in bytes when known.
    pub size_bytes: Option<u64>,
    /// Free-form metadata (JSON object string).
    pub metadata_json: Option<String>,
}

/// Reference input when appending a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationReferenceInput {
    /// Kind (`memory` / `document` / `citation` / `tool` / `other`).
    pub kind: String,
    /// Target identity when known.
    pub target_id: Option<String>,
    /// Human-readable label.
    pub label: Option<String>,
    /// URI when known.
    pub uri: Option<String>,
    /// Free-form metadata (JSON object string).
    pub metadata_json: Option<String>,
}
