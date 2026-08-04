//! VS Code-style split editors — split, independent per-pane view state,
//! drag-and-drop tab moves, closing a pane, and persisting the split layout
//! tree in `.jaymi/workspace.json`.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::editor_workspace::load_editor_workspace;
use jaymi::Application;
use jaymi_capabilities::{EditorLayoutNode, EditorPaneId, SplitDirection};
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn split_vertical_creates_second_pane_with_independent_cursor() {
    let data_dir = temp_dir("split-view-data");
    let root = temp_dir("split-view-root");
    let path = root.join("main.rs");
    fs::write(&path, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:split-view".into()),
            name: "Split View".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let path_str = path.to_string_lossy().into_owned();
    app.open_coding_file(&path_str).expect("open file");

    let left_pane = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .editors
        .focused_pane
        .as_str()
        .to_string();

    let right_pane = app
        .split_coding_editor(SplitDirection::Vertical)
        .expect("split vertical");
    assert_ne!(left_pane, right_pane, "split must allocate a new pane id");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.panes.len(), 2);
    assert_eq!(coding.editors.focused_pane.as_str(), right_pane.as_str());
    assert!(matches!(
        coding.editors.layout,
        EditorLayoutNode::Split {
            direction: SplitDirection::Vertical,
            ..
        }
    ));
    // Split clones the active tab into the new pane (VS Code behavior).
    assert!(coding
        .editors
        .sessions_in_pane(&EditorPaneId(right_pane.clone()))
        .iter()
        .any(|session| session.path == path_str));

    app.set_coding_tab_cursor_in_pane(&left_pane, &path_str, 0, 2)
        .expect("cursor left");
    app.set_coding_tab_cursor_in_pane(&right_pane, &path_str, 1, 5)
        .expect("cursor right");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let left_cursor = coding
        .editors
        .active_session_in_pane(&EditorPaneId(left_pane.clone()))
        .expect("left session")
        .view
        .cursor;
    let right_cursor = coding
        .editors
        .active_session_in_pane(&EditorPaneId(right_pane.clone()))
        .expect("right session")
        .view
        .cursor;
    assert_eq!((left_cursor.line, left_cursor.column), (0, 2));
    assert_eq!((right_cursor.line, right_cursor.column), (1, 5));

    // Shared buffer content is still visible from both panes.
    app.set_coding_tab_content_in_pane(
        &left_pane,
        &path_str,
        "fn main() { /* edited */ }\n".into(),
    )
    .expect("edit from left pane");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(
        coding
            .editors
            .active_session_in_pane(&EditorPaneId(right_pane))
            .unwrap()
            .content,
        "fn main() { /* edited */ }\n"
    );
}

#[test]
fn move_tab_between_panes_then_close_pane() {
    let data_dir = temp_dir("split-move-data");
    let root = temp_dir("split-move-root");
    let a = root.join("a.rs");
    let b = root.join("b.rs");
    fs::write(&a, "fn a() {}\n").unwrap();
    fs::write(&b, "fn b() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:split-move".into()),
            name: "Split Move".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let a_path = a.to_string_lossy().into_owned();
    let b_path = b.to_string_lossy().into_owned();
    app.open_coding_file(&a_path).expect("open a");
    app.open_coding_file(&b_path).expect("open b");

    let left_pane = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .editors
        .focused_pane
        .as_str()
        .to_string();
    let right_pane = app
        .split_coding_editor(SplitDirection::Horizontal)
        .expect("split horizontal");

    // Drag `a.rs` from the left pane's strip onto the right pane's strip.
    app.move_coding_editor_tab(&left_pane, &a_path, &right_pane, None)
        .expect("move tab");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert!(!coding
        .editors
        .sessions_in_pane(&EditorPaneId(left_pane.clone()))
        .iter()
        .any(|session| session.path == a_path));
    let right_paths: Vec<String> = coding
        .editors
        .sessions_in_pane(&EditorPaneId(right_pane.clone()))
        .into_iter()
        .map(|session| session.path)
        .collect();
    assert!(right_paths.contains(&a_path));
    assert!(right_paths.contains(&b_path));

    // Closing the right pane drops tabs unique to it (VS Code "Close Split");
    // `b.rs` was cloned into the right pane at split time but is still open in
    // the surviving left pane, so its shared buffer must stay alive.
    app.close_coding_editor_pane(&right_pane)
        .expect("close pane");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.editors.panes.len(), 1);
    assert!(matches!(
        coding.editors.layout,
        EditorLayoutNode::Leaf { .. }
    ));
    assert!(coding.editors.session_by_path(&a_path).is_none());
    assert!(coding.editors.session_by_path(&b_path).is_some());

    // Closing the last remaining pane must be rejected (it is the sole pane).
    let sole_pane = coding.editors.focused_pane.as_str().to_string();
    assert!(app.close_coding_editor_pane(&sole_pane).is_err());
}

#[test]
fn split_layout_persists_in_workspace_json_and_restores() {
    let data_dir = temp_dir("split-persist-data");
    let root = temp_dir("split-persist-root");
    let a = root.join("a.rs");
    let b = root.join("b.rs");
    fs::write(&a, "fn a() {}\n").unwrap();
    fs::write(&b, "fn b() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:split-persist".into()),
            name: "Split Persist".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let a_path = a.to_string_lossy().into_owned();
    let b_path = b.to_string_lossy().into_owned();
    app.open_coding_file(&a_path).expect("open a");
    let left_pane = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .editors
        .focused_pane
        .as_str()
        .to_string();
    let right_pane = app
        .split_coding_editor(SplitDirection::Vertical)
        .expect("split");
    app.open_coding_file(&b_path).expect("open b in right pane");
    app.set_coding_tab_cursor_in_pane(&right_pane, &b_path, 0, 3)
        .expect("cursor b");

    app.persist_coding_editor_workspace().expect("persist");

    let snapshot = load_editor_workspace(&root)
        .expect("load")
        .expect("snapshot present");
    assert_eq!(snapshot.panes.len(), 2);
    assert!(matches!(
        snapshot.layout,
        Some(EditorLayoutNode::Split {
            direction: SplitDirection::Vertical,
            ..
        })
    ));
    assert_eq!(
        snapshot.focused_pane.as_ref().map(|id| id.as_str()),
        Some(right_pane.as_str())
    );

    app.close_ui_workspace().expect("close").expect("was open");
    app.start_coding_project().expect("reopen coding");

    let coding = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("coding kind")
        .clone();
    assert_eq!(
        coding.editors.panes.len(),
        2,
        "split layout must survive restore"
    );
    assert!(matches!(
        coding.editors.layout,
        EditorLayoutNode::Split {
            direction: SplitDirection::Vertical,
            ..
        }
    ));
    assert_eq!(
        coding.editors.focused_pane.as_str(),
        right_pane.as_str(),
        "the pane focused at persist time must stay focused after restore"
    );
    let restored_left = coding
        .editors
        .sessions_in_pane(&EditorPaneId(left_pane))
        .into_iter()
        .find(|session| session.path == a_path)
        .expect("a.rs restored in left pane");
    assert!(restored_left.content.contains("fn a"));
    let restored_right = coding
        .editors
        .sessions_in_pane(&EditorPaneId(right_pane))
        .into_iter()
        .find(|session| session.path == b_path)
        .expect("b.rs restored in right pane");
    assert!(restored_right.content.contains("fn b"));
    assert_eq!(restored_right.view.cursor.column, 3);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-split-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
