//! Integration test for the Milestone 2 request pipeline.
//!
//! Verifies:
//! User request → Planner → Search capability → Search Files Tool →
//! Filesystem Provider → Filesystem → structured results.

use std::fs::{self, File};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::EntryType;
use jaymi_planner::PlannerResponse;
use jaymi_providers::FILESYSTEM_PROVIDER_ID;
use jaymi_tools::SEARCH_FILES_TOOL_ID;

#[test]
fn list_directory_request_flows_through_every_layer() {
    let dir = temp_dir("pipeline");
    write_file(&dir.join("alpha.txt"), "a");
    write_file(&dir.join("beta.txt"), "bb");
    fs::create_dir(dir.join("nested")).unwrap();
    write_file(&dir.join("nested").join("hidden.txt"), "nope");

    let app = Application::boot_with_data_dir(temp_dir("list-data")).expect("boot");
    let response: PlannerResponse = app.list_directory(&dir).expect("list");

    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("search")
    );
    assert_eq!(response.tool_id.as_deref(), Some(SEARCH_FILES_TOOL_ID));
    assert_eq!(response.provider_id.as_deref(), Some(FILESYSTEM_PROVIDER_ID));

    // Non-recursive: nested/hidden.txt must not appear.
    assert_eq!(response.entries.len(), 3);
    assert!(response.entries.iter().all(|entry| entry.name != "hidden.txt"));

    let alpha = response
        .entries
        .iter()
        .find(|entry| entry.name == "alpha.txt")
        .expect("alpha");
    assert_eq!(alpha.entry_type, EntryType::File);
    assert_eq!(alpha.size, 1);
    assert!(alpha.path.ends_with("alpha.txt"));
    assert!(alpha.modified.is_some());

    let nested = response
        .entries
        .iter()
        .find(|entry| entry.name == "nested")
        .expect("nested");
    assert_eq!(nested.entry_type, EntryType::Directory);

    let snapshot = app
        .diagnostics_with_listing(Some(response))
        .expect("diagnostics");
    assert_eq!(snapshot.provider_count, 3);
    assert_eq!(snapshot.tool_count, 5);
    assert_eq!(snapshot.capability_count, 5);
    assert_eq!(snapshot.entries.len(), 3);
    assert!(snapshot.listing_summary.is_some());
}

#[test]
fn list_directory_rejects_files() {
    let dir = temp_dir("not-dir");
    let file = dir.join("file.txt");
    write_file(&file, "x");

    let app = Application::boot_with_data_dir(temp_dir("list-reject-data")).expect("boot");
    let error = app.list_directory(&file).expect_err("file should fail");
    assert!(
        error.message().contains("not a directory")
            || error.message().contains("failed")
            || error.message().contains("cannot access"),
        "unexpected error: {}",
        error.message()
    );
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_file(path: &std::path::Path, contents: &str) {
    let mut file = File::create(path).unwrap();
    write!(file, "{contents}").unwrap();
}
