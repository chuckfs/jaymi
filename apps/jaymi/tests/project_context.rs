//! Integration tests for Layer 5 Slice 2 — Project Context.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{CreateConversationRequest, ProjectMemoryKind, StoreProjectMemoryRequest};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn project_engine_assembles_one_project_context_for_planner() {
    let data_dir = temp_dir("project-context-data");
    let root = temp_dir("project-context-root");
    let docs = root.join("docs");
    fs::create_dir_all(&docs).unwrap();
    fs::write(
        docs.join("architecture.md"),
        "# Architecture\n\nPlanner owns orchestration.\n",
    )
    .unwrap();
    fs::write(docs.join("notes.txt"), "recent scratch notes\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:jaymi".into()),
            name: "Jaymi".into(),
            description: Some("Personal AI environment".into()),
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project.id.as_str().to_string(),
        kind: ProjectMemoryKind::ArchitectureDecision,
        summary: "Planner owns orchestration".into(),
        content: "The Planner requests one ProjectContext from the Project Engine.".into(),
        conversation_id: None,
        importance: Some(95),
        confidence: Some(95),
        tags: vec!["architecture".into()],
        source: Some("test".into()),
    })
    .expect("architecture memory");

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project.id.as_str().to_string(),
        kind: ProjectMemoryKind::ImportantFile,
        summary: "docs/architecture.md".into(),
        content: docs.join("architecture.md").display().to_string(),
        conversation_id: None,
        importance: Some(80),
        confidence: Some(90),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("important file memory");

    app.create_conversation(&CreateConversationRequest {
        conversation_id: Some("conv-jaymi".into()),
        title: Some("Architecture chat".into()),
        project_id: Some(project.id.as_str().to_string()),
    })
    .expect("conversation");

    let context = app
        .open_project(project.id.as_str())
        .expect("open assembles context");

    assert!(context.is_open);
    assert_eq!(context.project.id, project.id);
    assert!(
        !context.indexed_files.is_empty(),
        "expected indexed files under project root"
    );
    assert!(context
        .indexed_files
        .iter()
        .any(|file| file.filename == "architecture.md"));
    assert_eq!(context.conversations.len(), 1);
    assert_eq!(context.memories.architecture_decisions.len(), 1);
    assert!(!context.important_documents.is_empty());
    assert!(!context.recent_work.is_empty());
    assert!(!context.architecture_documents.is_empty());
    assert!(context.search_index.has_root);
    assert!(context.search_index.indexed_file_count >= 1);

    // Planner requests one ProjectContext — it does not gather resources itself.
    let planner = app.container().resolve::<Planner>().expect("planner");
    let response = planner
        .handle(UserRequest::new("Continue working on Jaymi."))
        .expect("continue");
    let restored = response.project_context.expect("project context");
    assert_eq!(restored.project.id, project.id);
    assert!(!restored.memories.architecture_decisions.is_empty());
    assert!(response.content.contains("indexed_files="));
    assert!(response.content.contains("architecture="));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-project-context-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
