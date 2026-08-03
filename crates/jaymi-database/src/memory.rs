//! Intentional memory persistence (separate from content / inventory).

use rusqlite::{params, OptionalExtension};

use jaymi_core::{JaymiError, JaymiResult};

use crate::Database;

/// Persisted memory row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    /// Stable memory identity.
    pub memory_id: String,
    /// Scope label (`working` / `conversation` / `project` / `personal`).
    pub scope: String,
    /// Short summary.
    pub summary: String,
    /// Detailed content.
    pub content: String,
    /// Associated conversation id.
    pub conversation_id: Option<String>,
    /// Associated project id.
    pub project_id: Option<String>,
    /// Importance `0..=100`.
    pub importance: i64,
    /// Confidence `0..=100`.
    pub confidence: i64,
    /// JSON array of tags.
    pub tags_json: String,
    /// Provenance label.
    pub source: Option<String>,
    /// Optional structured kind (project memory categories, etc.).
    pub kind: Option<String>,
    /// Free-form JSON metadata (decision reasoning / relations, etc.).
    pub metadata_json: String,
    /// Status label (`active` / `archived` / `forgotten`).
    pub status: String,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds updated.
    pub updated_at: i64,
    /// Unix seconds archived.
    pub archived_at: Option<i64>,
}

/// Memory search filters.
#[derive(Debug, Clone, Default)]
pub struct MemorySearchQuery {
    /// Case-insensitive substring against summary/content/tags.
    pub text: Option<String>,
    /// Exact scope.
    pub scope: Option<String>,
    /// Exact project id.
    pub project_id: Option<String>,
    /// Exact conversation id.
    pub conversation_id: Option<String>,
    /// Exact structured kind.
    pub kind: Option<String>,
    /// Include archived rows (forgotten are never returned).
    pub include_archived: bool,
    /// Result limit.
    pub limit: Option<usize>,
}

/// Archived conversation row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationArchiveRecord {
    /// Archive identity.
    pub archive_id: String,
    /// Conversation identity.
    pub conversation_id: String,
    /// Optional title.
    pub title: Option<String>,
    /// Archived body.
    pub content: String,
    /// Unix seconds archived.
    pub archived_at: i64,
    /// Optional promoted memory id.
    pub promoted_memory_id: Option<String>,
}

impl Database {
    /// Insert or replace a memory row.
    pub fn upsert_memory(&self, record: &MemoryRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO memories (
                    memory_id, scope, summary, content, conversation_id, project_id,
                    importance, confidence, tags_json, source, kind, status,
                    created_at, updated_at, archived_at, metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(memory_id) DO UPDATE SET
                    scope = excluded.scope,
                    summary = excluded.summary,
                    content = excluded.content,
                    conversation_id = excluded.conversation_id,
                    project_id = excluded.project_id,
                    importance = excluded.importance,
                    confidence = excluded.confidence,
                    tags_json = excluded.tags_json,
                    source = excluded.source,
                    kind = excluded.kind,
                    status = excluded.status,
                    updated_at = excluded.updated_at,
                    archived_at = excluded.archived_at,
                    metadata_json = excluded.metadata_json",
                params![
                    record.memory_id,
                    record.scope,
                    record.summary,
                    record.content,
                    record.conversation_id,
                    record.project_id,
                    record.importance,
                    record.confidence,
                    record.tags_json,
                    record.source,
                    record.kind,
                    record.status,
                    record.created_at,
                    record.updated_at,
                    record.archived_at,
                    record.metadata_json,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Load one memory by id.
    pub fn get_memory(&self, memory_id: &str) -> JaymiResult<Option<MemoryRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT memory_id, scope, summary, content, conversation_id, project_id,
                        importance, confidence, tags_json, source, kind, status,
                        created_at, updated_at, archived_at, metadata_json
                 FROM memories WHERE memory_id = ?1",
                params![memory_id],
                map_memory_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Search memories with deterministic ordering (importance desc, updated desc, id).
    pub fn search_memories(&self, query: &MemorySearchQuery) -> JaymiResult<Vec<MemoryRecord>> {
        self.with_connection(|conn| {
            let mut sql = String::from(
                "SELECT memory_id, scope, summary, content, conversation_id, project_id,
                        importance, confidence, tags_json, source, kind, status,
                        created_at, updated_at, archived_at, metadata_json
                 FROM memories WHERE status != 'forgotten'",
            );
            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if !query.include_archived {
                sql.push_str(" AND status = 'active'");
            } else {
                sql.push_str(" AND status IN ('active', 'archived')");
            }
            if let Some(scope) = &query.scope {
                sql.push_str(" AND scope = ?");
                params_vec.push(Box::new(scope.clone()));
            }
            if let Some(project_id) = &query.project_id {
                sql.push_str(" AND project_id = ?");
                params_vec.push(Box::new(project_id.clone()));
            } else {
                // Isolation: project-scoped memories never leak across projects.
                sql.push_str(" AND scope != 'project'");
            }
            if let Some(conversation_id) = &query.conversation_id {
                sql.push_str(" AND conversation_id = ?");
                params_vec.push(Box::new(conversation_id.clone()));
            } else {
                // Isolation: conversation-scoped memories never leak across conversations.
                sql.push_str(" AND scope != 'conversation'");
            }
            if let Some(kind) = &query.kind {
                sql.push_str(" AND kind = ?");
                params_vec.push(Box::new(kind.clone()));
            }
            if let Some(text) = query
                .text
                .as_ref()
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
            {
                sql.push_str(
                    " AND (lower(summary) LIKE ? OR lower(content) LIKE ? OR lower(tags_json) LIKE ? OR lower(metadata_json) LIKE ?)",
                );
                let pattern = format!("%{text}%");
                params_vec.push(Box::new(pattern.clone()));
                params_vec.push(Box::new(pattern.clone()));
                params_vec.push(Box::new(pattern.clone()));
                params_vec.push(Box::new(pattern));
            }

            sql.push_str(" ORDER BY importance DESC, updated_at DESC, memory_id ASC");
            if let Some(limit) = query.limit {
                sql.push_str(" LIMIT ?");
                params_vec.push(Box::new(limit as i64));
            }

            let mut stmt = conn.prepare(&sql).map_err(db_error)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|value| value.as_ref()).collect();
            let rows = stmt
                .query_map(param_refs.as_slice(), map_memory_row)
                .map_err(db_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(db_error)?);
            }
            Ok(out)
        })
    }

    /// Mark a memory forgotten.
    pub fn forget_memory(&self, memory_id: &str, now: i64) -> JaymiResult<()> {
        self.with_connection(|conn| {
            let changed = conn
                .execute(
                    "UPDATE memories
                     SET status = 'forgotten', updated_at = ?2, archived_at = COALESCE(archived_at, ?2)
                     WHERE memory_id = ?1 AND status != 'forgotten'",
                    params![memory_id, now],
                )
                .map_err(db_error)?;
            if changed == 0 {
                return Err(JaymiError::new(format!(
                    "memory not found or already forgotten: {memory_id}"
                )));
            }
            Ok(())
        })
    }

    /// Insert or replace a conversation archive.
    pub fn upsert_conversation_archive(
        &self,
        record: &ConversationArchiveRecord,
    ) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO conversation_archives (
                    archive_id, conversation_id, title, content, archived_at, promoted_memory_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(archive_id) DO UPDATE SET
                    conversation_id = excluded.conversation_id,
                    title = excluded.title,
                    content = excluded.content,
                    archived_at = excluded.archived_at,
                    promoted_memory_id = excluded.promoted_memory_id",
                params![
                    record.archive_id,
                    record.conversation_id,
                    record.title,
                    record.content,
                    record.archived_at,
                    record.promoted_memory_id,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Active memory counts grouped by scope.
    pub fn memory_counts_by_scope(&self) -> JaymiResult<Vec<(String, u64)>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT scope, COUNT(*) FROM memories
                     WHERE status = 'active'
                     GROUP BY scope
                     ORDER BY scope ASC",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
                })
                .map_err(db_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(db_error)?);
            }
            Ok(out)
        })
    }
}

fn map_memory_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    Ok(MemoryRecord {
        memory_id: row.get(0)?,
        scope: row.get(1)?,
        summary: row.get(2)?,
        content: row.get(3)?,
        conversation_id: row.get(4)?,
        project_id: row.get(5)?,
        importance: row.get(6)?,
        confidence: row.get(7)?,
        tags_json: row.get(8)?,
        source: row.get(9)?,
        kind: row.get(10)?,
        status: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        archived_at: row.get(14)?,
        metadata_json: row.get(15)?,
    })
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
