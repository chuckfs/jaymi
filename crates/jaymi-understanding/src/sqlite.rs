//! SQLite-backed content store.

use std::sync::Arc;

use jaymi_core::{EntityId, JaymiError, JaymiResult};
use jaymi_database::{ContentRecord, Database};

use crate::content::Content;
use crate::enrichment::{ContentEnrichment, Heading, Section};
use crate::image_content::ImageContent;
use crate::store::ContentStore;

/// Content store that persists through the shared Database.
pub struct SqliteContentStore {
    database: Arc<Database>,
}

impl SqliteContentStore {
    /// Create a content store bound to the shared database.
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    /// Directory used for generated image thumbnails (sibling of the DB file).
    pub fn thumbnail_dir(&self) -> std::path::PathBuf {
        self.database
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("thumbnails")
    }
}

impl ContentStore for SqliteContentStore {
    fn get_by_source_id(&self, source_id: &str) -> JaymiResult<Option<Content>> {
        self.database
            .get_content_by_source_id(source_id)?
            .map(record_to_content)
            .transpose()
    }

    fn upsert(&self, content: &Content) -> JaymiResult<()> {
        self.database.upsert_content(&content_to_record(content)?)
    }

    fn remove_by_source_id(&self, source_id: &str) -> JaymiResult<()> {
        self.database.remove_content_by_source_id(source_id)
    }

    fn exists(&self, source_id: &str) -> JaymiResult<bool> {
        Ok(self.get_by_source_id(source_id)?.is_some())
    }

    fn document_count(&self) -> JaymiResult<u64> {
        Ok(self.database.content_counts()?.documents)
    }

    fn parser_usage(&self) -> JaymiResult<Vec<(String, u64)>> {
        self.database.content_parser_usage()
    }
}

impl SqliteContentStore {
    /// Count rows with applied enrichment (for diagnostics).
    pub fn enriched_count(&self) -> JaymiResult<u64> {
        self.database.content_enriched_count()
    }

    /// Count stored image content rows.
    pub fn image_count(&self) -> JaymiResult<u64> {
        self.database.content_image_count()
    }

    /// Full-text search over stored normalized content.
    pub fn search_full_text(
        &self,
        query: &str,
        limit: usize,
    ) -> JaymiResult<Vec<crate::api::ContentTextHit>> {
        self.search_full_text_in_prefix(query, None, limit)
    }

    /// Full-text search constrained to a source path prefix.
    pub fn search_full_text_in_prefix(
        &self,
        query: &str,
        path_prefix: Option<&str>,
        limit: usize,
    ) -> JaymiResult<Vec<crate::api::ContentTextHit>> {
        let rows = self
            .database
            .search_content_fts_in_prefix(query, path_prefix, limit)?;
        let mut hits = Vec::with_capacity(rows.len());
        for row in rows {
            let sections: Vec<Section> = serde_json::from_str(&row.sections_json).unwrap_or_default();
            hits.push(crate::api::ContentTextHit {
                source_id: row.source_id,
                title: row.title,
                plain_text: row.plain_text,
                sections,
            });
        }
        Ok(hits)
    }

    /// Structured metadata search (SQL filters — never FTS).
    pub fn search_metadata(
        &self,
        query: &jaymi_database::ContentMetadataQuery,
    ) -> JaymiResult<Vec<crate::api::ContentMetadataHit>> {
        let rows = self.database.search_content_metadata(query)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let headings: Vec<Heading> =
                    serde_json::from_str(&row.headings_json).unwrap_or_default();
                let tags: Vec<String> = serde_json::from_str(&row.tags_json).unwrap_or_default();
                crate::api::ContentMetadataHit {
                    source_id: row.source_id,
                    title: row.title,
                    content_type: row.content_type,
                    language: row.language,
                    author: row.author,
                    tags,
                    headings,
                    modified: row.modified,
                    created: row.created,
                    extraction_timestamp: row.extraction_timestamp,
                }
            })
            .collect())
    }
}

fn content_to_record(content: &Content) -> JaymiResult<ContentRecord> {
    let image_metadata_json = match &content.image {
        Some(image) => Some(
            serde_json::to_string(image)
                .map_err(|error| JaymiError::new(format!("image encode: {error}")))?,
        ),
        None => None,
    };
    Ok(ContentRecord {
        content_id: content.content_id.as_str().to_string(),
        source_id: content.source_id.clone(),
        content_type: content.content_type.clone(),
        plain_text: content.plain_text.clone(),
        title: content.title.clone(),
        language: content.language.clone(),
        parser_used: content.parser_used.clone(),
        parser_version: content.parser_version.clone(),
        extraction_timestamp: content.extraction_timestamp,
        word_count: content.enrichment.word_count,
        character_count: content.enrichment.character_count,
        reading_time_seconds: content.enrichment.reading_time_seconds,
        headings_json: serde_json::to_string(&content.enrichment.headings)
            .map_err(|error| JaymiError::new(format!("enrichment encode: {error}")))?,
        sections_json: serde_json::to_string(&content.enrichment.sections)
            .map_err(|error| JaymiError::new(format!("enrichment encode: {error}")))?,
        internal_links_json: serde_json::to_string(&content.enrichment.internal_links)
            .map_err(|error| JaymiError::new(format!("enrichment encode: {error}")))?,
        external_links_json: serde_json::to_string(&content.enrichment.external_links)
            .map_err(|error| JaymiError::new(format!("enrichment encode: {error}")))?,
        enrichment_version: content.enrichment.version.clone(),
        image_metadata_json,
        author: content.author.clone(),
        tags_json: serde_json::to_string(&content.tags)
            .map_err(|error| JaymiError::new(format!("tags encode: {error}")))?,
    })
}

fn record_to_content(record: ContentRecord) -> JaymiResult<Content> {
    let headings: Vec<Heading> = serde_json::from_str(&record.headings_json)
        .map_err(|error| JaymiError::new(format!("enrichment decode headings: {error}")))?;
    let sections: Vec<Section> = serde_json::from_str(&record.sections_json)
        .map_err(|error| JaymiError::new(format!("enrichment decode sections: {error}")))?;
    let internal_links: Vec<String> = serde_json::from_str(&record.internal_links_json)
        .map_err(|error| JaymiError::new(format!("enrichment decode internal links: {error}")))?;
    let external_links: Vec<String> = serde_json::from_str(&record.external_links_json)
        .map_err(|error| JaymiError::new(format!("enrichment decode external links: {error}")))?;

    let enrichment = ContentEnrichment {
        headings,
        sections,
        reading_time_seconds: record.reading_time_seconds,
        word_count: record.word_count,
        character_count: record.character_count,
        language: record.language.clone(),
        internal_links,
        external_links,
        version: record.enrichment_version,
    };

    let image = match record.image_metadata_json.as_deref() {
        Some(json) if !json.is_empty() => Some(
            serde_json::from_str::<ImageContent>(json)
                .map_err(|error| JaymiError::new(format!("image decode: {error}")))?,
        ),
        _ => None,
    };

    Ok(Content {
        content_id: EntityId::new(record.content_id),
        source_id: record.source_id,
        content_type: record.content_type,
        plain_text: record.plain_text,
        title: record.title,
        language: record.language,
        author: record.author,
        tags: serde_json::from_str(&record.tags_json).unwrap_or_default(),
        parser_used: record.parser_used,
        parser_version: record.parser_version,
        extraction_timestamp: record.extraction_timestamp,
        enrichment,
        image,
    })
}
