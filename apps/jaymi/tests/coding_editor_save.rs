//! Coding Editor save — dirty tracking and WriteFile through the Planner.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;
use jaymi_tools::WRITE_FILE_TOOL_ID;

#[test]
fn editing_marks_tab_dirty_and_save_clears_it() {
    let data_dir = temp_dir("edit-dirty-data");
    let root = temp_dir("edit-dirty-root");
    let path = root.join("main.rs");
    fs::write(&path, "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:edit-dirty".into()),
            name: "Edit Dirty".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let path = path.to_string_lossy().into_owned();
    app.open_coding_file(&path).expect("open");
    assert!(
        !app.capability_state()
            .unwrap()
            .unwrap()
            .coding()
            .unwrap()
            .editors
            .sessions()[0]
            .dirty
    );

    app.set_coding_tab_content(&path, "fn main() { println!(\"edited\"); }\n".into())
        .expect("edit");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert!(coding.editors.sessions()[0].dirty);
    assert!(coding.editors.sessions()[0].content.contains("edited"));

    app.save_coding_file(&path).expect("save");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert!(!coding.editors.sessions()[0].dirty);
}

#[test]
fn save_writes_file_contents_through_planner_pipeline() {
    let data_dir = temp_dir("edit-save-data");
    let root = temp_dir("edit-save-root");
    let path = root.join("notes.md");
    fs::write(&path, "# old\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:edit-save".into()),
            name: "Edit Save".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let path_str = path.to_string_lossy().into_owned();
    app.open_coding_file(&path_str).expect("open");
    app.set_coding_tab_content(&path_str, "# new content\n".into())
        .expect("edit");

    let response = app
        .write_file(&path, "# new content\n")
        .expect("write through planner");
    assert_eq!(response.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
    assert_eq!(
        response.provider_id.as_deref(),
        Some(FILESYSTEM_PROVIDER_ID)
    );
    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("file_management")
    );
    assert!(!response.blocked);
    assert_eq!(fs::read_to_string(&path).unwrap(), "# new content\n");

    app.save_coding_file(&path_str).expect("save coding");
    assert_eq!(fs::read_to_string(&path).unwrap(), "# new content\n");
    assert!(
        !app.capability_state()
            .unwrap()
            .unwrap()
            .coding()
            .unwrap()
            .editors
            .sessions()[0]
            .dirty
    );
}

#[test]
fn save_active_persists_editor_buffer_to_disk() {
    let data_dir = temp_dir("edit-active-data");
    let root = temp_dir("edit-active-root");
    let path = root.join("lib.rs");
    fs::write(&path, "pub fn a() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:edit-active".into()),
            name: "Edit Active".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let path_str = path.to_string_lossy().into_owned();
    app.open_coding_file(&path_str).expect("open");
    app.set_coding_tab_content(&path_str, "pub fn a() { /* saved */ }\n".into())
        .expect("edit");
    app.save_active_coding_file().expect("save active");

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "pub fn a() { /* saved */ }\n"
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-edit-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
