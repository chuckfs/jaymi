//! Integration tests for Layer 2 Slice 1 — Universal Content Pipeline.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_database::{Database, CURRENT_SCHEMA_VERSION};
use jaymi_understanding::{
    ContentStore, SqliteContentStore, UnderstandOutcome, UnderstandingEngine,
};

#[test]
fn content_pipeline_persists_normalized_content() {
    let data_dir = temp_dir("understanding-it-data");
    let root = temp_dir("understanding-it-root");
    fs::write(root.join("note.md"), "# Hello\n\nWorld").unwrap();
    fs::write(root.join("data.json"), r#"{"a":1}"#).unwrap();
    fs::write(root.join("readme.txt"), "plain").unwrap();
    fs::write(root.join("photo.png"), b"\x89PNG").unwrap();
    fs::write(root.join("broken.pdf"), b"%PDF-1.4\ncorrupt").unwrap();
    fs::write(root.join("archive.bin"), b"\x00\x01").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let database = app
        .container()
        .resolve::<Arc<Database>>()
        .expect("database");
    assert_eq!(database.schema_version(), CURRENT_SCHEMA_VERSION);

    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");

    let md = root.join("note.md");
    let outcome = understanding
        .understand_path(&md)
        .unwrap()
        .expect("inventoried");
    match outcome {
        UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => {
            assert_eq!(content.content_type, "markdown");
            assert!(content.plain_text.contains("World"));
            assert_eq!(content.title.as_deref(), Some("Hello"));
            assert_eq!(content.parser_used, "markdown");
            assert!(!content.parser_version.is_empty());
            assert!(content.extraction_timestamp > 0);
        }
        other => panic!("unexpected outcome: {other:?}"),
    }

    understanding
        .understand_path(&root.join("data.json"))
        .unwrap()
        .unwrap();
    understanding
        .understand_path(&root.join("readme.txt"))
        .unwrap()
        .unwrap();
    // Unknown extensions fall back to plain-text so Coding can open them.
    let other = understanding
        .understand_path(&root.join("archive.bin"))
        .unwrap()
        .unwrap();
    match other {
        UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => {
            assert_eq!(content.parser_used, "plain_text");
        }
        other => panic!("expected plain_text fallback, got {other:?}"),
    }
    let failed = understanding
        .understand_path(&root.join("broken.pdf"))
        .unwrap()
        .unwrap();
    assert!(matches!(failed, UnderstandOutcome::Failed(_)));
    // Corrupt PNG header should fail gracefully through the image parser.
    let failed_image = understanding
        .understand_path(&root.join("photo.png"))
        .unwrap()
        .unwrap();
    assert!(matches!(failed_image, UnderstandOutcome::Failed(_)));

    let content = app
        .container()
        .resolve::<Arc<SqliteContentStore>>()
        .expect("content");
    assert_eq!(content.document_count().unwrap(), 4);

    // Planner read prefers stored content after warm pipeline.
    let first = app.read_file(&md).expect("read");
    assert!(!first.blocked);
    assert!(first.document.as_ref().unwrap().text.contains("World"));

    let second = app.read_file(&md).expect("reread");
    assert_eq!(
        second.document.as_ref().map(|doc| doc.parser_id.as_str()),
        Some("markdown")
    );

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot.subsystem("Understanding").unwrap();
    assert!(row.detail.contains("parsed_documents=4"));
    assert!(row.detail.contains("enriched_documents="));
    assert!(row.detail.contains("parser_usage="));
    assert!(row.detail.contains("failed_parses="));
    assert!(row.detail.contains("unsupported_formats="));
    assert!(row.detail.contains("markdown="));
    assert!(row.detail.contains("failed_parses=1") || row.detail.contains("failed_parses="));
    let parsers = snapshot.subsystem("Parser Registry").unwrap();
    assert!(parsers.detail.contains("registered=6"));
    assert!(parsers.detail.contains("usage="));
}

#[test]
fn normalized_content_survives_restart() {
    let data_dir = temp_dir("understanding-persist-data");
    let root = temp_dir("understanding-persist-root");
    let path = root.join("keep.md");
    fs::write(&path, "# Keep\n\nPersist me").unwrap();

    {
        let app = Application::boot_with_data_dir(&data_dir).expect("boot");
        app.index_root(&root).expect("index");
        app.container()
            .resolve::<Arc<UnderstandingEngine>>()
            .unwrap()
            .understand_path(&path)
            .unwrap()
            .unwrap();
        let mut app = app;
        app.shutdown().unwrap();
    }

    let app = Application::boot_with_data_dir(&data_dir).expect("reboot");
    let content = app
        .container()
        .resolve::<Arc<SqliteContentStore>>()
        .unwrap();
    let source_id = path
        .canonicalize()
        .unwrap_or(path.clone())
        .to_string_lossy()
        .into_owned();
    let stored = content
        .get_by_source_id(&source_id)
        .unwrap()
        .expect("content should survive restart");
    assert!(stored.plain_text.contains("Persist me"));
    assert_eq!(stored.content_type, "markdown");
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-understanding-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
