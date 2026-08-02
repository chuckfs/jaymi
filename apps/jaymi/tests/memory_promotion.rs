//! Integration tests for Layer 4 Slice 5 — Memory Promotion.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    MemoryQuery, MemoryScope, PromoteMemoryRequest, PromotionAskDecision, PromotionSuggestQuery,
    RegisterProjectRequest, StoreMemoryRequest,
};
use jaymi_planner::Planner;

#[test]
fn promotion_ladder_suggestions_and_intentional_apply() {
    let data_dir = temp_dir("memory-promotion");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let project = app
        .register_project(&RegisterProjectRequest {
            project_id: Some("project:jaymi".into()),
            name: "Jaymi".into(),
            root_path: None,
        })
        .expect("register project");
    app.set_active_project(Some(project.id.as_str()))
        .expect("activate");

    let working = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "Durable working note".into(),
            content: "Keep the promotion ladder intentional.".into(),
            conversation_id: None,
            project_id: None,
            importance: Some(85),
            confidence: Some(90),
            tags: vec![],
            source: Some("test".into()),
            kind: None,
        })
        .expect("store working");

    // Suggestions are produced without applying.
    let suggestions = app
        .suggest_memory_promotions(&PromotionSuggestQuery {
            conversation_id: Some("conv-promo".into()),
            project_id: Some(project.id.as_str().to_string()),
            min_importance: Some(70),
            limit: Some(8),
        })
        .expect("suggest");
    assert!(suggestions.iter().any(|s| {
        s.memory_id == working.id.as_str()
            && s.from == MemoryScope::Working
            && s.to == MemoryScope::Conversation
    }));

    let still_working = app
        .retrieve_memory(&MemoryQuery {
            scope: Some(MemoryScope::Working),
            text: Some("promotion ladder".into()),
            ..MemoryQuery::default()
        })
        .expect("retrieve working");
    assert_eq!(still_working.len(), 1);
    assert_eq!(still_working[0].id, working.id);

    // Working → Conversation
    let in_conversation = app
        .promote_memory(&PromoteMemoryRequest {
            memory_id: working.id.as_str().to_string(),
            to: MemoryScope::Conversation,
            conversation_id: Some("conv-promo".into()),
            project_id: None,
            kind: None,
        })
        .expect("promote to conversation");
    assert_eq!(in_conversation.scope, MemoryScope::Conversation);
    assert_eq!(
        in_conversation.conversation_id.as_deref(),
        Some("conv-promo")
    );

    // Conversation → Project
    let in_project = app
        .promote_memory(&PromoteMemoryRequest {
            memory_id: working.id.as_str().to_string(),
            to: MemoryScope::Project,
            conversation_id: None,
            project_id: Some(project.id.as_str().to_string()),
            kind: Some("milestone".into()),
        })
        .expect("promote to project");
    assert_eq!(in_project.scope, MemoryScope::Project);
    assert_eq!(in_project.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(in_project.kind.as_deref(), Some("milestone"));

    // Project → Personal
    let personal = app
        .promote_memory(&PromoteMemoryRequest {
            memory_id: working.id.as_str().to_string(),
            to: MemoryScope::Personal,
            conversation_id: None,
            project_id: None,
            kind: Some("coding_preference".into()),
        })
        .expect("promote to personal");
    assert_eq!(personal.scope, MemoryScope::Personal);
    assert_eq!(personal.kind.as_deref(), Some("coding_preference"));

    // Demotion / sideways moves are rejected.
    let demote = app.promote_memory(&PromoteMemoryRequest {
        memory_id: working.id.as_str().to_string(),
        to: MemoryScope::Working,
        conversation_id: None,
        project_id: None,
        kind: None,
    });
    assert!(demote.is_err());

    // Planner surfaces ask decision without auto-promoting new candidates.
    let candidate = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "High importance scratch".into(),
            content: "Important enough to ask about".into(),
            conversation_id: None,
            project_id: None,
            importance: Some(92),
            confidence: Some(95),
            tags: vec![],
            source: None,
            kind: None,
        })
        .expect("store candidate");

    let planner = app.container().resolve::<Planner>().expect("planner");
    let response = planner
        .handle(UserRequest::new("list /tmp"))
        .expect("handle");
    assert!(response
        .promotion_suggestions
        .iter()
        .any(|s| s.memory_id == candidate.id.as_str()));
    assert_eq!(response.promotion_ask, PromotionAskDecision::AskUser);

    let untouched = app
        .retrieve_memory(&MemoryQuery {
            scope: Some(MemoryScope::Working),
            text: Some("Important enough".into()),
            ..MemoryQuery::default()
        })
        .expect("candidate still working");
    assert_eq!(untouched.len(), 1);
    assert_eq!(untouched[0].scope, MemoryScope::Working);
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
