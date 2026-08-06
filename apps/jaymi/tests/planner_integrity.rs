//! Architectural Integrity Slice 4 — Planner Integrity.
//!
//! User requests (search, project knowledge, list, read, discover, continue)
//! always enter `Planner::handle`. Application must not retrieve project
//! knowledge by calling the Project Engine directly.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_context::ContextEngine;
use jaymi_core::UserRequest;
use jaymi_memory::{MemoryScope, ProjectMemoryKind, StoreProjectMemoryRequest};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_understanding::UnderstandingEngine;

#[test]
fn project_knowledge_and_user_requests_traverse_the_planner() {
    let data_dir = temp_dir("planner-integrity-data");
    let root = temp_dir("planner-integrity-root");
    fs::create_dir_all(root.join("docs")).unwrap();
    let doc = root.join("docs").join("guide.md");
    fs::write(&doc, "# Guide\n\nunique-integrity-token-42 lives here.\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");
    app.container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding")
        .understand_path(&doc)
        .unwrap()
        .unwrap();

    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:integrity".into()),
            name: "Integrity".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Integrity task".into(),
        content: "integrity-task-token-99".into(),
        conversation_id: None,
        importance: Some(80),
        confidence: Some(80),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("store task");

    app.open_project(project.id.as_str()).expect("open");

    let planner = app.container().resolve::<Planner>().expect("planner");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");

    let handles_before = planner.handle_count();
    let assembles_before = context.assemble_count();

    // Application facade must go through Planner.handle (not ProjectEngine).
    let hits = app
        .search_project_knowledge(project.id.as_str(), "unique-integrity-token-42", Some(20))
        .expect("project knowledge via app");
    assert!(
        hits.iter().any(|hit| {
            hit.detail.contains("unique-integrity-token-42")
                || hit
                    .path
                    .as_ref()
                    .map(|path| path.ends_with("guide.md"))
                    .unwrap_or(false)
        }),
        "expected file/content hit; got {hits:?}"
    );
    assert_eq!(planner.handle_count(), handles_before + 1);
    assert_eq!(context.assemble_count(), assembles_before + 1);

    let task_hits = app
        .search_project_knowledge(project.id.as_str(), "integrity-task-token-99", Some(20))
        .expect("task knowledge via app");
    assert!(task_hits
        .iter()
        .any(|hit| hit.detail.contains("integrity-task-token-99")));
    assert_eq!(planner.handle_count(), handles_before + 2);
    assert_eq!(context.assemble_count(), assembles_before + 2);

    // Explicit UserRequest path also increments handle + assemble.
    let response = planner
        .handle(UserRequest::search_project_knowledge(
            project.id.as_str(),
            "integrity-task-token-99",
            Some(10),
        ))
        .expect("handle project knowledge");
    assert!(!response.project_knowledge.is_empty());
    assert_eq!(
        response.tool_id.as_deref(),
        Some(jaymi_tools::SEARCH_PROJECT_KNOWLEDGE_TOOL_ID)
    );
    assert!(response.policy_evaluation.is_some());
    assert!(response.permission_result.is_some());
    assert_eq!(planner.handle_count(), handles_before + 3);
    assert_eq!(context.assemble_count(), assembles_before + 3);

    // Inventory search and continue-project also traverse the Planner.
    let search = app
        .search(jaymi_core::SearchRequest::free_text(
            "unique-integrity-token-42",
        ))
        .expect("inventory search");
    assert!(
        !search.content.is_empty() || !search.entries.is_empty() || !search.citations.is_empty()
    );
    assert_eq!(planner.handle_count(), handles_before + 4);

    let continued = app.continue_project("Integrity").expect("continue");
    assert!(continued.project().is_some());
    assert_eq!(planner.handle_count(), handles_before + 5);
    assert_eq!(context.assemble_count(), assembles_before + 5);

    // Admin CRUD does not count as a Planner request (Slice 3).
    let _ = app
        .store_memory(&jaymi_memory::StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "admin note".into(),
            content: "not a user request".into(),
            conversation_id: None,
            project_id: None,
            importance: Some(10),
            confidence: Some(10),
            tags: vec![],
            source: Some("test".into()),
            kind: None,
            metadata_json: None,
        })
        .expect("admin store");
    assert_eq!(planner.handle_count(), handles_before + 5);

    let _ = fs::remove_dir_all(&data_dir);
    let _ = fs::remove_dir_all(&root);
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
