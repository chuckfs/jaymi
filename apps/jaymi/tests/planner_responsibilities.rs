//! Architectural Integrity Slice 3 — Planner Responsibilities.
//!
//! The Planner orchestrates requests. Memory/Project CRUD lives on the
//! owning engines (Application resolves them directly).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{MemoryEngine, MemoryEngineApi, MemoryScope, StoreMemoryRequest};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectEngine, ProjectEngineApi};

#[test]
fn planner_orchestrates_requests_engines_own_crud() {
    let data_dir = temp_dir("planner-responsibilities");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    // Project CRUD does not require Planner public APIs.
    let projects = app
        .container()
        .resolve::<std::sync::Arc<ProjectEngine>>()
        .expect("project engine");
    let project = projects
        .create(&CreateProjectRequest {
            project_id: Some("project:kernel".into()),
            name: "Kernel".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create via project engine");
    assert_eq!(projects.list().expect("list").len(), 1);

    // Application convenience methods resolve the owning engine.
    let via_app = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:app".into()),
            name: "App Facade".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create via app");
    assert_eq!(via_app.name, "App Facade");
    assert_eq!(app.list_projects().expect("list via app").len(), 2);

    // Memory CRUD does not require Planner public APIs.
    let memory = app
        .container()
        .resolve::<std::sync::Arc<MemoryEngine>>()
        .expect("memory engine");
    let stored = memory
        .store(&StoreMemoryRequest {
            scope: MemoryScope::Project,
            summary: "Planner is the orchestration kernel".into(),
            content: "CRUD belongs to owning engines.".into(),
            conversation_id: None,
            project_id: Some(project.id.as_str().to_string()),
            importance: Some(90),
            confidence: Some(95),
            tags: vec![],
            source: Some("test".into()),
            kind: Some("architecture_decision".into()),
            metadata_json: None,
        })
        .expect("store via memory engine");
    assert_eq!(stored.project_id.as_deref(), Some(project.id.as_str()));

    app.open_project(project.id.as_str())
        .expect("open project session");
    let via_app_memory = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "Facade note".into(),
            content: "Application resolves MemoryEngine directly.".into(),
            conversation_id: None,
            project_id: None,
            importance: Some(50),
            confidence: Some(50),
            tags: vec![],
            source: Some("test".into()),
            kind: None,
            metadata_json: None,
        })
        .expect("store via app");
    assert!(!via_app_memory.id.as_str().is_empty());

    // Planner still orchestrates Continue-working requests (same open lifecycle).
    let planner = app.container().resolve::<Planner>().expect("planner");
    let response = planner
        .handle(UserRequest::new("Continue working on Kernel."))
        .expect("handle continue");
    assert!(
        response.content.contains("Restored project") || response.project_context.is_some(),
        "expected continue-project orchestration; content={}",
        response.content
    );
    let context = response.project_context.expect("project context");
    assert_eq!(context.project.id, project.id);

    let _ = fs::remove_dir_all(&data_dir);
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-{label}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}
