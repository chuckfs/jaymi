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
