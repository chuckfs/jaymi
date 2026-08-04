//! Memory persistence (provider-independent).
//!
//! SQLite is the default Knowledge Store backend. An in-memory store supports tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jaymi_core::{EntityId, JaymiError, JaymiResult};
use jaymi_database::{
    ConversationArchiveRecord, ConversationAttachmentRecord, ConversationMessageRecord,
    ConversationRecord, ConversationReferenceRecord, Database, MemoryRecord as DbMemoryRecord,
    MemorySearchQuery,
};

use crate::conversation::{
    Conversation, ConversationAttachment, ConversationMessage, ConversationMeta,
    ConversationReference, ConversationStatus, MessageRole,
};
use crate::types::{
    ArchiveConversationRequest, ConversationArchive, MemoryQuery, MemoryRecord, MemoryScope,
    MemoryStatus, StoreMemoryRequest,
};

/// Persistence surface used by the Memory Engine.
pub trait MemoryStore: Send + Sync {
    /// Insert a memory row.
    fn insert(&self, record: &MemoryRecord) -> JaymiResult<()>;

    /// Load by identity.
    fn get(&self, memory_id: &str) -> JaymiResult<Option<MemoryRecord>>;

    /// Search active (and optionally archived) memories.
    fn search(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>>;

    /// Update an existing memory.
    fn update(&self, record: &MemoryRecord) -> JaymiResult<()>;

    /// Soft-forget a memory.
    fn forget(&self, memory_id: &str, now: i64) -> JaymiResult<()>;

    /// Persist a conversation archive.
    fn archive_conversation(&self, archive: &ConversationArchive) -> JaymiResult<()>;

    /// Count active memories by scope for diagnostics.
    fn counts_by_scope(&self) -> JaymiResult<Vec<(MemoryScope, u64)>>;

    /// Insert or update conversation metadata.
    fn upsert_conversation_meta(&self, meta: &ConversationMeta) -> JaymiResult<()>;

    /// Load conversation metadata.
    fn get_conversation_meta(&self, conversation_id: &str)
        -> JaymiResult<Option<ConversationMeta>>;

    /// Next sequence number for a conversation.
    fn next_message_sequence(&self, conversation_id: &str) -> JaymiResult<u64>;

    /// Persist a full message (attachments + references).
    fn insert_conversation_message(&self, message: &ConversationMessage) -> JaymiResult<()>;

    /// Load an entire conversation exactly as stored.
    fn load_conversation(&self, conversation_id: &str) -> JaymiResult<Option<Conversation>>;

    /// Count persisted conversations.
    fn conversation_count(&self) -> JaymiResult<u64>;

    /// Conversation ids attached to a project.
    fn list_conversation_ids_for_project(&self, project_id: &str) -> JaymiResult<Vec<String>>;

    /// Distinct `project_id` values referenced by active memories (not a project registry).
    fn referenced_project_count(&self) -> JaymiResult<u64>;
}

/// SQLite memory store — separate from providers and normalized content.
pub struct SqliteMemoryStore {
    database: Arc<Database>,
}

impl SqliteMemoryStore {
    /// Create a store bound to the shared database.
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

/// Process-local memory store for tests and ephemeral use.
#[derive(Default)]
pub struct InMemoryMemoryStore {
    inner: Mutex<InMemoryState>,
}

#[derive(Default)]
struct InMemoryState {
    memories: HashMap<String, MemoryRecord>,
    archives: HashMap<String, ConversationArchive>,
    conversations: HashMap<String, ConversationMeta>,
    /// Messages keyed by conversation id, kept in sequence order.
    messages: HashMap<String, Vec<ConversationMessage>>,
}

impl InMemoryMemoryStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryStore for InMemoryMemoryStore {
    fn insert(&self, record: &MemoryRecord) -> JaymiResult<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        state
            .memories
            .insert(record.id.as_str().to_string(), record.clone());
        Ok(())
    }

    fn get(&self, memory_id: &str) -> JaymiResult<Option<MemoryRecord>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        Ok(state.memories.get(memory_id).cloned())
    }

    fn search(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        let text = query
            .text
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let mut out: Vec<MemoryRecord> = state
            .memories
            .values()
            .filter(|record| record.status != MemoryStatus::Forgotten)
            .filter(|record| {
                if query.include_archived {
                    record.status == MemoryStatus::Active || record.status == MemoryStatus::Archived
                } else {
                    record.status == MemoryStatus::Active
                }
            })
            .filter(|record| {
                query
                    .scope
                    .map(|scope| record.scope == scope)
                    .unwrap_or(true)
            })
            .filter(|record| {
                query
                    .project_id
                    .as_ref()
                    .map(|id| record.project_id.as_deref() == Some(id.as_str()))
                    .unwrap_or_else(|| record.scope != MemoryScope::Project)
            })
            .filter(|record| {
                query
                    .conversation_id
                    .as_ref()
                    .map(|id| record.conversation_id.as_deref() == Some(id.as_str()))
                    .unwrap_or_else(|| record.scope != MemoryScope::Conversation)
            })
            .filter(|record| {
                query
                    .kind
                    .as_ref()
                    .map(|kind| record.kind.as_deref() == Some(kind.as_str()))
                    .unwrap_or(true)
            })
            .filter(|record| {
                let Some(text) = &text else {
                    return true;
                };
                record.summary.to_ascii_lowercase().contains(text)
                    || record.content.to_ascii_lowercase().contains(text)
                    || record.metadata_json.to_ascii_lowercase().contains(text)
                    || record
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(text))
            })
            .cloned()
            .collect();
        out.sort_by(|left, right| {
            right
                .importance
                .cmp(&left.importance)
                .then(right.updated_at.cmp(&left.updated_at))
                .then(left.id.as_str().cmp(right.id.as_str()))
        });
        if let Some(limit) = query.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    fn update(&self, record: &MemoryRecord) -> JaymiResult<()> {
        self.insert(record)
    }

    fn forget(&self, memory_id: &str, now: i64) -> JaymiResult<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        let Some(record) = state.memories.get_mut(memory_id) else {
            return Err(JaymiError::new(format!(
                "memory not found or already forgotten: {memory_id}"
            )));
        };
        if record.status == MemoryStatus::Forgotten {
            return Err(JaymiError::new(format!(
                "memory not found or already forgotten: {memory_id}"
            )));
        }
        record.status = MemoryStatus::Forgotten;
        record.updated_at = now;
        record.archived_at = Some(record.archived_at.unwrap_or(now));
        Ok(())
    }

    fn archive_conversation(&self, archive: &ConversationArchive) -> JaymiResult<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        state
            .archives
            .insert(archive.archive_id.clone(), archive.clone());
        Ok(())
    }

    fn counts_by_scope(&self) -> JaymiResult<Vec<(MemoryScope, u64)>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        let mut counts: HashMap<MemoryScope, u64> = HashMap::new();
        for record in state.memories.values() {
            if record.status == MemoryStatus::Active {
                *counts.entry(record.scope).or_default() += 1;
            }
        }
        let mut out: Vec<_> = counts.into_iter().collect();
        out.sort_by_key(|(scope, _)| scope.as_str());
        Ok(out)
    }

    fn upsert_conversation_meta(&self, meta: &ConversationMeta) -> JaymiResult<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        state
            .conversations
            .insert(meta.id.as_str().to_string(), meta.clone());
        Ok(())
    }

    fn get_conversation_meta(
        &self,
        conversation_id: &str,
    ) -> JaymiResult<Option<ConversationMeta>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        Ok(state.conversations.get(conversation_id).cloned())
    }

    fn next_message_sequence(&self, conversation_id: &str) -> JaymiResult<u64> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        Ok(state
            .messages
            .get(conversation_id)
            .map(|messages| messages.len() as u64)
            .unwrap_or(0))
    }

    fn insert_conversation_message(&self, message: &ConversationMessage) -> JaymiResult<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        if !state.conversations.contains_key(&message.conversation_id) {
            return Err(JaymiError::new(format!(
                "conversation not found: {}",
                message.conversation_id
            )));
        }
        let entry = state
            .messages
            .entry(message.conversation_id.clone())
            .or_default();
        entry.push(message.clone());
        entry.sort_by_key(|item| (item.sequence_no, item.id.as_str().to_string()));
        if let Some(meta) = state.conversations.get_mut(&message.conversation_id) {
            meta.updated_at = message.created_at.max(meta.updated_at);
        }
        Ok(())
    }

    fn load_conversation(&self, conversation_id: &str) -> JaymiResult<Option<Conversation>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        let Some(meta) = state.conversations.get(conversation_id).cloned() else {
            return Ok(None);
        };
        let messages = state
            .messages
            .get(conversation_id)
            .cloned()
            .unwrap_or_default();
        Ok(Some(Conversation { meta, messages }))
    }

    fn conversation_count(&self) -> JaymiResult<u64> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        Ok(state.conversations.len() as u64)
    }

    fn list_conversation_ids_for_project(&self, project_id: &str) -> JaymiResult<Vec<String>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        let mut metas: Vec<_> = state
            .conversations
            .values()
            .filter(|meta| meta.project_id.as_deref() == Some(project_id))
            .cloned()
            .collect();
        metas.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then(left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(metas
            .into_iter()
            .map(|meta| meta.id.as_str().to_string())
            .collect())
    }

    fn referenced_project_count(&self) -> JaymiResult<u64> {
        let state = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("memory store lock"))?;
        let mut ids = std::collections::BTreeSet::new();
        for record in state.memories.values() {
            if record.status == MemoryStatus::Active {
                if let Some(project_id) = &record.project_id {
                    if !project_id.is_empty() {
                        ids.insert(project_id.clone());
                    }
                }
            }
        }
        Ok(ids.len() as u64)
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn insert(&self, record: &MemoryRecord) -> JaymiResult<()> {
        self.database.upsert_memory(&to_db(record))
    }

    fn get(&self, memory_id: &str) -> JaymiResult<Option<MemoryRecord>> {
        Ok(self.database.get_memory(memory_id)?.map(from_db))
    }

    fn search(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>> {
        let rows = self.database.search_memories(&MemorySearchQuery {
            text: query.text.clone(),
            scope: query.scope.map(|scope| scope.as_str().to_string()),
            project_id: query.project_id.clone(),
            conversation_id: query.conversation_id.clone(),
            kind: query.kind.clone(),
            include_archived: query.include_archived,
            limit: query.limit,
        })?;
        Ok(rows.into_iter().map(from_db).collect())
    }

    fn update(&self, record: &MemoryRecord) -> JaymiResult<()> {
        self.database.upsert_memory(&to_db(record))
    }

    fn forget(&self, memory_id: &str, now: i64) -> JaymiResult<()> {
        self.database.forget_memory(memory_id, now)
    }

    fn archive_conversation(&self, archive: &ConversationArchive) -> JaymiResult<()> {
        self.database
            .upsert_conversation_archive(&ConversationArchiveRecord {
                archive_id: archive.archive_id.clone(),
                conversation_id: archive.conversation_id.clone(),
                title: archive.title.clone(),
                content: archive.content.clone(),
                archived_at: archive.archived_at,
                promoted_memory_id: archive.promoted_memory_id.clone(),
            })
    }

    fn counts_by_scope(&self) -> JaymiResult<Vec<(MemoryScope, u64)>> {
        let rows = self.database.memory_counts_by_scope()?;
        let mut out = Vec::new();
        for (scope, count) in rows {
            if let Some(scope) = MemoryScope::parse(&scope) {
                out.push((scope, count));
            }
        }
        Ok(out)
    }

    fn upsert_conversation_meta(&self, meta: &ConversationMeta) -> JaymiResult<()> {
        self.database.upsert_conversation(&ConversationRecord {
            conversation_id: meta.id.as_str().to_string(),
            title: meta.title.clone(),
            project_id: meta.project_id.clone(),
            created_at: meta.created_at,
            updated_at: meta.updated_at,
            status: meta.status.as_str().to_string(),
        })
    }

    fn get_conversation_meta(
        &self,
        conversation_id: &str,
    ) -> JaymiResult<Option<ConversationMeta>> {
        Ok(self
            .database
            .get_conversation(conversation_id)?
            .map(meta_from_db))
    }

    fn next_message_sequence(&self, conversation_id: &str) -> JaymiResult<u64> {
        Ok(self.database.next_conversation_sequence(conversation_id)? as u64)
    }

    fn insert_conversation_message(&self, message: &ConversationMessage) -> JaymiResult<()> {
        self.database
            .insert_conversation_message(&ConversationMessageRecord {
                message_id: message.id.as_str().to_string(),
                conversation_id: message.conversation_id.clone(),
                role: message.role.as_str().to_string(),
                content: message.content.clone(),
                created_at: message.created_at,
                sequence_no: message.sequence_no as i64,
            })?;
        for attachment in &message.attachments {
            self.database
                .insert_conversation_attachment(&ConversationAttachmentRecord {
                    attachment_id: attachment.id.as_str().to_string(),
                    message_id: message.id.as_str().to_string(),
                    kind: attachment.kind.clone(),
                    name: attachment.name.clone(),
                    uri: attachment.uri.clone(),
                    mime_type: attachment.mime_type.clone(),
                    size_bytes: attachment.size_bytes.map(|value| value as i64),
                    metadata_json: attachment.metadata_json.clone(),
                })?;
        }
        for reference in &message.references {
            self.database
                .insert_conversation_reference(&ConversationReferenceRecord {
                    reference_id: reference.id.as_str().to_string(),
                    message_id: message.id.as_str().to_string(),
                    kind: reference.kind.clone(),
                    target_id: reference.target_id.clone(),
                    label: reference.label.clone(),
                    uri: reference.uri.clone(),
                    metadata_json: reference.metadata_json.clone(),
                })?;
        }
        // Bump conversation updated_at.
        if let Some(mut meta) = self.get_conversation_meta(&message.conversation_id)? {
            meta.updated_at = message.created_at.max(meta.updated_at);
            self.upsert_conversation_meta(&meta)?;
        }
        Ok(())
    }

    fn load_conversation(&self, conversation_id: &str) -> JaymiResult<Option<Conversation>> {
        Ok(self
            .database
            .load_conversation(conversation_id)?
            .map(conversation_from_db))
    }

    fn conversation_count(&self) -> JaymiResult<u64> {
        self.database.conversation_count()
    }

    fn list_conversation_ids_for_project(&self, project_id: &str) -> JaymiResult<Vec<String>> {
        self.database.list_conversation_ids_for_project(project_id)
    }

    fn referenced_project_count(&self) -> JaymiResult<u64> {
        self.database.referenced_project_memory_count()
    }
}

fn meta_from_db(record: ConversationRecord) -> ConversationMeta {
    ConversationMeta {
        id: EntityId::new(record.conversation_id),
        title: record.title,
        project_id: record.project_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
        status: ConversationStatus::parse(&record.status).unwrap_or(ConversationStatus::Active),
    }
}

fn conversation_from_db(loaded: jaymi_database::LoadedConversationRecord) -> Conversation {
    Conversation {
        meta: meta_from_db(loaded.conversation),
        messages: loaded
            .messages
            .into_iter()
            .map(|item| ConversationMessage {
                id: EntityId::new(item.message.message_id),
                conversation_id: item.message.conversation_id,
                role: MessageRole::parse(&item.message.role).unwrap_or(MessageRole::User),
                content: item.message.content,
                created_at: item.message.created_at,
                sequence_no: item.message.sequence_no as u64,
                attachments: item
                    .attachments
                    .into_iter()
                    .map(|attachment| ConversationAttachment {
                        id: EntityId::new(attachment.attachment_id),
                        kind: attachment.kind,
                        name: attachment.name,
                        uri: attachment.uri,
                        mime_type: attachment.mime_type,
                        size_bytes: attachment.size_bytes.map(|value| value as u64),
                        metadata_json: attachment.metadata_json,
                    })
                    .collect(),
                references: item
                    .references
                    .into_iter()
                    .map(|reference| ConversationReference {
                        id: EntityId::new(reference.reference_id),
                        kind: reference.kind,
                        target_id: reference.target_id,
                        label: reference.label,
                        uri: reference.uri,
                        metadata_json: reference.metadata_json,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn to_db(record: &MemoryRecord) -> DbMemoryRecord {
    DbMemoryRecord {
        memory_id: record.id.as_str().to_string(),
        scope: record.scope.as_str().to_string(),
        summary: record.summary.clone(),
        content: record.content.clone(),
        conversation_id: record.conversation_id.clone(),
        project_id: record.project_id.clone(),
        importance: record.importance.min(100) as i64,
        confidence: record.confidence.min(100) as i64,
        tags_json: serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".into()),
        source: record.source.clone(),
        kind: record.kind.clone(),
        status: record.status.as_str().to_string(),
        created_at: record.created_at,
        updated_at: record.updated_at,
        archived_at: record.archived_at,
        metadata_json: if record.metadata_json.trim().is_empty() {
            "{}".into()
        } else {
            record.metadata_json.clone()
        },
    }
}

fn from_db(record: DbMemoryRecord) -> MemoryRecord {
    let tags: Vec<String> = serde_json::from_str(&record.tags_json).unwrap_or_default();
    MemoryRecord {
        id: EntityId::new(record.memory_id),
        scope: MemoryScope::parse(&record.scope).unwrap_or(MemoryScope::Conversation),
        summary: record.summary,
        content: record.content,
        conversation_id: record.conversation_id,
        project_id: record.project_id,
        importance: record.importance.clamp(0, 100) as u32,
        confidence: record.confidence.clamp(0, 100) as u32,
        tags,
        source: record.source,
        kind: record.kind,
        status: MemoryStatus::parse(&record.status).unwrap_or(MemoryStatus::Active),
        created_at: record.created_at,
        updated_at: record.updated_at,
        archived_at: record.archived_at,
        metadata_json: if record.metadata_json.trim().is_empty() {
            "{}".into()
        } else {
            record.metadata_json
        },
    }
}

/// Build a new active memory from a store request.
pub fn record_from_store(request: &StoreMemoryRequest, now: i64) -> JaymiResult<MemoryRecord> {
    if request.summary.trim().is_empty() && request.content.trim().is_empty() {
        return Err(JaymiError::new(
            "memory store requires a non-empty summary or content",
        ));
    }
    if request.scope == MemoryScope::Project && request.project_id.is_none() {
        return Err(JaymiError::new(
            "project memory requires an associated project_id",
        ));
    }
    if request.scope == MemoryScope::Conversation && request.conversation_id.is_none() {
        return Err(JaymiError::new(
            "conversation memory requires an associated conversation_id",
        ));
    }
    let summary = if request.summary.trim().is_empty() {
        truncate(request.content.trim(), 120)
    } else {
        request.summary.trim().to_string()
    };
    Ok(MemoryRecord {
        id: EntityId::new(format!("memory:{}", now_nanos())),
        scope: request.scope,
        summary,
        content: request.content.trim().to_string(),
        conversation_id: request.conversation_id.clone(),
        project_id: request.project_id.clone(),
        importance: request.importance.unwrap_or(50).min(100),
        confidence: request.confidence.unwrap_or(50).min(100),
        tags: request.tags.clone(),
        source: request.source.clone(),
        kind: request.kind.clone(),
        status: MemoryStatus::Active,
        created_at: now,
        updated_at: now,
        archived_at: None,
        metadata_json: request.metadata_json.clone().unwrap_or_else(|| "{}".into()),
    })
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let short: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{short}…")
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

/// Build a conversation archive record.
pub fn archive_from_request(
    request: &ArchiveConversationRequest,
    archive_id: String,
    now: i64,
    promoted_memory_id: Option<String>,
) -> ConversationArchive {
    ConversationArchive {
        archive_id,
        conversation_id: request.conversation_id.clone(),
        title: request.title.clone(),
        content: request.content.clone(),
        archived_at: now,
        promoted_memory_id,
    }
}
