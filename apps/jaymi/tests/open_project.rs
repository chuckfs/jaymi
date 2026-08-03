//! Open Project — create-or-reuse by folder, then load Coding Explorer.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{CapabilityState, ExplorerStatus, WorkspaceKind};

#[test]
fn open_project_from_path_creates_opens_and_loads_explorer() {
    let data_dir = temp_dir("open-project-data");
    let root = temp_dir("open-project-root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn ok() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    assert!(app.active_project_id().is_none());

    let context = app
        .open_project_from_path(&root)
        .expect("open project from path");
    assert_eq!(
        context.project.root_directory.as_ref().map(|p| p.as_path()),
        Some(root.canonicalize().unwrap().as_path())
    );
    assert_eq!(app.active_project_id().as_deref(), Some(context.project.id.as_str()));

    app.start_coding_project().expect("coding");
    let coding = match app.capability_state().expect("state").expect("coding") {
        CapabilityState::Coding(coding) => coding,
        other => panic!("expected coding state, got {other:?}"),
    };
    assert_eq!(coding.explorer_status, ExplorerStatus::Ready);
    assert!(
        coding
            .project_root
            .as_ref()
            .is_some_and(|p| PathBuf::from(p).canonicalize().ok().as_deref()
                == Some(root.canonicalize().unwrap().as_path())),
        "explorer root should match opened folder"
    );
    assert!(
        !coding.explorer_nodes.is_empty(),
        "explorer should list project files"
    );
    let experience = app.experience().expect("experience");
    assert_eq!(experience.active_workspace_kind(), Some(WorkspaceKind::Coding));
}

#[test]
fn open_project_from_path_reuses_existing_project() {
    let data_dir = temp_dir("open-project-reuse-data");
    let root = temp_dir("open-project-reuse-root");
    fs::create_dir_all(&root).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let first = app
        .open_project_from_path(&root)
        .expect("first open")
        .project
        .id;
    let second = app
        .open_project_from_path(&root)
        .expect("second open")
        .project
        .id;
    assert_eq!(first, second, "same folder must not create a duplicate project");
    assert_eq!(app.list_projects().expect("list").len(), 1);
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("jaymi-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}
