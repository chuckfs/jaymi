//! Integration tests for Layer 4 Slice 6 — Context Assembly.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    AssembleContextRequest, MemoryRelevanceKind, MemoryScope, StoreMemoryRequest,
};
use jaymi_planner::Planner;
use jaymi_project_engine::CreateProjectRequest;

#[test]
fn assemble_context_returns_only_relevant_memories_with_limits() {
    let data_dir = temp_dir("context-assembly");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let jaymi = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:jaymi".into()),
            name: "Jaymi".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create jaymi");
    let other = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:other".into()),
            name: "OtherApp".into(),
            description: None,
            root_directory: None,
            project_type: None,
        })
        .expect("create other");

    app.set_active_project(Some(jaymi.id.as_str()))
        .expect("activate jaymi");
    app.set_active_conversation(Some("conv-context"))
        .expect("activate conversation");

    let relevant_project = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Project,
            summary: "Planner owns orchestration".into(),
            content: "The Planner never touches storage directly.".into(),
            conversation_id: None,
            project_id: Some(jaymi.id.as_str().to_string()),
            importance: Some(90),
            confidence: Some(95),
            tags: vec!["architecture".into()],
            source: Some("test".into()),
            kind: Some("architecture_decision".into()),
                metadata_json: None,
            })
        .expect("jaymi project memory");

    let foreign = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Project,
            summary: "Foreign secret".into(),
            content: "other-project-token must stay isolated".into(),
            conversation_id: None,
            project_id: Some(other.id.as_str().to_string()),
            importance: Some(99),
            confidence: Some(99),
            tags: vec![],
            source: None,
            kind: Some("task".into()),
                metadata_json: None,
            })
        .expect("foreign project memory");

    let conversation = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Conversation,
            summary: "Conversation note about orchestration".into(),
            content: "Remember orchestration boundaries for this chat.".into(),
            conversation_id: Some("conv-context".into()),
            project_id: Some(jaymi.id.as_str().to_string()),
            importance: Some(75),
            confidence: Some(80),
            tags: vec![],
            source: None,
            kind: None,
                metadata_json: None,
            })
        .expect("conversation memory");

    let other_conversation = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Conversation,
            summary: "Other chat secret".into(),
            content: "other-conversation-token".into(),
            conversation_id: Some("conv-other".into()),
            project_id: None,
            importance: Some(99),
            confidence: Some(99),
            tags: vec![],
            source: None,
            kind: None,
                metadata_json: None,
            })
        .expect("other conversation memory");

    let working = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "Scratch orchestration idea".into(),
            content: "Recent work on context assembly.".into(),
            conversation_id: Some("conv-context".into()),
            project_id: None,
            importance: Some(60),
            confidence: Some(70),
            tags: vec![],
            source: None,
            kind: None,
                metadata_json: None,
            })
        .expect("working memory");

    let unrelated = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "Grocery list".into(),
            content: "buy milk and eggs".into(),
            conversation_id: None,
            project_id: None,
            importance: Some(10),
            confidence: Some(10),
            tags: vec![],
            source: None,
            kind: None,
                metadata_json: None,
            })
        .expect("unrelated working");

    // Flood the store with low-relevance noise; assembly must still honor limits.
    for index in 0..30 {
        app.store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: format!("noise-{index}"),
            content: format!("unrelated filler note {index}"),
            conversation_id: None,
            project_id: None,
            importance: Some(5),
            confidence: Some(5),
            tags: vec![],
            source: None,
            kind: None,
            metadata_json: None,
        })
        .expect("noise");
    }

    let assembled = app
        .assemble_memory_context(&AssembleContextRequest {
            text: "orchestration planner context".into(),
            conversation_id: Some("conv-context".into()),
            project_id: Some(jaymi.id.as_str().to_string()),
            limit: Some(5),
            working_limit: Some(8),
            recent_limit: Some(6),
            ..AssembleContextRequest::default()
        })
        .expect("assemble");

    assert!(assembled.len() <= 5, "must honor retrieval limit");
    assert!(assembled.truncated || assembled.candidate_count <= 5);
    assert_eq!(assembled.project_id.as_deref(), Some(jaymi.id.as_str()));
    assert_eq!(assembled.conversation_id.as_deref(), Some("conv-context"));

    let ids: Vec<_> = assembled
        .memories
        .iter()
        .map(|item| item.record.id.as_str().to_string())
        .collect();
    assert!(ids.contains(&relevant_project.id.as_str().to_string()));
    assert!(ids.contains(&conversation.id.as_str().to_string()) || ids.contains(&working.id.as_str().to_string()));
    assert!(!ids.contains(&foreign.id.as_str().to_string()));
    assert!(!ids.contains(&other_conversation.id.as_str().to_string()));
    assert!(
        !ids.contains(&unrelated.id.as_str().to_string())
            || assembled
                .memories
                .iter()
                .any(|item| item.reasons.contains(&MemoryRelevanceKind::RecentWork)
                    && item.record.id == unrelated.id),
        "unrelated grocery note should not outrank request matches"
    );

    for item in &assembled.memories {
        assert!(!item.reasons.is_empty());
        assert!(!item.why.is_empty());
        assert!(item.score > 0);
        if item.record.scope == MemoryScope::Project {
            assert_eq!(item.record.project_id.as_deref(), Some(jaymi.id.as_str()));
        }
        if item.record.scope == MemoryScope::Conversation {
            assert_eq!(item.record.conversation_id.as_deref(), Some("conv-context"));
        }
    }

    // Planner path uses assembly and never dumps the whole store.
    let planner = app.container().resolve::<Planner>().expect("planner");
    let response = planner
        .handle(UserRequest::list_directory(std::env::temp_dir()))
        .expect("handle");
    let context = response.memory_context.expect("memory context on response");
    assert!(context.len() <= 12);
    assert!(!context
        .records()
        .iter()
        .any(|record| record.id == foreign.id));
    assert!(!context
        .records()
        .iter()
        .any(|record| record.content.contains("other-conversation-token")));
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
