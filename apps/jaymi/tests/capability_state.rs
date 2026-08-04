//! Integration tests for Layer 6 Slice 5 — Capability State.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{
    workspace_expansion_for, Capability, CapabilityState, ResearchNoteState, ResearchSourceState,
    TerminalSessionState, WorkspaceKind,
};
use jaymi_core::UserRequest;

#[test]
fn coding_state_is_independent_and_cleared_on_close() {
    let data_dir = temp_dir("coding-state");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    app.handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("coding request");

    let state = app
        .capability_state()
        .expect("state")
        .expect("coding state");
    assert_eq!(state.workspace_kind(), WorkspaceKind::Coding);
    assert_eq!(state.entry_count(), 0);

    app.with_coding_state(|coding| {
        coding.upsert_tab("src/main.rs", "main.rs", "fn main() {}".into(), 0.0);
        coding.terminal_sessions.push(TerminalSessionState {
            id: "term-1".into(),
            title: "Terminal".into(),
            cwd: Some("/tmp/app".into()),
            last_command: Some("cargo check".into()),
            output: String::new(),
            history: vec!["cargo check".into()],
            input: String::new(),
            history_index: None,
            scroll_offset: 0.0,
        });
    })
    .expect("mutate coding");

    let state = app.capability_state().expect("state").expect("populated");
    assert_eq!(state.entry_count(), 3);
    assert_eq!(
        state.coding().expect("coding").open_files()[0].path,
        "src/main.rs"
    );

    // Research mutation must fail while Coding is active (isolation).
    let research_err = app.with_research_state(|research| {
        research.notes.push(ResearchNoteState {
            id: "n1".into(),
            content: "should not land".into(),
        });
    });
    assert!(research_err.is_err());

    let turns_before_close = app.experience().expect("experience").turn_count();
    app.close_ui_workspace().expect("close").expect("was open");

    assert!(app.capability_state().expect("state").is_none());
    assert_eq!(
        app.experience().expect("experience").turn_count(),
        turns_before_close,
        "conversation survives workspace close"
    );
}

#[test]
fn switching_workspace_kinds_isolates_capability_state() {
    let data_dir = temp_dir("state-isolation");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    app.handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("coding");
    app.with_coding_state(|coding| {
        coding.upsert_tab("lib.rs", "lib.rs", String::new(), 0.0);
    })
    .expect("coding mutate");
    assert_eq!(
        app.capability_state()
            .expect("state")
            .expect("coding")
            .entry_count(),
        2
    );

    // Switch to Research — coding state must be replaced, not merged.
    app.handle_with_workspace(UserRequest::new("search fungi biology notes"))
        .expect("research");
    let state = app
        .capability_state()
        .expect("state")
        .expect("research state");
    assert_eq!(state.workspace_kind(), WorkspaceKind::Research);
    assert_eq!(state.entry_count(), 0);
    assert!(state.coding().is_none());

    app.with_research_state(|research| {
        research.sources.push(ResearchSourceState {
            id: "src-1".into(),
            title: "Fungi notes".into(),
            uri: Some("notes/fungi.md".into()),
        });
        research.notes.push(ResearchNoteState {
            id: "note-1".into(),
            content: "Basidiomycota overview".into(),
        });
    })
    .expect("research mutate");

    // Coding mutation must fail while Research is active.
    assert!(app
        .with_coding_state(|coding| {
            coding.upsert_tab("should-not-exist", "should-not-exist", String::new(), 0.0);
        })
        .is_err());

    assert_eq!(
        app.capability_state()
            .expect("state")
            .expect("research")
            .entry_count(),
        2
    );

    // Creation is a third isolated kind.
    let creation = workspace_expansion_for(Capability::GenerateImages, "concept art")
        .expect("creation expansion");
    app.expand_ui_workspace(creation).expect("expand creation");
    let state = app
        .capability_state()
        .expect("state")
        .expect("creation state");
    assert_eq!(state.workspace_kind(), WorkspaceKind::Creation);
    assert_eq!(state.entry_count(), 0);
    assert!(state.research().is_none());
}

#[test]
fn promoted_entries_survive_workspace_close() {
    let data_dir = temp_dir("promote-state");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    app.handle_with_workspace(UserRequest::new("search fungi biology notes"))
        .expect("research");
    app.with_research_state(|research| {
        research.notes.push(ResearchNoteState {
            id: "note-keep".into(),
            content: "Keep this finding about mycelium.".into(),
        });
    })
    .expect("mutate");

    let summary = app.promote_capability_entry("note-keep").expect("promote");
    assert!(summary.contains("mycelium"));

    let experience = app.experience().expect("experience");
    assert_eq!(experience.promoted_entries().len(), 1);
    let turns_with_promotion = experience.turn_count();

    app.close_ui_workspace().expect("close").expect("was open");

    let experience = app.experience().expect("after close");
    assert!(experience.capability_state().is_none());
    assert_eq!(experience.promoted_entries().len(), 1);
    assert_eq!(experience.turn_count(), turns_with_promotion);
    assert!(experience
        .conversation()
        .iter()
        .any(|turn| turn.content.contains("mycelium")));
}

#[test]
fn empty_capability_state_matches_workspace_kind() {
    assert!(matches!(
        CapabilityState::empty_for(WorkspaceKind::Coding),
        Some(CapabilityState::Coding(_))
    ));
    assert!(matches!(
        CapabilityState::empty_for(WorkspaceKind::Creation),
        Some(CapabilityState::Creation(_))
    ));
    assert!(matches!(
        CapabilityState::empty_for(WorkspaceKind::Research),
        Some(CapabilityState::Research(_))
    ));
    assert!(CapabilityState::empty_for(WorkspaceKind::Conversation).is_none());
    assert_eq!(
        CapabilityState::empty_for_capability(Capability::Code)
            .expect("code")
            .workspace_kind(),
        WorkspaceKind::Coding
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-capability-state-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
