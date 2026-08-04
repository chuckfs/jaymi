//! Integration tests for Layer 5 Slice 4 — Project Conversations.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{AppendMessageRequest, CreateConversationRequest, MessageRole};
use jaymi_planner::Planner;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn project_conversations_attach_load_and_resume_on_reopen() {
    let data_dir = temp_dir("project-conversations-data");
    let root = temp_dir("project-conversations-root");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let project_a = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:alpha".into()),
            name: "Alpha".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create alpha");
    let project_b = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:beta".into()),
            name: "Beta".into(),
            description: None,
            root_directory: None,
            project_type: Some(ProjectType::General),
        })
        .expect("create beta");

    // Explicit attachment to exactly one project.
    let conversation = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-alpha".into()),
            title: Some("Alpha architecture chat".into()),
            project_id: Some(project_a.id.as_str().to_string()),
        })
        .expect("create conversation");
    assert_eq!(
        conversation.project_id.as_deref(),
        Some(project_a.id.as_str())
    );

    app.append_message(&AppendMessageRequest {
        conversation_id: conversation.id.as_str().to_string(),
        role: MessageRole::User,
        content: "Remember the layered planner design.".into(),
        created_at: Some(1_700_000_100),
        attachments: vec![],
        references: vec![],
    })
    .expect("user message");
    app.append_message(&AppendMessageRequest {
        conversation_id: conversation.id.as_str().to_string(),
        role: MessageRole::Assistant,
        content: "Noted — Planner requests one ProjectContext.".into(),
        created_at: Some(1_700_000_110),
        attachments: vec![],
        references: vec![],
    })
    .expect("assistant message");

    let listed = app
        .list_project_conversations(project_a.id.as_str())
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, conversation.id);
    assert!(app
        .list_project_conversations(project_b.id.as_str())
        .expect("list b")
        .is_empty());

    // Reopen loads conversation history into project context.
    let context = app
        .open_project(project_a.id.as_str())
        .expect("open loads history");
    assert_eq!(context.conversations.len(), 1);
    assert_eq!(
        context.conversations[0].conversation_id,
        conversation.id.as_str()
    );
    assert_eq!(context.conversations[0].message_count, 2);
    assert!(context.conversations[0]
        .messages
        .iter()
        .any(|message| message.content.contains("layered planner")));
    assert_eq!(
        app.active_conversation_id().as_deref(),
        Some(conversation.id.as_str()),
        "opening a project with no prior active conversation resumes its latest chat"
    );

    // Creating while a project is active auto-attaches.
    let auto = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-auto".into()),
            title: Some("Auto attached".into()),
            project_id: None,
        })
        .expect("auto attach");
    assert_eq!(auto.project_id.as_deref(), Some(project_a.id.as_str()));

    // Reassign to another project — exactly one owner.
    let moved = app
        .attach_conversation_to_project(conversation.id.as_str(), Some(project_b.id.as_str()))
        .expect("move");
    assert_eq!(moved.project_id.as_deref(), Some(project_b.id.as_str()));
    assert!(app
        .list_project_conversations(project_a.id.as_str())
        .expect("after move a")
        .iter()
        .all(|meta| meta.id.as_str() != conversation.id.as_str()));
    assert!(app
        .list_project_conversations(project_b.id.as_str())
        .expect("after move b")
        .iter()
        .any(|meta| meta.id == conversation.id));

    // Detach → global.
    let detached = app
        .attach_conversation_to_project(conversation.id.as_str(), None)
        .expect("detach");
    assert!(detached.project_id.is_none());
    let loaded = app
        .load_conversation(conversation.id.as_str())
        .expect("load")
        .expect("still exists");
    assert_eq!(loaded.messages.len(), 2);

    // Re-attach and continue — Planner resumes previous discussion.
    app.attach_conversation_to_project(conversation.id.as_str(), Some(project_a.id.as_str()))
        .expect("re-attach");
    app.set_active_conversation(None).expect("clear active");

    let planner = app.container().resolve::<Planner>().expect("planner");
    let resumed = planner
        .handle(UserRequest::new("Continue working on Alpha."))
        .expect("continue");
    let restored = resumed.project_context.expect("context");
    assert_eq!(restored.project.id, project_a.id);
    assert!(
        restored
            .conversations
            .iter()
            .any(|entry| entry.conversation_id == conversation.id.as_str()
                && entry.message_count == 2)
    );
    assert_eq!(
        app.active_conversation_id().as_deref(),
        Some(conversation.id.as_str()),
        "continuing a project retrieves its previous conversation"
    );
    assert!(resumed.content.contains("conversations="));
    assert!(resumed.content.contains("conversation_messages="));
}

#[test]
fn global_conversation_created_without_active_project_stays_unattached() {
    let data_dir = temp_dir("global-conversation-data");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let conversation = app
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-global".into()),
            title: Some("Global chat".into()),
            project_id: None,
        })
        .expect("create global");
    assert!(conversation.project_id.is_none());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-project-conversations-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
