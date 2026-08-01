//! Integration test for the universal content-read pipeline.
//!
//! Verifies:
//! User request → Planner → ReadContent → Content Tool →
//! Provider → Content Registry → Content Parser → Unified Content

use std::fs::{self, File};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::{ContentSource, ContentType};
use jaymi_tools::READ_CONTENT_TOOL_ID;

#[test]
fn read_request_flows_through_every_layer() {
    let dir = temp_dir("read-pipeline");
    let path = dir.join("guide.md");
    let mut file = File::create(&path).unwrap();
    write!(file, "# Guide\n\nWelcome to Jaymi.\n").unwrap();

    let app = Application::boot().expect("boot");
    let response = app.read_file(&path).expect("read");

    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("read_content")
    );
    assert_eq!(response.tool_id.as_deref(), Some(READ_CONTENT_TOOL_ID));
    assert_eq!(response.provider_id.as_deref(), Some("filesystem"));

    let content = response.content.as_ref().expect("unified content");
    assert_eq!(content.source, ContentSource::File);
    assert_eq!(content.content_type, ContentType::Markdown);
    assert_eq!(content.mime_type, "text/markdown");
    assert_eq!(content.title.as_deref(), Some("Guide"));
    assert_eq!(content.parser_id, "markdown");
    assert!(content.text.contains("Welcome to Jaymi."));
    assert!(content.character_count() > 0);
    assert_eq!(content.path.as_deref(), Some(path.as_path()));

    let snapshot = app
        .diagnostics_from_response(Some(response))
        .expect("diagnostics");
    assert!(snapshot.read_success);
    assert_eq!(snapshot.read_source.as_deref(), Some("File"));
    assert_eq!(snapshot.read_file_type.as_deref(), Some("Markdown"));
    assert_eq!(snapshot.read_mime_type.as_deref(), Some("text/markdown"));
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

    let app = Application::boot().expect("boot");

    let txt_content = app.read_file(&txt).unwrap().content.unwrap();
    assert_eq!(txt_content.source, ContentSource::File);
    assert_eq!(txt_content.content_type, ContentType::PlainText);
    assert_eq!(txt_content.parser_id, "plain_text");

    let json_content = app.read_file(&json).unwrap().content.unwrap();
    assert_eq!(json_content.content_type, ContentType::Json);
    assert_eq!(json_content.parser_id, "json");
    assert_eq!(json_content.title.as_deref(), Some("Demo"));
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
