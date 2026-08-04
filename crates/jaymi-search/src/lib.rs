//! Unified Search Engine for Jaymi.
//!
//! The Search Engine is the single entry point for retrieval. Planner tools
//! call this crate and never query the database or Knowledge Store directly.
//!
//! No embeddings. No AI ranking. No internet.

#![forbid(unsafe_code)]

mod citation;
mod content_rank;
mod embedding_queue;
mod engine;
mod hybrid_rank;
mod locate;
mod result;
mod stats;
mod strategy;

pub use citation::{ensure_hit_previews, hits_to_citations};
pub use content_rank::{rank_content_match, ContentRank};
pub use embedding_queue::{EmbeddingQueue, EmbeddingQueueDiagnostics};
pub use engine::{
    hits_to_entries, request_from_discovery, SearchEngine, SearchEngineApi, SemanticDeps,
};
pub use hybrid_rank::{
    fuse_relevance, normalize_channel, ranking_now_unix, recency_score,
    semantic_signal_from_similarity, RankSignals, SCORE_SCALE,
};
pub use locate::{locate_matches, replace_matches, LocatedMatch};
pub use result::{MatchReason, SearchHit, SearchResults};
pub use stats::{SearchHealth, SearchStats};
pub use strategy::{select_strategy, SearchStrategy};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use jaymi_core::{Lifecycle, MetadataFilters, SearchRequest};
    use jaymi_database::Database;
    use jaymi_knowledge::{normalize_path, KnowledgeItem, KnowledgeStore, SqliteKnowledgeStore};
    use jaymi_parsers::default_registry;
    use jaymi_providers::{FilesystemProvider, Provider};
    use jaymi_understanding::{ContentIntelligenceApi, SqliteContentStore, UnderstandingEngine};

    fn boot_engine(data: &std::path::Path) -> (Arc<SqliteKnowledgeStore>, SearchEngine) {
        let mut db = Database::with_data_dir(data);
        db.initialize().unwrap();
        let db = Arc::new(db);
        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);
        let mut engine = SearchEngine::new(Arc::clone(&knowledge), None);
        engine.initialize().unwrap();
        (knowledge, engine)
    }

    fn publish(
        knowledge: &SqliteKnowledgeStore,
        path: &std::path::Path,
        filename: &str,
        extension: Option<&str>,
        size: u64,
        dir: bool,
    ) {
        let path = normalize_path(path).unwrap();
        let parent = path.parent().map(|p| p.to_path_buf());
        knowledge
            .publish(
                &KnowledgeItem {
                    path,
                    filename: filename.into(),
                    extension: extension.map(|v| v.into()),
                    size,
                    created: Some(100),
                    modified: Some(200),
                    is_directory: dir,
                    hidden: filename.starts_with('.'),
                    parent,
                    first_discovered: Some(100),
                    last_indexed: Some(100),
                    last_modified: Some(200),
                    last_verified: Some(100),
                    device_id: None,
                    inode: None,
                },
                100,
            )
            .unwrap();
    }

    #[test]
    fn every_strategy_returns_deterministic_results() {
        let data = temp_dir("search-data");
        let root = temp_dir("search-root");
        let docs = root.join("Documents");
        fs::create_dir_all(&docs).unwrap();
        let pdf = docs.join("biology_fungi.pdf");
        let md = docs.join("notes.md");
        let txt = docs.join("readme.txt");
        fs::write(&pdf, b"%PDF").unwrap();
        fs::write(&md, b"# fungi").unwrap();
        fs::write(&txt, b"hello").unwrap();

        let (knowledge, engine) = boot_engine(&data);
        publish(&knowledge, &pdf, "biology_fungi.pdf", Some("pdf"), 4, false);
        publish(&knowledge, &md, "notes.md", Some("md"), 7, false);
        publish(&knowledge, &txt, "readme.txt", Some("txt"), 5, false);
        publish(
            &knowledge,
            &docs,
            "Documents",
            None,
            0,
            true,
        );

        // Filename
        let filename = engine
            .search(&SearchRequest::filename("biology_fungi.pdf"))
            .unwrap();
        assert_eq!(filename.strategy, SearchStrategy::Filename);
        assert_eq!(filename.hits.len(), 1);
        assert_eq!(filename.hits[0].title, "biology_fungi.pdf");
        assert_eq!(filename.hits[0].match_reason, MatchReason::FilenameExact);
        let filename_again = engine
            .search(&SearchRequest::filename("biology_fungi.pdf"))
            .unwrap();
        assert_eq!(filename.hits, filename_again.hits);

        // Extension
        let extension = engine.search(&SearchRequest::extension("pdf")).unwrap();
        assert_eq!(extension.strategy, SearchStrategy::Extension);
        assert_eq!(extension.hits.len(), 1);
        assert_eq!(extension.hits[0].match_reason, MatchReason::Extension);

        // Folder
        let folder = engine
            .search(&SearchRequest::folder(&docs, true))
            .unwrap();
        assert_eq!(folder.strategy, SearchStrategy::Folder);
        assert!(folder.hits.iter().any(|hit| hit.title == "notes.md"));
        let folder_again = engine
            .search(&SearchRequest::folder(&docs, true))
            .unwrap();
        assert_eq!(folder.hits, folder_again.hits);

        // Free text
        let free = engine
            .search(&SearchRequest::free_text("fungi"))
            .unwrap();
        assert_eq!(free.strategy, SearchStrategy::FreeText);
        assert!(free.hits.iter().any(|hit| hit.title.contains("fungi")));
        let free_again = engine
            .search(&SearchRequest::free_text("fungi"))
            .unwrap();
        assert_eq!(free.hits, free_again.hits);

        // Metadata (largest)
        let meta = engine
            .search(&SearchRequest {
                metadata: MetadataFilters {
                    largest: true,
                    files_only: true,
                    ..MetadataFilters::default()
                },
                limit: Some(10),
                ..SearchRequest::default()
            })
            .unwrap();
        assert_eq!(meta.strategy, SearchStrategy::Metadata);
        assert!(!meta.hits.is_empty());

        // Combined
        let combined = engine
            .search(&SearchRequest {
                free_text: Some("biology".into()),
                extension: Some("pdf".into()),
                limit: Some(10),
                ..SearchRequest::default()
            })
            .unwrap();
        assert_eq!(combined.strategy, SearchStrategy::Combined);
        assert_eq!(combined.hits.len(), 1);

        let stats = engine.stats().unwrap();
        assert!(stats.search_count >= 6);
        assert!(stats.last_strategy.is_some());
        let health = engine.health().unwrap();
        assert!(health.healthy);
        assert!(health.detail.contains("searches="));
    }

    fn boot_engine_with_content(
        data: &std::path::Path,
    ) -> (Arc<SqliteKnowledgeStore>, Arc<UnderstandingEngine>, SearchEngine) {
        let mut db = Database::with_data_dir(data);
        db.initialize().unwrap();
        let db = Arc::new(db);

        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);

        let content_store = Arc::new(SqliteContentStore::new(Arc::clone(&db)));
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let filesystem = Arc::new(filesystem);
        let parsers = Arc::new(default_registry().unwrap());

        let mut understanding = UnderstandingEngine::new(
            Arc::clone(&knowledge),
            content_store,
            filesystem,
            parsers,
        );
        understanding.initialize().unwrap();
        let understanding = Arc::new(understanding);
        let content_api = Arc::new(ContentIntelligenceApi::new(Arc::clone(&understanding)));

        let mut engine = SearchEngine::new(Arc::clone(&knowledge), Some(content_api));
        engine.initialize().unwrap();
        (knowledge, understanding, engine)
    }

    #[test]
    fn free_text_search_expands_content_matches_with_locations() {
        let data = temp_dir("search-locate-data");
        let root = temp_dir("search-locate-root");
        let path = root.join("main.rs");
        fs::write(
            &path,
            "fn main() {\n    let needle = find_the_needle();\n    println!(\"{needle}\");\n}\n",
        )
        .unwrap();

        let (knowledge, understanding, engine) = boot_engine_with_content(&data);
        publish(&knowledge, &path, "main.rs", Some("rs"), 64, false);
        let item = knowledge.get_by_path(&path).unwrap().unwrap();
        understanding.understand_item(&item).unwrap();

        let results = engine
            .search(&SearchRequest::free_text("find_the_needle"))
            .unwrap();
        let hit = results
            .hits
            .iter()
            .find(|hit| hit.path == normalize_path(&path).unwrap())
            .expect("expected a located content hit");
        assert_eq!(hit.line, Some(1));
        assert!(hit.column.unwrap() > 0);
        assert_eq!(hit.end_line, Some(1));
        assert!(hit.preview.as_deref().unwrap().contains("find_the_needle"));

        let citation = hit.to_citation();
        assert_eq!(citation.line, Some(1));

        // filename_only must never expand into per-line hits.
        let filename_only = SearchRequest::free_text("find_the_needle").with_filename_only(true);
        let filename_results = engine.search(&filename_only).unwrap();
        assert!(filename_results
            .hits
            .iter()
            .all(|hit| hit.line.is_none()));
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-search-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
