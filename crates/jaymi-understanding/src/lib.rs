//! Content Intelligence pipeline for Jaymi.
//!
//! Converts Knowledge Items into normalized Content and persists them.
//! Consumers access content through [`ContentIntelligence`] — never parsers
//! or SQLite directly. No OCR engines, embeddings, summaries, or LLMs.

#![forbid(unsafe_code)]

mod api;
mod content;
mod engine;
mod enrichment;
mod image_content;
mod service;
mod sqlite;
mod store;

pub use api::{
    ContentHealth, ContentIntelligence, ContentLoad, ContentMetadataView, ContentSource,
    ContentStatistics, ParserInfo,
};
pub use content::Content;
pub use engine::{
    format_parser_usage, usage_map, UnderstandOutcome, UnderstandingEngine, UnderstandingStats,
};
pub use enrichment::{
    ContentEnrichment, Heading, Section, ENRICHMENT_VERSION, READING_WORDS_PER_MINUTE,
};
pub use image_content::{ImageContent, THUMBNAIL_MAX_EDGE};
pub use service::ContentIntelligenceApi;
pub use sqlite::SqliteContentStore;
pub use store::ContentStore;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use jaymi_core::Lifecycle;
    use jaymi_database::Database;
    use jaymi_knowledge::{normalize_path, KnowledgeStore, SqliteKnowledgeStore};
    use jaymi_parsers::default_registry;
    use jaymi_providers::{FilesystemProvider, Provider};

    fn boot_engine(data: &std::path::Path) -> (Arc<SqliteKnowledgeStore>, UnderstandingEngine) {
        let mut db = Database::with_data_dir(data);
        db.initialize().unwrap();
        let db = Arc::new(db);

        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);

        let content = Arc::new(SqliteContentStore::new(Arc::clone(&db)));
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let filesystem = Arc::new(filesystem);
        let parsers = Arc::new(default_registry().unwrap());

        let mut engine =
            UnderstandingEngine::new(Arc::clone(&knowledge), content, filesystem, parsers);
        engine.initialize().unwrap();
        (knowledge, engine)
    }

    fn publish_file(
        knowledge: &SqliteKnowledgeStore,
        path: &std::path::Path,
        root: &std::path::Path,
        filename: &str,
        extension: &str,
        size: u64,
    ) {
        knowledge
            .publish(
                &jaymi_knowledge::KnowledgeItem {
                    path: normalize_path(path).unwrap(),
                    filename: filename.into(),
                    extension: Some(extension.into()),
                    size,
                    created: Some(100),
                    modified: Some(100),
                    is_directory: false,
                    hidden: false,
                    parent: Some(normalize_path(root).unwrap()),
                    first_discovered: Some(100),
                    last_indexed: Some(100),
                    last_modified: Some(100),
                    last_verified: Some(100),
                    device_id: None,
                    inode: None,
                },
                100,
            )
            .unwrap();
    }

    #[test]
    fn pipeline_parses_persists_and_reuses_content() {
        let data = temp_dir("understanding-data");
        let root = temp_dir("understanding-root");
        let path = root.join("note.md");
        fs::write(&path, "# Title\n\nBody").unwrap();

        let (knowledge, engine) = boot_engine(&data);
        publish_file(&knowledge, &path, &root, "note.md", "md", 14);

        let item = knowledge.get_by_path(&path).unwrap().unwrap();
        let first = engine.understand_item(&item).unwrap();
        match first {
            UnderstandOutcome::Parsed(content) => {
                assert_eq!(content.content_type, "markdown");
                assert!(content.plain_text.contains("Body"));
                assert_eq!(content.title.as_deref(), Some("Title"));
                assert_eq!(content.parser_used, "markdown");
                assert!(!content.parser_version.is_empty());
                assert_eq!(content.enrichment.headings[0].text, "Title");
                assert_eq!(content.enrichment.word_count, 3);
                assert_eq!(content.enrichment.version, ENRICHMENT_VERSION);
            }
            other => panic!("expected Parsed, got {other:?}"),
        }

        let second = engine.understand_item(&item).unwrap();
        assert!(matches!(second, UnderstandOutcome::Cached(_)));

        let stored = engine
            .content_store()
            .get_by_source_id(normalize_path(&path).unwrap().to_string_lossy().as_ref())
            .unwrap()
            .expect("persisted");
        assert!(stored.plain_text.contains("Body"));
        assert_eq!(stored.enrichment.headings[0].text, "Title");

        let (content, source) = engine.read_for_planner(&path).unwrap();
        assert_eq!(source, "stored");
        assert_eq!(content.plain_text, stored.plain_text);
        assert_eq!(content.enrichment, stored.enrichment);

        let stats = engine.stats().unwrap();
        assert_eq!(stats.parsed_documents, 1);
        assert_eq!(stats.enriched_documents, 1);
        assert!(stats.cache_hits >= 1);
        assert!(stats
            .parser_usage
            .iter()
            .any(|(parser, count)| parser == "markdown" && *count == 1));
    }

    #[test]
    fn enrichment_is_deterministic_across_pipeline() {
        let data = temp_dir("understanding-enrich-data");
        let root = temp_dir("understanding-enrich-root");
        let path = root.join("guide.md");
        let body = "# Guide\n\nThe and of to a in is that for on with as this be are by from or an it.\n\nSee [local](./a.md) and https://example.com.\n";
        fs::write(&path, body).unwrap();

        let (knowledge, engine) = boot_engine(&data);
        publish_file(
            &knowledge,
            &path,
            &root,
            "guide.md",
            "md",
            body.len() as u64,
        );

        let item = knowledge.get_by_path(&path).unwrap().unwrap();
        let UnderstandOutcome::Parsed(first) = engine.understand_item(&item).unwrap() else {
            panic!("expected parsed");
        };
        // Force re-parse by clearing and republishing with newer mtime.
        engine
            .content_store()
            .remove_by_source_id(normalize_path(&path).unwrap().to_string_lossy().as_ref())
            .unwrap();
        let UnderstandOutcome::Parsed(second) = engine.understand_item(&item).unwrap() else {
            panic!("expected parsed again");
        };
        assert_eq!(first.enrichment, second.enrichment);
        assert_eq!(first.enrichment.internal_links, vec!["./a.md".to_string()]);
        assert_eq!(
            first.enrichment.external_links,
            vec!["https://example.com".to_string()]
        );
        assert_eq!(first.language.as_deref(), Some("en"));
    }

    #[test]
    fn unsupported_formats_are_tracked() {
        let data = temp_dir("understanding-unsup-data");
        let root = temp_dir("understanding-unsup-root");
        let path = root.join("archive.bin");
        fs::write(&path, b"\x00\x01\x02").unwrap();

        let (knowledge, engine) = boot_engine(&data);
        publish_file(&knowledge, &path, &root, "archive.bin", "bin", 3);

        let item = knowledge.get_by_path(&path).unwrap().unwrap();
        let outcome = engine.understand_item(&item).unwrap();
        assert!(matches!(outcome, UnderstandOutcome::Unsupported(_)));
        let stats = engine.stats().unwrap();
        assert!(stats.unsupported_formats >= 1);
        assert_eq!(stats.parsed_documents, 0);
    }

    #[test]
    fn corrupted_documents_fail_gracefully() {
        let data = temp_dir("understanding-corrupt-data");
        let root = temp_dir("understanding-corrupt-root");
        let path = root.join("broken.pdf");
        fs::write(&path, b"%PDF-1.4\nnot-a-real-pdf").unwrap();

        let (knowledge, engine) = boot_engine(&data);
        publish_file(&knowledge, &path, &root, "broken.pdf", "pdf", 20);

        let item = knowledge.get_by_path(&path).unwrap().unwrap();
        let outcome = engine.understand_item(&item).unwrap();
        assert!(matches!(outcome, UnderstandOutcome::Failed(_)));
        let stats = engine.stats().unwrap();
        assert!(stats.failed_parses >= 1);
        assert_eq!(stats.parsed_documents, 0);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-understanding-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
