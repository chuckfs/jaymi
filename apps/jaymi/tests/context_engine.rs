//! Integration tests for Architectural Integrity — Context Engine.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_context::{ContextEngine, ContextSource};
use jaymi_core::{Lifecycle, SearchRequest, UserRequest};
use jaymi_planner::Planner;

#[test]
fn every_planner_request_flows_through_context_engine() {
    let data_dir = temp_dir("context-flow");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context engine");

    assert!(context.sources_bound(), "sources must be bound at boot");
    assert_eq!(context.assemble_count(), 0);
    assert!(
        context.health_check().healthy,
        "context engine should be operational after bind"
    );

    app.handle(UserRequest::new("hello there"))
        .expect("unsupported-but-handled request still assembles context");
    assert_eq!(context.assemble_count(), 1);

    app.handle(UserRequest::new("Help me build an app."))
        .expect("plan work");
    assert_eq!(context.assemble_count(), 2);

    app.search(SearchRequest::free_text("fungi"))
        .expect("search");
    assert_eq!(context.assemble_count(), 3);

    app.discover_inventory().expect("discover");
    assert_eq!(context.assemble_count(), 4);

    let planner = app.container().resolve::<Planner>().expect("planner");
    let before = context.assemble_count();
    planner
        .handle(UserRequest::new("plan code"))
        .expect("direct planner handle");
    assert_eq!(
        context.assemble_count(),
        before + 1,
        "Planner::handle must call ContextEngine::assemble exactly once per request"
    );
}

#[test]
fn context_bundle_includes_workspace_and_user_request() {
    let data_dir = temp_dir("context-bundle");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");

    context.set_session_workspace(Some("coding".into()));
    let bundle = context
        .assemble(&UserRequest::new("remember this workspace"))
        .expect("assemble");

    assert!(
        bundle.sources().contains(&ContextSource::RetrievedMemories),
        "MemoryProvider contributes a Memory Engine snapshot when it participates"
    );
    assert!(bundle.sources().contains(&ContextSource::ActiveWorkspace));
    assert_eq!(bundle.workspace_kind(), Some("coding"));
    assert!(bundle.assemble_generation() >= 1);

    // Canonical contract: memory accessor is on ContextBundle only (may be empty bodies).
    let response = app
        .handle(UserRequest::new("unsupported unique phrase xyzzy"))
        .expect("handle");
    assert!(response.context().is_some(), "handle must attach ContextBundle");
    assert!(response.memory().is_some());
    let bundle = response.context().unwrap();
    assert!(bundle.sources().contains(&ContextSource::UserRequest));
}

#[test]
fn diagnostics_report_context_engine_operational() {
    let data_dir = temp_dir("context-diagnostics");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot
        .subsystem("Context Engine")
        .expect("context engine row");
    assert_eq!(row.status.label(), "Operational");
    assert!(row.detail.contains("sources_bound=true"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-engine-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
