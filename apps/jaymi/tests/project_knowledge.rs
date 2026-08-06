//! Integration tests for Layer 5 Slice 5 — Project Knowledge.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    AppendMessageRequest, CreateConversationRequest, MessageRole, ProjectMemoryKind,
    StoreProjectMemoryRequest,
};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_understanding::UnderstandingEngine;

#[test]
fn project_knowledge_is_isolated_and_search_is_project_aware() {
    let data_dir = temp_dir("project-knowledge-data");
    let root_a = temp_dir("project-knowledge-a");
    let root_b = temp_dir("project-knowledge-b");
    fs::create_dir_all(root_a.join("docs")).unwrap();
    fs::create_dir_all(root_b.join("docs")).unwrap();

    let alpha_doc = root_a.join("docs").join("alpha-guide.md");
    let beta_doc = root_b.join("docs").join("beta-guide.md");
    fs::write(
        &alpha_doc,
        "# Alpha Guide\n\nAlpha unique token alphaknowledge99 lives here.\n",
    )
    .unwrap();
    fs::write(
        &beta_doc,
        "# Beta Guide\n\nBeta unique token betaknowledge88 lives here.\n",
    )
    .unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root_a).expect("index a");
    app.index_root(&root_b).expect("index b");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    understanding.understand_path(&alpha_doc).unwrap().unwrap();
    understanding.understand_path(&beta_doc).unwrap().unwrap();

    let project_a = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:alpha".into()),
            name: "Alpha".into(),
            description: Some("Knowledge A".into()),
            root_directory: Some(root_a.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create alpha");
    let project_b = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:beta".into()),
            name: "Beta".into(),
            description: Some("Knowledge B".into()),
            root_directory: Some(root_b.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create beta");

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project_a.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Alpha task".into(),
        content: "alphatask-token-11".into(),
        conversation_id: None,
        importance: Some(90),
        confidence: Some(90),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("alpha task");
    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project_b.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Beta task".into(),
        content: "betatask-token-22".into(),
        conversation_id: None,
        importance: Some(90),
        confidence: Some(90),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("beta task");
    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project_a.id.as_str().to_string(),
        kind: ProjectMemoryKind::ArchitectureDecision,
        summary: "Alpha architecture".into(),
        content: "alpha-arch-token-33".into(),
        conversation_id: None,
        importance: Some(95),
        confidence: Some(95),
        tags: vec!["architecture".into()],
        source: Some("test".into()),
    })
    .expect("alpha architecture");

    let conversation = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-alpha-knowledge".into()),
            title: Some("Alpha knowledge chat".into()),
            project_id: Some(project_a.id.as_str().to_string()),
        })
        .expect("conversation");
    app.append_message(&AppendMessageRequest {
        conversation_id: conversation.id.as_str().to_string(),
        role: MessageRole::User,
        content: "Remember alphaconv-token-44 for Alpha.".into(),
        created_at: None,
        attachments: vec![],
        references: vec![],
    })
    .expect("append");

    let context = app.open_project(project_a.id.as_str()).expect("open alpha");
    assert!(
        context
            .indexed_files
            .iter()
            .any(|file| file.filename == "alpha-guide.md"),
        "expected indexed alpha guide"
    );
    assert!(
        context
            .parsed_content
            .iter()
            .any(|item| item.path.ends_with("alpha-guide.md")),
        "expected parsed alpha content"
    );
    assert!(
        context
            .parsed_content
            .iter()
            .all(|item| !item.path.ends_with("beta-guide.md")),
        "parsed content must not include Beta files"
    );
    assert!(!context.documentation.is_empty());
    assert!(!context.architecture_documents.is_empty());
    assert_eq!(context.tasks.len(), 1);
    assert!(context.tasks[0].content.contains("alphatask-token-11"));
    assert!(context.search_index.has_root);

    // Project knowledge search (Planner-mediated; Application does not call PE directly).
    let hits = app
        .search_project_knowledge(project_a.id.as_str(), "alphaknowledge99", Some(20))
        .expect("search alpha content");
    assert!(
        hits.iter().any(|hit| {
            hit.path
                .as_ref()
                .map(|path| path.ends_with("alpha-guide.md"))
                .unwrap_or(false)
        }),
        "expected alpha file hit; got {hits:?}"
    );
    assert!(
        hits.iter().all(|hit| {
            hit.path
                .as_ref()
                .map(|path| !path.ends_with("beta-guide.md"))
                .unwrap_or(true)
        }),
        "file hits must not include Beta files; got {hits:?}"
    );

    let task_hits = app
        .search_project_knowledge(project_a.id.as_str(), "alphatask-token-11", Some(20))
        .expect("search alpha task");
    assert!(task_hits
        .iter()
        .any(|hit| hit.detail.contains("alphatask-token-11")));
    assert!(task_hits
        .iter()
        .all(|hit| !hit.detail.contains("betatask-token-22")));

    let planner = app.container().resolve::<Planner>().expect("planner");
    let search = planner
        .handle(UserRequest::new("search alphaknowledge99"))
        .expect("planner search");
    assert!(
        search.content.contains("alphaknowledge99")
            || search
                .entries
                .iter()
                .any(|entry| entry.path.ends_with("alpha-guide.md")),
        "active project search should find Alpha content; content={}",
        search.content
    );
    assert!(
        !search.content.contains("betaknowledge88"),
        "Alpha search must not surface Beta content"
    );
    assert!(search
        .entries
        .iter()
        .all(|entry| !entry.path.ends_with("beta-guide.md")));

    // Switch projects — retrieval boundary moves.
    let switched = planner
        .handle(UserRequest::new("switch to project Beta"))
        .expect("switch");
    assert_eq!(
        switched
            .project()
            .map(|context| context.project.id.as_str()),
        Some(project_b.id.as_str())
    );
    let beta_context = switched.project().cloned().expect("beta context");
    assert!(beta_context
        .tasks
        .iter()
        .any(|task| task.content.contains("betatask-token-22")));
    assert!(beta_context
        .tasks
        .iter()
        .all(|task| !task.content.contains("alphatask-token-11")));

    let beta_search = planner
        .handle(UserRequest::new("search betaknowledge88"))
        .expect("beta search");
    assert!(
        beta_search
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("beta-guide.md"))
            || beta_search.content.contains("betaknowledge88"),
        "Beta workspace should find beta content"
    );
    assert!(beta_search
        .entries
        .iter()
        .all(|entry| !entry.path.ends_with("alpha-guide.md")));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-project-knowledge-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
