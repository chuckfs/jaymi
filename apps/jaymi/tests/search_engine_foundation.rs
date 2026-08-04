//! Integration tests for Layer 3 Slice 1 — Search Engine Foundation.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_core::{MetadataFilters, SearchRequest};
use jaymi_search::{SearchEngine, SearchEngineApi, SearchStrategy};

#[test]
fn search_engine_strategies_are_deterministic_through_planner() {
    let data_dir = temp_dir("search-it-data");
    let root = temp_dir("search-it-root");
    let docs = root.join("Documents");
    fs::create_dir_all(&docs).unwrap();
    fs::write(docs.join("biology_fungi.pdf"), b"%PDF").unwrap();
    fs::write(docs.join("notes.md"), b"# hello fungi").unwrap();
    fs::write(docs.join("readme.txt"), b"plain").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let engine = app
        .container()
        .resolve::<Arc<SearchEngine>>()
        .expect("search engine");

    let filename = engine
        .search(&SearchRequest::filename("biology_fungi.pdf"))
        .expect("filename");
    assert_eq!(filename.strategy, SearchStrategy::Filename);
    assert_eq!(filename.hits.len(), 1);
    assert_eq!(filename.hits[0].title, "biology_fungi.pdf");
    let filename_again = engine
        .search(&SearchRequest::filename("biology_fungi.pdf"))
        .expect("filename again");
    assert_eq!(filename.hits, filename_again.hits);

    let extension = engine
        .search(&SearchRequest::extension("pdf"))
        .expect("extension");
    assert_eq!(extension.strategy, SearchStrategy::Extension);
    assert_eq!(extension.hits.len(), 1);

    let folder = engine
        .search(&SearchRequest::folder(&docs, true))
        .expect("folder");
    assert_eq!(folder.strategy, SearchStrategy::Folder);
    assert!(folder.hits.iter().any(|hit| hit.title == "notes.md"));

    let free = engine
        .search(&SearchRequest::free_text("fungi"))
        .expect("free text");
    // Free-text upgrades to Semantic when an embedding provider is available.
    assert!(matches!(
        free.strategy,
        SearchStrategy::FreeText | SearchStrategy::Semantic
    ));
    assert!(free.hits.iter().any(|hit| hit.title.contains("fungi")));

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
        .expect("metadata");
    assert_eq!(meta.strategy, SearchStrategy::Metadata);
    assert!(!meta.hits.is_empty());

    // Planner retrieval path uses Search Engine (not Knowledge Store / SQLite).
    let response = app
        .search(SearchRequest::free_text("fungi"))
        .expect("planner search");
    assert_eq!(response.tool_id.as_deref(), Some("search_knowledge"));
    assert!(response
        .entries
        .iter()
        .any(|entry| entry.name.contains("fungi")));
    assert!(response.content.contains("search_knowledge"));

    let discover = app.discover_inventory().expect("discover");
    assert_eq!(discover.tool_id.as_deref(), Some("query_inventory"));
    assert!(discover.content.contains("search"));

    let stats = engine.stats().expect("stats");
    assert!(stats.search_count >= 6);
    assert!(stats.last_strategy.is_some());

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot
        .subsystem("Search Engine")
        .expect("search engine row");
    assert_eq!(row.status, OperationalStatus::Operational);
    assert!(row.detail.contains("searches="));
    assert!(row.detail.contains("avg_ms="));
    assert!(row.detail.contains("strategy="));
    assert!(row.detail.contains("citations="));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-search-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
