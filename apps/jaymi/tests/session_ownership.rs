//! Architectural Cleanup — Session Ownership.
//!
//! Exactly one project session lifecycle: Application delegates → Planner
//! orchestrates → Project Engine owns open state (Memory mirrors the id).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{MemoryEngine, MemoryEngineApi};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectEngine, ProjectEngineApi};

#[test]
fn project_session_has_one_planner_orchestrated_lifecycle() {
    let data_dir = temp_dir("session-ownership");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let alpha = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:alpha".into()),
            name: "Alpha".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create alpha");
    let beta = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:beta".into()),
            name: "Beta".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create beta");

    let planner = app.container().resolve::<Planner>().expect("planner");
    let projects = app
        .container()
        .resolve::<Arc<ProjectEngine>>()
        .expect("project engine");
    let memory = app
        .container()
        .resolve::<Arc<MemoryEngine>>()
        .expect("memory");

    assert!(projects.active_project_id().is_none());
    assert!(memory.active_project_id().is_none());

    let handles_before = planner.handle_count();

    // Canonical open: Application → Planner::handle → PE.open + Memory bind.
    let opened = app.open_project(alpha.id.as_str()).expect("open alpha");
    assert_eq!(opened.project.id.as_str(), alpha.id.as_str());
    assert_eq!(planner.handle_count(), handles_before + 1);
    assert_eq!(
        projects.active_project_id().as_deref(),
        Some(alpha.id.as_str())
    );
    assert_eq!(
        memory.active_project_id().as_deref(),
        Some(alpha.id.as_str())
    );
    assert_eq!(
        app.active_project_id().as_deref(),
        Some(alpha.id.as_str()),
        "Application reads Project Engine as source of truth"
    );

    // set_active_project is not a second mutation path — it delegates to open.
    app.set_active_project(Some(beta.id.as_str()))
        .expect("activate beta via set_active_project");
    assert_eq!(planner.handle_count(), handles_before + 2);
    assert_eq!(
        projects.active_project_id().as_deref(),
        Some(beta.id.as_str())
    );
    assert_eq!(
        memory.active_project_id().as_deref(),
        Some(beta.id.as_str())
    );

    // Natural-language Continue uses the same open lifecycle.
    let continued = app
        .handle(UserRequest::new("Continue working on Alpha."))
        .expect("continue");
    assert!(continued.project_context.is_some());
    assert_eq!(planner.handle_count(), handles_before + 3);
    assert_eq!(
        projects.active_project_id().as_deref(),
        Some(alpha.id.as_str())
    );
    assert_eq!(
        memory.active_project_id().as_deref(),
        Some(alpha.id.as_str())
    );

    // Canonical close clears PE and Memory together.
    let closed = app.close_project().expect("close").expect("closed");
    assert_eq!(closed.id.as_str(), alpha.id.as_str());
    assert_eq!(planner.handle_count(), handles_before + 4);
    assert!(projects.active_project_id().is_none());
    assert!(memory.active_project_id().is_none());
    assert!(app.active_project_id().is_none());

    // set_active_project(None) also delegates to close (same lifecycle).
    app.open_project(beta.id.as_str()).expect("re-open beta");
    assert_eq!(planner.handle_count(), handles_before + 5);
    app.set_active_project(None)
        .expect("clear via set_active_project");
    assert_eq!(planner.handle_count(), handles_before + 6);
    assert!(projects.active_project_id().is_none());
    assert!(memory.active_project_id().is_none());

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
