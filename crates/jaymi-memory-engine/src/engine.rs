//! Memory Engine — centralized intentional memory.
//!
//! Planner requests memory through this engine. Persistence is provider-independent
//! and never accessed directly by the Planner.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

use crate::context::{
    explain_reasons, score_candidate, tokenize, AssembleContextRequest, AssembledMemoryContext,
    RelevantMemory, DEFAULT_CONTEXT_LIMIT, DEFAULT_CONVERSATION_LIMIT, DEFAULT_PERSONAL_LIMIT,
    DEFAULT_PROJECT_LIMIT, DEFAULT_RECENT_LIMIT, DEFAULT_WORKING_LIMIT,
};
use crate::conversation::{
    AppendMessageRequest, Conversation, ConversationAttachment, ConversationMessage,
    ConversationMeta, ConversationReference, ConversationStatus, CreateConversationRequest,
};
use crate::personal::{
    CreatePersonalMemoryRequest, PersonalContext, PersonalMemoryKind, UpdatePersonalMemoryRequest,
};
use crate::project::{
    encode_decision_metadata, project_decision_from_record, ListProjectDecisionsQuery,
    ProjectDecision, ProjectMemoryBundle, ProjectMemoryKind, StoreProjectDecisionRequest,
    StoreProjectMemoryRequest,
};
use crate::promotion::{
    is_upward_promotion, next_scope, score_promotion_candidate, suggestion_reason,
    PromoteMemoryRequest, PromotionSuggestQuery, PromotionSuggestion,
};
use crate::store::{archive_from_request, record_from_store, MemoryStore, SqliteMemoryStore};
use crate::types::{
    ArchiveConversationRequest, ConversationArchive, MemoryQuery, MemoryRecord, MemoryScope,
    MemoryStatus, StoreMemoryRequest,
};

const NAME: &str = "memory_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
];

/// Consumer-facing Memory Engine API (Planner-facing surface).
pub trait MemoryEngineApi: Send + Sync {
    /// Retrieve memories relevant to a request.
    ///
    /// Conversation-scoped intentional memories remain isolated: they are only
    /// returned when `query.conversation_id` is set.
    fn retrieve(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>>;

    /// Assemble only the memories relevant to the current request.
    ///
    /// Considers active project, active conversation, request text, and recent
    /// work. Never loads every memory. Honors retrieval limits.
    fn assemble_context(
        &self,
        request: &AssembleContextRequest,
    ) -> JaymiResult<AssembledMemoryContext>;

    /// Store a new intentional memory.
    fn store(&self, request: &StoreMemoryRequest) -> JaymiResult<MemoryRecord>;

    /// Forget a memory (soft delete).
    fn forget(&self, memory_id: &str) -> JaymiResult<()>;

    /// Promote a memory up the durability ladder (intentional only).
    ///
    /// Supported direction: Working → Conversation → Project → Personal.
    /// Never called automatically — callers must decide explicitly.
    fn promote(&self, request: &PromoteMemoryRequest) -> JaymiResult<MemoryRecord>;

    /// Produce conservative promotion suggestions (never applies them).
    fn suggest_promotions(
        &self,
        query: &PromotionSuggestQuery,
    ) -> JaymiResult<Vec<PromotionSuggestion>>;

    /// Archive a conversation (optionally promoting a summary memory).
    fn archive_conversation(
        &self,
        request: &ArchiveConversationRequest,
    ) -> JaymiResult<ConversationArchive>;

    /// Create a persisted conversation transcript.
    fn create_conversation(
        &self,
        request: &CreateConversationRequest,
    ) -> JaymiResult<ConversationMeta>;

    /// Append a user/assistant message (with attachments and references).
    fn append_message(&self, request: &AppendMessageRequest) -> JaymiResult<ConversationMessage>;

    /// Load an entire conversation exactly as stored.
    fn load_conversation(&self, conversation_id: &str) -> JaymiResult<Option<Conversation>>;

    /// List conversation metadata attached to a project (most recently updated first).
    fn list_conversations_for_project(
        &self,
        project_id: &str,
    ) -> JaymiResult<Vec<ConversationMeta>>;

    /// Attach a conversation to exactly one project, or detach it (`None`).
    ///
    /// `project_id` is a reference owned by the Project Engine — Memory does
    /// not validate project identity.
    fn attach_conversation_to_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
    ) -> JaymiResult<ConversationMeta>;

    /// Set the active project id used when assembling memory context.
    ///
    /// Session hint only — Project Engine owns whether a project is open.
    /// For application session open/close, the Planner is the sole orchestrator
    /// that pairs Project Engine `open`/`close` with this hint. Do not treat
    /// this as a second project-session API.
    fn set_active_project(&self, project_id: Option<&str>) -> JaymiResult<()>;

    /// Current active project id, when any.
    fn active_project_id(&self) -> Option<String>;

    /// Activate a conversation for context assembly.
    fn set_active_conversation(&self, conversation_id: Option<&str>) -> JaymiResult<()>;

    /// Current active conversation id, when any.
    fn active_conversation_id(&self) -> Option<String>;

    /// Store categorized project memory.
    fn store_project_memory(
        &self,
        request: &StoreProjectMemoryRequest,
    ) -> JaymiResult<MemoryRecord>;

    /// Persist a structured architectural decision in the project decision log.
    fn store_project_decision(
        &self,
        request: &StoreProjectDecisionRequest,
    ) -> JaymiResult<ProjectDecision>;

    /// List decisions for a project (most recent first).
    fn list_project_decisions(
        &self,
        query: &ListProjectDecisionsQuery,
    ) -> JaymiResult<Vec<ProjectDecision>>;

    /// Load one project decision by memory id.
    fn get_project_decision(&self, memory_id: &str) -> JaymiResult<Option<ProjectDecision>>;

    /// Restore categorized project memories by `project_id` (no identity lookup).
    fn restore_project_memories(&self, project_id: &str) -> JaymiResult<ProjectMemoryBundle>;

    /// Create an intentional personal preference memory.
    ///
    /// Personal memory is never auto-captured. At most one active entry exists
    /// per [`PersonalMemoryKind`].
    fn create_personal_memory(
        &self,
        request: &CreatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord>;

    /// Update an existing personal preference memory.
    fn update_personal_memory(
        &self,
        request: &UpdatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord>;

    /// Delete (forget) a personal preference memory.
    fn delete_personal_memory(&self, memory_id: &str) -> JaymiResult<()>;

    /// Load all active personal preferences grouped by kind.
    fn personal_context(&self) -> JaymiResult<PersonalContext>;

    /// Aggregate diagnostics.
    fn stats(&self) -> JaymiResult<MemoryStats>;

    /// Subsystem health.
    fn health(&self) -> JaymiResult<MemoryHealth>;
}

/// Aggregate Memory Engine statistics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryStats {
    /// Active memories by scope (`scope=count`).
    pub active_by_scope: Vec<(String, u64)>,
    /// Total active memories.
    pub active_total: u64,
    /// Persisted conversation transcripts.
    pub conversation_count: u64,
    /// Distinct project ids referenced by active memories (not a registry count).
    pub project_count: u64,
}

/// Health snapshot for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHealth {
    /// Whether initialization completed.
    pub initialized: bool,
    /// Whether the engine can serve memory operations.
    pub healthy: bool,
    /// Version string.
    pub version: String,
    /// Short detail.
    pub detail: String,
    /// Latest stats.
    pub statistics: MemoryStats,
}

/// Centralized Memory Engine.
pub struct MemoryEngine {
    initialized: bool,
    store: Arc<dyn MemoryStore>,
    /// Session-active project for automatic retrieval.
    active_project: Mutex<Option<String>>,
    /// Session-active conversation for context assembly.
    active_conversation: Mutex<Option<String>>,
}

impl MemoryEngine {
    /// Create an uninitialized engine backed by SQLite.
    pub fn new(store: Arc<SqliteMemoryStore>) -> Self {
        Self {
            initialized: false,
            store,
            active_project: Mutex::new(None),
            active_conversation: Mutex::new(None),
        }
    }

    /// Create from any [`MemoryStore`] implementation.
    pub fn with_store(store: Arc<dyn MemoryStore>) -> Self {
        Self {
            initialized: false,
            store,
            active_project: Mutex::new(None),
            active_conversation: Mutex::new(None),
        }
    }

    fn ensure_ready(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("memory engine is not initialized"))
        }
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    }

    fn snapshot_stats(&self) -> JaymiResult<MemoryStats> {
        let counts = self.store.counts_by_scope()?;
        let active_total = counts.iter().map(|(_, count)| *count).sum();
        Ok(MemoryStats {
            active_by_scope: counts
                .into_iter()
                .map(|(scope, count)| (scope.as_str().to_string(), count))
                .collect(),
            active_total,
            conversation_count: self.store.conversation_count()?,
            project_count: self.store.referenced_project_count()?,
        })
    }
}

impl MemoryEngineApi for MemoryEngine {
    fn retrieve(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>> {
        self.ensure_ready()?;
        // Conversation / project memory stay isolated to their owners.
        if query.scope == Some(MemoryScope::Conversation) && query.conversation_id.is_none() {
            return Ok(Vec::new());
        }
        if query.scope == Some(MemoryScope::Project) && query.project_id.is_none() {
            return Ok(Vec::new());
        }
        self.store.search(query)
    }

    fn assemble_context(
        &self,
        request: &AssembleContextRequest,
    ) -> JaymiResult<AssembledMemoryContext> {
        self.ensure_ready()?;
        let limit = request.limit.unwrap_or(DEFAULT_CONTEXT_LIMIT).max(1);
        let personal_limit = request
            .personal_limit
            .unwrap_or(DEFAULT_PERSONAL_LIMIT)
            .max(1);
        let project_limit = request
            .project_limit
            .unwrap_or(DEFAULT_PROJECT_LIMIT)
            .max(1);
        let conversation_limit = request
            .conversation_limit
            .unwrap_or(DEFAULT_CONVERSATION_LIMIT)
            .max(1);
        let working_limit = request
            .working_limit
            .unwrap_or(DEFAULT_WORKING_LIMIT)
            .max(1);
        let recent_limit = request.recent_limit.unwrap_or(DEFAULT_RECENT_LIMIT).max(1);

        let project_id = request
            .project_id
            .clone()
            .or_else(|| self.active_project_id())
            .filter(|id| !id.trim().is_empty());
        let conversation_id = request
            .conversation_id
            .clone()
            .or_else(|| self.active_conversation_id())
            .filter(|id| !id.trim().is_empty());

        let tokens = tokenize(&request.text);
        let now = Self::now();
        let recent_cutoff = now.saturating_sub(86_400);

        let mut candidates: Vec<MemoryRecord> = Vec::new();

        // Personal preferences — curated and bounded.
        let personal = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Personal),
            limit: Some(personal_limit),
            ..MemoryQuery::default()
        })?;
        candidates.extend(personal);

        // Active project memory only (score relevance in-process; do not require
        // a contiguous store substring match on the full request text).
        if let Some(project_id) = &project_id {
            let project = self.store.search(&MemoryQuery {
                scope: Some(MemoryScope::Project),
                project_id: Some(project_id.clone()),
                limit: Some(project_limit),
                ..MemoryQuery::default()
            })?;
            candidates.extend(project);
        }

        // Active conversation memory only.
        if let Some(conversation_id) = &conversation_id {
            let conversation = self.store.search(&MemoryQuery {
                scope: Some(MemoryScope::Conversation),
                conversation_id: Some(conversation_id.clone()),
                limit: Some(conversation_limit),
                ..MemoryQuery::default()
            })?;
            candidates.extend(conversation);
        }

        // Working / recent session memory.
        let working = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Working),
            limit: Some(working_limit),
            ..MemoryQuery::default()
        })?;
        candidates.extend(working);

        // Recent work across scopes (importance + updated_at ranking from store).
        let recent = self.store.search(&MemoryQuery {
            limit: Some(recent_limit.saturating_mul(2)),
            ..MemoryQuery::default()
        })?;
        candidates.extend(recent);

        // Deduplicate by id while preserving first-seen order.
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|record| seen.insert(record.id.as_str().to_string()));

        let mut scored = Vec::new();
        for record in candidates {
            let Some((score, reasons)) = score_candidate(
                &record,
                &tokens,
                project_id.as_deref(),
                conversation_id.as_deref(),
                now,
                recent_cutoff,
            ) else {
                continue;
            };
            scored.push(RelevantMemory {
                why: explain_reasons(&reasons, score),
                score,
                reasons,
                record,
            });
        }

        scored.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then(right.record.updated_at.cmp(&left.record.updated_at))
                .then(left.record.id.as_str().cmp(right.record.id.as_str()))
        });

        let candidate_count = scored.len();
        let truncated = candidate_count > limit;
        scored.truncate(limit);

        jaymi_logging::info(
            "memory",
            format!(
                "assembled context memories={} candidates={} truncated={} project={:?} conversation={:?}",
                scored.len(),
                candidate_count,
                truncated,
                project_id,
                conversation_id
            ),
        );

        Ok(AssembledMemoryContext {
            memories: scored,
            project_id,
            conversation_id,
            candidate_count,
            truncated,
        })
    }

    fn store(&self, request: &StoreMemoryRequest) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        let record = record_from_store(request, Self::now())?;
        self.store.insert(&record)?;
        jaymi_logging::info(
            "memory",
            format!(
                "stored memory id={} scope={}",
                record.id.as_str(),
                record.scope
            ),
        );
        Ok(record)
    }

    fn forget(&self, memory_id: &str) -> JaymiResult<()> {
        self.ensure_ready()?;
        self.store.forget(memory_id, Self::now())?;
        jaymi_logging::info("memory", format!("forgot memory id={memory_id}"));
        Ok(())
    }

    fn promote(&self, request: &PromoteMemoryRequest) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        if request.memory_id.trim().is_empty() {
            return Err(JaymiError::new("promote requires memory_id"));
        }
        let Some(mut record) = self.store.get(&request.memory_id)? else {
            return Err(JaymiError::new(format!(
                "memory not found: {}",
                request.memory_id
            )));
        };
        if record.status == MemoryStatus::Forgotten {
            return Err(JaymiError::new(format!(
                "cannot promote forgotten memory: {}",
                request.memory_id
            )));
        }
        if !is_upward_promotion(record.scope, request.to) {
            return Err(JaymiError::new(format!(
                "invalid promotion {} → {} (only upward Working→Conversation→Project→Personal)",
                record.scope, request.to
            )));
        }
        if request.to == MemoryScope::Conversation {
            let conversation_id = request
                .conversation_id
                .clone()
                .or_else(|| record.conversation_id.clone())
                .filter(|value| !value.trim().is_empty());
            let Some(conversation_id) = conversation_id else {
                return Err(JaymiError::new(
                    "promoting to conversation requires conversation_id",
                ));
            };
            record.conversation_id = Some(conversation_id);
        }
        if request.to == MemoryScope::Project {
            let project_id = request
                .project_id
                .clone()
                .or_else(|| record.project_id.clone())
                .filter(|value| !value.trim().is_empty());
            let Some(project_id) = project_id else {
                return Err(JaymiError::new("promoting to project requires project_id"));
            };
            record.project_id = Some(project_id);
        }
        if let Some(kind) = &request.kind {
            record.kind = Some(kind.clone());
        }
        let from = record.scope;
        record.scope = request.to;
        record.status = MemoryStatus::Active;
        record.updated_at = Self::now();
        record.archived_at = None;
        self.store.update(&record)?;
        jaymi_logging::info(
            "memory",
            format!(
                "promoted memory id={} from={} to={}",
                request.memory_id, from, request.to
            ),
        );
        Ok(record)
    }

    fn suggest_promotions(
        &self,
        query: &PromotionSuggestQuery,
    ) -> JaymiResult<Vec<PromotionSuggestion>> {
        self.ensure_ready()?;
        let limit = query.limit.unwrap_or(8).max(1);
        let mut candidates = Vec::new();

        // Working → Conversation (working notes often lack conversation_id yet).
        let working = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Working),
            limit: Some(50),
            ..MemoryQuery::default()
        })?;
        candidates.extend(working);

        // Conversation → Project (project_id may be supplied only on the query).
        let conversation = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Conversation),
            conversation_id: query.conversation_id.clone(),
            limit: Some(50),
            ..MemoryQuery::default()
        })?;
        candidates.extend(conversation);

        // Project → Personal
        let project = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: query.project_id.clone(),
            limit: Some(50),
            ..MemoryQuery::default()
        })?;
        candidates.extend(project);

        let mut suggestions = Vec::new();
        for record in candidates {
            let Some(to) = next_scope(record.scope) else {
                continue;
            };
            let score = score_promotion_candidate(&record, to, query);
            if score == 0 {
                continue;
            }
            suggestions.push(PromotionSuggestion {
                memory_id: record.id.as_str().to_string(),
                summary: record.summary.clone(),
                from: record.scope,
                to,
                reason: suggestion_reason(&record, to),
                score,
            });
        }

        suggestions.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then(left.memory_id.cmp(&right.memory_id))
        });
        suggestions.truncate(limit);
        Ok(suggestions)
    }

    fn archive_conversation(
        &self,
        request: &ArchiveConversationRequest,
    ) -> JaymiResult<ConversationArchive> {
        self.ensure_ready()?;
        if request.conversation_id.trim().is_empty() {
            return Err(JaymiError::new(
                "archive_conversation requires conversation_id",
            ));
        }
        if request.content.trim().is_empty() {
            return Err(JaymiError::new("archive_conversation requires content"));
        }

        let now = Self::now();
        let mut promoted_memory_id = None;
        if request.promote_summary {
            let summary = request
                .summary
                .clone()
                .unwrap_or_else(|| format!("Archived conversation {}", request.conversation_id));
            let stored = self.store(&StoreMemoryRequest {
                scope: MemoryScope::Conversation,
                summary,
                content: request.content.clone(),
                conversation_id: Some(request.conversation_id.clone()),
                project_id: None,
                importance: Some(60),
                confidence: Some(70),
                tags: vec!["archived_conversation".into()],
                source: Some("conversation_archive".into()),
                kind: None,
                metadata_json: None,
            })?;
            promoted_memory_id = Some(stored.id.as_str().to_string());
        }

        let archive = archive_from_request(
            request,
            format!("archive:{}:{now}", request.conversation_id),
            now,
            promoted_memory_id,
        );
        self.store.archive_conversation(&archive)?;
        jaymi_logging::info(
            "memory",
            format!(
                "archived conversation id={} archive={}",
                request.conversation_id, archive.archive_id
            ),
        );
        Ok(archive)
    }

    fn create_conversation(
        &self,
        request: &CreateConversationRequest,
    ) -> JaymiResult<ConversationMeta> {
        self.ensure_ready()?;
        let now = Self::now();
        let id = request
            .conversation_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("conversation:{}", now_nanos()));
        if self.store.get_conversation_meta(&id)?.is_some() {
            return Err(JaymiError::new(format!(
                "conversation already exists: {id}"
            )));
        }
        let project_id = request
            .project_id
            .clone()
            .or_else(|| self.active_project_id());
        // project_id is a Project Engine reference — Memory does not validate identity.
        let meta = ConversationMeta {
            id: jaymi_core::EntityId::new(id),
            title: request.title.clone(),
            project_id,
            created_at: now,
            updated_at: now,
            status: ConversationStatus::Active,
        };
        self.store.upsert_conversation_meta(&meta)?;
        jaymi_logging::info(
            "memory",
            format!("created conversation id={}", meta.id.as_str()),
        );
        Ok(meta)
    }

    fn append_message(&self, request: &AppendMessageRequest) -> JaymiResult<ConversationMessage> {
        self.ensure_ready()?;
        if request.conversation_id.trim().is_empty() {
            return Err(JaymiError::new("append_message requires conversation_id"));
        }
        let Some(meta) = self.store.get_conversation_meta(&request.conversation_id)? else {
            return Err(JaymiError::new(format!(
                "conversation not found: {}",
                request.conversation_id
            )));
        };
        let _ = meta;
        let created_at = request.created_at.unwrap_or_else(Self::now);
        let sequence_no = self.store.next_message_sequence(&request.conversation_id)?;
        let message_id = format!(
            "message:{}:{}:{}",
            request.conversation_id,
            sequence_no,
            now_nanos()
        );
        let message = ConversationMessage {
            id: jaymi_core::EntityId::new(message_id.clone()),
            conversation_id: request.conversation_id.clone(),
            role: request.role,
            content: request.content.clone(),
            created_at,
            sequence_no,
            attachments: request
                .attachments
                .iter()
                .enumerate()
                .map(|(index, attachment)| ConversationAttachment {
                    id: jaymi_core::EntityId::new(format!(
                        "attachment:{message_id}:{index}:{}",
                        now_nanos()
                    )),
                    kind: attachment.kind.clone(),
                    name: attachment.name.clone(),
                    uri: attachment.uri.clone(),
                    mime_type: attachment.mime_type.clone(),
                    size_bytes: attachment.size_bytes,
                    metadata_json: attachment
                        .metadata_json
                        .clone()
                        .unwrap_or_else(|| "{}".into()),
                })
                .collect(),
            references: request
                .references
                .iter()
                .enumerate()
                .map(|(index, reference)| ConversationReference {
                    id: jaymi_core::EntityId::new(format!(
                        "reference:{message_id}:{index}:{}",
                        now_nanos()
                    )),
                    kind: reference.kind.clone(),
                    target_id: reference.target_id.clone(),
                    label: reference.label.clone(),
                    uri: reference.uri.clone(),
                    metadata_json: reference
                        .metadata_json
                        .clone()
                        .unwrap_or_else(|| "{}".into()),
                })
                .collect(),
        };
        self.store.insert_conversation_message(&message)?;
        jaymi_logging::info(
            "memory",
            format!(
                "appended message id={} conversation={} role={}",
                message.id.as_str(),
                message.conversation_id,
                message.role
            ),
        );
        Ok(message)
    }

    fn load_conversation(&self, conversation_id: &str) -> JaymiResult<Option<Conversation>> {
        self.ensure_ready()?;
        if conversation_id.trim().is_empty() {
            return Err(JaymiError::new(
                "load_conversation requires conversation_id",
            ));
        }
        self.store.load_conversation(conversation_id)
    }

    fn list_conversations_for_project(
        &self,
        project_id: &str,
    ) -> JaymiResult<Vec<ConversationMeta>> {
        self.ensure_ready()?;
        let project_id = project_id.trim();
        if project_id.is_empty() {
            return Err(JaymiError::new(
                "list_conversations_for_project requires project_id",
            ));
        }
        let ids = self.store.list_conversation_ids_for_project(project_id)?;
        let mut metas = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(meta) = self.store.get_conversation_meta(&id)? {
                metas.push(meta);
            }
        }
        Ok(metas)
    }

    fn attach_conversation_to_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
    ) -> JaymiResult<ConversationMeta> {
        self.ensure_ready()?;
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            return Err(JaymiError::new(
                "attach_conversation_to_project requires conversation_id",
            ));
        }
        let Some(mut meta) = self.store.get_conversation_meta(conversation_id)? else {
            return Err(JaymiError::new(format!(
                "conversation not found: {conversation_id}"
            )));
        };
        let project_id = project_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        // project_id is a Project Engine reference — Memory does not validate identity.
        meta.project_id = project_id;
        meta.updated_at = Self::now();
        self.store.upsert_conversation_meta(&meta)?;
        jaymi_logging::info(
            "memory",
            format!(
                "attached conversation id={} project={:?}",
                meta.id.as_str(),
                meta.project_id
            ),
        );
        Ok(meta)
    }

    fn set_active_project(&self, project_id: Option<&str>) -> JaymiResult<()> {
        self.ensure_ready()?;
        let mut guard = self
            .active_project
            .lock()
            .map_err(|_| JaymiError::new("active project lock"))?;
        match project_id {
            None => {
                *guard = None;
                Ok(())
            }
            Some(id) => {
                let id = id.trim();
                if id.is_empty() {
                    return Err(JaymiError::new("set_active_project requires project_id"));
                }
                // Session hint only — Project Engine owns whether the id exists.
                *guard = Some(id.to_string());
                Ok(())
            }
        }
    }

    fn active_project_id(&self) -> Option<String> {
        self.active_project
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn set_active_conversation(&self, conversation_id: Option<&str>) -> JaymiResult<()> {
        self.ensure_ready()?;
        let mut guard = self
            .active_conversation
            .lock()
            .map_err(|_| JaymiError::new("active conversation lock"))?;
        match conversation_id {
            None => {
                *guard = None;
                Ok(())
            }
            Some(id) => {
                let id = id.trim();
                if id.is_empty() {
                    return Err(JaymiError::new(
                        "set_active_conversation requires conversation_id",
                    ));
                }
                // Allow activating before transcript creation; isolation still
                // requires the id when retrieving conversation-scoped memory.
                *guard = Some(id.to_string());
                Ok(())
            }
        }
    }

    fn active_conversation_id(&self) -> Option<String> {
        self.active_conversation
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn store_project_memory(
        &self,
        request: &StoreProjectMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        if request.project_id.trim().is_empty() {
            return Err(JaymiError::new("store_project_memory requires project_id"));
        }
        let mut tags = request.tags.clone();
        let kind_tag = format!("kind:{}", request.kind.as_str());
        if !tags.iter().any(|tag| tag == &kind_tag) {
            tags.push(kind_tag);
        }
        self.store(&StoreMemoryRequest {
            scope: MemoryScope::Project,
            summary: request.summary.clone(),
            content: request.content.clone(),
            conversation_id: request.conversation_id.clone(),
            project_id: Some(request.project_id.clone()),
            importance: request.importance,
            confidence: request.confidence,
            tags,
            source: request.source.clone(),
            kind: Some(request.kind.as_str().to_string()),
            metadata_json: None,
        })
    }

    fn store_project_decision(
        &self,
        request: &StoreProjectDecisionRequest,
    ) -> JaymiResult<ProjectDecision> {
        self.ensure_ready()?;
        if request.project_id.trim().is_empty() {
            return Err(JaymiError::new(
                "store_project_decision requires project_id",
            ));
        }
        if request.title.trim().is_empty() {
            return Err(JaymiError::new("store_project_decision requires a title"));
        }
        let mut related_conversations = request.related_conversations.clone();
        if let Some(conversation_id) = &request.conversation_id {
            if !related_conversations.iter().any(|id| id == conversation_id) {
                related_conversations.insert(0, conversation_id.clone());
            }
        }
        let metadata_json = encode_decision_metadata(
            &request.reasoning,
            &request.related_files,
            &related_conversations,
        );
        let mut tags = vec!["decision-log".into()];
        let kind_tag = format!("kind:{}", ProjectMemoryKind::ArchitectureDecision.as_str());
        tags.push(kind_tag);
        let record = self.store(&StoreMemoryRequest {
            scope: MemoryScope::Project,
            summary: request.title.clone(),
            content: request.description.clone(),
            conversation_id: request.conversation_id.clone(),
            project_id: Some(request.project_id.clone()),
            importance: request.importance.or(Some(90)),
            confidence: request.confidence.or(Some(90)),
            tags,
            source: request.source.clone(),
            kind: Some(ProjectMemoryKind::ArchitectureDecision.as_str().to_string()),
            metadata_json: Some(metadata_json),
        })?;
        project_decision_from_record(&record)
            .ok_or_else(|| JaymiError::new("failed to decode stored project decision"))
    }

    fn list_project_decisions(
        &self,
        query: &ListProjectDecisionsQuery,
    ) -> JaymiResult<Vec<ProjectDecision>> {
        self.ensure_ready()?;
        let project_id = query.project_id.trim();
        if project_id.is_empty() {
            return Err(JaymiError::new(
                "list_project_decisions requires project_id",
            ));
        }
        let records = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: Some(project_id.to_string()),
            kind: Some(ProjectMemoryKind::ArchitectureDecision.as_str().to_string()),
            text: query.text.clone(),
            limit: query.limit.or(Some(50)),
            ..MemoryQuery::default()
        })?;
        let mut decisions: Vec<_> = records
            .iter()
            .filter_map(project_decision_from_record)
            .collect();
        decisions.sort_by(|left, right| {
            right
                .timestamp
                .cmp(&left.timestamp)
                .then(left.memory_id.cmp(&right.memory_id))
        });
        Ok(decisions)
    }

    fn get_project_decision(&self, memory_id: &str) -> JaymiResult<Option<ProjectDecision>> {
        self.ensure_ready()?;
        let id = memory_id.trim();
        if id.is_empty() {
            return Err(JaymiError::new("get_project_decision requires memory_id"));
        }
        let Some(record) = self.store.get(id)? else {
            return Ok(None);
        };
        Ok(project_decision_from_record(&record))
    }

    fn restore_project_memories(&self, project_id: &str) -> JaymiResult<ProjectMemoryBundle> {
        self.ensure_ready()?;
        let project_id = project_id.trim();
        if project_id.is_empty() {
            return Err(JaymiError::new(
                "restore_project_memories requires project_id",
            ));
        }
        let memories = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: Some(project_id.to_string()),
            limit: Some(200),
            ..MemoryQuery::default()
        })?;

        let mut context = ProjectMemoryBundle {
            project_id: project_id.to_string(),
            name: project_id.to_string(),
            conversation_ids: self.store.list_conversation_ids_for_project(project_id)?,
            ..ProjectMemoryBundle::default()
        };

        for record in memories {
            match record
                .kind
                .as_deref()
                .and_then(ProjectMemoryKind::parse)
                .unwrap_or(ProjectMemoryKind::Conversation)
            {
                ProjectMemoryKind::Conversation => context.conversations.push(record),
                ProjectMemoryKind::ArchitectureDecision => {
                    context.architecture_decisions.push(record)
                }
                ProjectMemoryKind::Task => context.tasks.push(record),
                ProjectMemoryKind::CodingPreference => context.coding_preferences.push(record),
                ProjectMemoryKind::ImportantFile => context.important_files.push(record),
                ProjectMemoryKind::Milestone => context.milestones.push(record),
            }
        }
        Ok(context)
    }

    fn create_personal_memory(
        &self,
        request: &CreatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        if request.summary.trim().is_empty() && request.content.trim().is_empty() {
            return Err(JaymiError::new(
                "create_personal_memory requires a non-empty summary or content",
            ));
        }
        let existing = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Personal),
            kind: Some(request.kind.as_str().to_string()),
            limit: Some(1),
            ..MemoryQuery::default()
        })?;
        if !existing.is_empty() {
            return Err(JaymiError::new(format!(
                "personal memory kind '{}' already exists (id={}); update or delete it instead",
                request.kind,
                existing[0].id.as_str()
            )));
        }
        let mut tags = request.tags.clone();
        let kind_tag = format!("kind:{}", request.kind.as_str());
        if !tags.iter().any(|tag| tag == &kind_tag) {
            tags.push(kind_tag);
        }
        let stored = self.store(&StoreMemoryRequest {
            scope: MemoryScope::Personal,
            summary: request.summary.clone(),
            content: request.content.clone(),
            conversation_id: None,
            project_id: None,
            importance: request.importance.or(Some(70)),
            confidence: request.confidence.or(Some(80)),
            tags,
            source: request
                .source
                .clone()
                .or_else(|| Some("intentional_personal".into())),
            kind: Some(request.kind.as_str().to_string()),
            metadata_json: None,
        })?;
        jaymi_logging::info(
            "memory",
            format!(
                "created personal memory id={} kind={}",
                stored.id.as_str(),
                request.kind
            ),
        );
        Ok(stored)
    }

    fn update_personal_memory(
        &self,
        request: &UpdatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        if request.memory_id.trim().is_empty() {
            return Err(JaymiError::new("update_personal_memory requires memory_id"));
        }
        let Some(mut record) = self.store.get(&request.memory_id)? else {
            return Err(JaymiError::new(format!(
                "memory not found: {}",
                request.memory_id
            )));
        };
        if record.scope != MemoryScope::Personal {
            return Err(JaymiError::new(format!(
                "memory {} is not personal scope",
                request.memory_id
            )));
        }
        if record.status == MemoryStatus::Forgotten {
            return Err(JaymiError::new(format!(
                "cannot update forgotten personal memory: {}",
                request.memory_id
            )));
        }
        if let Some(summary) = &request.summary {
            record.summary = summary.trim().to_string();
        }
        if let Some(content) = &request.content {
            record.content = content.trim().to_string();
        }
        if let Some(importance) = request.importance {
            record.importance = importance.min(100);
        }
        if let Some(confidence) = request.confidence {
            record.confidence = confidence.min(100);
        }
        if let Some(tags) = &request.tags {
            record.tags = tags.clone();
        }
        if record.summary.trim().is_empty() && record.content.trim().is_empty() {
            return Err(JaymiError::new(
                "personal memory requires a non-empty summary or content",
            ));
        }
        record.updated_at = Self::now();
        self.store.update(&record)?;
        jaymi_logging::info(
            "memory",
            format!("updated personal memory id={}", record.id.as_str()),
        );
        Ok(record)
    }

    fn delete_personal_memory(&self, memory_id: &str) -> JaymiResult<()> {
        self.ensure_ready()?;
        let Some(record) = self.store.get(memory_id)? else {
            return Err(JaymiError::new(format!("memory not found: {memory_id}")));
        };
        if record.scope != MemoryScope::Personal {
            return Err(JaymiError::new(format!(
                "memory {memory_id} is not personal scope"
            )));
        }
        self.forget(memory_id)?;
        jaymi_logging::info("memory", format!("deleted personal memory id={memory_id}"));
        Ok(())
    }

    fn personal_context(&self) -> JaymiResult<PersonalContext> {
        self.ensure_ready()?;
        let memories = self.store.search(&MemoryQuery {
            scope: Some(MemoryScope::Personal),
            limit: Some(100),
            ..MemoryQuery::default()
        })?;
        let mut context = PersonalContext::default();
        for record in memories {
            match record.kind.as_deref().and_then(PersonalMemoryKind::parse) {
                Some(PersonalMemoryKind::PreferredName) => context.preferred_name.push(record),
                Some(PersonalMemoryKind::WritingStyle) => context.writing_style.push(record),
                Some(PersonalMemoryKind::CodeStyle) => context.code_style.push(record),
                Some(PersonalMemoryKind::FavoriteEditor) => context.favorite_editor.push(record),
                Some(PersonalMemoryKind::PreferredTheme) => context.preferred_theme.push(record),
                None => {
                    // Intentionally ignore non-preference personal rows.
                }
            }
        }
        Ok(context)
    }

    fn stats(&self) -> JaymiResult<MemoryStats> {
        self.ensure_ready()?;
        self.snapshot_stats()
    }

    fn health(&self) -> JaymiResult<MemoryHealth> {
        let report = self.health_check();
        let statistics = if self.initialized {
            self.snapshot_stats().unwrap_or_default()
        } else {
            MemoryStats::default()
        };
        let detail = if !report.initialized {
            "memory engine is not initialized".to_string()
        } else {
            let scopes = if statistics.active_by_scope.is_empty() {
                "none".to_string()
            } else {
                statistics
                    .active_by_scope
                    .iter()
                    .map(|(scope, count)| format!("{scope}={count}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!(
                "active={} conversations={} projects={} scopes=[{scopes}]",
                statistics.active_total, statistics.conversation_count, statistics.project_count
            )
        };
        Ok(MemoryHealth {
            initialized: report.initialized,
            healthy: report.healthy && report.initialized,
            version: report.version,
            detail,
            statistics,
        })
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

impl Lifecycle for MemoryEngine {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![("status".to_string(), "operational".to_string())])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppendMessageRequest, AssembleContextRequest, ConversationAttachmentInput,
        ConversationReferenceInput, CreateConversationRequest, CreatePersonalMemoryRequest,
        InMemoryMemoryStore, MemoryQuery, MemoryScope, MessageRole, PersonalMemoryKind,
        PromoteMemoryRequest, PromotionSuggestQuery, StoreMemoryRequest,
        UpdatePersonalMemoryRequest,
    };
    use jaymi_core::Lifecycle;
    use std::sync::Arc;

    #[test]
    fn store_retrieve_forget_and_promote() {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();

        let stored = engine
            .store(&StoreMemoryRequest {
                scope: MemoryScope::Working,
                summary: "Temporary fact".into(),
                content: "Remember the temporary fact about rustc.".into(),
                conversation_id: None,
                project_id: None,
                importance: Some(55),
                confidence: Some(60),
                tags: vec!["temp".into()],
                source: None,
                kind: None,
                metadata_json: None,
            })
            .unwrap();

        let hits = engine
            .retrieve(&MemoryQuery {
                text: Some("rustc".into()),
                ..MemoryQuery::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, stored.id);

        let promoted = engine
            .promote(&PromoteMemoryRequest {
                memory_id: stored.id.as_str().to_string(),
                to: MemoryScope::Conversation,
                conversation_id: Some("conv-promote".into()),
                project_id: None,
                kind: None,
            })
            .unwrap();
        assert_eq!(promoted.scope, MemoryScope::Conversation);
        assert_eq!(promoted.conversation_id.as_deref(), Some("conv-promote"));

        engine.forget(stored.id.as_str()).unwrap();
        let after = engine
            .retrieve(&MemoryQuery {
                text: Some("rustc".into()),
                include_archived: true,
                conversation_id: Some("conv-promote".into()),
                ..MemoryQuery::default()
            })
            .unwrap();
        assert!(after.is_empty());
    }

    #[test]
    fn promotion_rejects_demotion_and_suggestions_never_apply() {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();

        let project = engine
            .store(&StoreMemoryRequest {
                scope: MemoryScope::Project,
                summary: "Coding preference".into(),
                content: "Prefer explicit Rust APIs.".into(),
                conversation_id: None,
                project_id: Some("project:jaymi".into()),
                importance: Some(95),
                confidence: Some(90),
                tags: vec!["preference".into()],
                source: None,
                kind: Some("coding_preference".into()),
                metadata_json: None,
            })
            .unwrap();

        let demote = engine.promote(&PromoteMemoryRequest {
            memory_id: project.id.as_str().to_string(),
            to: MemoryScope::Working,
            conversation_id: None,
            project_id: None,
            kind: None,
        });
        assert!(demote.is_err());

        let suggestions = engine
            .suggest_promotions(&PromotionSuggestQuery {
                project_id: Some("project:jaymi".into()),
                min_importance: Some(70),
                limit: Some(5),
                ..PromotionSuggestQuery::default()
            })
            .unwrap();
        assert!(suggestions
            .iter()
            .any(|s| s.memory_id == project.id.as_str() && s.to == MemoryScope::Personal));

        let still = engine
            .retrieve(&MemoryQuery {
                scope: Some(MemoryScope::Project),
                project_id: Some("project:jaymi".into()),
                text: Some("explicit Rust".into()),
                ..MemoryQuery::default()
            })
            .unwrap();
        assert_eq!(still.len(), 1);
        assert_eq!(still[0].scope, MemoryScope::Project);
    }

    #[test]
    fn conversation_transcript_persists_and_stays_isolated() {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();

        let meta = engine
            .create_conversation(&CreateConversationRequest {
                conversation_id: Some("conv-a".into()),
                title: Some("Planning".into()),
                project_id: None,
            })
            .unwrap();
        assert_eq!(meta.id.as_str(), "conv-a");

        let user = engine
            .append_message(&AppendMessageRequest {
                conversation_id: "conv-a".into(),
                role: MessageRole::User,
                content: "Remember this attachment".into(),
                created_at: Some(100),
                attachments: vec![ConversationAttachmentInput {
                    kind: "file".into(),
                    name: Some("notes.md".into()),
                    uri: Some("/tmp/notes.md".into()),
                    mime_type: Some("text/markdown".into()),
                    size_bytes: Some(12),
                    metadata_json: None,
                }],
                references: vec![ConversationReferenceInput {
                    kind: "document".into(),
                    target_id: Some("doc-1".into()),
                    label: Some("notes".into()),
                    uri: Some("file:///tmp/notes.md".into()),
                    metadata_json: None,
                }],
            })
            .unwrap();
        let assistant = engine
            .append_message(&AppendMessageRequest {
                conversation_id: "conv-a".into(),
                role: MessageRole::Assistant,
                content: "Saved.".into(),
                created_at: Some(101),
                attachments: vec![],
                references: vec![],
            })
            .unwrap();

        let loaded = engine.load_conversation("conv-a").unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].id, user.id);
        assert_eq!(loaded.messages[0].role, MessageRole::User);
        assert_eq!(loaded.messages[0].created_at, 100);
        assert_eq!(loaded.messages[0].attachments.len(), 1);
        assert_eq!(loaded.messages[0].references.len(), 1);
        assert_eq!(loaded.messages[1].id, assistant.id);
        assert_eq!(loaded.messages[1].role, MessageRole::Assistant);

        // Conversation-scoped intentional memory must not leak without conversation_id.
        engine
            .store(&StoreMemoryRequest {
                scope: MemoryScope::Conversation,
                summary: "Secret to A".into(),
                content: "Only for conversation A".into(),
                conversation_id: Some("conv-a".into()),
                project_id: None,
                importance: Some(90),
                confidence: Some(90),
                tags: vec![],
                source: None,
                kind: None,
                metadata_json: None,
            })
            .unwrap();
        let leaked = engine
            .retrieve(&MemoryQuery {
                text: Some("Only for conversation".into()),
                ..MemoryQuery::default()
            })
            .unwrap();
        assert!(leaked.is_empty());
        let scoped = engine
            .retrieve(&MemoryQuery {
                text: Some("Only for conversation".into()),
                conversation_id: Some("conv-a".into()),
                ..MemoryQuery::default()
            })
            .unwrap();
        assert_eq!(scoped.len(), 1);
    }

    #[test]
    fn personal_preferences_create_update_delete_intentionally() {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();

        let created = engine
            .create_personal_memory(&CreatePersonalMemoryRequest {
                kind: PersonalMemoryKind::PreferredName,
                summary: "Preferred name".into(),
                content: "Charlie".into(),
                importance: Some(90),
                confidence: Some(95),
                tags: vec![],
                source: Some("user_request".into()),
            })
            .unwrap();
        assert_eq!(created.scope, MemoryScope::Personal);
        assert_eq!(created.kind.as_deref(), Some("preferred_name"));

        let duplicate = engine.create_personal_memory(&CreatePersonalMemoryRequest {
            kind: PersonalMemoryKind::PreferredName,
            summary: "Preferred name".into(),
            content: "Other".into(),
            importance: None,
            confidence: None,
            tags: vec![],
            source: None,
        });
        assert!(duplicate.is_err());

        let updated = engine
            .update_personal_memory(&UpdatePersonalMemoryRequest {
                memory_id: created.id.as_str().to_string(),
                summary: None,
                content: Some("Chuck".into()),
                importance: Some(95),
                confidence: None,
                tags: None,
            })
            .unwrap();
        assert_eq!(updated.content, "Chuck");
        assert_eq!(updated.importance, 95);

        let context = engine.personal_context().unwrap();
        assert_eq!(context.preferred_name.len(), 1);
        assert_eq!(context.preferred_name[0].content, "Chuck");

        engine.delete_personal_memory(created.id.as_str()).unwrap();
        let after = engine.personal_context().unwrap();
        assert!(after.preferred_name.is_empty());
    }

    #[test]
    fn assemble_context_respects_limits_and_isolation() {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();

        // Project Engine owns identity — Memory only needs the project_id string.
        engine.set_active_project(Some("project:jaymi")).unwrap();
        engine.set_active_conversation(Some("conv-a")).unwrap();

        let kept = engine
            .store(&StoreMemoryRequest {
                scope: MemoryScope::Project,
                summary: "Architecture decision".into(),
                content: "Planner orchestrates memory assembly.".into(),
                conversation_id: None,
                project_id: Some("project:jaymi".into()),
                importance: Some(90),
                confidence: Some(90),
                tags: vec![],
                source: None,
                kind: Some("architecture_decision".into()),
                metadata_json: None,
            })
            .unwrap();
        engine
            .store(&StoreMemoryRequest {
                scope: MemoryScope::Project,
                summary: "Foreign".into(),
                content: "foreign-token".into(),
                conversation_id: None,
                project_id: Some("project:other".into()),
                importance: Some(99),
                confidence: Some(99),
                tags: vec![],
                source: None,
                kind: None,
                metadata_json: None,
            })
            .unwrap();

        for index in 0..20 {
            engine
                .store(&StoreMemoryRequest {
                    scope: MemoryScope::Working,
                    summary: format!("noise-{index}"),
                    content: format!("filler {index}"),
                    conversation_id: None,
                    project_id: None,
                    importance: Some(5),
                    confidence: Some(5),
                    tags: vec![],
                    source: None,
                    kind: None,
                    metadata_json: None,
                })
                .unwrap();
        }

        let assembled = engine
            .assemble_context(&AssembleContextRequest {
                text: "planner memory assembly".into(),
                limit: Some(4),
                ..AssembleContextRequest::default()
            })
            .unwrap();

        assert!(assembled.len() <= 4);
        assert!(assembled
            .records()
            .iter()
            .any(|record| record.id == kept.id));
        assert!(assembled
            .records()
            .iter()
            .all(|record| record.content != "foreign-token"));
        assert_eq!(assembled.project_id.as_deref(), Some("project:jaymi"));
        assert_eq!(assembled.conversation_id.as_deref(), Some("conv-a"));
    }
}
