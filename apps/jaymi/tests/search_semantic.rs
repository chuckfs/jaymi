//! Integration tests for Layer 3 Slice 5 — Semantic Search.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::SearchRequest;
use jaymi_providers::{EmbeddingProvider, LOCAL_EMBEDDING_MODEL};
use jaymi_search::{EmbeddingQueue, MatchReason, SearchEngine, SearchEngineApi, SearchStrategy};
use jaymi_understanding::UnderstandingEngine;

#[test]
fn semantic_retrieval_finds_meaning_not_exact_words() {
    let data_dir = temp_dir("semantic-data");
    let root = temp_dir("semantic-root");
    let docs = root.join("Documents");
    fs::create_dir_all(&docs).unwrap();

    // No shared exact query tokens with "where do fungi live" after concept fold,
    // but mushrooms/damp/woodland map to the same semantic concepts.
    let biology = docs.join("field_notes.md");
    let shopping = docs.join("errands.md");
    fs::write(
        &biology,
        "# Field Notes\n\nMushrooms thrive in damp woodland soil near oak trees.\n",
    )
    .unwrap();
    fs::write(&shopping, "# Errands\n\nBuy milk and bread tomorrow.\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    for path in [&biology, &shopping] {
        understanding.understand_path(path).unwrap().unwrap();
    }

    // Async embedding generation — flush until both documents are indexed.
    let queue = app
        .container()
        .resolve::<Arc<EmbeddingQueue>>()
        .expect("queue");
    let mut indexed = 0u64;
    for _ in 0..50 {
        let _ = queue.process_pending().expect("flush embeddings");
        indexed = queue.diagnostics().expect("diagnostics").indexed_embeddings;
        if indexed >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        indexed >= 2,
        "expected embeddings for both docs, indexed={indexed}"
    );

    let diagnostics = queue.diagnostics().expect("diagnostics");
    assert_eq!(diagnostics.model_id, LOCAL_EMBEDDING_MODEL);
    assert!(diagnostics.indexed_embeddings >= 2);

    let snapshot = app.diagnostics().expect("app diagnostics");
    let provider_row = snapshot
        .subsystem("Embedding Provider")
        .expect("embedding provider row");
    assert!(provider_row.detail.contains(LOCAL_EMBEDDING_MODEL));
    let queue_row = snapshot
        .subsystem("Embedding Queue")
        .expect("embedding queue row");
    assert!(queue_row.detail.contains("indexed="));
    assert!(queue_row.detail.contains("queue="));

    let engine = app
        .container()
        .resolve::<Arc<SearchEngine>>()
        .expect("search");
    assert!(engine.semantic_available());

    // Query uses different surface words than the biology document body.
    let results = engine
        .search(&SearchRequest::free_text("where do fungi live"))
        .expect("semantic search");
    assert_eq!(results.strategy, SearchStrategy::Semantic);
    assert!(
        results
            .hits
            .iter()
            .any(|hit| hit.path.ends_with("field_notes.md")),
        "expected meaning-based hit, got {:?}",
        results
            .hits
            .iter()
            .map(|hit| hit
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
    );
    assert!(
        !results
            .hits
            .iter()
            .any(|hit| hit.path.ends_with("errands.md") && hit.signals.semantic > 0),
        "shopping list should not rank as a semantic fungi hit"
    );

    let biology_hit = results
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("field_notes.md"))
        .expect("biology hit");
    assert!(biology_hit.signals.semantic > 0);
    assert!(matches!(
        biology_hit.match_reason,
        MatchReason::Semantic | MatchReason::Combined { .. }
    ));

    // Deterministic ranking across repeated queries.
    let again = engine
        .search(&SearchRequest::free_text("where do fungi live"))
        .expect("again");
    assert_eq!(
        results
            .hits
            .iter()
            .map(|hit| (&hit.item_id, hit.score, hit.signals.semantic))
            .collect::<Vec<_>>(),
        again
            .hits
            .iter()
            .map(|hit| (&hit.item_id, hit.score, hit.signals.semantic))
            .collect::<Vec<_>>()
    );

    // Planner stays on SearchRequest — no embedding types in the tool path.
    let response = app
        .search(SearchRequest::free_text("where do fungi live"))
        .expect("planner");
    assert_eq!(response.tool_id.as_deref(), Some("search_knowledge"));
    assert!(!response.entries.is_empty());

    let provider = app
        .container()
        .resolve::<Arc<jaymi_providers::LocalEmbeddingProvider>>()
        .expect("provider");
    assert_eq!(provider.model_id(), LOCAL_EMBEDDING_MODEL);
    assert!(provider.embedding_status().available);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-semantic-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
