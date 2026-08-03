//! Architectural Cleanup — Pipeline Consistency.
//!
//! Every user request enters Planner::handle and project-knowledge retrieval
//! traverses Cap → Policy → Permission → Tool (no Project Engine bypass).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_context::ContextEngine;
use jaymi_core::UserRequest;
use jaymi_memory::{ProjectMemoryKind, StoreProjectMemoryRequest};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_tools::SEARCH_PROJECT_KNOWLEDGE_TOOL_ID;
use jaymi_understanding::UnderstandingEngine;

#[test]
fn project_knowledge_traverses_full_request_pipeline() {
    let data_dir = temp_dir("pipeline-consistency-data");
    let root = temp_dir("pipeline-consistency-root");
    fs::create_dir_all(root.join("docs")).unwrap();
    let doc = root.join("docs").join("guide.md");
    fs::write(&doc, "# Guide\n\npipeline-token-alpha-77 lives here.\n").unwrap();

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
            project_id: Some("project:pipeline".into()),
            name: "Pipeline".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Pipeline task".into(),
        content: "pipeline-task-token-88".into(),
        conversation_id: None,
        importance: Some(80),
        confidence: Some(80),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("store task");

    // Open goes through Planner::handle (not a duplicate Application PE path).
    let planner = app.container().resolve::<Planner>().expect("planner");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    let handles_before = planner.handle_count();
    let assembles_before = context.assemble_count();

    let opened = app.open_project(project.id.as_str()).expect("open");
    assert_eq!(opened.project.id.as_str(), project.id.as_str());
    assert_eq!(planner.handle_count(), handles_before + 1);
    assert_eq!(context.assemble_count(), assembles_before + 1);

    let response = app
        .handle(UserRequest::search_project_knowledge(
            project.id.as_str(),
            "pipeline-token-alpha-77",
            Some(20),
        ))
        .expect("project knowledge");

    assert_eq!(
        response.tool_id.as_deref(),
        Some(SEARCH_PROJECT_KNOWLEDGE_TOOL_ID)
    );
    assert_eq!(
        response.capability.map(|c| c.id()),
        Some("search")
    );
    assert!(response.policy_evaluation.is_some());
    assert!(response
        .policy_evaluation
        .as_ref()
        .map(|evaluation| evaluation.allowed)
        .unwrap_or(false));
    assert!(response.permission_result.is_some());
    assert!(response
        .permission_result
        .as_ref()
        .map(|result| result.allows_execution())
        .unwrap_or(false));
    assert!(!response.blocked);
    assert!(response.provider_id.is_some());
    assert!(
        response.project_knowledge.iter().any(|hit| {
            hit.detail.contains("pipeline-token-alpha-77")
                || hit
                    .path
                    .as_ref()
                    .map(|path| path.ends_with("guide.md"))
                    .unwrap_or(false)
        }),
        "expected knowledge hit; got {:?}",
        response.project_knowledge
    );
    assert_eq!(planner.handle_count(), handles_before + 2);
    assert_eq!(context.assemble_count(), assembles_before + 2);

    let task_hits = app
        .search_project_knowledge(project.id.as_str(), "pipeline-task-token-88", Some(20))
        .expect("task hits");
    assert!(task_hits
        .iter()
        .any(|hit| hit.detail.contains("pipeline-task-token-88")));
    assert_eq!(planner.handle_count(), handles_before + 3);

    let closed = app.close_project().expect("close").expect("closed project");
    assert_eq!(closed.id.as_str(), project.id.as_str());
    assert_eq!(planner.handle_count(), handles_before + 4);
    assert_eq!(context.assemble_count(), assembles_before + 4);
    assert!(app.active_project_id().is_none());

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
