//! Architectural Integrity Slice 2 — Project Ownership.
//!
//! Project Engine is the single source of truth for project identity.
//! Memory / Search / Knowledge reference projects only by `project_id`.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_memory::{
    MemoryEngine, MemoryEngineApi, MemoryQuery, MemoryScope, ProjectMemoryKind,
    StoreProjectMemoryRequest,
};
use jaymi_project_engine::{CreateProjectRequest, ProjectEngine, ProjectEngineApi, ProjectType};

#[test]
fn project_engine_is_sole_owner_of_project_identity() {
    let data_dir = temp_dir("project-ownership");
    let root = temp_dir("project-ownership-root");
    fs::create_dir_all(root.join("src")).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    // Creation / registration / lookup live on the Project Engine only.
    let projects = app
        .container()
        .resolve::<std::sync::Arc<ProjectEngine>>()
        .expect("project engine");
    let created = projects
        .create(&CreateProjectRequest {
            project_id: Some("project:ownership".into()),
            name: "Ownership".into(),
            description: Some("Single source of truth".into()),
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create via project engine");

    assert_eq!(created.id.as_str(), "project:ownership");
    assert_eq!(
        projects.get("project:ownership").expect("get").expect("found").name,
        "Ownership"
    );
    assert_eq!(
        projects
            .find_by_name("Ownership")
            .expect("find")
            .expect("named")
            .id
            .as_str(),
        "project:ownership"
    );
    assert_eq!(projects.list().expect("list").len(), 1);

    // Application create_project is a thin Project Engine facade (no Memory registry).
    let via_app = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:second".into()),
            name: "Second".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create via app");
    assert_eq!(via_app.id.as_str(), "project:second");
    assert_eq!(app.list_projects().expect("list").len(), 2);

    // Memory references the project only by id — no identity lookup required.
    let memory = app
        .container()
        .resolve::<std::sync::Arc<MemoryEngine>>()
        .expect("memory");
    let stored = memory
        .store_project_memory(&StoreProjectMemoryRequest {
            project_id: created.id.as_str().to_string(),
            kind: ProjectMemoryKind::ArchitectureDecision,
            summary: "Project Engine owns identity".into(),
            content: "Memory stores only project_id references.".into(),
            conversation_id: None,
            importance: Some(90),
            confidence: Some(95),
            tags: vec![],
            source: Some("test".into()),
        })
        .expect("store by project_id");
    assert_eq!(
        stored.project_id.as_deref(),
        Some(created.id.as_str())
    );

    let restored = memory
        .restore_project_memories(created.id.as_str())
        .expect("restore by project_id");
    assert_eq!(restored.project_id, created.id.as_str());
    assert_eq!(restored.architecture_decisions.len(), 1);
    assert!(restored.architecture_decisions[0]
        .content
        .contains("project_id"));

    // Isolation: memories for another project_id never appear.
    let other_only = memory
        .retrieve(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: Some("project:second".into()),
            text: Some("project_id".into()),
            ..MemoryQuery::default()
        })
        .expect("retrieve other");
    assert!(other_only.is_empty());

    // Assembled ProjectContext is the only ProjectContext type (PE-owned).
    let context = app
        .open_project(created.id.as_str())
        .expect("open");
    assert_eq!(context.project.id, created.id);
    assert_eq!(context.memories.project_id, created.id.as_str());
    assert_eq!(context.memories.name, "Ownership");
    assert_eq!(context.memories.architecture_decisions.len(), 1);

    // Knowledge search is scoped by project_id and enters the Planner.
    let hits = app
        .search_project_knowledge(created.id.as_str(), "project_id", Some(8))
        .expect("project knowledge");
    assert!(
        hits.iter().any(|hit| hit.project_id == created.id.as_str()),
        "expected knowledge hits keyed by project_id; hits={hits:?}"
    );

    // Deletion is owned by the Project Engine.
    app.delete_project("project:second").expect("delete second");
    assert_eq!(app.list_projects().expect("after delete").len(), 1);
    assert!(projects.get("project:second").expect("get deleted").is_some());

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
