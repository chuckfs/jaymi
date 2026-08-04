//! Coding editor workspace persistence (`.jaymi/workspace.json`).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{
    EditorSettings, FoldedRegion, DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_EXPLORER_WIDTH,
    DEFAULT_WORKSPACE_PANEL_WIDTH,
};
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_projects::structure::JaymiProjectLayout;

#[test]
fn editor_workspace_survives_close_and_reopen_without_serializing_contents() {
    let data_dir = temp_dir("editor-persist-data");
    let root = temp_dir("editor-persist-root");
    fs::create_dir_all(root.join("src")).expect("src");
    let main_path = root.join("src/main.rs");
    let lib_path = root.join("src/lib.rs");
    fs::write(&main_path, "fn main() {\n    println!(\"hi\");\n}\n").expect("main");
    fs::write(&lib_path, "pub fn helper() {}\n").expect("lib");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-persist".into()),
            name: "Editor Persist".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    let _ = app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let main_str = main_path.to_string_lossy().into_owned();
    let lib_str = lib_path.to_string_lossy().into_owned();
    app.open_coding_file(&main_str).expect("open main");
    app.open_coding_file(&lib_str).expect("open lib");
    app.set_coding_tab_cursor(&main_str, 1, 4).expect("cursor");
    app.set_coding_tab_scroll(&main_str, 24.0).expect("scroll");
    app.set_coding_tab_folds(
        &main_str,
        vec![FoldedRegion {
            start_line: 0,
            end_line: 2,
        }],
    )
    .expect("folds");
    app.set_coding_editor_settings(EditorSettings {
        minimap: false,
        word_wrap: true,
        font_size: 16,
    })
    .expect("settings");
    app.activate_coding_tab(&main_str).expect("activate main");

    app.persist_coding_editor_workspace().expect("persist");
    let workspace_path = JaymiProjectLayout::for_root(&root).workspace_json;
    let body = fs::read_to_string(&workspace_path).expect("read workspace.json");
    assert!(body.contains("main.rs"));
    assert!(!body.contains("println"), "must not serialize buffer contents");
    assert!(body.contains("\"font_size\": 16"));
    assert!(body.contains("\"word_wrap\": true"));

    app.close_ui_workspace().expect("close").expect("was open");
    assert!(app.capability_state().expect("state").is_none());

    app.start_coding_project().expect("reopen coding");
    let coding = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("coding kind")
        .clone();
    assert_eq!(coding.editors.len(), 2);
    assert_eq!(coding.active_tab_path(), Some(main_str.as_str()));
    let main = coding
        .editors
        .session_by_path(&main_str)
        .expect("main session");
    assert!(main.content.contains("println"), "content reloaded from disk");
    assert_eq!(main.view.cursor.line, 1);
    assert_eq!(main.view.cursor.column, 4);
    assert_eq!(main.view.scroll_top, 24.0);
    assert_eq!(main.view.folded_regions.len(), 1);
    assert!(!coding.editor_settings.minimap);
    assert!(coding.editor_settings.word_wrap);
    assert_eq!(coding.editor_settings.font_size, 16);
}

#[test]
fn project_switch_restores_each_projects_editor_state() {
    let data_dir = temp_dir("editor-switch-data");
    let root_a = temp_dir("editor-switch-a");
    let root_b = temp_dir("editor-switch-b");
    fs::create_dir_all(root_a.join("src")).unwrap();
    fs::create_dir_all(root_b.join("src")).unwrap();
    let a_path = root_a.join("src/a.rs");
    let b_path = root_b.join("src/b.rs");
    fs::write(&a_path, "fn a() {}\n").unwrap();
    fs::write(&b_path, "fn b() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project_a = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:switch-a".into()),
            name: "A".into(),
            description: None,
            root_directory: Some(root_a.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create a");
    let project_b = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:switch-b".into()),
            name: "B".into(),
            description: None,
            root_directory: Some(root_b.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create b");

    let _ = app.open_project(project_a.id.as_str()).expect("open a");
    app.start_coding_project().expect("coding a");
    let a_str = a_path.to_string_lossy().into_owned();
    app.open_coding_file(&a_str).expect("open a.rs");
    app.set_coding_tab_cursor(&a_str, 0, 3).expect("cursor a");
    app.persist_coding_editor_workspace().expect("persist a");

    let _ = app.open_project(project_b.id.as_str()).expect("open b");
    // Simulate UI: persist previous (already done), then start coding for new project.
    app.start_coding_project().expect("coding b");
    let b_str = b_path.to_string_lossy().into_owned();
    app.open_coding_file(&b_str).expect("open b.rs");
    app.set_coding_editor_settings(EditorSettings {
        minimap: true,
        word_wrap: false,
        font_size: 14,
    })
    .expect("settings b");
    app.persist_coding_editor_workspace().expect("persist b");

    // Back to A — should restore A's tab, not B's.
    app.persist_coding_editor_workspace().expect("persist before switch");
    let _ = app.open_project(project_a.id.as_str()).expect("reopen a");
    app.start_coding_project().expect("coding a again");
    let coding = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("coding")
        .clone();
    assert_eq!(coding.editors.len(), 1);
    assert_eq!(coding.active_tab_path(), Some(a_str.as_str()));
    assert_eq!(
        coding
            .editors
            .session_by_path(&a_str)
            .expect("a")
            .view
            .cursor
            .column,
        3
    );
    assert!(coding.editors.session_by_path(&b_str).is_none());
    assert_eq!(
        coding
            .explorer
            .project_root
            .as_deref()
            .map(|root| PathBuf::from(root)),
        Some(root_a.canonicalize().unwrap_or(root_a))
    );
}

#[test]
fn shell_chrome_sizes_survive_close_and_reopen() {
    let data_dir = temp_dir("editor-chrome-data");
    let root = temp_dir("editor-chrome-root");
    fs::write(root.join("main.rs"), "fn main() {}\n").expect("main");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:editor-chrome".into()),
            name: "Editor Chrome".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    let _ = app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    // Defaults before any resize.
    let coding = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("coding kind")
        .clone();
    assert_eq!(coding.explorer_width, DEFAULT_EXPLORER_WIDTH);
    assert_eq!(coding.bottom_panel_height, DEFAULT_BOTTOM_PANEL_HEIGHT);
    assert_eq!(coding.workspace_panel_width, DEFAULT_WORKSPACE_PANEL_WIDTH);

    app.with_coding_state(|coding| {
        coding.set_explorer_width(260.0);
        coding.set_bottom_panel_height(230.0);
        coding.set_workspace_panel_width(710.0);
    })
    .expect("resize shell chrome");
    app.persist_coding_editor_workspace().expect("persist");

    app.close_ui_workspace().expect("close").expect("was open");
    app.start_coding_project().expect("reopen coding");

    let coding = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("coding kind")
        .clone();
    assert_eq!(coding.explorer_width, 260.0);
    assert_eq!(coding.bottom_panel_height, 230.0);
    assert_eq!(coding.workspace_panel_width, 710.0);
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-{label}-{nanos}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}
