//! Integration tests for Layer 5 Slice 1 — Project Manager.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_project_engine::{
    CreateProjectRequest, ProjectEngineApi, ProjectStatus, ProjectType,
};

#[test]
fn project_manager_creates_loads_and_deletes_persistently() {
    let data_dir = temp_dir("project-manager-data");
    let root = temp_dir("project-manager-root");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let created = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:jaymi".into()),
            name: "Jaymi".into(),
            description: Some("Personal AI environment".into()),
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");

    assert_eq!(created.id.as_str(), "project:jaymi");
    assert_eq!(created.name, "Jaymi");
    assert_eq!(created.description, "Personal AI environment");
    assert_eq!(created.project_type, ProjectType::Code);
    assert_eq!(created.root_directory.as_deref(), Some(root.as_path()));
    assert!(created.last_opened_at.is_none());
    assert!(root.join(".jaymi").join("project.json").is_file());

    let listed = app.list_projects().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    let opened = app.open_project(created.id.as_str()).expect("open");
    assert!(opened.is_open);
    assert!(opened.project.last_opened_at.is_some());

    let context = app
        .project_context(None)
        .expect("context")
        .expect("open context");
    assert!(context.is_open);
    assert_eq!(context.project.id, created.id);

    let closed = app.close_project().expect("close").expect("closed project");
    assert_eq!(closed.id, created.id);
    assert!(app.project_context(None).expect("after close").is_none());

    // Persistence across restart.
    drop(app);
    let app = Application::boot_with_data_dir(&data_dir).expect("reboot");
    let loaded = app
        .list_projects()
        .expect("list after reboot")
        .into_iter()
        .find(|project| project.id.as_str() == "project:jaymi")
        .expect("persisted project");
    assert_eq!(loaded.name, "Jaymi");
    assert_eq!(loaded.description, "Personal AI environment");
    assert!(loaded.last_opened_at.is_some());

    app.delete_project(loaded.id.as_str()).expect("delete");
    assert!(app.list_projects().expect("list after delete").is_empty());

    let deleted = app
        .container()
        .resolve::<std::sync::Arc<jaymi_project_engine::ProjectEngine>>()
        .expect("project engine")
        .get(loaded.id.as_str())
        .expect("get deleted")
        .expect("soft-deleted row");
    assert_eq!(deleted.status, ProjectStatus::Deleted);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-project-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
