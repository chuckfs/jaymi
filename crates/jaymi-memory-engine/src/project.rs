//! Project-scoped memory kinds and restored project context.

use jaymi_core::EntityId;

use crate::types::MemoryRecord;

/// Categories every project maintains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectMemoryKind {
    /// Conversation linked to the project.
    Conversation,
    /// Architecture / design decision.
    ArchitectureDecision,
    /// Task / TODO.
    Task,
    /// Coding preference or convention.
    CodingPreference,
    /// Important file reference.
    ImportantFile,
    /// Milestone / checkpoint.
    Milestone,
}

impl ProjectMemoryKind {
    /// Stable string identity persisted in `memories.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::ArchitectureDecision => "architecture_decision",
            Self::Task => "task",
            Self::CodingPreference => "coding_preference",
            Self::ImportantFile => "important_file",
            Self::Milestone => "milestone",
        }
    }

    /// Parse a persisted kind label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "conversation" => Some(Self::Conversation),
            "architecture_decision" | "architecture" | "decision" => {
                Some(Self::ArchitectureDecision)
            }
            "task" | "todo" => Some(Self::Task),
            "coding_preference" | "preference" | "convention" => Some(Self::CodingPreference),
            "important_file" | "file" => Some(Self::ImportantFile),
            "milestone" => Some(Self::Milestone),
            _ => None,
        }
    }

    /// All kinds a project maintains.
    pub fn all() -> &'static [Self] {
        &[
            Self::Conversation,
            Self::ArchitectureDecision,
            Self::Task,
            Self::CodingPreference,
            Self::ImportantFile,
            Self::Milestone,
        ]
    }
}

impl std::fmt::Display for ProjectMemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Registered project identity for memory attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMeta {
    /// Stable project identity.
    pub id: EntityId,
    /// Display name (e.g. "Jaymi").
    pub name: String,
    /// Normalized slug.
    pub slug: String,
    /// Optional workspace root.
    pub root_path: Option<String>,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds updated.
    pub updated_at: i64,
}

/// Request to register a project for memory attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterProjectRequest {
    /// Optional explicit identity.
    pub project_id: Option<String>,
    /// Display name (required).
    pub name: String,
    /// Optional workspace root.
    pub root_path: Option<String>,
}

/// Request to store a categorized project memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreProjectMemoryRequest {
    /// Owning project.
    pub project_id: String,
    /// Category.
    pub kind: ProjectMemoryKind,
    /// Short summary.
    pub summary: String,
    /// Detailed content.
    pub content: String,
    /// Optional linked conversation.
    pub conversation_id: Option<String>,
    /// Importance `0..=100`.
    pub importance: Option<u32>,
    /// Confidence `0..=100`.
    pub confidence: Option<u32>,
    /// Extra tags.
    pub tags: Vec<String>,
    /// Provenance.
    pub source: Option<String>,
}

/// Structured architectural decision in a project's decision log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDecision {
    /// Underlying memory id.
    pub memory_id: String,
    /// Owning project.
    pub project_id: String,
    /// When the decision was recorded (unix seconds).
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
    /// Importance `0..=100`.
    pub importance: u32,
    /// Confidence `0..=100`.
    pub confidence: u32,
    /// Provenance label.
    pub source: Option<String>,
}

/// Request to persist a project decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreProjectDecisionRequest {
    /// Owning project.
    pub project_id: String,
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
    /// Optional primary conversation provenance.
    pub conversation_id: Option<String>,
    /// Importance `0..=100`.
    pub importance: Option<u32>,
    /// Confidence `0..=100`.
    pub confidence: Option<u32>,
    /// Provenance.
    pub source: Option<String>,
}

/// Query for listing project decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListProjectDecisionsQuery {
    /// Owning project.
    pub project_id: String,
    /// Optional free-text filter (title/description/reasoning).
    pub text: Option<String>,
    /// Maximum rows.
    pub limit: Option<usize>,
}

/// Restored project context for "continue working on …".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectContext {
    /// Project identity.
    pub project_id: String,
    /// Display name.
    pub name: String,
    /// Conversation-linked memories / notes.
    pub conversations: Vec<MemoryRecord>,
    /// Architecture decisions.
    pub architecture_decisions: Vec<MemoryRecord>,
    /// Tasks.
    pub tasks: Vec<MemoryRecord>,
    /// Coding preferences.
    pub coding_preferences: Vec<MemoryRecord>,
    /// Important files.
    pub important_files: Vec<MemoryRecord>,
    /// Milestones.
    pub milestones: Vec<MemoryRecord>,
    /// Conversation transcript ids attached to the project.
    pub conversation_ids: Vec<String>,
}

impl ProjectContext {
    /// Total restored memory entries across categories.
    pub fn entry_count(&self) -> usize {
        self.conversations.len()
            + self.architecture_decisions.len()
            + self.tasks.len()
            + self.coding_preferences.len()
            + self.important_files.len()
            + self.milestones.len()
    }

    /// Flat list of all restored memories (stable category order).
    pub fn all_memories(&self) -> Vec<&MemoryRecord> {
        let mut out = Vec::with_capacity(self.entry_count());
        out.extend(self.conversations.iter());
        out.extend(self.architecture_decisions.iter());
        out.extend(self.tasks.iter());
        out.extend(self.coding_preferences.iter());
        out.extend(self.important_files.iter());
        out.extend(self.milestones.iter());
        out
    }
}

/// Build a URL-safe slug from a project name.
pub fn slugify_project_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "project".into()
    } else {
        slug
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct DecisionMetadata {
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    related_files: Vec<String>,
    #[serde(default)]
    related_conversations: Vec<String>,
}

/// Encode decision-specific fields into memory metadata JSON.
pub fn encode_decision_metadata(
    reasoning: &str,
    related_files: &[String],
    related_conversations: &[String],
) -> String {
    let payload = DecisionMetadata {
        reasoning: reasoning.trim().to_string(),
        related_files: related_files.to_vec(),
        related_conversations: related_conversations.to_vec(),
    };
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
}

/// Parse a project decision from a memory record.
pub fn project_decision_from_record(record: &MemoryRecord) -> Option<ProjectDecision> {
    if record.kind.as_deref() != Some(ProjectMemoryKind::ArchitectureDecision.as_str()) {
        return None;
    }
    let project_id = record.project_id.clone()?;
    let meta: DecisionMetadata =
        serde_json::from_str(&record.metadata_json).unwrap_or_default();
    let mut related_conversations = meta.related_conversations;
    if let Some(conversation_id) = &record.conversation_id {
        if !related_conversations.iter().any(|id| id == conversation_id) {
            related_conversations.insert(0, conversation_id.clone());
        }
    }
    Some(ProjectDecision {
        memory_id: record.id.as_str().to_string(),
        project_id,
        timestamp: record.created_at,
        title: record.summary.clone(),
        description: record.content.clone(),
        reasoning: meta.reasoning,
        related_files: meta.related_files,
        related_conversations,
        importance: record.importance,
        confidence: record.confidence,
        source: record.source.clone(),
    })
}
