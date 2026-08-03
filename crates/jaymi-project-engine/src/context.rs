//! Assembled project context — what the Planner requests as one object.
//!
//! The Project Engine decides what belongs. The Planner never gathers
//! indexed files, conversations, memories, or search state manually.

use std::path::PathBuf;

use jaymi_knowledge::KnowledgeItem;
use jaymi_memory_engine::{
    Conversation, ConversationMessage, ConversationMeta, MemoryRecord, ProjectMemoryBundle,
};

use crate::types::Project;

/// Default limits for assembled project context sections.
pub const DEFAULT_INDEXED_FILE_LIMIT: usize = 64;
pub const DEFAULT_CONVERSATION_LIMIT: usize = 16;
pub const DEFAULT_CONVERSATION_MESSAGE_LIMIT: usize = 64;
pub const DEFAULT_RECENT_LIMIT: usize = 12;
pub const DEFAULT_IMPORTANT_DOC_LIMIT: usize = 16;
pub const DEFAULT_ARCHITECTURE_LIMIT: usize = 16;
pub const DEFAULT_PARSED_CONTENT_LIMIT: usize = 32;

/// Lightweight indexed file reference for project context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileEntry {
    /// Absolute path.
    pub path: PathBuf,
    /// Filename.
    pub filename: String,
    /// Extension without dot, when any.
    pub extension: Option<String>,
    /// Size in bytes.
    pub size: u64,
    /// Whether this entry is a directory.
    pub is_directory: bool,
    /// Last modified unix seconds, when known.
    pub modified: Option<i64>,
}

impl ProjectFileEntry {
    /// Build from a knowledge inventory item.
    pub fn from_knowledge(item: &KnowledgeItem) -> Self {
        Self {
            path: item.path.clone(),
            filename: item.filename.clone(),
            extension: item.extension.clone(),
            size: item.size,
            is_directory: item.is_directory,
            modified: item.modified.or(item.last_modified),
        }
    }
}

/// Conversation attached to the project (history included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConversationEntry {
    /// Conversation id.
    pub conversation_id: String,
    /// Optional title.
    pub title: Option<String>,
    /// Owning project id when attached (`None` = global).
    pub project_id: Option<String>,
    /// Unix seconds updated.
    pub updated_at: i64,
    /// Total messages in the transcript.
    pub message_count: usize,
    /// Loaded transcript messages (may be capped for context size).
    pub messages: Vec<ConversationMessage>,
}

impl ProjectConversationEntry {
    /// Build from a loaded conversation transcript.
    pub fn from_conversation(conversation: &Conversation, message_limit: usize) -> Self {
        let message_count = conversation.messages.len();
        let messages = if message_limit == 0 || message_count <= message_limit {
            conversation.messages.clone()
        } else {
            conversation.messages[message_count - message_limit..].to_vec()
        };
        Self {
            conversation_id: conversation.meta.id.as_str().to_string(),
            title: conversation.meta.title.clone(),
            project_id: conversation.meta.project_id.clone(),
            updated_at: conversation.meta.updated_at,
            message_count,
            messages,
        }
    }

    /// Build a lightweight entry when only metadata is known.
    pub fn from_meta(meta: &ConversationMeta) -> Self {
        Self {
            conversation_id: meta.id.as_str().to_string(),
            title: meta.title.clone(),
            project_id: meta.project_id.clone(),
            updated_at: meta.updated_at,
            message_count: 0,
            messages: Vec::new(),
        }
    }
}

/// Search index coverage summary for the project root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectSearchIndex {
    /// Whether a project root is available for scoped search.
    pub has_root: bool,
    /// Indexed file count under the project root.
    pub indexed_file_count: u64,
    /// Indexed folder count under the project root.
    pub indexed_folder_count: u64,
    /// Global search engine healthy flag, when available.
    pub search_healthy: bool,
    /// Short status detail.
    pub detail: String,
}

/// Recent work item (file or memory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecentWorkItem {
    /// Kind label (`file` / `memory` / `conversation`).
    pub kind: String,
    /// Display title / summary.
    pub title: String,
    /// Optional path or memory id.
    pub reference: String,
    /// Unix seconds for ordering.
    pub at: i64,
}

/// Architecture document or decision surfaced for the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectArchitectureItem {
    /// Source (`memory` / `file`).
    pub source: String,
    /// Title / summary.
    pub title: String,
    /// Body or path reference.
    pub detail: String,
    /// Optional path.
    pub path: Option<PathBuf>,
}

/// Parsed / normalized content under the project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectParsedContent {
    /// Content store source id (usually absolute path).
    pub source_id: String,
    /// Filesystem path.
    pub path: PathBuf,
    /// Optional document title.
    pub title: Option<String>,
    /// Content type label.
    pub content_type: String,
    /// Short plain-text preview.
    pub preview: String,
}

/// Project task surfaced from project memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTaskEntry {
    /// Memory id.
    pub memory_id: String,
    /// Short summary.
    pub summary: String,
    /// Task body.
    pub content: String,
    /// Unix seconds updated.
    pub updated_at: i64,
}

/// Structured decision from the project decision log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDecisionEntry {
    /// Memory id.
    pub memory_id: String,
    /// When the decision was recorded.
    pub timestamp: i64,
    /// Decision title.
    pub title: String,
    /// What was decided.
    pub description: String,
    /// Why the decision was made.
    pub reasoning: String,
    /// Related filesystem paths.
    pub related_files: Vec<String>,
    /// Related conversation ids.
    pub related_conversations: Vec<String>,
}

/// One assembled project context for the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// Project identity and metadata.
    pub project: Project,
    /// True when this project is the session-open project.
    pub is_open: bool,
    /// Indexed files under the project root.
    pub indexed_files: Vec<ProjectFileEntry>,
    /// Conversations owned by the project.
    pub conversations: Vec<ProjectConversationEntry>,
    /// Categorized project memories (from Memory Engine).
    pub memories: ProjectMemoryBundle,
    /// Search index summary for the project.
    pub search_index: ProjectSearchIndex,
    /// Important documents (memory refs + notable inventory docs).
    pub important_documents: Vec<ProjectFileEntry>,
    /// Documentation files (markdown / text docs under the project).
    pub documentation: Vec<ProjectFileEntry>,
    /// Recent work across files and memories.
    pub recent_work: Vec<ProjectRecentWorkItem>,
    /// Architecture documents / decisions (file + memory summaries).
    pub architecture_documents: Vec<ProjectArchitectureItem>,
    /// Parsed content previews for indexed files.
    pub parsed_content: Vec<ProjectParsedContent>,
    /// Project tasks (from project memory).
    pub tasks: Vec<ProjectTaskEntry>,
    /// Structured decision log (why decisions were made).
    pub decisions: Vec<ProjectDecisionEntry>,
}

impl ProjectContext {
    /// Total entries across assembled sections (for diagnostics / summaries).
    pub fn entry_count(&self) -> usize {
        self.indexed_files.len()
            + self.conversations.len()
            + self.memories.entry_count()
            + self.important_documents.len()
            + self.documentation.len()
            + self.recent_work.len()
            + self.architecture_documents.len()
            + self.parsed_content.len()
            + self.tasks.len()
            + self.decisions.len()
    }

    /// Flat memory list (stable category order).
    pub fn all_memories(&self) -> Vec<&MemoryRecord> {
        self.memories.all_memories()
    }
}

/// Optional backends used to assemble project context.
#[derive(Clone)]
pub struct ProjectContextSources {
    /// Memory Engine for conversations + project memories.
    pub memory: std::sync::Arc<dyn jaymi_memory_engine::MemoryEngineApi>,
    /// Knowledge inventory for indexed files.
    pub knowledge: std::sync::Arc<dyn jaymi_knowledge::KnowledgeStore>,
    /// Search Engine for index health / scoped discovery.
    pub search: std::sync::Arc<dyn jaymi_search::SearchEngineApi>,
    /// Content Intelligence for parsed content (optional until bound).
    pub content: Option<std::sync::Arc<dyn jaymi_understanding::ContentIntelligence>>,
}
