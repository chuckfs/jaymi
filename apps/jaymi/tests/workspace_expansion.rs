//! Integration tests for Layer 6 Slice 4 — Workspace Expansion.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{Capability, WorkspaceKind, WorkspacePanel};
use jaymi_core::UserRequest;
use jaymi_memory::MessageRole;

#[test]
fn capabilities_expand_and_close_workspace_without_destroying_conversation() {
    let data_dir = temp_dir("workspace-expansion");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    // Conversation starts empty and conversation-only.
    let session = app.experience().expect("experience");
    assert_eq!(session.turn_count(), 0);
    assert!(session.active_workspace().is_none());
    assert!(!session.workspace_expanded());

    let coding = app
        .handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("coding request");
    assert_eq!(coding.capability, Some(Capability::Code));
    let workspace = coding.workspace.expect("coding workspace");
    assert_eq!(workspace.kind, WorkspaceKind::Coding);
    assert_eq!(
        workspace.expands_from,
        jaymi_capabilities::WorkspaceEdge::Right
    );
    assert!(workspace.panels.contains(&WorkspacePanel::Editor));
    assert!(workspace.panels.contains(&WorkspacePanel::Terminal));

    let session = app.experience().expect("experience after coding");
    assert!(session.workspace_expanded());
    assert_eq!(session.active_workspace_kind(), Some(WorkspaceKind::Coding));
    assert!(session.turn_count() >= 2);
    assert_eq!(session.conversation()[0].role, MessageRole::User);
    assert!(session.conversation()[0]
        .content
        .contains("Help me build an app."));
    let turns_while_open = session.turn_count();

    // Closing the workspace must keep the conversation exactly as it was.
    let closed = app.close_ui_workspace().expect("close").expect("was open");
    assert_eq!(closed.kind, WorkspaceKind::Coding);
    let session = app.experience().expect("experience after close");
    assert!(!session.workspace_expanded());
    assert!(session.active_workspace().is_none());
    assert_eq!(session.turn_count(), turns_while_open);
    assert_eq!(
        app.active_ui_workspace().expect("active"),
        None,
        "UI returns to conversation-only"
    );

    // Research capability expands a different workspace; conversation grows.
    let research = app
        .handle_with_workspace(UserRequest::new("search fungi biology notes"))
        .expect("research request");
    assert_eq!(research.capability, Some(Capability::Search));
    let research_workspace = research.workspace.expect("research workspace");
    assert_eq!(research_workspace.kind, WorkspaceKind::Research);
    assert!(research_workspace
        .panels
        .contains(&WorkspacePanel::Citations));
    assert!(research_workspace.panels.contains(&WorkspacePanel::Search));

    let session = app.experience().expect("experience after research");
    assert_eq!(
        session.active_workspace_kind(),
        Some(WorkspaceKind::Research)
    );
    assert!(session.turn_count() > turns_while_open);

    // Creation mapping stays available even when the capability is not registered.
    let creation = jaymi_capabilities::workspace_expansion_for(
        Capability::GenerateImages,
        "create concept art",
    )
    .expect("creation expansion");
    assert_eq!(creation.kind, WorkspaceKind::Creation);
    assert!(creation.panels.contains(&WorkspacePanel::Canvas));
}

#[test]
fn capability_workspace_mapping_is_stable() {
    assert_eq!(
        jaymi_capabilities::capability_workspace(Capability::Code),
        Some(WorkspaceKind::Coding)
    );
    assert_eq!(
        jaymi_capabilities::capability_workspace(Capability::GenerateImages),
        Some(WorkspaceKind::Creation)
    );
    assert_eq!(
        jaymi_capabilities::capability_workspace(Capability::Search),
        Some(WorkspaceKind::Research)
    );
    assert_eq!(
        jaymi_capabilities::capability_workspace(Capability::Chat),
        None
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-workspace-expansion-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
