//! Integration tests for Layer 2 Slice 2 — Expanded Document Parsing.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_parsers::fixtures;
use jaymi_understanding::{
    ContentStore, SqliteContentStore, UnderstandOutcome, UnderstandingEngine,
};

#[test]
fn expanded_parsers_normalize_all_supported_formats() {
    let data_dir = temp_dir("slice2-data");
    let root = temp_dir("slice2-root");

    fs::write(root.join("note.txt"), fixtures::plain_text()).unwrap();
    fs::write(root.join("note.md"), fixtures::markdown()).unwrap();
    fs::write(root.join("note.json"), fixtures::json()).unwrap();
    fs::write(root.join("note.pdf"), fixtures::minimal_pdf()).unwrap();
    fs::write(root.join("note.docx"), fixtures::minimal_docx()).unwrap();
    fs::write(root.join("photo.png"), b"\x89PNG").unwrap();
    fs::write(root.join("broken.pdf"), fixtures::corrupt_pdf()).unwrap();
    fs::write(root.join("archive.zzzz"), b"\x00\x01").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let snapshot = app.diagnostics().expect("diagnostics");
    let parsers = snapshot.subsystem("Parser Registry").unwrap();
    assert!(parsers.detail.contains("registered=6"));
    assert!(parsers.detail.contains("pdf"));
    assert!(parsers.detail.contains("docx"));
    assert!(parsers.detail.contains("image"));
    assert!(parsers.detail.contains("usage="));
    assert_eq!(snapshot.parser_count, 6);
    assert_eq!(
        snapshot.parser_ids,
        vec![
            "docx".to_string(),
            "image".to_string(),
            "json".to_string(),
            "markdown".to_string(),
            "pdf".to_string(),
            "plain_text".to_string()
        ]
    );

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");

    let cases = [
        ("note.txt", "plain_text", "Hello plain text"),
        ("note.md", "markdown", "Body paragraph"),
        ("note.json", "json", "Fixture JSON"),
        ("note.pdf", "pdf", "Hello PDF"),
        ("note.docx", "docx", "Hello DOCX"),
    ];

    for (name, content_type, needle) in cases {
        let path = root.join(name);
        let outcome = understanding
            .understand_path(&path)
            .unwrap()
            .expect("inventoried");
        match outcome {
            UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => {
                assert_eq!(content.content_type, content_type, "{name}");
                assert!(
                    content.plain_text.contains(needle),
                    "{name} missing {needle:?} in {}",
                    content.plain_text
                );
                assert_eq!(content.parser_used, content_type, "{name}");
            }
            other => panic!("{name}: unexpected {other:?}"),
        }
    }

    // Unknown extensions fall back to the plain-text parser so Coding can open them.
    let other = understanding
        .understand_path(&root.join("archive.zzzz"))
        .unwrap()
        .unwrap();
    match other {
        UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => {
            assert_eq!(content.content_type, "plain_text");
            assert_eq!(content.parser_used, "plain_text");
        }
        other => panic!("expected plain_text fallback, got {other:?}"),
    }

    let failed = understanding
        .understand_path(&root.join("broken.pdf"))
        .unwrap()
        .unwrap();
    assert!(matches!(failed, UnderstandOutcome::Failed(_)));

    let failed_image = understanding
        .understand_path(&root.join("photo.png"))
        .unwrap()
        .unwrap();
    assert!(matches!(failed_image, UnderstandOutcome::Failed(_)));

    let content = app
        .container()
        .resolve::<Arc<SqliteContentStore>>()
        .expect("content");
    assert_eq!(content.document_count().unwrap(), 6);

    let stats = understanding.stats().unwrap();
    assert_eq!(stats.parsed_documents, 6);
    assert!(stats.failed_parses >= 1);
    assert!(stats
        .parser_usage
        .iter()
        .any(|(p, c)| p == "pdf" && *c == 1));
    assert!(stats
        .parser_usage
        .iter()
        .any(|(p, c)| p == "docx" && *c == 1));

    let diag = app.diagnostics().expect("diagnostics after parse");
    let understanding_row = diag.subsystem("Understanding").unwrap();
    assert!(understanding_row.detail.contains("parsed_documents=6"));
    assert!(understanding_row.detail.contains("pdf=1"));
    assert!(understanding_row.detail.contains("docx=1"));
    let parser_row = diag.subsystem("Parser Registry").unwrap();
    assert!(parser_row.detail.contains("usage="));
    assert!(parser_row.detail.contains("pdf=1"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-slice2-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
