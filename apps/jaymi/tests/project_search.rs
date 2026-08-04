//! Project Search — Quick Open / Find in Files through the Planner.
//!
//! Every product search surface goes through `Application::project_search`,
//! which enters `Application::search` (Planner → `search_knowledge` →
//! Search Engine). There is no second index: content matches come from the
//! same normalized store used by full-text search elsewhere.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::SearchRequest;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_understanding::UnderstandingEngine;

/// Index a project root and warm normalized content so free-text/location
/// search can find bodies (mirrors the indexing step other search tests use).
fn index_and_warm(app: &Application, root: &std::path::Path, paths: &[&std::path::Path]) {
    app.index_root(root).expect("index");
    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    for path in paths {
        understanding.understand_path(path).unwrap().unwrap();
    }
}

fn open_project(app: &Application, root: PathBuf, project_id: &str) {
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some(project_id.into()),
            name: project_id.into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create project");
    app.open_project(project.id.as_str()).expect("open project");
    app.start_coding_project().expect("start coding");
}

#[test]
fn project_search_finds_content_line_and_opens_at_location() {
    let data_dir = temp_dir("project-search-data");
    let root = temp_dir("project-search-root");
    let path = root.join("main.rs");
    fs::write(
        &path,
        "fn main() {\n    let needle = find_the_needle();\n    println!(\"{needle}\");\n}\n",
    )
    .unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    open_project(&app, root.clone(), "project:search-content");
    index_and_warm(&app, &root, &[&path]);

    let path_str = path.to_string_lossy().into_owned();
    let mut request = SearchRequest::free_text("find_the_needle");
    request.folder = Some(root.clone());
    let results = app.project_search(request).expect("project search");

    let hit = results
        .iter()
        .find(|hit| hit.path == path_str)
        .unwrap_or_else(|| panic!("expected a hit for {path_str}, got {results:?}"));
    assert_eq!(hit.line, Some(1), "match is on the second (0-based) line");
    assert!(hit.preview.contains("find_the_needle"));

    app.open_search_result(&hit.path, hit.line, hit.column)
        .expect("open search result");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.active_tab_path(), Some(path_str.as_str()));
    let session = coding.editors.session_by_path(&path_str).expect("open session");
    assert_eq!(session.view.cursor.line, 1);
    assert_eq!(session.view.cursor.column, hit.column.unwrap());
}

#[test]
fn project_search_filename_only_supports_quick_open() {
    let data_dir = temp_dir("project-search-qo-data");
    let root = temp_dir("project-search-qo-root");
    let alpha = root.join("alpha_widget.rs");
    let beta = root.join("beta.rs");
    fs::write(&alpha, "pub struct AlphaWidget;\n").unwrap();
    fs::write(&beta, "pub struct Beta;\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    open_project(&app, root.clone(), "project:search-quick-open");
    app.index_root(&root).expect("index");

    let mut request = SearchRequest::filename("alpha_widget");
    request.folder = Some(root.clone());
    let results = app.project_search(request).expect("quick open search");

    assert_eq!(results.len(), 1, "expected exactly one filename match: {results:?}");
    let hit = &results[0];
    assert_eq!(hit.path, alpha.to_string_lossy());
    assert!(hit.line.is_none(), "filename-only hits carry no location");

    app.open_search_result(&hit.path, None, None)
        .expect("open quick open result");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(
        coding.active_tab_path(),
        Some(alpha.to_string_lossy().as_ref())
    );
}

#[test]
fn replace_in_search_results_updates_disk_and_open_buffers() {
    let data_dir = temp_dir("project-search-replace-data");
    let root = temp_dir("project-search-replace-root");
    let on_disk = root.join("on_disk.rs");
    let open_file = root.join("open_file.rs");
    fs::write(&on_disk, "let legacy_name = 1;\nlegacy_name += 1;\n").unwrap();
    fs::write(&open_file, "let legacy_name = 2;\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    open_project(&app, root.clone(), "project:search-replace");
    index_and_warm(&app, &root, &[&on_disk, &open_file]);

    // Open one of the two files so Replace All must go through the buffer
    // path instead of writing straight to disk.
    let open_file_str = open_file.to_string_lossy().into_owned();
    app.open_coding_file(&open_file_str).expect("open file");

    let mut request = SearchRequest::free_text("legacy_name");
    request.folder = Some(root.clone());
    let replaced = app
        .replace_in_search_results(request, "renamed_value")
        .expect("replace all");
    assert!(replaced >= 3, "expected at least 3 matches, got {replaced}");

    // Untouched buffer file: written straight to disk through the Planner.
    let on_disk_text = fs::read_to_string(&on_disk).unwrap();
    assert!(!on_disk_text.contains("legacy_name"));
    assert!(on_disk_text.contains("renamed_value"));

    // Open buffer: updated in place (dirty) without an implicit save.
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let buffer = coding
        .editors
        .buffer_by_path(&open_file_str)
        .expect("open buffer");
    assert!(buffer.content.contains("renamed_value"));
    assert!(!buffer.content.contains("legacy_name"));
    assert!(buffer.dirty, "buffer edit should be dirty until saved");

    // Disk copy of the open file is untouched until the user saves.
    let open_file_disk = fs::read_to_string(&open_file).unwrap();
    assert!(open_file_disk.contains("legacy_name"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-project-search-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    // macOS `/tmp` is a symlink to `/private/tmp`; canonicalize so string
    // comparisons against normalized search-hit paths line up.
    dir.canonicalize().unwrap_or(dir)
}
