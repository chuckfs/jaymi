//! Integration tests for Layer 3 Slice 6 — Citations and Previews.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_core::SearchRequest;
use jaymi_search::{SearchEngine, SearchEngineApi};
use jaymi_understanding::UnderstandingEngine;

#[test]
fn every_search_result_has_traceable_citation_provenance() {
    let data_dir = temp_dir("cite-data");
    let root = temp_dir("cite-root");
    let docs = root.join("Documents");
    fs::create_dir_all(&docs).unwrap();

    let paper = docs.join("biology_notes.md");
    let shopping = docs.join("errands.md");
    fs::write(
        &paper,
        "# Biology Notes\n\n## Habitat\n\nFungi grow in damp soil near oak trees.\n",
    )
    .unwrap();
    fs::write(&shopping, "# Errands\n\nBuy milk and bread.\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    for path in [&paper, &shopping] {
        understanding.understand_path(path).unwrap().unwrap();
    }

    let engine = app
        .container()
        .resolve::<Arc<SearchEngine>>()
        .expect("search");

    let results = engine
        .search(&SearchRequest::free_text("fungi"))
        .expect("search");
    assert!(!results.hits.is_empty());

    let citations = results.citations();
    assert_eq!(citations.len(), results.hits.len());

    for (hit, citation) in results.hits.iter().zip(citations.iter()) {
        assert!(
            hit.has_traceable_provenance(),
            "hit missing provenance: {:?}",
            hit.path
        );
        assert!(!citation.title.trim().is_empty());
        assert!(!citation.location.as_os_str().is_empty());
        assert!(!citation.preview.trim().is_empty());
        assert!(!citation.why_matched.trim().is_empty());
        assert!(citation.confidence <= 10_000);
        assert_eq!(citation.location, hit.path);
        assert_eq!(citation.confidence, hit.score);
    }

    // Filename-only hits also get required citation fields.
    let by_name = engine
        .search(&SearchRequest::filename("errands.md"))
        .expect("filename");
    assert_eq!(by_name.hits.len(), 1);
    let name_cite = by_name.citations();
    assert_eq!(name_cite.len(), 1);
    assert!(!name_cite[0].preview.is_empty());
    assert!(name_cite[0].why_matched.contains("filename"));

    // Planner can cite retrieved information.
    let response = app
        .search(SearchRequest::free_text("\"damp soil\""))
        .expect("planner");
    assert_eq!(response.tool_id.as_deref(), Some("search_knowledge"));
    assert!(!response.citations.is_empty());
    assert!(response.content.contains("Citations:"));
    assert!(response.content.contains("confidence"));
    assert!(response.content.contains("preview:"));
    for citation in &response.citations {
        assert!(!citation.title.is_empty());
        assert!(!citation.preview.is_empty());
        assert!(!citation.why_matched.is_empty());
    }

    // Diagnostics display citation generation.
    let stats = engine.stats().expect("stats");
    assert!(stats.citations_generated > 0);
    assert!(stats.last_citation_count.unwrap_or(0) > 0);

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot.subsystem("Search Engine").expect("search row");
    assert_eq!(row.status, OperationalStatus::Operational);
    assert!(
        row.detail.contains("citations="),
        "expected citation diagnostics, got {}",
        row.detail
    );
    assert!(row.detail.contains("last_citations="));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-cite-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
