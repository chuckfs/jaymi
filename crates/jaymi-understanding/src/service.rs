//! Content Intelligence API implementation.

use std::path::Path;
use std::sync::Arc;

use jaymi_core::{JaymiResult, Lifecycle};

use crate::api::{
    ContentHealth, ContentIntelligence, ContentLoad, ContentMetadataView, ContentSource,
    ContentStatistics, ParserInfo,
};
use crate::content::Content;
use crate::engine::UnderstandingEngine;
use crate::store::ContentStore;

/// Consumer-facing Content Intelligence API.
///
/// Wraps the understanding pipeline and normalized content store. The Planner
/// and tools use this type — never parsers or SQLite content tables.
pub struct ContentIntelligenceApi {
    engine: Arc<UnderstandingEngine>,
}

impl ContentIntelligenceApi {
    /// Create an API bound to an initialized understanding engine.
    pub fn new(engine: Arc<UnderstandingEngine>) -> Self {
        Self { engine }
    }

    /// Borrow the underlying understanding engine (pipeline / indexing only).
    pub fn engine(&self) -> &Arc<UnderstandingEngine> {
        &self.engine
    }
}

impl ContentIntelligence for ContentIntelligenceApi {
    fn load_content(&self, path: &Path) -> JaymiResult<ContentLoad> {
        let (content, source) = self.engine.read_for_planner(path)?;
        let source = match source {
            "stored" => ContentSource::Stored,
            _ => ContentSource::Parsed,
        };
        Ok(ContentLoad { content, source })
    }

    fn get_by_source_id(&self, source_id: &str) -> JaymiResult<Option<Content>> {
        self.engine.content_store().get_by_source_id(source_id)
    }

    fn retrieve_metadata(&self, path: &Path) -> JaymiResult<ContentMetadataView> {
        let loaded = self.load_content(path)?;
        Ok(ContentMetadataView::from_content(&loaded.content))
    }

    fn retrieve_plain_text(&self, path: &Path) -> JaymiResult<String> {
        let loaded = self.load_content(path)?;
        Ok(loaded.content.plain_text)
    }

    fn retrieve_parser_info(&self, path: &Path) -> JaymiResult<ParserInfo> {
        let loaded = self.load_content(path)?;
        Ok(ParserInfo::from_content(&loaded.content))
    }

    fn retrieve_statistics(&self) -> JaymiResult<ContentStatistics> {
        let stats = self.engine.stats()?;
        Ok(ContentStatistics {
            document_count: stats.parsed_documents,
            enriched_count: stats.enriched_documents,
            image_count: self.engine.content_store().image_count()?,
            parser_usage: stats.parser_usage,
            failed_parses: stats.failed_parses,
            unsupported_formats: stats.unsupported_formats,
            cache_hits: stats.cache_hits,
        })
    }

    fn retrieve_health(&self) -> JaymiResult<ContentHealth> {
        let report = self.engine.health_check();
        let statistics = self.retrieve_statistics().unwrap_or_default();
        let detail = if !report.initialized {
            "content intelligence is not initialized".to_string()
        } else {
            format!(
                "documents={} enriched={} images={} parser_kinds={}",
                statistics.document_count,
                statistics.enriched_count,
                statistics.image_count,
                statistics.parser_usage.len()
            )
        };
        Ok(ContentHealth {
            initialized: report.initialized,
            healthy: report.healthy && report.initialized,
            version: report.version,
            detail,
            statistics,
        })
    }
}
