//! Normalized content persistence (Layer 2) and full-text search (Layer 3).

use rusqlite::{params, OptionalExtension, Row};

use jaymi_core::{JaymiError, JaymiResult};

use crate::Database;

/// Persisted normalized content extracted from a knowledge source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRecord {
    /// Stable content identity.
    pub content_id: String,
    /// Source identity (normalized discovered path).
    pub source_id: String,
    /// Logical content type (for example `markdown`).
    pub content_type: String,
    /// Extracted plain text.
    pub plain_text: String,
    /// Optional document title.
    pub title: Option<String>,
    /// Optional language tag.
    pub language: Option<String>,
    /// Parser identifier that produced this content.
    pub parser_used: String,
    /// Parser version string.
    pub parser_version: String,
    /// Unix seconds when extraction completed.
    pub extraction_timestamp: i64,
    /// Whitespace-delimited word count.
    pub word_count: u64,
    /// Unicode scalar character count.
    pub character_count: u64,
    /// Estimated reading time in seconds.
    pub reading_time_seconds: u64,
    /// JSON array of heading objects.
    pub headings_json: String,
    /// JSON array of section objects.
    pub sections_json: String,
    /// JSON array of internal link strings.
    pub internal_links_json: String,
    /// JSON array of external link strings.
    pub external_links_json: String,
    /// Enrichment algorithm version.
    pub enrichment_version: String,
    /// Optional JSON object for image metadata (`ImageContent`).
    pub image_metadata_json: Option<String>,
    /// Optional document author.
    pub author: Option<String>,
    /// JSON array of tag strings.
    pub tags_json: String,
}

/// One hit from the content FTS index (raw row + FTS rank).
#[derive(Debug, Clone, PartialEq)]
pub struct ContentFtsHit {
    /// Source path identity.
    pub source_id: String,
    /// Optional document title.
    pub title: Option<String>,
    /// Full plain text body.
    pub plain_text: String,
    /// JSON array of section objects.
    pub sections_json: String,
    /// SQLite `bm25(content_fts)` score (lower is better in SQLite).
    pub bm25: f64,
}

/// Structured metadata filter for content SQL search (never uses FTS).
#[derive(Debug, Clone, Default)]
pub struct ContentMetadataQuery {
    /// Exact content type identity.
    pub content_type: Option<String>,
    /// Exact language tag.
    pub language: Option<String>,
    /// Author substring (case-insensitive).
    pub author_contains: Option<String>,
    /// Exact tag (case-insensitive), matched against JSON array elements.
    pub tag: Option<String>,
    /// Heading text substring (case-insensitive).
    pub heading_contains: Option<String>,
    /// Title substring (case-insensitive).
    pub title_contains: Option<String>,
    /// Inclusive lower bound on filesystem modification time.
    pub modified_after: Option<i64>,
    /// Inclusive upper bound on filesystem modification time.
    pub modified_before: Option<i64>,
    /// Inclusive lower bound on filesystem creation time.
    pub created_after: Option<i64>,
    /// Inclusive upper bound on filesystem creation time.
    pub created_before: Option<i64>,
    /// Inclusive lower bound on extraction timestamp.
    pub extracted_after: Option<i64>,
    /// Inclusive upper bound on extraction timestamp.
    pub extracted_before: Option<i64>,
    /// Result limit.
    pub limit: Option<usize>,
}

/// One structured metadata hit from the content store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentMetadataHit {
    /// Source path identity.
    pub source_id: String,
    /// Optional document title.
    pub title: Option<String>,
    /// Logical content type.
    pub content_type: String,
    /// Optional language tag.
    pub language: Option<String>,
    /// Optional author.
    pub author: Option<String>,
    /// JSON array of tags.
    pub tags_json: String,
    /// JSON array of headings.
    pub headings_json: String,
    /// Filesystem modification time when joined from inventory.
    pub modified: Option<i64>,
    /// Filesystem creation time when joined from inventory.
    pub created: Option<i64>,
    /// Content extraction timestamp.
    pub extraction_timestamp: i64,
}

/// Aggregate counters for content diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentCounts {
    /// Total normalized content rows.
    pub documents: u64,
}

impl Database {
    /// Insert or replace one content row.
    pub fn upsert_content(&self, record: &ContentRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO content (
                    content_id, source_id, content_type, plain_text, title, language,
                    parser_used, parser_version, extraction_timestamp,
                    word_count, character_count, reading_time_seconds,
                    headings_json, sections_json, internal_links_json, external_links_json,
                    enrichment_version, image_metadata_json, author, tags_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
                 ON CONFLICT(source_id) DO UPDATE SET
                    content_id = excluded.content_id,
                    content_type = excluded.content_type,
                    plain_text = excluded.plain_text,
                    title = excluded.title,
                    language = excluded.language,
                    parser_used = excluded.parser_used,
                    parser_version = excluded.parser_version,
                    extraction_timestamp = excluded.extraction_timestamp,
                    word_count = excluded.word_count,
                    character_count = excluded.character_count,
                    reading_time_seconds = excluded.reading_time_seconds,
                    headings_json = excluded.headings_json,
                    sections_json = excluded.sections_json,
                    internal_links_json = excluded.internal_links_json,
                    external_links_json = excluded.external_links_json,
                    enrichment_version = excluded.enrichment_version,
                    image_metadata_json = excluded.image_metadata_json,
                    author = excluded.author,
                    tags_json = excluded.tags_json",
                params![
                    record.content_id,
                    record.source_id,
                    record.content_type,
                    record.plain_text,
                    record.title,
                    record.language,
                    record.parser_used,
                    record.parser_version,
                    record.extraction_timestamp,
                    record.word_count as i64,
                    record.character_count as i64,
                    record.reading_time_seconds as i64,
                    record.headings_json,
                    record.sections_json,
                    record.internal_links_json,
                    record.external_links_json,
                    record.enrichment_version,
                    record.image_metadata_json,
                    record.author,
                    record.tags_json,
                ],
            )
            .map_err(db_error)?;
            // Explicit FTS refresh — external-content FTS5 update triggers are
            // unreliable with INSERT … ON CONFLICT DO UPDATE.
            reindex_content_fts(
                conn,
                &record.source_id,
                record.title.as_deref(),
                &record.plain_text,
            )?;
            Ok(())
        })
    }

    /// Load content by source path identity.
    pub fn get_content_by_source_id(&self, source_id: &str) -> JaymiResult<Option<ContentRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT content_id, source_id, content_type, plain_text, title, language,
                        parser_used, parser_version, extraction_timestamp,
                        word_count, character_count, reading_time_seconds,
                        headings_json, sections_json, internal_links_json, external_links_json,
                        enrichment_version, image_metadata_json, author, tags_json
                 FROM content WHERE source_id = ?1",
                params![source_id],
                map_content_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Remove content for a source path.
    pub fn remove_content_by_source_id(&self, source_id: &str) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM content_fts WHERE source_id = ?1",
                params![source_id],
            )
            .map_err(db_error)?;
            conn.execute(
                "DELETE FROM content WHERE source_id = ?1",
                params![source_id],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Full-text search over normalized content (`title` + `plain_text`).
    ///
    /// Supports words and phrases. Queries wrapped in double quotes are treated
    /// as exact phrase matches. Multi-word queries without quotes also use
    /// phrase MATCH so adjacency is preferred; callers may score term frequency
    /// in Rust for ranking.
    pub fn search_content_fts(
        &self,
        query: &str,
        limit: usize,
    ) -> JaymiResult<Vec<ContentFtsHit>> {
        let Some(match_query) = build_fts_match_query(query) else {
            return Ok(Vec::new());
        };
        let limit = limit.max(1).min(10_000);
        let mut hits = self.query_content_fts(&match_query, limit)?;
        if hits.is_empty() {
            if let Some(and_query) = build_fts_and_query(query) {
                hits = self.query_content_fts(&and_query, limit)?;
            }
        }
        Ok(hits)
    }

    fn query_content_fts(&self, match_query: &str, limit: usize) -> JaymiResult<Vec<ContentFtsHit>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT c.source_id, c.title, c.plain_text, c.sections_json,
                            bm25(content_fts) AS rank
                     FROM content_fts
                     JOIN content c ON c.source_id = content_fts.source_id
                     WHERE content_fts MATCH ?1
                     ORDER BY rank
                     LIMIT ?2",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map(params![match_query, limit as i64], |row| {
                    Ok(ContentFtsHit {
                        source_id: row.get(0)?,
                        title: row.get(1)?,
                        plain_text: row.get(2)?,
                        sections_json: row.get(3)?,
                        bm25: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                    })
                })
                .map_err(db_error)?;
            let mut hits = Vec::new();
            for row in rows {
                hits.push(row.map_err(db_error)?);
            }
            Ok(hits)
        })
    }

    /// Structured metadata search over normalized content (SQL filters only — never FTS).
    pub fn search_content_metadata(
        &self,
        query: &ContentMetadataQuery,
    ) -> JaymiResult<Vec<ContentMetadataHit>> {
        let limit = query.limit.unwrap_or(100).max(1).min(10_000) as i64;
        let content_type = query
            .content_type
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let language = query
            .language
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let author = query
            .author_contains
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{}%", value.to_ascii_lowercase()));
        let tag = query
            .tag
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let heading = query
            .heading_contains
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{}%", value.to_ascii_lowercase()));
        let title = query
            .title_contains
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{}%", value.to_ascii_lowercase()));

        let needs_inventory = query.modified_after.is_some()
            || query.modified_before.is_some()
            || query.created_after.is_some()
            || query.created_before.is_some();

        self.with_connection(|conn| {
            let sql = if needs_inventory {
                "SELECT c.source_id, c.title, c.content_type, c.language, c.author, c.tags_json,
                        c.headings_json, d.modified, d.created, c.extraction_timestamp
                 FROM content c
                 LEFT JOIN discovered_items d ON d.path = c.source_id
                 WHERE (?1 IS NULL OR lower(c.content_type) = ?1)
                   AND (?2 IS NULL OR lower(COALESCE(c.language, '')) = ?2)
                   AND (?3 IS NULL OR lower(COALESCE(c.author, '')) LIKE ?3)
                   AND (?4 IS NULL OR EXISTS (
                        SELECT 1 FROM json_each(c.tags_json) AS t
                        WHERE lower(t.value) = ?4
                   ))
                   AND (?5 IS NULL OR lower(c.headings_json) LIKE ?5)
                   AND (?6 IS NULL OR lower(COALESCE(c.title, '')) LIKE ?6)
                   AND (?7 IS NULL OR COALESCE(d.modified, -1) >= ?7)
                   AND (?8 IS NULL OR COALESCE(d.modified, -1) <= ?8)
                   AND (?9 IS NULL OR COALESCE(d.created, -1) >= ?9)
                   AND (?10 IS NULL OR COALESCE(d.created, -1) <= ?10)
                   AND (?11 IS NULL OR c.extraction_timestamp >= ?11)
                   AND (?12 IS NULL OR c.extraction_timestamp <= ?12)
                 ORDER BY c.source_id
                 LIMIT ?13"
            } else {
                "SELECT c.source_id, c.title, c.content_type, c.language, c.author, c.tags_json,
                        c.headings_json, NULL, NULL, c.extraction_timestamp
                 FROM content c
                 WHERE (?1 IS NULL OR lower(c.content_type) = ?1)
                   AND (?2 IS NULL OR lower(COALESCE(c.language, '')) = ?2)
                   AND (?3 IS NULL OR lower(COALESCE(c.author, '')) LIKE ?3)
                   AND (?4 IS NULL OR EXISTS (
                        SELECT 1 FROM json_each(c.tags_json) AS t
                        WHERE lower(t.value) = ?4
                   ))
                   AND (?5 IS NULL OR lower(c.headings_json) LIKE ?5)
                   AND (?6 IS NULL OR lower(COALESCE(c.title, '')) LIKE ?6)
                   AND (?7 IS NULL OR 1)
                   AND (?8 IS NULL OR 1)
                   AND (?9 IS NULL OR 1)
                   AND (?10 IS NULL OR 1)
                   AND (?11 IS NULL OR c.extraction_timestamp >= ?11)
                   AND (?12 IS NULL OR c.extraction_timestamp <= ?12)
                 ORDER BY c.source_id
                 LIMIT ?13"
            };

            let mut stmt = conn.prepare(sql).map_err(db_error)?;
            let rows = stmt
                .query_map(
                    params![
                        content_type,
                        language,
                        author,
                        tag,
                        heading,
                        title,
                        query.modified_after,
                        query.modified_before,
                        query.created_after,
                        query.created_before,
                        query.extracted_after,
                        query.extracted_before,
                        limit,
                    ],
                    |row| {
                        Ok(ContentMetadataHit {
                            source_id: row.get(0)?,
                            title: row.get(1)?,
                            content_type: row.get(2)?,
                            language: row.get(3)?,
                            author: row.get(4)?,
                            tags_json: row.get(5)?,
                            headings_json: row.get(6)?,
                            modified: row.get(7)?,
                            created: row.get(8)?,
                            extraction_timestamp: row.get(9)?,
                        })
                    },
                )
                .map_err(db_error)?;
            let mut hits = Vec::new();
            for row in rows {
                hits.push(row.map_err(db_error)?);
            }
            Ok(hits)
        })
    }

    /// Count stored content documents.
    pub fn content_counts(&self) -> JaymiResult<ContentCounts> {
        self.with_connection(|conn| {
            let documents: i64 = conn
                .query_row("SELECT COUNT(*) FROM content", [], |row| row.get(0))
                .map_err(db_error)?;
            Ok(ContentCounts {
                documents: documents as u64,
            })
        })
    }

    /// Parser usage histogram for diagnostics.
    pub fn content_parser_usage(&self) -> JaymiResult<Vec<(String, u64)>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT parser_used, COUNT(*) FROM content
                     GROUP BY parser_used ORDER BY parser_used",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
                })
                .map_err(db_error)?;
            let mut usage = Vec::new();
            for row in rows {
                usage.push(row.map_err(db_error)?);
            }
            Ok(usage)
        })
    }

    /// Count content rows that have structural enrichment applied.
    pub fn content_enriched_count(&self) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM content
                     WHERE enrichment_version IS NOT NULL AND enrichment_version != ''",
                    [],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            Ok(count as u64)
        })
    }

    /// Count content rows that carry image metadata.
    pub fn content_image_count(&self) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM content
                     WHERE content_type = 'image'
                        OR (image_metadata_json IS NOT NULL AND image_metadata_json != '')",
                    [],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            Ok(count as u64)
        })
    }
}

/// Build a safe FTS5 MATCH expression from user input.
///
/// Returns `None` when the query has no searchable tokens.
pub fn build_fts_match_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (inner, force_phrase) =
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            (&trimmed[1..trimmed.len() - 1], true)
        } else {
            (trimmed, false)
        };

    let sanitized = sanitize_fts_token_source(inner);
    if sanitized.is_empty() {
        return None;
    }

    let tokens: Vec<&str> = sanitized.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    if force_phrase || tokens.len() > 1 {
        // Phrase match: words must appear adjacent and in order.
        Some(format!("\"{}\"", tokens.join(" ")))
    } else {
        // Single word: quote the token so FTS operators in the text cannot inject.
        Some(format!("\"{}\"", tokens[0]))
    }
}

/// Build an AND-of-tokens MATCH query for non-adjacent word search.
///
/// Returns `None` for quoted phrases, single tokens, or empty input.
pub fn build_fts_and_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return None;
    }
    let sanitized = sanitize_fts_token_source(trimmed);
    let tokens: Vec<&str> = sanitized.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|token| format!("\"{token}\""))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

fn sanitize_fts_token_source(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '"' | '*' | '^' | '(' | ')' | '{' | '}' | '[' | ']' | ':' | ',' => ' ',
            other if other.is_control() => ' ',
            other => other,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn reindex_content_fts(
    conn: &rusqlite::Connection,
    source_id: &str,
    title: Option<&str>,
    plain_text: &str,
) -> JaymiResult<()> {
    conn.execute(
        "DELETE FROM content_fts WHERE source_id = ?1",
        params![source_id],
    )
    .map_err(db_error)?;
    conn.execute(
        "INSERT INTO content_fts(source_id, title, plain_text) VALUES (?1, ?2, ?3)",
        params![source_id, title.unwrap_or(""), plain_text],
    )
    .map_err(db_error)?;
    Ok(())
}

fn map_content_row(row: &Row<'_>) -> rusqlite::Result<ContentRecord> {
    Ok(ContentRecord {
        content_id: row.get(0)?,
        source_id: row.get(1)?,
        content_type: row.get(2)?,
        plain_text: row.get(3)?,
        title: row.get(4)?,
        language: row.get(5)?,
        parser_used: row.get(6)?,
        parser_version: row.get(7)?,
        extraction_timestamp: row.get(8)?,
        word_count: row.get::<_, i64>(9)? as u64,
        character_count: row.get::<_, i64>(10)? as u64,
        reading_time_seconds: row.get::<_, i64>(11)? as u64,
        headings_json: row.get(12)?,
        sections_json: row.get(13)?,
        internal_links_json: row.get(14)?,
        external_links_json: row.get(15)?,
        enrichment_version: row.get(16)?,
        image_metadata_json: row.get(17)?,
        author: row.get(18)?,
        tags_json: row.get(19)?,
    })
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use jaymi_core::Lifecycle;

    use crate::inventory::DiscoveredItemRecord;
    use crate::Database;

    fn boot_db() -> Database {
        let dir = temp_dir("content-fts");
        let mut db = Database::with_data_dir(&dir);
        db.initialize().unwrap();
        db
    }

    fn publish_source(db: &Database, path: &str) {
        db.upsert_discovered_item(&DiscoveredItemRecord {
            path: path.into(),
            filename: path.rsplit('/').next().unwrap_or(path).into(),
            extension: Some("md".into()),
            size: 10,
            created: Some(1),
            modified: Some(1),
            is_directory: false,
            hidden: false,
            parent: Some("/tmp".into()),
            first_discovered: Some(1),
            last_indexed: Some(1),
            last_modified: Some(1),
            last_verified: Some(1),
            device_id: None,
            inode: None,
        })
        .unwrap();
    }

    fn sample_record(source_id: &str, title: &str, body: &str) -> ContentRecord {
        ContentRecord {
            content_id: format!("content:{source_id}"),
            source_id: source_id.into(),
            content_type: "markdown".into(),
            plain_text: body.into(),
            title: Some(title.into()),
            language: Some("en".into()),
            parser_used: "markdown".into(),
            parser_version: "0.1.0".into(),
            extraction_timestamp: 1,
            word_count: body.split_whitespace().count() as u64,
            character_count: body.chars().count() as u64,
            reading_time_seconds: 1,
            headings_json: "[]".into(),
            sections_json: "[]".into(),
            internal_links_json: "[]".into(),
            external_links_json: "[]".into(),
            enrichment_version: "1".into(),
            image_metadata_json: None,
            author: None,
            tags_json: "[]".into(),
        }
    }

    #[test]
    fn fts_indexes_words_phrases_and_updates() {
        let db = boot_db();
        publish_source(&db, "/docs/a.md");
        publish_source(&db, "/docs/b.md");

        db.upsert_content(&sample_record(
            "/docs/a.md",
            "Biology Notes",
            "# Habitat\n\nFungi grow in damp soil near oak trees.\n",
        ))
        .unwrap();
        db.upsert_content(&sample_record(
            "/docs/b.md",
            "Shopping",
            "Buy milk and bread tomorrow.\n",
        ))
        .unwrap();

        let word = db.search_content_fts("fungi", 10).unwrap();
        assert_eq!(word.len(), 1);
        assert_eq!(word[0].source_id, "/docs/a.md");

        let phrase = db.search_content_fts("\"damp soil\"", 10).unwrap();
        assert_eq!(phrase.len(), 1);
        assert!(phrase[0].plain_text.contains("damp soil"));

        let multi = db.search_content_fts("damp soil", 10).unwrap();
        assert_eq!(multi.len(), 1);

        let title = db.search_content_fts("Biology", 10).unwrap();
        assert_eq!(title.len(), 1);

        db.upsert_content(&sample_record(
            "/docs/a.md",
            "Biology Notes",
            "# Habitat\n\nMoss grows on rocks.\n",
        ))
        .unwrap();
        let after = db.search_content_fts("fungi", 10).unwrap();
        assert!(after.is_empty());
        let moss = db.search_content_fts("moss", 10).unwrap();
        assert_eq!(moss.len(), 1);

        db.remove_content_by_source_id("/docs/a.md").unwrap();
        assert!(db.search_content_fts("moss", 10).unwrap().is_empty());
    }

    #[test]
    fn metadata_search_is_independent_of_fts() {
        let db = boot_db();
        publish_source(&db, "/docs/a.md");
        publish_source(&db, "/docs/b.md");

        let mut biology = sample_record(
            "/docs/a.md",
            "Biology Paper",
            "# Habitat\n\nFungi grow here.\n",
        );
        biology.language = Some("en".into());
        biology.author = Some("Ada Lovelace".into());
        biology.tags_json = r#"["biology","research"]"#.into();
        biology.headings_json =
            r#"[{"level":1,"text":"Habitat","offset":0}]"#.into();
        db.upsert_content(&biology).unwrap();

        let mut shopping = sample_record("/docs/b.md", "Errands", "Buy milk.\n");
        shopping.language = Some("es".into());
        shopping.author = Some("Other".into());
        shopping.tags_json = r#"["chores"]"#.into();
        db.upsert_content(&shopping).unwrap();

        let by_lang = db
            .search_content_metadata(&ContentMetadataQuery {
                language: Some("en".into()),
                limit: Some(10),
                ..ContentMetadataQuery::default()
            })
            .unwrap();
        assert_eq!(by_lang.len(), 1);
        assert_eq!(by_lang[0].source_id, "/docs/a.md");

        let by_author = db
            .search_content_metadata(&ContentMetadataQuery {
                author_contains: Some("Ada".into()),
                limit: Some(10),
                ..ContentMetadataQuery::default()
            })
            .unwrap();
        assert_eq!(by_author.len(), 1);

        let by_tag = db
            .search_content_metadata(&ContentMetadataQuery {
                tag: Some("biology".into()),
                limit: Some(10),
                ..ContentMetadataQuery::default()
            })
            .unwrap();
        assert_eq!(by_tag.len(), 1);

        let by_heading = db
            .search_content_metadata(&ContentMetadataQuery {
                heading_contains: Some("Habitat".into()),
                limit: Some(10),
                ..ContentMetadataQuery::default()
            })
            .unwrap();
        assert_eq!(by_heading.len(), 1);

        let by_type = db
            .search_content_metadata(&ContentMetadataQuery {
                content_type: Some("markdown".into()),
                limit: Some(10),
                ..ContentMetadataQuery::default()
            })
            .unwrap();
        assert_eq!(by_type.len(), 2);

        // Determinism
        let again = db
            .search_content_metadata(&ContentMetadataQuery {
                language: Some("en".into()),
                limit: Some(10),
                ..ContentMetadataQuery::default()
            })
            .unwrap();
        assert_eq!(by_lang, again);
    }

    #[test]
    fn build_fts_match_query_sanitizes_operators() {
        assert_eq!(build_fts_match_query("fungi"), Some("\"fungi\"".into()));
        assert_eq!(
            build_fts_match_query("damp soil"),
            Some("\"damp soil\"".into())
        );
        assert_eq!(
            build_fts_match_query("\"exact phrase\""),
            Some("\"exact phrase\"".into())
        );
        assert_eq!(build_fts_match_query("   "), None);
        assert_eq!(
            build_fts_match_query("hello\" OR 1=1"),
            Some("\"hello OR 1=1\"".into())
        );
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-content-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
