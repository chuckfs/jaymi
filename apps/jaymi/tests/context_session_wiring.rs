//! Context session wiring — live UI state → ContextSessionInputs → ContextBundle.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{
    DiagnosticState, ProblemIssue, ProblemSeverity, SearchResultEntry, WorkspaceKind,
};
use jaymi_context::ContextEngine;
use jaymi_core::UserRequest;

#[test]
fn prepare_pushes_coding_editor_diagnostics_and_permissions() {
    let data_dir = temp_dir("session-wiring");
    let root = data_dir.join("proj");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("main.rs");
    fs::write(&file, "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let _ = app
        .handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("open coding");
    assert_eq!(
        app.experience()
            .expect("experience")
            .active_workspace_kind(),
        Some(WorkspaceKind::Coding)
    );

    app.with_coding_state(|coding| {
        coding.open_permanent(file.to_string_lossy().as_ref(), "fn main() {}\n".into());
        coding.problems.push(ProblemIssue {
            id: "lsp:0".into(),
            severity: ProblemSeverity::Warning,
            source: "lsp".into(),
            source_label: "rust-analyzer".into(),
            path: Some(file.to_string_lossy().into_owned()),
            line: Some(0),
            column: Some(0),
            end_line: None,
            end_column: None,
            message: "unused variable".into(),
        });
        coding.search.results.push(SearchResultEntry {
            path: file.to_string_lossy().into_owned(),
            title: "main.rs".into(),
            line: Some(0),
            column: Some(0),
            end_line: None,
            end_column: None,
            preview: "fn main".into(),
            why_matched: "text".into(),
        });
        coding.diagnostics.push(DiagnosticState::simple(
            "fallback diagnostic",
            Some(file.to_string_lossy().into_owned()),
            "info",
        ));
    })
    .expect("seed coding state");

    let _ = app
        .handle(UserRequest::new("explain the open file"))
        .expect("handle");

    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    let session = context.session_inputs();

    assert_eq!(session.workspace_kind.as_deref(), Some("coding"));
    assert_eq!(
        session.current_file.path.as_deref(),
        Some(file.to_string_lossy().as_ref())
    );
    assert_eq!(session.current_file.language.as_deref(), Some("rust"));
    assert_eq!(
        session.current_selection.path.as_deref(),
        Some(file.to_string_lossy().as_ref())
    );
    assert!(
        session
            .open_files
            .files
            .iter()
            .any(|entry| entry.path == file.to_string_lossy() && entry.active),
        "open files must include the active tab"
    );
    assert!(
        session
            .diagnostics
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("unused variable")),
        "problems panel should win over raw diagnostics when present"
    );
    assert_eq!(session.search_hits.len(), 1);
    assert!(!session.permissions.entries.is_empty());
    assert!(
        session
            .permissions
            .entries
            .iter()
            .any(|entry| entry.category == "filesystem" && entry.decision == "allowed")
    );
    assert!(
        session.active_capabilities.capability_ids.is_empty(),
        "capability catalog must not be mirrored into session; Planner owns selection"
    );

    let report = app.inspect_context().expect("inspect").expect("report");
    assert!(
        report.providers.iter().any(|provider| {
            provider.id == "editor" && provider.outcome.contributed()
                || provider.id == "permission" && provider.outcome.contributed()
        }),
        "editor and/or permission providers should contribute from session; providers={:?}",
        report
            .providers
            .iter()
            .map(|p| format!("{}:{:?}", p.id, p.outcome))
            .collect::<Vec<_>>()
    );

    // Request-selected capabilities land on the bundle via AssembleHints, not session.
    let bundle = report
        .providers
        .iter()
        .any(|p| p.id == "workspace");
    let _ = bundle;
    let inspector = app.inspect_context().expect("inspect").expect("report");
    let notes = inspector.notes.join("\n");
    assert!(
        notes.contains("pipeline intent="),
        "Planner intent must drive assemble; notes={notes}"
    );
}

#[test]
fn closing_coding_clears_editor_session_fields() {
    let data_dir = temp_dir("session-clear");
    let root = data_dir.join("proj");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("lib.rs");
    fs::write(&file, "pub fn x() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let _ = app
        .handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("open coding");
    app.with_coding_state(|coding| {
        coding.open_permanent(file.to_string_lossy().as_ref(), "pub fn x() {}\n".into());
    })
    .expect("open file");
    let _ = app.handle(UserRequest::new("while coding")).expect("handle");

    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    assert!(context.session_inputs().current_file.path.is_some());

    let _ = app.close_ui_workspace().expect("close");
    // prepare_context_session runs inside close_ui_workspace.
    let session = context.session_inputs();
    assert!(session.workspace_kind.is_none());
    assert!(session.current_file.path.is_none());
    assert!(session.open_files.files.is_empty());
    assert!(session.diagnostics.diagnostics.is_empty());
    assert!(session.search_hits.is_empty());
    // Permissions remain engine-backed summaries; capabilities stay Planner-owned.
    assert!(!session.permissions.entries.is_empty());
    assert!(session.active_capabilities.capability_ids.is_empty());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-session-wiring-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
