//! Understanding Engine — Knowledge Item → Content Pipeline → Database.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
use jaymi_knowledge::{normalize_path, KnowledgeItem, KnowledgeStore, SqliteKnowledgeStore};
use jaymi_parsers::ParserRegistry;
use jaymi_providers::FilesystemProvider;

use crate::content::Content;
use crate::embedding_schedule::EmbeddingScheduler;
use crate::sqlite::SqliteContentStore;
use crate::store::ContentStore;

const NAME: &str = "understanding_engine";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database", "knowledge"];

/// Outcome of one understand attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnderstandOutcome {
    /// Fresh content returned from the store without re-parsing.
    Cached(Content),
    /// Newly extracted and persisted content.
    Parsed(Content),
    /// Source skipped because it is a directory.
    SkippedDirectory,
    /// Source skipped because no parser is registered.
    Unsupported(String),
    /// Parser was registered but the document could not be parsed.
    Failed(String),
}

/// Aggregate pipeline statistics for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnderstandingStats {
    /// Successfully stored documents (from DB).
    pub parsed_documents: u64,
    /// Documents with structural enrichment applied.
    pub enriched_documents: u64,
    /// Parser usage histogram `parser=count,...`.
    pub parser_usage: Vec<(String, u64)>,
    /// Failed parse attempts since boot.
    pub failed_parses: u64,
    /// Unsupported format encounters since boot.
    pub unsupported_formats: u64,
    /// Cache hits since boot.
    pub cache_hits: u64,
    /// Last failure message, when any.
    pub last_failure: Option<String>,
    /// Last unsupported extension/type, when any.
    pub last_unsupported: Option<String>,
}

#[derive(Default)]
struct RuntimeStats {
    failed_parses: u64,
    unsupported_formats: u64,
    cache_hits: u64,
    last_failure: Option<String>,
    last_unsupported: Option<String>,
}

/// Content Intelligence pipeline.
pub struct UnderstandingEngine {
    initialized: bool,
    knowledge: Arc<SqliteKnowledgeStore>,
    content: Arc<SqliteContentStore>,
    filesystem: Arc<FilesystemProvider>,
    parsers: Arc<ParserRegistry>,
    embedding_scheduler: Option<Arc<dyn EmbeddingScheduler>>,
    runtime: Mutex<RuntimeStats>,
}

impl UnderstandingEngine {
    /// Create an uninitialized understanding engine.
    pub fn new(
        knowledge: Arc<SqliteKnowledgeStore>,
        content: Arc<SqliteContentStore>,
        filesystem: Arc<FilesystemProvider>,
        parsers: Arc<ParserRegistry>,
    ) -> Self {
        Self::with_embedding_scheduler(knowledge, content, filesystem, parsers, None)
    }

    /// Create an uninitialized engine that schedules embeddings after upserts.
    pub fn with_embedding_scheduler(
        knowledge: Arc<SqliteKnowledgeStore>,
        content: Arc<SqliteContentStore>,
        filesystem: Arc<FilesystemProvider>,
        parsers: Arc<ParserRegistry>,
        embedding_scheduler: Option<Arc<dyn EmbeddingScheduler>>,
    ) -> Self {
        Self {
            initialized: false,
            knowledge,
            content,
            filesystem,
            parsers,
            embedding_scheduler,
            runtime: Mutex::new(RuntimeStats::default()),
        }
    }

    /// Shared content store.
    pub fn content_store(&self) -> &Arc<SqliteContentStore> {
        &self.content
    }

    /// Pipeline statistics for diagnostics.
    pub fn stats(&self) -> JaymiResult<UnderstandingStats> {
        self.ensure_initialized()?;
        let runtime = self
            .runtime
            .lock()
            .map(|guard| RuntimeStats {
                failed_parses: guard.failed_parses,
                unsupported_formats: guard.unsupported_formats,
                cache_hits: guard.cache_hits,
                last_failure: guard.last_failure.clone(),
                last_unsupported: guard.last_unsupported.clone(),
            })
            .unwrap_or_default();
        Ok(UnderstandingStats {
            parsed_documents: self.content.document_count()?,
            enriched_documents: self.content.enriched_count()?,
            parser_usage: self.content.parser_usage()?,
            failed_parses: runtime.failed_parses,
            unsupported_formats: runtime.unsupported_formats,
            cache_hits: runtime.cache_hits,
            last_failure: runtime.last_failure,
            last_unsupported: runtime.last_unsupported,
        })
    }

    /// Understand one knowledge item: cache hit, parse+store, or skip.
    pub fn understand_item(&self, item: &KnowledgeItem) -> JaymiResult<UnderstandOutcome> {
        self.ensure_initialized()?;
        if item.is_directory {
            return Ok(UnderstandOutcome::SkippedDirectory);
        }

        let path = normalize_path(&item.path)?;
        let source_id = path.to_string_lossy().into_owned();

        if let Some(existing) = self.content.get_by_source_id(&source_id)? {
            if is_fresh(&existing, item) {
                self.record_cache_hit();
                return Ok(UnderstandOutcome::Cached(existing));
            }
        }

        let Some(file_type) = ParserRegistry::detect_type(&path) else {
            self.record_unsupported(path.extension().map(|v| v.to_string_lossy().into_owned()));
            return Ok(UnderstandOutcome::Unsupported(format!(
                "cannot detect file type for {}",
                path.display()
            )));
        };

        let parser = match self.parsers.resolve(&file_type) {
            Ok(parser) => parser,
            Err(error) => {
                self.record_unsupported(Some(file_type.id().to_string()));
                return Ok(UnderstandOutcome::Unsupported(error.message().to_string()));
            }
        };

        let bytes = match self.filesystem.read_file(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.record_failure(error.message());
                return Err(error);
            }
        };

        let document = match parser.parse(&path, &bytes) {
            Ok(document) => document,
            Err(error) => {
                self.record_failure(error.message());
                return Ok(UnderstandOutcome::Failed(error.message().to_string()));
            }
        };

        let mut content = Content::from_document(&document, parser.version());
        self.attach_image_thumbnail(&mut content, &bytes);
        self.content.upsert(&content)?;
        if let Some(scheduler) = &self.embedding_scheduler {
            if let Err(error) = scheduler.schedule(&content.source_id) {
                jaymi_logging::warn(
                    "understanding",
                    format!(
                        "embedding schedule failed for {}: {}",
                        content.source_id,
                        error.message()
                    ),
                );
            }
        }
        Ok(UnderstandOutcome::Parsed(content))
    }

    /// Understand a path if it exists in the knowledge inventory.
    pub fn understand_path(&self, path: &Path) -> JaymiResult<Option<UnderstandOutcome>> {
        self.ensure_initialized()?;
        let normalized = normalize_path(path)?;
        let Some(item) = self.knowledge.get_by_path(&normalized)? else {
            return Ok(None);
        };
        Ok(Some(self.understand_item(&item)?))
    }

    /// Read content for the Planner, preferring stored normalized content.
    ///
    /// Returns `(content, source)` where source is `"stored"` or `"parsed"`.
    pub fn read_for_planner(&self, path: &Path) -> JaymiResult<(Content, &'static str)> {
        self.ensure_initialized()?;
        let normalized = normalize_path(path)?;

        if let Some(item) = self.knowledge.get_by_path(&normalized)? {
            if item.is_directory {
                return Err(JaymiError::new(format!(
                    "cannot read directory as document: {}",
                    normalized.display()
                )));
            }
            let source_id = normalized.to_string_lossy().into_owned();
            if let Some(existing) = self.content.get_by_source_id(&source_id)? {
                if is_fresh(&existing, &item) {
                    self.record_cache_hit();
                    return Ok((existing, "stored"));
                }
            }
            match self.understand_item(&item)? {
                UnderstandOutcome::Cached(content) => Ok((content, "stored")),
                UnderstandOutcome::Parsed(content) => Ok((content, "parsed")),
                UnderstandOutcome::SkippedDirectory => Err(JaymiError::new(format!(
                    "cannot read directory as document: {}",
                    normalized.display()
                ))),
                UnderstandOutcome::Unsupported(message) | UnderstandOutcome::Failed(message) => {
                    Err(JaymiError::new(message))
                }
            }
        } else {
            let file_type = ParserRegistry::detect_type(&normalized).ok_or_else(|| {
                JaymiError::new(format!(
                    "cannot detect file type for {}",
                    normalized.display()
                ))
            })?;
            let parser = self.parsers.resolve(&file_type)?;
            let bytes = self.filesystem.read_file(&normalized)?;
            let document = parser.parse(&normalized, &bytes)?;
            let mut content = Content::from_document(&document, parser.version());
            self.attach_image_thumbnail(&mut content, &bytes);
            Ok((content, "parsed"))
        }
    }

    fn attach_image_thumbnail(&self, content: &mut Content, bytes: &[u8]) {
        let Some(image) = content.image.as_mut() else {
            return;
        };
        let thumb_dir = self.content.thumbnail_dir();
        if let Err(error) = image.ensure_thumbnail(bytes, &content.source_id, &thumb_dir) {
            // Thumbnail failure should not drop metadata extraction.
            jaymi_logging::warn(
                "understanding",
                format!("thumbnail generation failed: {}", error.message()),
            );
        }
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("understanding engine is not initialized"))
        }
    }

    fn record_cache_hit(&self) {
        if let Ok(mut stats) = self.runtime.lock() {
            stats.cache_hits = stats.cache_hits.saturating_add(1);
        }
    }

    fn record_failure(&self, message: &str) {
        if let Ok(mut stats) = self.runtime.lock() {
            stats.failed_parses = stats.failed_parses.saturating_add(1);
            stats.last_failure = Some(message.to_string());
        }
    }

    fn record_unsupported(&self, label: Option<String>) {
        if let Ok(mut stats) = self.runtime.lock() {
            stats.unsupported_formats = stats.unsupported_formats.saturating_add(1);
            stats.last_unsupported = label.or_else(|| Some("unknown".to_string()));
        }
    }
}

impl Lifecycle for UnderstandingEngine {
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
        jaymi_logging::info("understanding", "understanding engine initialized");
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let stats = self.initialized.then(|| self.stats().ok()).flatten();
        let mut details = vec![];
        if let Some(stats) = stats {
            details.push((
                "parsed_documents".to_string(),
                stats.parsed_documents.to_string(),
            ));
            details.push((
                "enriched_documents".to_string(),
                stats.enriched_documents.to_string(),
            ));
            details.push(("failed_parses".to_string(), stats.failed_parses.to_string()));
            details.push((
                "unsupported_formats".to_string(),
                stats.unsupported_formats.to_string(),
            ));
            details.push((
                "parser_usage".to_string(),
                format_parser_usage(&stats.parser_usage),
            ));
        }
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(details)
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

fn is_fresh(content: &Content, item: &KnowledgeItem) -> bool {
    match item.modified.or(item.last_modified) {
        Some(modified) => content.extraction_timestamp >= modified,
        None => true,
    }
}

/// Format parser usage for diagnostics detail strings.
pub fn format_parser_usage(usage: &[(String, u64)]) -> String {
    if usage.is_empty() {
        return "-".to_string();
    }
    usage
        .iter()
        .map(|(parser, count)| format!("{parser}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Build a compact usage map for tests.
pub fn usage_map(usage: Vec<(String, u64)>) -> BTreeMap<String, u64> {
    usage.into_iter().collect()
}
