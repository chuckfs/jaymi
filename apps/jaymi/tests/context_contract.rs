//! ContextBundle is the sole Planner request-context contract.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::{SearchRequest, UserRequest};

#[test]
fn handle_always_attaches_context_bundle() {
    let data_dir = temp_dir("contract-bundle");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let chat = app
        .handle(UserRequest::new("hello contract"))
        .expect("chat");
    assert!(chat.context().is_some());
    assert!(chat.context_bundle.is_some());

    let search = app
        .handle(UserRequest::search(SearchRequest::free_text("fungi")))
        .expect("search");
    assert!(search.context().is_some());
}

#[test]
fn parallel_memory_and_project_fields_are_gone_accessors_use_bundle() {
    let data_dir = temp_dir("contract-accessors");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let response = app
        .handle(UserRequest::new("accessor check"))
        .expect("handle");

    let bundle = response.context().expect("bundle");
    assert!(std::ptr::eq(
        response.memory().expect("memory"),
        bundle.memory()
    ));
    // No open project → project accessor is None and matches the bundle.
    assert!(response.project().is_none());
    assert!(bundle.project().is_none());
    assert_eq!(
        response.promotion_ask(),
        bundle.promotion_ask()
    );
    assert_eq!(
        response.promotion_suggestions().len(),
        bundle.promotion_suggestions().len()
    );
}

#[test]
fn open_project_exposes_project_only_via_context_bundle() {
    let data_dir = temp_dir("contract-open");
    let root = data_dir.join("proj");
    std::fs::create_dir_all(&root).unwrap();
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&jaymi_project_engine::CreateProjectRequest {
            project_id: Some("project:contract".into()),
            name: "ContractProj".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(jaymi_project_engine::ProjectType::Code),
        })
        .expect("create");

    let context = app.open_project(project.id.as_str()).expect("open");
    assert_eq!(context.project.id, project.id);

    let report = app.inspect_context().expect("inspect").expect("report");
    assert!(
        report
            .sections
            .iter()
            .any(|section| section.name.contains("Project") && section.present),
        "post-open assemble should surface project in ContextBundle; sections={:?}",
        report
            .sections
            .iter()
            .map(|s| format!("{} present={}", s.name, s.present))
            .collect::<Vec<_>>()
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-contract-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
