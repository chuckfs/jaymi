//! Normalized content persistence (Layer 2).

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
                    enrichment_version, image_metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
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
                    image_metadata_json = excluded.image_metadata_json",
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
                ],
            )
            .map_err(db_error)?;
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
                        enrichment_version, image_metadata_json
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
                "DELETE FROM content WHERE source_id = ?1",
                params![source_id],
            )
            .map_err(db_error)?;
            Ok(())
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
    })
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
