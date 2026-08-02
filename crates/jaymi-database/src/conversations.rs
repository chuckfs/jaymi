//! Conversation transcript persistence (history, not intentional memory).
//!
//! Conversations are isolated: loading one never mutates or leaks into another.

use rusqlite::{params, OptionalExtension};

use jaymi_core::{JaymiError, JaymiResult};

use crate::Database;

/// Persisted conversation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationRecord {
    /// Stable conversation identity.
    pub conversation_id: String,
    /// Optional title.
    pub title: Option<String>,
    /// Optional owning project.
    pub project_id: Option<String>,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds last updated.
    pub updated_at: i64,
    /// Status label (`active` / `archived` / `closed`).
    pub status: String,
}

/// Persisted conversation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessageRecord {
    /// Stable message identity.
    pub message_id: String,
    /// Owning conversation.
    pub conversation_id: String,
    /// Role label (`user` / `assistant` / `system`).
    pub role: String,
    /// Message body.
    pub content: String,
    /// Unix seconds created.
    pub created_at: i64,
    /// Deterministic order within the conversation.
    pub sequence_no: i64,
}

/// Attachment linked to a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationAttachmentRecord {
    /// Stable attachment identity.
    pub attachment_id: String,
    /// Parent message.
    pub message_id: String,
    /// Kind label (`file` / `image` / `url` / `other`).
    pub kind: String,
    /// Display name.
    pub name: Option<String>,
    /// Local path or URI.
    pub uri: Option<String>,
    /// MIME type when known.
    pub mime_type: Option<String>,
    /// Size in bytes when known.
    pub size_bytes: Option<i64>,
    /// JSON metadata object.
    pub metadata_json: String,
}

/// Reference linked to a message (citation, document, memory, tool, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationReferenceRecord {
    /// Stable reference identity.
    pub reference_id: String,
    /// Parent message.
    pub message_id: String,
    /// Kind label (`memory` / `document` / `citation` / `tool` / `other`).
    pub kind: String,
    /// Target identity when known.
    pub target_id: Option<String>,
    /// Human-readable label.
    pub label: Option<String>,
    /// URI when known.
    pub uri: Option<String>,
    /// JSON metadata object.
    pub metadata_json: String,
}

/// Fully loaded conversation for exact reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConversationRecord {
    /// Conversation metadata.
    pub conversation: ConversationRecord,
    /// Ordered messages with attachments and references.
    pub messages: Vec<LoadedMessageRecord>,
}

/// Message plus its attachments and references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedMessageRecord {
    /// Message row.
    pub message: ConversationMessageRecord,
    /// Attachments in stable id order.
    pub attachments: Vec<ConversationAttachmentRecord>,
    /// References in stable id order.
    pub references: Vec<ConversationReferenceRecord>,
}

impl Database {
    /// Insert or update conversation metadata.
    pub fn upsert_conversation(&self, record: &ConversationRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO conversations (
                    conversation_id, title, project_id, created_at, updated_at, status
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(conversation_id) DO UPDATE SET
                    title = excluded.title,
                    project_id = excluded.project_id,
                    updated_at = excluded.updated_at,
                    status = excluded.status",
                params![
                    record.conversation_id,
                    record.title,
                    record.project_id,
                    record.created_at,
                    record.updated_at,
                    record.status,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Load conversation metadata.
    pub fn get_conversation(
        &self,
        conversation_id: &str,
    ) -> JaymiResult<Option<ConversationRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT conversation_id, title, project_id, created_at, updated_at, status
                 FROM conversations WHERE conversation_id = ?1",
                params![conversation_id],
                |row| {
                    Ok(ConversationRecord {
                        conversation_id: row.get(0)?,
                        title: row.get(1)?,
                        project_id: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        status: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Insert a message row (identity must be unique).
    pub fn insert_conversation_message(
        &self,
        record: &ConversationMessageRecord,
    ) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO conversation_messages (
                    message_id, conversation_id, role, content, created_at, sequence_no
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    record.message_id,
                    record.conversation_id,
                    record.role,
                    record.content,
                    record.created_at,
                    record.sequence_no,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Next sequence number for a conversation (0-based).
    pub fn next_conversation_sequence(&self, conversation_id: &str) -> JaymiResult<i64> {
        self.with_connection(|conn| {
            let max: Option<i64> = conn
                .query_row(
                    "SELECT MAX(sequence_no) FROM conversation_messages
                     WHERE conversation_id = ?1",
                    params![conversation_id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            Ok(max.map(|value| value + 1).unwrap_or(0))
        })
    }

    /// Insert an attachment row.
    pub fn insert_conversation_attachment(
        &self,
        record: &ConversationAttachmentRecord,
    ) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO conversation_attachments (
                    attachment_id, message_id, kind, name, uri, mime_type, size_bytes, metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    record.attachment_id,
                    record.message_id,
                    record.kind,
                    record.name,
                    record.uri,
                    record.mime_type,
                    record.size_bytes,
                    record.metadata_json,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Insert a reference row.
    pub fn insert_conversation_reference(
        &self,
        record: &ConversationReferenceRecord,
    ) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO conversation_references (
                    reference_id, message_id, kind, target_id, label, uri, metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    record.reference_id,
                    record.message_id,
                    record.kind,
                    record.target_id,
                    record.label,
                    record.uri,
                    record.metadata_json,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Load an entire conversation exactly as stored (ordered messages).
    pub fn load_conversation(
        &self,
        conversation_id: &str,
    ) -> JaymiResult<Option<LoadedConversationRecord>> {
        let Some(conversation) = self.get_conversation(conversation_id)? else {
            return Ok(None);
        };

        self.with_connection(|conn| {
            let mut msg_stmt = conn
                .prepare(
                    "SELECT message_id, conversation_id, role, content, created_at, sequence_no
                     FROM conversation_messages
                     WHERE conversation_id = ?1
                     ORDER BY sequence_no ASC, message_id ASC",
                )
                .map_err(db_error)?;
            let message_rows = msg_stmt
                .query_map(params![conversation_id], |row| {
                    Ok(ConversationMessageRecord {
                        message_id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        role: row.get(2)?,
                        content: row.get(3)?,
                        created_at: row.get(4)?,
                        sequence_no: row.get(5)?,
                    })
                })
                .map_err(db_error)?;

            let mut messages = Vec::new();
            for row in message_rows {
                let message = row.map_err(db_error)?;
                let attachments = load_attachments(conn, &message.message_id)?;
                let references = load_references(conn, &message.message_id)?;
                messages.push(LoadedMessageRecord {
                    message,
                    attachments,
                    references,
                });
            }

            Ok(Some(LoadedConversationRecord {
                conversation,
                messages,
            }))
        })
    }

    /// Count persisted conversations for diagnostics.
    pub fn conversation_count(&self) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
                .map_err(db_error)?;
            Ok(count as u64)
        })
    }
}

fn load_attachments(
    conn: &rusqlite::Connection,
    message_id: &str,
) -> JaymiResult<Vec<ConversationAttachmentRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT attachment_id, message_id, kind, name, uri, mime_type, size_bytes, metadata_json
             FROM conversation_attachments
             WHERE message_id = ?1
             ORDER BY attachment_id ASC",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![message_id], |row| {
            Ok(ConversationAttachmentRecord {
                attachment_id: row.get(0)?,
                message_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                uri: row.get(4)?,
                mime_type: row.get(5)?,
                size_bytes: row.get(6)?,
                metadata_json: row.get(7)?,
            })
        })
        .map_err(db_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(db_error)?);
    }
    Ok(out)
}

fn load_references(
    conn: &rusqlite::Connection,
    message_id: &str,
) -> JaymiResult<Vec<ConversationReferenceRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT reference_id, message_id, kind, target_id, label, uri, metadata_json
             FROM conversation_references
             WHERE message_id = ?1
             ORDER BY reference_id ASC",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![message_id], |row| {
            Ok(ConversationReferenceRecord {
                reference_id: row.get(0)?,
                message_id: row.get(1)?,
                kind: row.get(2)?,
                target_id: row.get(3)?,
                label: row.get(4)?,
                uri: row.get(5)?,
                metadata_json: row.get(6)?,
            })
        })
        .map_err(db_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(db_error)?);
    }
    Ok(out)
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
