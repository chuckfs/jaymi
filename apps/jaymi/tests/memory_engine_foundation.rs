//! Integration tests for Layer 4 Slice 1 — Memory Engine Foundation.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_memory::{
    ArchiveConversationRequest, MemoryEngine, MemoryEngineApi, MemoryQuery, MemoryScope,
    PromoteMemoryRequest, StoreMemoryRequest,
};

#[test]
fn memory_engine_stores_and_retrieves_through_planner() {
    let data_dir = temp_dir("memory-foundation");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let stored = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Personal,
            summary: "Prefers concise answers".into(),
            content: "User prefers concise technical answers without filler.".into(),
            conversation_id: None,
            project_id: None,
            importance: Some(80),
            confidence: Some(90),
            tags: vec!["preference".into()],
            source: Some("test".into()),
            kind: None,
            metadata_json: None,
        })
        .expect("store");
    assert_eq!(stored.scope, MemoryScope::Personal);
    assert_eq!(stored.summary, "Prefers concise answers");

    let working = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Working,
            summary: "Scratch note".into(),
            content: "Ephemeral working scratch for this turn.".into(),
            conversation_id: Some("conv-1".into()),
            project_id: None,
            importance: Some(20),
            confidence: Some(40),
            tags: vec![],
            source: None,
            kind: None,
            metadata_json: None,
        })
        .expect("store working");
    assert_eq!(working.scope, MemoryScope::Working);

    let project = app
        .store_memory(&StoreMemoryRequest {
            scope: MemoryScope::Project,
            summary: "Use SQLite for memory".into(),
            content: "Jaymi stores intentional memory in SQLite tables.".into(),
            conversation_id: Some("conv-1".into()),
            project_id: Some("proj-jaymi".into()),
            importance: Some(70),
            confidence: Some(85),
            tags: vec!["architecture".into()],
            source: Some("decision".into()),
            kind: None,
            metadata_json: None,
        })
        .expect("store project");

    let hits = app
        .retrieve_memory(&MemoryQuery {
            text: Some("concise".into()),
            scope: Some(MemoryScope::Personal),
            limit: Some(10),
            ..MemoryQuery::default()
        })
        .expect("retrieve");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, stored.id);

    let project_hits = app
        .retrieve_memory(&MemoryQuery {
            project_id: Some("proj-jaymi".into()),
            scope: Some(MemoryScope::Project),
            ..MemoryQuery::default()
        })
        .expect("project retrieve");
    assert_eq!(project_hits.len(), 1);
    assert_eq!(project_hits[0].id, project.id);

    let promoted = app
        .promote_memory(&PromoteMemoryRequest {
            memory_id: working.id.as_str().to_string(),
            to: MemoryScope::Conversation,
            conversation_id: Some("conv-1".into()),
            project_id: None,
            kind: None,
        })
        .expect("promote");
    assert_eq!(promoted.scope, MemoryScope::Conversation);

    app.forget_memory(working.id.as_str()).expect("forget");
    let after_forget = app
        .retrieve_memory(&MemoryQuery {
            text: Some("scratch".into()),
            include_archived: true,
            ..MemoryQuery::default()
        })
        .expect("retrieve after forget");
    assert!(after_forget.iter().all(|record| record.id != working.id));

    let archive = app
        .archive_conversation(&ArchiveConversationRequest {
            conversation_id: "conv-1".into(),
            title: Some("Architecture chat".into()),
            content: "Discussed SQLite-backed memory scopes.".into(),
            promote_summary: true,
            summary: Some("Archived architecture discussion".into()),
        })
        .expect("archive");
    assert_eq!(archive.conversation_id, "conv-1");
    assert!(archive.promoted_memory_id.is_some());

    let conversation_hits = app
        .retrieve_memory(&MemoryQuery {
            text: Some("architecture discussion".into()),
            scope: Some(MemoryScope::Conversation),
            conversation_id: Some("conv-1".into()),
            ..MemoryQuery::default()
        })
        .expect("conversation retrieve");
    assert!(!conversation_hits.is_empty());

    // Planner path uses Memory Engine (not Database / Knowledge Store).
    let engine = app
        .container()
        .resolve::<Arc<MemoryEngine>>()
        .expect("memory engine");
    let stats = engine.stats().expect("stats");
    assert!(stats.active_total >= 3);

    let health = engine.health().expect("health");
    assert!(health.healthy);
    assert!(health.detail.contains("active="));

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot
        .subsystem("Memory Status")
        .expect("memory status row");
    assert_eq!(row.status, OperationalStatus::Operational);
    assert!(row.detail.contains("active="));

    // Direct MemoryEngineApi surface matches Planner façade.
    let via_api: &dyn MemoryEngineApi = engine.as_ref();
    let again = via_api
        .retrieve(&MemoryQuery {
            text: Some("concise".into()),
            scope: Some(MemoryScope::Personal),
            ..MemoryQuery::default()
        })
        .expect("api retrieve");
    assert_eq!(again.len(), 1);
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
