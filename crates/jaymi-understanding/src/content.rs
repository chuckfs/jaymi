//! Normalized Content model for Layer 2.

use jaymi_core::{Document, DocumentMetadata, EntityId, FileType};

use crate::enrichment::ContentEnrichment;
use crate::image_content::ImageContent;

/// Normalized content produced by the Content Intelligence pipeline.
///
/// Every supported source yields this structure. Embeddings, summaries, and
/// AI-generated fields are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    /// Stable content identity.
    pub content_id: EntityId,
    /// Source identity (normalized discovered path).
    pub source_id: String,
    /// Logical content type identity.
    pub content_type: String,
    /// Extracted plain text.
    pub plain_text: String,
    /// Optional document title.
    pub title: Option<String>,
    /// Optional language tag from deterministic enrichment.
    pub language: Option<String>,
    /// Optional document author preserved from parser metadata.
    pub author: Option<String>,
    /// User / document tags preserved as searchable metadata.
    pub tags: Vec<String>,
    /// Parser that produced this content.
    pub parser_used: String,
    /// Parser version string.
    pub parser_version: String,
    /// Unix seconds when extraction completed.
    pub extraction_timestamp: i64,
    /// Deterministic structural enrichment.
    pub enrichment: ContentEnrichment,
    /// Image metadata when the source is an image.
    pub image: Option<ImageContent>,
}

impl Content {
    /// Build a content identity for a source path.
    pub fn id_for_source(source_id: &str) -> EntityId {
        EntityId::new(format!("content:{source_id}"))
    }

    /// Convert a parsed document into normalized content with enrichment.
    pub fn from_document(document: &Document, parser_version: &str) -> Self {
        let source_id = document.path.to_string_lossy().into_owned();
        let content_type = document.file_type.id().to_string();
        let enrichment = ContentEnrichment::extract(
            &document.text,
            &content_type,
            document.title.as_deref(),
        );
        let image = if content_type == "image" {
            ImageContent::from_document(document).ok()
        } else {
            None
        };
        let author = document
            .metadata
            .get("author")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let tags = parse_tags_from_metadata(&document.metadata);
        Self {
            content_id: Self::id_for_source(&source_id),
            source_id,
            content_type,
            plain_text: document.text.clone(),
            title: document.title.clone(),
            language: enrichment.language.clone(),
            author,
            tags,
            parser_used: document.parser_id.clone(),
            parser_version: parser_version.to_string(),
            extraction_timestamp: document.parsed_at as i64,
            enrichment,
            image,
        }
    }

    /// Rebuild an ephemeral document for Planner/tool responses.
    pub fn to_document(&self) -> Document {
        let mut metadata = enrichment_metadata(&self.enrichment, self.language.as_deref());
        if let Some(author) = &self.author {
            metadata.insert("author", author.as_str());
        }
        if !self.tags.is_empty() {
            metadata.insert("tags", self.tags.join(","));
        }
        if let Some(image) = &self.image {
            append_image_metadata(&mut metadata, image);
        }
        Document {
            id: self.content_id.clone(),
            path: std::path::PathBuf::from(&self.source_id),
            file_type: file_type_from_id(&self.content_type),
            title: self.title.clone(),
            text: self.plain_text.clone(),
            metadata,
            parsed_at: self.extraction_timestamp as u64,
            parser_id: self.parser_used.clone(),
        }
    }
}

fn parse_tags_from_metadata(metadata: &DocumentMetadata) -> Vec<String> {
    let Some(raw) = metadata.get("tags") else {
        return Vec::new();
    };
    raw.split([',', ';'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn append_image_metadata(metadata: &mut DocumentMetadata, image: &ImageContent) {
    metadata.insert("width", image.width.to_string());
    metadata.insert("height", image.height.to_string());
    metadata.insert("image_format", image.format.as_str());
    if let Some(profile) = &image.color_profile {
        metadata.insert("color_profile", profile.as_str());
    }
    if let Some(capture_date) = &image.capture_date {
        metadata.insert("capture_date", capture_date.as_str());
    }
    if let Some(thumbnail_path) = &image.thumbnail_path {
        metadata.insert("thumbnail_path", thumbnail_path.as_str());
    }
    if !image.exif.is_empty() {
        if let Ok(json) = serde_json::to_string(&image.exif) {
            metadata.insert("exif_json", json);
        }
    }
}

fn enrichment_metadata(
    enrichment: &ContentEnrichment,
    language: Option<&str>,
) -> DocumentMetadata {
    let mut metadata = DocumentMetadata::new();
    metadata.insert("word_count", enrichment.word_count.to_string());
    metadata.insert("character_count", enrichment.character_count.to_string());
    metadata.insert(
        "reading_time_seconds",
        enrichment.reading_time_seconds.to_string(),
    );
    metadata.insert("heading_count", enrichment.headings.len().to_string());
    metadata.insert("section_count", enrichment.sections.len().to_string());
    metadata.insert(
        "internal_link_count",
        enrichment.internal_links.len().to_string(),
    );
    metadata.insert(
        "external_link_count",
        enrichment.external_links.len().to_string(),
    );
    metadata.insert("enrichment_version", enrichment.version.as_str());
    if let Some(language) = language {
        metadata.insert("language", language);
    }
    if !enrichment.headings.is_empty() {
        let headings = enrichment
            .headings
            .iter()
            .map(|heading| format!("h{}:{}", heading.level, heading.text))
            .collect::<Vec<_>>()
            .join(" | ");
        metadata.insert("headings", headings);
    }
    if !enrichment.sections.is_empty() {
        let sections = enrichment
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        metadata.insert("sections", sections);
    }
    if !enrichment.internal_links.is_empty() {
        metadata.insert("internal_links", enrichment.internal_links.join(" | "));
    }
    if !enrichment.external_links.is_empty() {
        metadata.insert("external_links", enrichment.external_links.join(" | "));
    }
    metadata
}

fn file_type_from_id(id: &str) -> FileType {
    match id {
        "plain_text" => FileType::PlainText,
        "markdown" => FileType::Markdown,
        "json" => FileType::Json,
        "pdf" => FileType::Pdf,
        "docx" => FileType::Docx,
        "image" => FileType::Image,
        other => FileType::Other(other.to_string()),
    }
}
