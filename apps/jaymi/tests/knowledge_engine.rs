//! Integration test for Layer 1 knowledge engine.
//!
//! Verifies:
//! Index roots → SQLite metadata index → Planner “what exists?” → indexed results

use std::fs::{self, File};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;

#[test]
fn what_exists_answers_from_knowledge_index() {
    let root = temp_dir("knowledge");
    write!(File::create(root.join("budget.csv")).unwrap(), "a,b").unwrap();
    fs::create_dir(root.join("receipts")).unwrap();
    write!(
        File::create(root.join("receipts").join("coffee.txt")).unwrap(),
        "latte"
    )
    .unwrap();

    // Isolate this test's SQLite file and index only the temp root.
    std::env::set_var(
        "JAYMI_DATA_DIR",
        root.join("jaymi-data").display().to_string(),
    );
    std::env::set_current_dir(&root).unwrap();

    let app = Application::boot().expect("boot");
    let indexed = app
        .send_message("index my files")
        .expect("index through planner");
    assert_eq!(
        indexed.capability.map(|capability| capability.id()),
        Some("search")
    );
    assert_eq!(indexed.tool_id.as_deref(), Some("index_files"));
    assert!(indexed.summary.to_ascii_lowercase().contains("indexed"));

    let response = app
        .send_message("What exists?")
        .expect("query through planner");
    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("search")
    );
    assert_eq!(response.tool_id.as_deref(), Some("search_index"));
    assert!(
        response
            .entries
            .iter()
            .any(|entry| entry.name == "budget.csv"),
        "expected budget.csv in {:?}",
        response
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(response.assistant_text().contains("budget.csv"));
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-knowledge-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
