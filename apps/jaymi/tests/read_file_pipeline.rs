//! Integration test for the Slice 3 universal file reader pipeline.
//!
//! Verifies:
//! User request → Planner → ReadDocuments → Read File Tool →
//! Filesystem Provider → Parser Registry → Parser → Unified Document

use std::fs::{self, File};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::FileType;
use jaymi_tools::READ_FILE_TOOL_ID;

#[test]
fn read_request_flows_through_every_layer() {
    let dir = temp_dir("read-pipeline");
    let path = dir.join("guide.md");
    let mut file = File::create(&path).unwrap();
    write!(file, "# Guide\n\nWelcome to Jaymi.\n").unwrap();

    let app = Application::boot_with_data_dir(temp_dir("read-data")).expect("boot");
    let response = app.read_file(&path).expect("read");

    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("read_documents")
    );
    assert_eq!(response.tool_id.as_deref(), Some(READ_FILE_TOOL_ID));
    assert_eq!(response.provider_id.as_deref(), Some("filesystem"));

    let document = response.document.as_ref().expect("unified document");
    assert_eq!(document.file_type, FileType::Markdown);
    assert_eq!(document.title.as_deref(), Some("Guide"));
    assert_eq!(document.parser_id, "markdown");
    assert!(document.text.contains("Welcome to Jaymi."));
    assert!(document.character_count() > 0);

    let snapshot = app
        .diagnostics_from_response(Some(response))
        .expect("diagnostics");
    assert!(snapshot.read_success);
    assert_eq!(snapshot.read_file_type.as_deref(), Some("Markdown"));
    assert_eq!(snapshot.read_parser.as_deref(), Some("markdown"));
    assert!(snapshot.read_character_count.unwrap_or(0) > 0);
    assert!(snapshot
        .read_text
        .as_deref()
        .unwrap_or("")
        .contains("Welcome to Jaymi."));
}

#[test]
fn read_supports_txt_and_json() {
    let dir = temp_dir("formats");
    let txt = dir.join("a.txt");
    let json = dir.join("b.json");
    write!(File::create(&txt).unwrap(), "plain").unwrap();
    write!(File::create(&json).unwrap(), "{{\"name\":\"Demo\"}}").unwrap();

    let app = Application::boot_with_data_dir(temp_dir("formats-data")).expect("boot");

    let txt_doc = app.read_file(&txt).unwrap().document.unwrap();
    assert_eq!(txt_doc.file_type, FileType::PlainText);
    assert_eq!(txt_doc.parser_id, "plain_text");

    let json_doc = app.read_file(&json).unwrap().document.unwrap();
    assert_eq!(json_doc.file_type, FileType::Json);
    assert_eq!(json_doc.parser_id, "json");
    assert_eq!(json_doc.title.as_deref(), Some("Demo"));
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-read-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
