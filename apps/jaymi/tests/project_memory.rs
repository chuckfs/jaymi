//! Integration tests for Layer 4 Slice 3 — Project Memory.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    MemoryQuery, MemoryScope, ProjectMemoryKind, RegisterProjectRequest, StoreProjectMemoryRequest,
};
use jaymi_planner::Planner;

#[test]
fn continue_working_on_jaymi_restores_project_context_with_isolation() {
    let data_dir = temp_dir("project-memory");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let jaymi = app
        .register_project(&RegisterProjectRequest {
            project_id: Some("project:jaymi".into()),
            name: "Jaymi".into(),
            root_path: Some("/Users/charlie/jaymi".into()),
        })
        .expect("register jaymi");
    assert_eq!(jaymi.name, "Jaymi");

    let other = app
        .register_project(&RegisterProjectRequest {
            project_id: Some("project:other".into()),
            name: "OtherApp".into(),
            root_path: None,
        })
        .expect("register other");

    let kinds = [
        (
            ProjectMemoryKind::ArchitectureDecision,
            "Planner owns orchestration",
            "The Planner never touches storage directly.",
        ),
        (
            ProjectMemoryKind::Task,
            "Ship project memory",
            "Implement Layer 4 Slice 3 project memory.",
        ),
        (
            ProjectMemoryKind::CodingPreference,
            "Prefer Rust workspaces",
            "Keep crates small and provider-independent.",
        ),
        (
            ProjectMemoryKind::ImportantFile,
            "docs/memory.md",
            "Canonical memory design document.",
        ),
        (
            ProjectMemoryKind::Milestone,
            "Memory Engine foundation",
            "Slice 1 delivered centralized Memory Engine.",
        ),
        (
            ProjectMemoryKind::Conversation,
            "Architecture chat",
            "Discussed project-scoped memory isolation.",
        ),
    ];
    for (kind, summary, content) in kinds {
        app.store_project_memory(&StoreProjectMemoryRequest {
            project_id: jaymi.id.as_str().to_string(),
            kind,
            summary: summary.into(),
            content: content.into(),
            conversation_id: None,
            importance: Some(80),
            confidence: Some(90),
            tags: vec![],
            source: Some("test".into()),
        })
        .expect("store project memory");
    }

    app.store_project_memory(&StoreProjectMemoryRequest {
        project_id: other.id.as_str().to_string(),
        kind: ProjectMemoryKind::Task,
        summary: "Secret other task".into(),
        content: "other-project-token".into(),
        conversation_id: None,
        importance: Some(99),
        confidence: Some(99),
        tags: vec![],
        source: None,
    })
    .expect("store other memory");

    // Isolation: Jaymi project retrieve never sees OtherApp memory.
    let jaymi_only = app
        .retrieve_memory(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: Some(jaymi.id.as_str().to_string()),
            text: Some("other-project-token".into()),
            ..MemoryQuery::default()
        })
        .expect("jaymi retrieve");
    assert!(jaymi_only.is_empty());

    let global = app
        .retrieve_memory(&MemoryQuery {
            text: Some("other-project-token".into()),
            ..MemoryQuery::default()
        })
        .expect("global retrieve");
    assert!(
        global.is_empty(),
        "project memory must not leak into global retrieve"
    );

    let restored = app
        .continue_project("Jaymi")
        .expect("continue working on Jaymi");
    assert!(
        restored.content.contains("Restored project \"Jaymi\""),
        "unexpected content: {}",
        restored.content
    );
    let context = restored.project_context.expect("project context");
    assert_eq!(context.project_id, jaymi.id.as_str());
    assert_eq!(context.architecture_decisions.len(), 1);
    assert_eq!(context.tasks.len(), 1);
    assert_eq!(context.coding_preferences.len(), 1);
    assert_eq!(context.important_files.len(), 1);
    assert_eq!(context.milestones.len(), 1);
    assert_eq!(context.conversations.len(), 1);
    assert_eq!(context.entry_count(), 6);

    // Active project memory is retrieved automatically on later requests.
    let planner = app.container().resolve::<Planner>().expect("planner");
    let _ = planner
        .handle(UserRequest::new("what about the planner orchestration token"))
        .expect("follow-up");
    // Direct check that active project retrieve works after continue.
    let active = app
        .retrieve_memory(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: Some(jaymi.id.as_str().to_string()),
            text: Some("Planner owns".into()),
            ..MemoryQuery::default()
        })
        .expect("active retrieve");
    assert_eq!(active.len(), 1);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-memory-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
