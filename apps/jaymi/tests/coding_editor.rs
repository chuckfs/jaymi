//! Coding Editor panel — open / focus / close through Planner → read_file.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{ExplorerStatus, WorkspaceKind};
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_tools::{LIST_PROJECT_TREE_TOOL_ID, READ_FILE_TOOL_ID};

#[test]
fn selecting_file_selects_only_open_opens_editor_tab_through_planner() {
    let data_dir = temp_dir("editor-open-data");
    let root = temp_dir("editor-open-root");
    fs::create_dir_all(root.join("src")).unwrap();
    let main_rs = root.join("src").join("main.rs");
    fs::write(&main_rs, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    fs::write(root.join(".hidden"), "secret").unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-open".into()),
            name: "Editor Open".into(),
            project_type: Some(ProjectType::Code),
            root_directory: Some(root.clone()),
            description: None,
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open project");
    app.start_coding_project().expect("start coding");

    let coding = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("borrow")
        .clone();
    assert_eq!(coding.explorer.status, ExplorerStatus::Ready);
    assert!(!coding.explorer.nodes.is_empty());
    assert!(coding
        .explorer
        .nodes
        .iter()
        .any(|node| node.name == "src" && node.is_dir));
    assert!(
        coding
            .explorer
            .nodes
            .iter()
            .filter(|node| node.is_dir)
            .all(|node| coding.explorer.expanded_paths.contains(&node.path)),
        "top-level folders should start expanded"
    );
    assert!(coding
        .explorer
        .nodes
        .iter()
        .any(|node| node.name == "Cargo.toml" && !node.is_dir));
    assert!(!coding
        .explorer
        .nodes
        .iter()
        .any(|node| node.name.starts_with('.')));

    let main_path = main_rs.to_string_lossy().into_owned();
    app.select_coding_path(&main_path, false)
        .expect("select only");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.explorer.selected_path.as_deref(), Some(main_path.as_str()));
    assert!(
        coding.editors.is_empty(),
        "select_coding_path must not open files (UI Select opens preview)"
    );

    app.open_coding_file(&main_path).expect("double-click open");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 1);
    assert_eq!(coding.active_tab_path(), Some(main_path.as_str()));
    assert_eq!(coding.explorer.selected_path.as_deref(), Some(main_path.as_str()));
    assert!(coding.editors.sessions()[0].content.contains("println"));
    assert!(!coding.editors.sessions()[0].dirty);
    assert!(!coding.editors.sessions()[0].preview);
}

#[test]
fn preview_open_marks_preview_and_permanent_promotes() {
    let data_dir = temp_dir("editor-preview-data");
    let root = temp_dir("editor-preview-root");
    let a = root.join("a.rs");
    let b = root.join("b.rs");
    fs::write(&a, "fn a() {}").unwrap();
    fs::write(&b, "fn b() {}").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-preview".into()),
            name: "Editor Preview".into(),
            project_type: Some(ProjectType::Code),
            root_directory: Some(root),
            description: None,
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let a_path = a.to_string_lossy().into_owned();
    let b_path = b.to_string_lossy().into_owned();
    app.open_coding_file_preview(&a_path).expect("preview a");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 1);
    assert!(coding.editors.sessions()[0].preview);

    app.open_coding_file_preview(&b_path).expect("preview b replaces a");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 1, "preview replaces previous preview");
    assert_eq!(coding.active_tab_path(), Some(b_path.as_str()));
    assert!(coding.editors.sessions()[0].preview);

    app.open_coding_file(&b_path).expect("permanent open");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 1);
    assert!(!coding.editors.sessions()[0].preview);
}

#[test]
fn reopening_file_focuses_existing_tab_without_duplicate() {
    let data_dir = temp_dir("editor-refocus-data");
    let root = temp_dir("editor-refocus-root");
    let a = root.join("a.rs");
    let b = root.join("b.rs");
    fs::write(&a, "fn a() {}").unwrap();
    fs::write(&b, "fn b() {}").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-refocus".into()),
            name: "Editor Refocus".into(),
            project_type: Some(ProjectType::Code),
            root_directory: Some(root),
            description: None,
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let a_path = a.to_string_lossy().into_owned();
    let b_path = b.to_string_lossy().into_owned();
    app.open_coding_file(&a_path).expect("open a");
    app.open_coding_file(&b_path).expect("open b");
    assert_eq!(
        app.capability_state()
            .unwrap()
            .unwrap()
            .coding()
            .unwrap()
            .editors
            .len(),
        2
    );
    assert_eq!(
        app.capability_state()
            .unwrap()
            .unwrap()
            .coding()
            .unwrap()
            .active_tab_path(),
        Some(b_path.as_str())
    );

    app.open_coding_file(&a_path).expect("reopen a");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 2, "reopen must not duplicate tabs");
    assert_eq!(coding.active_tab_path(), Some(a_path.as_str()));
    assert_eq!(coding.explorer.selected_path.as_deref(), Some(a_path.as_str()));
}

#[test]
fn closing_tab_updates_active_and_preserves_neighbor() {
    let data_dir = temp_dir("editor-close-data");
    let root = temp_dir("editor-close-root");
    let a = root.join("notes.md");
    let b = root.join("data.json");
    fs::write(&a, "# Notes").unwrap();
    fs::write(&b, "{\"ok\":true}").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-close".into()),
            name: "Editor Close".into(),
            project_type: Some(ProjectType::Code),
            root_directory: Some(root),
            description: None,
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let a_path = a.to_string_lossy().into_owned();
    let b_path = b.to_string_lossy().into_owned();
    app.open_coding_file(&a_path).expect("open a");
    app.set_coding_tab_scroll(&a_path, 40.0).expect("scroll a");
    app.open_coding_file(&b_path).expect("open b");

    app.close_coding_tab(&b_path).expect("close b");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 1);
    assert_eq!(coding.active_tab_path(), Some(a_path.as_str()));
    assert_eq!(
        coding
            .editors
            .session_by_path(&a_path)
            .map(|session| session.view.scroll_top),
        Some(40.0),
        "closing another tab must preserve scroll on the remaining session"
    );
}

#[test]
fn open_and_tree_exercise_planner_tool_provider_path() {
    let data_dir = temp_dir("editor-planner-data");
    let root = temp_dir("editor-planner-root");
    let file = root.join("readme.txt");
    fs::write(&file, "hello editor").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-planner".into()),
            name: "Editor Planner".into(),
            project_type: Some(ProjectType::Code),
            root_directory: Some(root.clone()),
            description: None,
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");

    let tree = app.list_project_tree(&root).expect("tree");
    assert_eq!(tree.tool_id.as_deref(), Some(LIST_PROJECT_TREE_TOOL_ID));
    assert_eq!(tree.provider_id.as_deref(), Some("filesystem"));
    assert!(tree.entries.iter().any(|entry| entry.name == "readme.txt"));

    app.start_coding_project().expect("coding");
    let path = file.to_string_lossy().into_owned();
    let read = app.read_file(&path).expect("read through planner");
    assert_eq!(read.tool_id.as_deref(), Some(READ_FILE_TOOL_ID));
    assert!(read.document.as_ref().unwrap().text.contains("hello editor"));

    app.open_coding_file(&path).expect("open via coding API");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.len(), 1);
    assert!(coding.editors.sessions()[0].content.contains("hello editor"));
    assert_eq!(
        app.experience()
            .unwrap()
            .active_workspace_kind(),
        Some(WorkspaceKind::Coding)
    );
}

#[test]
fn explorer_expansion_and_selection_persist_while_open() {
    let data_dir = temp_dir("explorer-persist-data");
    let root = temp_dir("explorer-persist-root");
    fs::create_dir_all(root.join("src")).unwrap();
    let lib = root.join("src").join("lib.rs");
    fs::write(&lib, "pub fn x() {}").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:explorer-persist".into()),
            name: "Explorer Persist".into(),
            project_type: Some(ProjectType::Code),
            root_directory: Some(root.clone()),
            description: None,
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let src = root.join("src").to_string_lossy().into_owned();
    let lib_path = lib.to_string_lossy().into_owned();
    app.toggle_coding_expand(&src).expect("expand");
    app.select_coding_path(&lib_path, false).expect("select");
    app.open_coding_file(&lib_path).expect("open");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert!(coding.explorer.expanded_paths.contains(&src));
    assert_eq!(coding.explorer.selected_path.as_deref(), Some(lib_path.as_str()));
    assert_eq!(coding.active_tab_path(), Some(lib_path.as_str()));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-editor-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
