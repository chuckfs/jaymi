//! Content embeddings persistence (separate from normalized content).

use rusqlite::{params, OptionalExtension};

use jaymi_core::{JaymiError, JaymiResult};

use crate::Database;

/// One stored embedding row.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRecord {
    /// Source path identity (matches `content.source_id`).
    pub source_id: String,
    /// Model that produced the vector.
    pub model_id: String,
    /// Vector dimensionality.
    pub dims: u32,
    /// Dense embedding values.
    pub vector: Vec<f32>,
    /// Hash of embedded text (skip re-embed when unchanged).
    pub content_hash: String,
    /// Unix seconds when the embedding was written.
    pub embedded_at: i64,
}

/// One similarity hit from brute-force vector scan.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingSimilarityHit {
    /// Source path identity.
    pub source_id: String,
    /// Cosine similarity in `[-1, 1]` (higher is better).
    pub similarity: f32,
    /// Model that produced the stored vector.
    pub model_id: String,
}

/// Pending embedding job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingQueueItem {
    /// Source path identity.
    pub source_id: String,
    /// Unix seconds when enqueued.
    pub enqueued_at: i64,
    /// Prior processing attempts.
    pub attempts: u32,
    /// Last error message, when any.
    pub last_error: Option<String>,
}

/// Aggregate embedding counters for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmbeddingCounts {
    /// Rows in `content_embeddings`.
    pub indexed: u64,
    /// Rows waiting in `embedding_queue`.
    pub queued: u64,
}

impl Database {
    /// Insert or replace one embedding (never mutates `content` rows).
    pub fn upsert_embedding(&self, record: &EmbeddingRecord) -> JaymiResult<()> {
        let blob = vector_to_blob(&record.vector);
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO content_embeddings (
                    source_id, model_id, dims, vector, content_hash, embedded_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(source_id) DO UPDATE SET
                    model_id = excluded.model_id,
                    dims = excluded.dims,
                    vector = excluded.vector,
                    content_hash = excluded.content_hash,
                    embedded_at = excluded.embedded_at",
                params![
                    record.source_id,
                    record.model_id,
                    record.dims as i64,
                    blob,
                    record.content_hash,
                    record.embedded_at,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Load one embedding by source identity.
    pub fn get_embedding_by_source_id(
        &self,
        source_id: &str,
    ) -> JaymiResult<Option<EmbeddingRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT source_id, model_id, dims, vector, content_hash, embedded_at
                 FROM content_embeddings WHERE source_id = ?1",
                params![source_id],
                |row| {
                    let blob: Vec<u8> = row.get(3)?;
                    Ok(EmbeddingRecord {
                        source_id: row.get(0)?,
                        model_id: row.get(1)?,
                        dims: row.get::<_, i64>(2)? as u32,
                        vector: blob_to_vector(&blob).unwrap_or_default(),
                        content_hash: row.get(4)?,
                        embedded_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Remove one embedding by source identity.
    pub fn remove_embedding_by_source_id(&self, source_id: &str) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM content_embeddings WHERE source_id = ?1",
                params![source_id],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Brute-force cosine similarity against stored embeddings for a model.
    pub fn search_embeddings_similar(
        &self,
        query: &[f32],
        model_id: &str,
        limit: usize,
        min_similarity: f32,
    ) -> JaymiResult<Vec<EmbeddingSimilarityHit>> {
        let rows = self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT source_id, model_id, vector FROM content_embeddings
                     WHERE model_id = ?1",
                )
                .map_err(db_error)?;
            let iter = stmt
                .query_map(params![model_id], |row| {
                    let blob: Vec<u8> = row.get(2)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        blob_to_vector(&blob).unwrap_or_default(),
                    ))
                })
                .map_err(db_error)?;
            let mut out = Vec::new();
            for item in iter {
                out.push(item.map_err(db_error)?);
            }
            Ok(out)
        })?;

        let mut hits: Vec<EmbeddingSimilarityHit> = rows
            .into_iter()
            .filter_map(|(source_id, model_id, vector)| {
                let similarity = cosine(query, &vector);
                if similarity >= min_similarity {
                    Some(EmbeddingSimilarityHit {
                        source_id,
                        similarity,
                        model_id,
                    })
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|left, right| {
            right
                .similarity
                .partial_cmp(&left.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.source_id.cmp(&right.source_id))
        });
        hits.truncate(limit.max(1));
        Ok(hits)
    }

    /// Enqueue a source for asynchronous embedding generation.
    pub fn enqueue_embedding(&self, source_id: &str, enqueued_at: i64) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO embedding_queue (source_id, enqueued_at, attempts, last_error)
                 VALUES (?1, ?2, 0, NULL)
                 ON CONFLICT(source_id) DO UPDATE SET
                    enqueued_at = excluded.enqueued_at,
                    last_error = NULL",
                params![source_id, enqueued_at],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Claim up to `limit` pending queue rows (oldest first).
    pub fn claim_embedding_queue(&self, limit: usize) -> JaymiResult<Vec<EmbeddingQueueItem>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT source_id, enqueued_at, attempts, last_error
                     FROM embedding_queue
                     ORDER BY enqueued_at ASC, source_id ASC
                     LIMIT ?1",
                )
                .map_err(db_error)?;
            let iter = stmt
                .query_map(params![limit as i64], |row| {
                    Ok(EmbeddingQueueItem {
                        source_id: row.get(0)?,
                        enqueued_at: row.get(1)?,
                        attempts: row.get::<_, i64>(2)? as u32,
                        last_error: row.get(3)?,
                    })
                })
                .map_err(db_error)?;
            let mut out = Vec::new();
            for item in iter {
                out.push(item.map_err(db_error)?);
            }
            Ok(out)
        })
    }

    /// Remove a finished queue item.
    pub fn complete_embedding_queue(&self, source_id: &str) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM embedding_queue WHERE source_id = ?1",
                params![source_id],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Record a failed embedding attempt without dropping the queue row.
    pub fn fail_embedding_queue(&self, source_id: &str, error: &str) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "UPDATE embedding_queue
                 SET attempts = attempts + 1, last_error = ?2
                 WHERE source_id = ?1",
                params![source_id, error],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Embedding diagnostics counters.
    pub fn embedding_counts(&self) -> JaymiResult<EmbeddingCounts> {
        self.with_connection(|conn| {
            let indexed: i64 = conn
                .query_row("SELECT COUNT(*) FROM content_embeddings", [], |row| {
                    row.get(0)
                })
                .map_err(db_error)?;
            let queued: i64 = conn
                .query_row("SELECT COUNT(*) FROM embedding_queue", [], |row| row.get(0))
                .map_err(db_error)?;
            Ok(EmbeddingCounts {
                indexed: indexed as u64,
                queued: queued as u64,
            })
        })
    }
}

/// Encode f32 vector as little-endian bytes.
pub fn vector_to_blob(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len().saturating_mul(4));
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Decode little-endian f32 vector bytes.
pub fn blob_to_vector(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
        out.push(f32::from_le_bytes(bytes));
    }
    Some(out)
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (a, b) in left.iter().zip(right.iter()) {
        let a = f64::from(*a);
        let b = f64::from(*b);
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    let denom = left_norm.sqrt() * right_norm.sqrt();
    if denom <= f64::EPSILON {
        0.0
    } else {
        (dot / denom) as f32
    }
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}

/// Stable content hash for embed-skip decisions.
pub fn content_embedding_hash(title: Option<&str>, plain_text: &str) -> String {
    let mut data = String::new();
    if let Some(title) = title {
        data.push_str(title);
        data.push('\n');
    }
    data.push_str(plain_text);
    // FNV-1a 64-bit hex — deterministic, no extra deps.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in data.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentRecord, Database, Lifecycle};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn embeddings_are_separate_from_content_and_rank_by_similarity() {
        let dir = temp_dir("emb");
        let mut db = Database::with_data_dir(&dir);
        db.initialize().unwrap();

        upsert_doc(&db, "/tmp/a.md", "Mushrooms in damp soil");
        upsert_doc(&db, "/tmp/b.md", "Milk and bread");

        let v_a = vec![1.0, 0.0, 0.0, 0.0];
        let v_b = vec![0.0, 1.0, 0.0, 0.0];
        db.upsert_embedding(&EmbeddingRecord {
            source_id: "/tmp/a.md".into(),
            model_id: "test".into(),
            dims: 4,
            vector: v_a.clone(),
            content_hash: "a".into(),
            embedded_at: 1,
        })
        .unwrap();
        db.upsert_embedding(&EmbeddingRecord {
            source_id: "/tmp/b.md".into(),
            model_id: "test".into(),
            dims: 4,
            vector: v_b,
            content_hash: "b".into(),
            embedded_at: 1,
        })
        .unwrap();

        let hits = db
            .search_embeddings_similar(&[1.0, 0.0, 0.0, 0.0], "test", 10, 0.0)
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].source_id, "/tmp/a.md");
        assert!(hits[0].similarity > hits[1].similarity);

        let content = db.get_content_by_source_id("/tmp/a.md").unwrap().unwrap();
        assert!(content.plain_text.contains("Mushrooms"));
        assert!(db.get_embedding_by_source_id("/tmp/a.md").unwrap().is_some());

        db.enqueue_embedding("/tmp/a.md", 10).unwrap();
        let counts = db.embedding_counts().unwrap();
        assert_eq!(counts.indexed, 2);
        assert_eq!(counts.queued, 1);
    }

    fn upsert_doc(db: &Database, source_id: &str, text: &str) {
        // discovered_items FK for content — insert minimal inventory row first.
        db.with_connection(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO discovered_items (
                    path, filename, extension, size, created, modified,
                    is_directory, hidden, parent
                 ) VALUES (?1, ?2, 'md', 1, 1, 1, 0, 0, NULL)",
                params![source_id, source_id.rsplit('/').next().unwrap_or(source_id)],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .unwrap();
        db.upsert_content(&ContentRecord {
            content_id: format!("content:{source_id}"),
            source_id: source_id.into(),
            content_type: "markdown".into(),
            plain_text: text.into(),
            title: None,
            language: Some("en".into()),
            parser_used: "markdown".into(),
            parser_version: "1".into(),
            extraction_timestamp: 1,
            word_count: 3,
            character_count: text.len() as u64,
            reading_time_seconds: 1,
            headings_json: "[]".into(),
            sections_json: "[]".into(),
            internal_links_json: "[]".into(),
            external_links_json: "[]".into(),
            enrichment_version: "1".into(),
            image_metadata_json: None,
            author: None,
            tags_json: "[]".into(),
        })
        .unwrap();
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-emb-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
