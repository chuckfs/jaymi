//! Integration tests for Layer 5 Slice 6 — Project Decisions.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    CreateConversationRequest, ListProjectDecisionsQuery, StoreProjectDecisionRequest,
};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn project_decisions_persist_and_planner_retrieves_them() {
    let data_dir = temp_dir("project-decisions-data");
    let root_a = temp_dir("project-decisions-a");
    let root_b = temp_dir("project-decisions-b");
    fs::create_dir_all(root_a.join("src")).unwrap();
    fs::create_dir_all(&root_b).unwrap();
    let related_file = root_a.join("src").join("planner.rs");
    fs::write(&related_file, "// planner owns orchestration\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root_a).expect("index a");

    let project_a = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:decisions-alpha".into()),
            name: "Decisions Alpha".into(),
            description: Some("Decision log A".into()),
            root_directory: Some(root_a.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create alpha");
    let project_b = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:decisions-beta".into()),
            name: "Decisions Beta".into(),
            description: Some("Decision log B".into()),
            root_directory: Some(root_b.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create beta");

    let conversation = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-decision-alpha".into()),
            title: Some("Architecture chat".into()),
            project_id: Some(project_a.id.as_str().to_string()),
        })
        .expect("conversation");

    let stored = app
        .store_project_decision(&StoreProjectDecisionRequest {
            project_id: project_a.id.as_str().to_string(),
            title: "Planner owns orchestration".into(),
            description: "All user goals route through the Planner kernel.".into(),
            reasoning: "Direct storage access from tools would break isolation and auditing (whydidwechooseplanner99).".into(),
            related_files: vec![related_file.display().to_string()],
            related_conversations: vec![conversation.id.as_str().to_string()],
            conversation_id: Some(conversation.id.as_str().to_string()),
            importance: Some(95),
            confidence: Some(95),
            source: Some("test".into()),
        })
        .expect("store decision");

    assert!(!stored.memory_id.is_empty());
    assert_eq!(stored.title, "Planner owns orchestration");
    assert!(stored.reasoning.contains("whydidwechooseplanner99"));
    assert_eq!(stored.related_files.len(), 1);
    assert!(stored
        .related_conversations
        .iter()
        .any(|id| id == conversation.id.as_str()));

    app.store_project_decision(&StoreProjectDecisionRequest {
        project_id: project_b.id.as_str().to_string(),
        title: "Beta storage choice".into(),
        description: "Beta uses a different store.".into(),
        reasoning: "betadecision-secret-token-77 must stay isolated.".into(),
        related_files: vec![],
        related_conversations: vec![],
        conversation_id: None,
        importance: Some(90),
        confidence: Some(90),
        source: Some("test".into()),
    })
    .expect("store beta decision");

    let listed = app
        .list_project_decisions(&ListProjectDecisionsQuery {
            project_id: project_a.id.as_str().to_string(),
            text: None,
            limit: Some(20),
        })
        .expect("list alpha decisions");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].memory_id, stored.memory_id);
    assert!(listed[0].reasoning.contains("whydidwechooseplanner99"));
    assert!(listed
        .iter()
        .all(|decision| !decision.reasoning.contains("betadecision-secret-token-77")));

    let by_reasoning = app
        .list_project_decisions(&ListProjectDecisionsQuery {
            project_id: project_a.id.as_str().to_string(),
            text: Some("whydidwechooseplanner99".into()),
            limit: Some(10),
        })
        .expect("search by reasoning");
    assert_eq!(by_reasoning.len(), 1);

    let fetched = app
        .get_project_decision(&stored.memory_id)
        .expect("get decision")
        .expect("decision exists");
    assert_eq!(fetched.description, stored.description);
    assert_eq!(fetched.reasoning, stored.reasoning);

    // Re-open project — structured decision log is part of ProjectContext.
    let context = app
        .open_project(project_a.id.as_str())
        .expect("open alpha");
    assert_eq!(context.decisions.len(), 1);
    assert_eq!(context.decisions[0].title, "Planner owns orchestration");
    assert!(context.decisions[0]
        .reasoning
        .contains("whydidwechooseplanner99"));
    assert_eq!(
        context.decisions[0].related_files,
        stored.related_files
    );
    assert!(context
        .search_index
        .detail
        .contains("decisions=1"));

    // Persistence across Application reboot.
    drop(app);
    let app = Application::boot_with_data_dir(&data_dir).expect("reboot");
    let reloaded = app
        .list_project_decisions(&ListProjectDecisionsQuery {
            project_id: project_a.id.as_str().to_string(),
            text: None,
            limit: Some(20),
        })
        .expect("list after reboot");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].memory_id, stored.memory_id);
    assert!(reloaded[0].reasoning.contains("whydidwechooseplanner99"));

    let restored = app
        .open_project(project_a.id.as_str())
        .expect("open after reboot");
    assert_eq!(restored.decisions.len(), 1);
    assert!(restored.decisions[0]
        .reasoning
        .contains("whydidwechooseplanner99"));

    // Planner automatically retrieves the decision into memory_context.
    let planner = app.container().resolve::<Planner>().expect("planner");
    app.set_active_project(Some(project_a.id.as_str()))
        .expect("activate alpha");
    let response = planner
        .handle(UserRequest::new(
            "Remind me why we chose the planner for orchestration whydidwechooseplanner99",
        ))
        .expect("planner handle");
    let memory = response.memory_context.expect("memory context");
    assert!(
        memory.records().iter().any(|record| {
            record.summary.contains("Planner owns orchestration")
                || record.content.contains("All user goals route through the Planner")
                || record.metadata_json.contains("whydidwechooseplanner99")
        }),
        "expected decision recalled in memory_context; records={:?}",
        memory
            .records()
            .iter()
            .map(|record| (&record.summary, &record.kind))
            .collect::<Vec<_>>()
    );
    assert!(
        memory.records().iter().all(|record| {
            !record
                .metadata_json
                .contains("betadecision-secret-token-77")
                && !record.content.contains("betadecision-secret-token-77")
        }),
        "Alpha memory context must not include Beta decisions"
    );

    // Knowledge search matches reasoning-only tokens (Planner-mediated).
    let hits = app
        .search_project_knowledge(project_a.id.as_str(), "whydidwechooseplanner99", Some(20))
        .expect("search reasoning");
    assert!(
        hits.iter().any(|hit| hit.detail.contains("whydidwechooseplanner99")),
        "expected reasoning hit; got {hits:?}"
    );
    assert!(hits
        .iter()
        .all(|hit| !hit.detail.contains("betadecision-secret-token-77")));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-project-decisions-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
