//! Stable Content Intelligence API types and trait.
//!
//! Consumers (Planner tools, Search, Memory, Projects, future Reasoning) use
//! this surface and never talk to parsers, the filesystem, or SQLite directly.

use std::path::Path;

use jaymi_core::JaymiResult;

use crate::content::Content;
use crate::enrichment::ContentEnrichment;
use crate::image_content::ImageContent;

/// Where loaded content came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentSource {
    /// Returned from the normalized content store.
    Stored,
    /// Freshly parsed / enriched by the understanding pipeline.
    Parsed,
}

impl ContentSource {
    /// Stable label for diagnostics and tool messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Parsed => "parsed",
        }
    }
}

/// Result of loading normalized content through the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLoad {
    /// Normalized content.
    pub content: Content,
    /// Whether the payload was stored or freshly produced.
    pub source: ContentSource,
}

/// Metadata view without parser implementation details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentMetadataView {
    /// Content identity.
    pub content_id: String,
    /// Source path identity.
    pub source_id: String,
    /// Logical content type (`markdown`, `image`, …).
    pub content_type: String,
    /// Optional title.
    pub title: Option<String>,
    /// Optional language tag.
    pub language: Option<String>,
    /// Structural enrichment snapshot.
    pub enrichment: ContentEnrichment,
    /// Image metadata when present.
    pub image: Option<ImageContent>,
}

impl ContentMetadataView {
    /// Build a metadata view from normalized content.
    pub fn from_content(content: &Content) -> Self {
        Self {
            content_id: content.content_id.as_str().to_string(),
            source_id: content.source_id.clone(),
            content_type: content.content_type.clone(),
            title: content.title.clone(),
            language: content.language.clone(),
            enrichment: content.enrichment.clone(),
            image: content.image.clone(),
        }
    }
}

/// Parser provenance recorded with content — not a live parser handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserInfo {
    /// Parser identity that produced the content (`markdown`, `image`, …).
    pub parser_used: String,
    /// Parser package/version string.
    pub parser_version: String,
    /// Unix seconds when extraction completed.
    pub extraction_timestamp: i64,
}

impl ParserInfo {
    /// Build parser info from normalized content.
    pub fn from_content(content: &Content) -> Self {
        Self {
            parser_used: content.parser_used.clone(),
            parser_version: content.parser_version.clone(),
            extraction_timestamp: content.extraction_timestamp,
        }
    }
}

/// Aggregate content-store statistics for diagnostics and future Search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentStatistics {
    /// Stored normalized documents.
    pub document_count: u64,
    /// Documents with enrichment applied.
    pub enriched_count: u64,
    /// Documents with image metadata.
    pub image_count: u64,
    /// Parser usage histogram `(parser_id, count)`.
    pub parser_usage: Vec<(String, u64)>,
    /// Failed parse attempts since boot.
    pub failed_parses: u64,
    /// Unsupported format encounters since boot.
    pub unsupported_formats: u64,
    /// Cache hits since boot.
    pub cache_hits: u64,
}

/// Health snapshot for the Content Intelligence subsystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentHealth {
    /// Whether the API backing engine is initialized.
    pub initialized: bool,
    /// Whether the subsystem is healthy for reads.
    pub healthy: bool,
    /// API / engine version string.
    pub version: String,
    /// Short detail string for diagnostics.
    pub detail: String,
    /// Latest statistics snapshot when available.
    pub statistics: ContentStatistics,
}

/// Stable internal API for accessing normalized content.
///
/// Hides parser implementations, the understanding pipeline internals, and the
/// knowledge database schema from consumers.
pub trait ContentIntelligence: Send + Sync {
    /// Load normalized content for a filesystem path (stored or parse-on-demand).
    fn load_content(&self, path: &Path) -> JaymiResult<ContentLoad>;

    /// Load content already present in the store by source identity.
    fn get_by_source_id(&self, source_id: &str) -> JaymiResult<Option<Content>>;

    /// Retrieve metadata for a path without exposing parsers.
    fn retrieve_metadata(&self, path: &Path) -> JaymiResult<ContentMetadataView>;

    /// Retrieve plain text for a path.
    fn retrieve_plain_text(&self, path: &Path) -> JaymiResult<String>;

    /// Retrieve parser provenance recorded with the content.
    fn retrieve_parser_info(&self, path: &Path) -> JaymiResult<ParserInfo>;

    /// Retrieve aggregate content statistics.
    fn retrieve_statistics(&self) -> JaymiResult<ContentStatistics>;

    /// Retrieve content subsystem health.
    fn retrieve_health(&self) -> JaymiResult<ContentHealth>;
}
