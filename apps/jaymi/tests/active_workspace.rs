//! Integration tests for Layer 5 Slice 3 — Active Workspace.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    AppendMessageRequest, CreateConversationRequest, ProjectMemoryKind, StoreProjectMemoryRequest,
};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn open_switch_and_close_keep_one_active_project_and_persist_conversation() {
    let data_dir = temp_dir("active-workspace-data");
    let root_a = temp_dir("active-workspace-a");
    let root_b = temp_dir("active-workspace-b");
    fs::write(root_a.join("alpha.txt"), "alpha-token\n").unwrap();
    fs::write(root_b.join("beta.txt"), "beta-token\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root_a).expect("index a");
    app.index_root(&root_b).expect("index b");

    let project_a = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:alpha".into()),
            name: "Alpha".into(),
            description: Some("First workspace".into()),
            root_directory: Some(root_a.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create alpha");
    let project_b = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:beta".into()),
            name: "Beta".into(),
            description: Some("Second workspace".into()),
            root_directory: Some(root_b.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create beta");

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project_a.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Alpha task".into(),
        content: "alpha-memory-token".into(),
        conversation_id: None,
        importance: Some(90),
        confidence: Some(90),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("alpha memory");
    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: project_b.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Beta task".into(),
        content: "beta-memory-token".into(),
        conversation_id: None,
        importance: Some(90),
        confidence: Some(90),
        tags: vec![],
        source: Some("test".into()),
    })
    .expect("beta memory");

    let conversation = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-workspace".into()),
            title: Some("Persistent chat".into()),
            project_id: None,
        })
        .expect("conversation");
    app.set_active_conversation(Some(conversation.id.as_str()))
        .expect("activate conversation");
    app.append_message(&AppendMessageRequest {
        conversation_id: conversation.id.as_str().to_string(),
        role: jaymi_memory::MessageRole::User,
        content: "hello before switch".into(),
        created_at: None,
        attachments: vec![],
        references: vec![],
    })
    .expect("append");

    let opened = app.open_project(project_a.id.as_str()).expect("open alpha");
    assert!(opened.is_open);
    assert_eq!(opened.project.id, project_a.id);
    assert_eq!(
        app.active_project_id().as_deref(),
        Some(project_a.id.as_str())
    );
    assert_eq!(
        app.active_conversation_id().as_deref(),
        Some(conversation.id.as_str())
    );

    // Planner scopes search to the active project root.
    let planner = app.container().resolve::<Planner>().expect("planner");
    let search_a = planner
        .handle(UserRequest::new("find file alpha.txt"))
        .expect("search in alpha");
    assert_eq!(
        search_a
            .project()
            .map(|context| context.project.id.as_str()),
        Some(project_a.id.as_str())
    );
    assert_eq!(
        search_a.listed_path.as_deref(),
        Some(root_a.as_path()),
        "search should be folder-scoped to the active project root"
    );
    assert!(
        search_a
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("alpha.txt")),
        "active Alpha workspace should surface alpha.txt; entries={:?}",
        search_a
            .entries
            .iter()
            .map(|entry| entry.path.display().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        search_a
            .entries
            .iter()
            .all(|entry| !entry.path.ends_with("beta.txt")),
        "Alpha workspace must not return Beta files"
    );

    // Switch project — conversation remains active; context changes.
    let switched = planner
        .handle(UserRequest::new("switch to project Beta"))
        .expect("switch");
    let beta_context = switched.project().cloned().expect("beta context");
    assert_eq!(beta_context.project.id, project_b.id);
    assert!(beta_context.is_open);
    assert_eq!(
        app.active_project_id().as_deref(),
        Some(project_b.id.as_str())
    );
    assert_eq!(
        app.active_conversation_id().as_deref(),
        Some(conversation.id.as_str()),
        "conversation must persist across project switch"
    );
    assert!(beta_context
        .memories
        .tasks
        .iter()
        .any(|record| record.content.contains("beta-memory-token")));
    assert!(
        !beta_context
            .memories
            .tasks
            .iter()
            .any(|record| record.content.contains("alpha-memory-token")),
        "switched workspace must not include prior project memory"
    );

    let search_b = planner
        .handle(UserRequest::new("find file beta.txt"))
        .expect("search in beta");
    assert_eq!(
        search_b.listed_path.as_deref(),
        Some(root_b.as_path()),
        "switched workspace should scope search to Beta root"
    );
    assert!(
        search_b
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("beta.txt")),
        "active Beta workspace should surface beta.txt"
    );
    assert!(search_b
        .entries
        .iter()
        .all(|entry| !entry.path.ends_with("alpha.txt")));

    // Relative list resolves under the active project root.
    let listed = planner
        .handle(UserRequest::new("list ."))
        .expect("list active root");
    assert_eq!(
        listed.listed_path.as_deref(),
        Some(root_b.as_path()),
        "list . should resolve to active project root"
    );

    // Close clears active project; conversation stays.
    let closed = planner
        .handle(UserRequest::new("close project"))
        .expect("close");
    assert!(closed.content.contains("Closed project \"Beta\""));
    assert!(closed.project().is_none());
    assert!(app.active_project_id().is_none());
    assert_eq!(
        app.active_conversation_id().as_deref(),
        Some(conversation.id.as_str())
    );

    let loaded = app
        .load_conversation(conversation.id.as_str())
        .expect("load")
        .expect("conversation still exists");
    assert!(loaded
        .messages
        .iter()
        .any(|message| message.content.contains("hello before switch")));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-active-workspace-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
