//! Layer 6 Slice 7 — Coding Workspace Shell.
//!
//! The Coding expansion materializes five shell panels beside conversation.
//! Coding Workspace shell — Explorer + Editor + Terminal bound to CodingState.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{coding_shell_summary, Application};
use jaymi_capabilities::{
    DiagnosticState, ExplorerNode, ExplorerStatus, GitStatusState, ProblemIssue, ProblemSeverity,
    TerminalSessionState, WorkspaceKind, WorkspacePanel,
};
use jaymi_core::UserRequest;
use jaymi_memory::MessageRole;
use std::collections::BTreeSet;

#[test]
fn coding_workspace_shell_materializes_five_panels_beside_conversation() {
    let data_dir = temp_dir("coding-shell");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let response = app
        .handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("coding request");
    assert_eq!(
        response.workspace.as_ref().map(|workspace| workspace.kind),
        Some(WorkspaceKind::Coding)
    );

    let session = app.experience().expect("experience");
    assert!(session.workspace_expanded());
    assert_eq!(session.active_workspace_kind(), Some(WorkspaceKind::Coding));
    assert!(session.turn_count() >= 2);
    assert_eq!(session.conversation()[0].role, MessageRole::User);

    let panels = session.active_panels();
    for expected in [
        WorkspacePanel::ProjectExplorer,
        WorkspacePanel::Editor,
        WorkspacePanel::Terminal,
        WorkspacePanel::Git,
        WorkspacePanel::Diagnostics,
    ] {
        assert!(
            panels.contains(&expected),
            "missing coding panel {expected:?}; got {panels:?}"
        );
    }

    assert!(
        session.turn_count() >= 2,
        "conversation must stay visible/populated while Coding is open"
    );
}

#[test]
fn coding_shell_reflects_state_and_clears_on_close() {
    let data_dir = temp_dir("coding-shell-state");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    app.handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("coding");

    app.with_coding_state(|coding| {
        coding.explorer.selected_path = Some("/tmp/project/src/lib.rs".into());
        coding.explorer.project_root = Some("/tmp/project".into());
        coding.explorer.status = ExplorerStatus::Ready;
        coding.explorer.nodes = vec![
            ExplorerNode {
                name: "src".into(),
                path: "/tmp/project/src".into(),
                is_dir: true,
                children: vec![ExplorerNode {
                    name: "lib.rs".into(),
                    path: "/tmp/project/src/lib.rs".into(),
                    is_dir: false,
                    children: Vec::new(),
                }],
            },
            ExplorerNode {
                name: "Cargo.toml".into(),
                path: "/tmp/project/Cargo.toml".into(),
                is_dir: false,
                children: Vec::new(),
            },
        ];
        coding.explorer.expanded_paths = BTreeSet::from(["/tmp/project/src".into()]);
        coding.upsert_tab(
            "/tmp/project/src/lib.rs",
            "lib.rs",
            "fn x() {}".into(),
            0.0,
        );
        coding.terminal_sessions.push(TerminalSessionState {
            id: "term-1".into(),
            title: "Terminal".into(),
            cwd: Some("/tmp/project".into()),
            last_command: Some("cargo test".into()),
            output: "$ cargo test\n".into(),
            history: vec!["cargo test".into()],
            input: String::new(),
            history_index: None,
            scroll_offset: 0.0,
        });
        coding.git = Some(GitStatusState {
            is_repository: true,
            branch: Some("feature/shell".into()),
            summary: "1 modified".into(),
            ..GitStatusState::default()
        });
        coding.diagnostics.push(DiagnosticState::simple(
            "missing semicolon",
            Some("/tmp/project/src/lib.rs".into()),
            "error",
        ));
        coding.problems.push(ProblemIssue {
            id: "lsp:0".into(),
            severity: ProblemSeverity::Error,
            source: "lsp".into(),
            source_label: "rust-analyzer".into(),
            path: Some("/tmp/project/src/lib.rs".into()),
            line: Some(0),
            column: Some(0),
            end_line: Some(0),
            end_column: Some(1),
            message: "missing semicolon".into(),
        });
    })
    .expect("populate coding state");

    let state = app
        .capability_state()
        .expect("state")
        .expect("coding")
        .coding()
        .expect("coding borrow")
        .clone();
    let summary = coding_shell_summary(&state, None);
    assert!(summary.contains("## Project Explorer"));
    assert!(summary.contains("## Editor"));
    assert!(summary.contains("## Terminal"));
    assert!(summary.contains("## Git"));
    assert!(summary.contains("## Diagnostics"));
    assert!(summary.contains("lib.rs"));
    assert!(summary.contains("cargo test"));
    assert!(summary.contains("branch=feature/shell"));
    assert!(summary.contains("missing semicolon"));
    assert!(!summary.contains("stub explorer"));
    assert!(state.entry_count() >= 5);

    let turns = app.experience().expect("experience").turn_count();
    app.close_ui_workspace().expect("close").expect("was open");

    assert!(app.capability_state().expect("state").is_none());
    assert!(!app.experience().expect("experience").workspace_expanded());
    assert_eq!(
        app.experience().expect("experience").turn_count(),
        turns,
        "closing the Coding shell must keep conversation intact"
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-shell-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
