//! Integration tests for Layer 2 Slice 3 — Metadata Enrichment.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_parsers::fixtures;
use jaymi_understanding::{
    ContentStore, SqliteContentStore, UnderstandOutcome, UnderstandingEngine, ENRICHMENT_VERSION,
};

#[test]
fn enrichment_is_stored_for_every_supported_format() {
    let data_dir = temp_dir("enrich-data");
    let root = temp_dir("enrich-root");

    fs::write(
        root.join("note.txt"),
        "INTRODUCTION\n\nThe and of to a in is that for on with as this be are by from or an it.\n",
    )
    .unwrap();
    fs::write(
        root.join("note.md"),
        "# Overview\n\nThe and of to a in is that for on with as this be are by from.\n\n## Links\n\nSee [local](./x.md) and https://example.org/docs.\n",
    )
    .unwrap();
    fs::write(
        root.join("note.json"),
        r#"{"title":"Config","enabled":true,"url":"https://example.com"}"#,
    )
    .unwrap();
    fs::write(root.join("note.pdf"), fixtures::minimal_pdf()).unwrap();
    fs::write(root.join("note.docx"), fixtures::minimal_docx()).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");

    let expected = [
        ("note.txt", "plain_text"),
        ("note.md", "markdown"),
        ("note.json", "json"),
        ("note.pdf", "pdf"),
        ("note.docx", "docx"),
    ];

    for (name, content_type) in expected {
        let path = root.join(name);
        let outcome = understanding
            .understand_path(&path)
            .unwrap()
            .expect("inventoried");
        let content = match outcome {
            UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => content,
            other => panic!("{name}: unexpected {other:?}"),
        };
        assert_eq!(content.content_type, content_type);
        assert_eq!(content.enrichment.version, ENRICHMENT_VERSION);
        assert_eq!(
            content.enrichment.character_count,
            content.plain_text.chars().count() as u64
        );
        assert!(!content.enrichment.sections.is_empty());
        assert!(content.enrichment.word_count > 0 || content_type == "json");

        // Determinism: re-extract matches stored enrichment.
        let again = jaymi_understanding::ContentEnrichment::extract(
            &content.plain_text,
            &content.content_type,
            content.title.as_deref(),
        );
        assert_eq!(again, content.enrichment, "deterministic mismatch for {name}");
    }

    let md = understanding
        .understand_path(&root.join("note.md"))
        .unwrap()
        .unwrap();
    let md = match md {
        UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => content,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(md.enrichment.headings.len(), 2);
    assert_eq!(md.enrichment.internal_links, vec!["./x.md".to_string()]);
    assert_eq!(
        md.enrichment.external_links,
        vec!["https://example.org/docs".to_string()]
    );
    assert_eq!(md.language.as_deref(), Some("en"));

    let json = understanding
        .content_store()
        .get_by_source_id(
            root.join("note.json")
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .as_ref(),
        )
        .unwrap()
        .expect("json content");
    assert!(json
        .enrichment
        .headings
        .iter()
        .any(|heading| heading.text == "title"));

    let store = app
        .container()
        .resolve::<Arc<SqliteContentStore>>()
        .unwrap();
    assert_eq!(store.document_count().unwrap(), 5);
    assert_eq!(store.enriched_count().unwrap(), 5);

    let diag = app.diagnostics().unwrap();
    let row = diag.subsystem("Understanding").unwrap();
    assert!(row.detail.contains("enriched_documents=5"));
    assert!(row.detail.contains("parsed_documents=5"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-enrich-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
