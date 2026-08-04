//! Integration tests for Layer 3 Slice 3 — Metadata Search.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::{MetadataFilters, SearchRequest};
use jaymi_search::{MatchReason, SearchEngine, SearchEngineApi, SearchStrategy};
use jaymi_understanding::{
    ContentIntelligence, ContentIntelligenceApi, ContentStore, UnderstandingEngine,
};

#[test]
fn metadata_search_is_deterministic_and_independent_of_fts() {
    let data_dir = temp_dir("meta-data");
    let root = temp_dir("meta-root");
    let docs = root.join("Documents");
    fs::create_dir_all(&docs).unwrap();

    let paper = docs.join("report.md");
    let shopping = docs.join("list.md");
    let spanish = docs.join("nota.md");

    fs::write(
        &paper,
        "# Habitat\n\nThe and of to a in is that for on with as this be are by from or an it.\n\nFungi details.\n",
    )
    .unwrap();
    fs::write(&shopping, "# Errands\n\nBuy milk and bread tomorrow.\n").unwrap();
    // Spanish stopword density for language detection.
    fs::write(
        &spanish,
        "# Notas\n\nEl de la que y en un ser se no haber por con su para como estar tener.\n",
    )
    .unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    for path in [&paper, &shopping, &spanish] {
        understanding.understand_path(path).unwrap().unwrap();
    }

    // Enrich author/tags on the paper (first-class searchable metadata).
    let content_store = app
        .container()
        .resolve::<Arc<jaymi_understanding::SqliteContentStore>>()
        .expect("content store");
    let source_id = jaymi_knowledge::normalize_path(&paper)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut content = content_store
        .get_by_source_id(&source_id)
        .unwrap()
        .expect("paper content");
    content.author = Some("Ada Lovelace".into());
    content.tags = vec!["biology".into(), "research".into()];
    content_store.upsert(&content).unwrap();

    let engine = app
        .container()
        .resolve::<Arc<SearchEngine>>()
        .expect("search");

    // Language
    let by_lang = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            language: Some("en".into()),
            ..MetadataFilters::default()
        }))
        .expect("language");
    assert_eq!(by_lang.strategy, SearchStrategy::StructuredMetadata);
    assert!(by_lang
        .hits
        .iter()
        .any(|hit| hit.path.ends_with("report.md")));
    assert!(!by_lang.hits.iter().any(|hit| hit.path.ends_with("nota.md")));
    let by_lang_again = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            language: Some("en".into()),
            ..MetadataFilters::default()
        }))
        .expect("language again");
    assert_eq!(by_lang.hits, by_lang_again.hits);

    // File type
    let by_type = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            content_type: Some("markdown".into()),
            ..MetadataFilters::default()
        }))
        .expect("type");
    assert_eq!(by_type.strategy, SearchStrategy::StructuredMetadata);
    assert!(by_type.hits.len() >= 3);
    assert!(by_type
        .hits
        .iter()
        .all(|hit| matches!(hit.match_reason, MatchReason::MetadataContentType)));

    // Author
    let by_author = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            author: Some("Ada".into()),
            ..MetadataFilters::default()
        }))
        .expect("author");
    assert_eq!(by_author.hits.len(), 1);
    assert_eq!(by_author.hits[0].match_reason, MatchReason::MetadataAuthor);

    // Tags
    let by_tag = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            tag: Some("biology".into()),
            ..MetadataFilters::default()
        }))
        .expect("tag");
    assert_eq!(by_tag.hits.len(), 1);
    assert_eq!(by_tag.hits[0].match_reason, MatchReason::MetadataTag);

    // Headings
    let by_heading = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            heading_contains: Some("Habitat".into()),
            ..MetadataFilters::default()
        }))
        .expect("heading");
    assert_eq!(by_heading.hits.len(), 1);
    assert_eq!(
        by_heading.hits[0].match_reason,
        MatchReason::MetadataHeading
    );
    assert_eq!(
        by_heading.hits[0].matching_section.as_deref(),
        Some("Habitat")
    );

    // Dates (extraction window that includes all understood docs)
    let by_date = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            extracted_after: Some(0),
            extracted_before: Some(i64::MAX),
            content_type: Some("markdown".into()),
            ..MetadataFilters::default()
        }))
        .expect("dates");
    assert!(!by_date.hits.is_empty());
    assert!(by_date.hits.iter().any(|hit| {
        matches!(
            hit.match_reason,
            MatchReason::MetadataDate
                | MatchReason::MetadataContentType
                | MatchReason::Combined { .. }
        )
    }));

    // Independence: free-text FTS must not run for metadata-only requests.
    // Searching metadata for a body word that appears only in plain_text via FTS
    // should return nothing when using structured language filter alone on Spanish.
    let spanish_only = engine
        .search(&SearchRequest::metadata(MetadataFilters {
            language: Some("es".into()),
            ..MetadataFilters::default()
        }))
        .expect("es");
    assert!(spanish_only
        .hits
        .iter()
        .any(|hit| hit.path.ends_with("nota.md")));
    assert!(!spanish_only
        .hits
        .iter()
        .any(|hit| hit.path.ends_with("report.md")));

    // Combined free-text + metadata intersects independently.
    let combined = engine
        .search(&SearchRequest {
            free_text: Some("Fungi".into()),
            metadata: MetadataFilters {
                language: Some("en".into()),
                ..MetadataFilters::default()
            },
            limit: Some(10),
            ..SearchRequest::default()
        })
        .expect("combined");
    assert_eq!(combined.strategy, SearchStrategy::Combined);
    assert!(combined
        .hits
        .iter()
        .any(|hit| hit.path.ends_with("report.md")));

    // Content Intelligence API surface.
    let api = app
        .container()
        .resolve::<Arc<ContentIntelligenceApi>>()
        .expect("api");
    let api_hits = api
        .search_metadata(
            &MetadataFilters {
                author: Some("Ada".into()),
                ..MetadataFilters::default()
            },
            10,
        )
        .expect("api metadata");
    assert_eq!(api_hits.len(), 1);

    // Planner remains unaware of SQL/FTS — just SearchRequest → tool.
    let response = app
        .search(SearchRequest::metadata(MetadataFilters {
            tag: Some("biology".into()),
            ..MetadataFilters::default()
        }))
        .expect("planner");
    assert_eq!(response.tool_id.as_deref(), Some("search_knowledge"));
    assert!(!response.entries.is_empty());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-meta-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
