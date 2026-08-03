//! Coding Workspace activation from the conversation action menu.
//!
//! Proves the same Application path the UI ⋯ menu uses: open Coding beside the
//! conversation, then close without destroying chat history.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{CapabilityState, WorkspaceKind, WorkspacePanel};
use jaymi_memory::MessageRole;

#[test]
fn start_coding_project_opens_shell_without_replacing_conversation() {
    let data_dir = temp_dir("coding-activation");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    // Seed the durable conversation before opening Coding.
    app.record_user_message("Remember this before coding.")
        .expect("user turn");
    app.record_user_message("Still in the same chat.")
        .expect("second user turn");

    let before = app.experience().expect("experience before");
    assert!(!before.workspace_expanded());
    assert_eq!(before.turn_count(), 2);
    assert_eq!(before.conversation()[0].role, MessageRole::User);
    assert!(before.conversation()[0]
        .content
        .contains("Remember this before coding."));
    let turns_before = before.turn_count();
    let history_before: Vec<_> = before
        .conversation()
        .iter()
        .map(|turn| (turn.role, turn.content.clone()))
        .collect();

    // Same API the conversation ⋯ menu calls.
    app.start_coding_project().expect("start coding project");

    let open = app.experience().expect("experience while coding");
    assert!(open.workspace_expanded(), "Coding Workspace must open");
    assert_eq!(open.active_workspace_kind(), Some(WorkspaceKind::Coding));
    assert_eq!(
        open.turn_count(),
        turns_before,
        "opening Coding must not create a second conversation or clear turns"
    );
    let history_open: Vec<_> = open
        .conversation()
        .iter()
        .map(|turn| (turn.role, turn.content.clone()))
        .collect();
    assert_eq!(history_open, history_before, "conversation remains mounted");

    let panels = open.active_panels();
    for expected in [
        WorkspacePanel::ProjectExplorer,
        WorkspacePanel::Editor,
        WorkspacePanel::Terminal,
        WorkspacePanel::Git,
        WorkspacePanel::Diagnostics,
    ] {
        assert!(panels.contains(&expected), "missing panel {expected:?}");
    }

    let state = app
        .capability_state()
        .expect("state")
        .expect("coding state allocated");
    assert!(matches!(state, CapabilityState::Coding(_)));
    assert_eq!(state.workspace_kind(), WorkspaceKind::Coding);

    // Close returns to the exact same conversation.
    let closed = app.close_ui_workspace().expect("close").expect("was open");
    assert_eq!(closed.kind, WorkspaceKind::Coding);

    let after = app.experience().expect("experience after close");
    assert!(!after.workspace_expanded());
    assert!(after.active_workspace().is_none());
    assert!(app.capability_state().expect("state").is_none());
    assert_eq!(after.turn_count(), turns_before);
    let history_after: Vec<_> = after
        .conversation()
        .iter()
        .map(|turn| (turn.role, turn.content.clone()))
        .collect();
    assert_eq!(
        history_after, history_before,
        "conversation history must be preserved after close"
    );
}

#[test]
fn conversation_action_workspaces_do_not_spawn_new_chats() {
    let data_dir = temp_dir("coding-activation-switch");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.record_user_message("Anchor turn").expect("seed");

    app.start_coding_project().expect("coding");
    assert_eq!(app.experience().expect("e").turn_count(), 1);
    assert_eq!(
        app.active_ui_workspace().expect("active"),
        Some(WorkspaceKind::Coding)
    );

    app.start_research_workspace().expect("research");
    assert_eq!(app.experience().expect("e").turn_count(), 1);
    assert_eq!(
        app.active_ui_workspace().expect("active"),
        Some(WorkspaceKind::Research)
    );

    app.start_creation_workspace().expect("creation");
    assert_eq!(app.experience().expect("e").turn_count(), 1);
    assert_eq!(
        app.active_ui_workspace().expect("active"),
        Some(WorkspaceKind::Creation)
    );

    app.close_ui_workspace().expect("close");
    let session = app.experience().expect("final");
    assert_eq!(session.turn_count(), 1);
    assert!(session.conversation()[0].content.contains("Anchor turn"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-activation-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
