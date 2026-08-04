//! Integration tests for Layer 4 Slice 2 — Conversation Memory.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_memory::{
    AppendMessageRequest, ConversationAttachmentInput, ConversationReferenceInput,
    CreateConversationRequest, MemoryQuery, MemoryScope, MessageRole, StoreMemoryRequest,
};

#[test]
fn conversations_reopen_exactly_as_stored_and_stay_isolated() {
    let data_dir = temp_dir("conversation-memory");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let meta = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-reopen".into()),
            title: Some("Exact reopen".into()),
            project_id: None,
        })
        .expect("create");
    assert_eq!(meta.title.as_deref(), Some("Exact reopen"));

    let user = app
        .append_message(&AppendMessageRequest {
            conversation_id: "conv-reopen".into(),
            role: MessageRole::User,
            content: "Please keep this note with an attachment.".into(),
            created_at: Some(1_700_000_000),
            attachments: vec![ConversationAttachmentInput {
                kind: "file".into(),
                name: Some("brief.pdf".into()),
                uri: Some("/docs/brief.pdf".into()),
                mime_type: Some("application/pdf".into()),
                size_bytes: Some(2048),
                metadata_json: Some(r#"{"pages":2}"#.into()),
            }],
            references: vec![ConversationReferenceInput {
                kind: "citation".into(),
                target_id: Some("content:brief".into()),
                label: Some("brief.pdf".into()),
                uri: Some("file:///docs/brief.pdf".into()),
                metadata_json: None,
            }],
        })
        .expect("append user");

    let assistant = app
        .append_message(&AppendMessageRequest {
            conversation_id: "conv-reopen".into(),
            role: MessageRole::Assistant,
            content: "Noted — attachment and citation stored.".into(),
            created_at: Some(1_700_000_010),
            attachments: vec![],
            references: vec![ConversationReferenceInput {
                kind: "tool".into(),
                target_id: Some("search_knowledge".into()),
                label: Some("search".into()),
                uri: None,
                metadata_json: None,
            }],
        })
        .expect("append assistant");

    // Reopen through Planner → Memory Engine (not Database).
    let loaded = app
        .load_conversation("conv-reopen")
        .expect("load")
        .expect("conversation exists");
    assert_eq!(loaded.meta.id.as_str(), "conv-reopen");
    assert_eq!(loaded.meta.title.as_deref(), Some("Exact reopen"));
    assert_eq!(loaded.messages.len(), 2);

    assert_eq!(loaded.messages[0].id, user.id);
    assert_eq!(loaded.messages[0].role, MessageRole::User);
    assert_eq!(
        loaded.messages[0].content,
        "Please keep this note with an attachment."
    );
    assert_eq!(loaded.messages[0].created_at, 1_700_000_000);
    assert_eq!(loaded.messages[0].sequence_no, 0);
    assert_eq!(loaded.messages[0].attachments.len(), 1);
    assert_eq!(
        loaded.messages[0].attachments[0].name.as_deref(),
        Some("brief.pdf")
    );
    assert_eq!(
        loaded.messages[0].attachments[0].uri.as_deref(),
        Some("/docs/brief.pdf")
    );
    assert_eq!(loaded.messages[0].attachments[0].size_bytes, Some(2048));
    assert_eq!(loaded.messages[0].references.len(), 1);
    assert_eq!(
        loaded.messages[0].references[0].target_id.as_deref(),
        Some("content:brief")
    );

    assert_eq!(loaded.messages[1].id, assistant.id);
    assert_eq!(loaded.messages[1].role, MessageRole::Assistant);
    assert_eq!(
        loaded.messages[1].content,
        "Noted — attachment and citation stored."
    );
    assert_eq!(loaded.messages[1].created_at, 1_700_000_010);
    assert_eq!(loaded.messages[1].sequence_no, 1);
    assert_eq!(loaded.messages[1].references.len(), 1);

    // Survive process restart (new Application, same data dir).
    drop(app);
    let app = Application::boot_with_data_dir(&data_dir).expect("reboot");
    let reopened = app
        .load_conversation("conv-reopen")
        .expect("reload")
        .expect("still present");
    assert_eq!(reopened, loaded);

    // Isolation: conversation memory does not influence other conversations / global retrieve.
    app.store_memory(&StoreMemoryRequest {
        scope: MemoryScope::Conversation,
        summary: "Private to conv-reopen".into(),
        content: "secret-conversation-token".into(),
        conversation_id: Some("conv-reopen".into()),
        project_id: None,
        importance: Some(99),
        confidence: Some(99),
        tags: vec![],
        source: None,
        kind: None,
        metadata_json: None,
    })
    .expect("store conversation memory");

    let other = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-other".into()),
            title: Some("Other".into()),
            project_id: None,
        })
        .expect("other conversation");
    assert_eq!(other.id.as_str(), "conv-other");

    let global = app
        .retrieve_memory(&MemoryQuery {
            text: Some("secret-conversation-token".into()),
            ..MemoryQuery::default()
        })
        .expect("global retrieve");
    assert!(
        global.is_empty(),
        "conversation memory must not influence future/global retrieve"
    );

    let other_scoped = app
        .retrieve_memory(&MemoryQuery {
            text: Some("secret-conversation-token".into()),
            conversation_id: Some("conv-other".into()),
            ..MemoryQuery::default()
        })
        .expect("other scoped");
    assert!(other_scoped.is_empty());

    let own = app
        .retrieve_memory(&MemoryQuery {
            text: Some("secret-conversation-token".into()),
            conversation_id: Some("conv-reopen".into()),
            ..MemoryQuery::default()
        })
        .expect("own scoped");
    assert_eq!(own.len(), 1);
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
