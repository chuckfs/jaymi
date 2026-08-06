//! Sprint B1.13.4 — conversational generation uses the same context preparation
//! path as every other Planner request.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, BeginGeneration, PumpGeneration};
use jaymi_capabilities::{ProblemIssue, ProblemSeverity, WorkspaceKind};
use jaymi_context::ContextEngine;
use jaymi_core::UserRequest;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-b1134-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn drain_generation(app: &Arc<Application>) {
    // Background start must finish prepare/assemble (or soft-complete) before we
    // assert session inputs. Poll with a short sleep so Empty Starting doesn't
    // spin-cancel before the worker runs.
    for _ in 0..200 {
        match app.pump_generation(8).unwrap() {
            PumpGeneration::Finished(_) | PumpGeneration::Idle => return,
            PumpGeneration::Active { .. } => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    if app.generation_active() {
        let _ = app.cancel_generation();
        for _ in 0..64 {
            match app.pump_generation(8).unwrap() {
                PumpGeneration::Finished(_) | PumpGeneration::Idle => break,
                PumpGeneration::Active { .. } => {}
            }
        }
    }
}

#[test]
fn conversation_preparation_pushes_session_before_generation() {
    let data_dir = temp_dir("conversation-prep");
    let app = Arc::new(Application::boot_with_data_dir(&data_dir).expect("boot"));

    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    // Clear any boot-time session noise by running a no-op handle first? Not required —
    // begin_generation must push a complete snapshot itself.
    let _ = app.begin_generation("What is ownership?");
    drain_generation(&app);

    let session = context.session_inputs();
    // Permissions are always filled by prepare_context_session.
    assert!(
        !session.permissions.entries.is_empty(),
        "conversation must run prepare_context_session (permissions present)"
    );
    assert!(
        session.active_capabilities.capability_ids.is_empty(),
        "capabilities remain Planner-owned via AssembleHints, not session prep"
    );
}

#[test]
fn workspace_preparation_enriches_conversation_session() {
    let data_dir = temp_dir("workspace-prep");
    let root = data_dir.join("proj");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("main.rs");
    fs::write(&file, "fn main() {}\n").unwrap();

    let app = Arc::new(Application::boot_with_data_dir(&data_dir).expect("boot"));
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
    })
    .expect("seed coding");

    match app.begin_generation("explain the open file").unwrap() {
        BeginGeneration::Started | BeginGeneration::Completed(_) => {}
    }
    drain_generation(&app);

    let session = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context")
        .session_inputs();
    assert_eq!(session.workspace_kind.as_deref(), Some("coding"));
    assert_eq!(
        session.current_file.path.as_deref(),
        Some(file.to_string_lossy().as_ref())
    );
    assert!(
        session
            .diagnostics
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("unused variable")),
        "coding problems must enrich conversational prepare"
    );
}

#[test]
fn session_reuse_stays_consistent_across_conversation_and_handle() {
    let data_dir = temp_dir("session-reuse");
    let root = data_dir.join("proj");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("lib.rs");
    fs::write(&file, "pub fn x() {}\n").unwrap();

    let app = Arc::new(Application::boot_with_data_dir(&data_dir).expect("boot"));
    let _ = app
        .handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("open coding");
    app.with_coding_state(|coding| {
        coding.open_permanent(file.to_string_lossy().as_ref(), "pub fn x() {}\n".into());
    })
    .expect("open file");

    let _ = app.begin_generation("first conversational turn");
    drain_generation(&app);
    let after_conversation = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context")
        .session_inputs();

    let _ = app
        .handle(UserRequest::new("second tool-or-chat turn"))
        .expect("handle");
    let after_handle = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context")
        .session_inputs();

    assert_eq!(
        after_conversation.workspace_kind,
        after_handle.workspace_kind
    );
    assert_eq!(
        after_conversation.current_file.path,
        after_handle.current_file.path
    );
    assert_eq!(
        after_conversation.open_files.files.len(),
        after_handle.open_files.files.len()
    );
    assert_eq!(
        after_conversation.permissions.entries.len(),
        after_handle.permissions.entries.len()
    );
}

#[test]
fn streaming_path_also_prepares_context_session() {
    let data_dir = temp_dir("streaming-prep");
    let app = Arc::new(Application::boot_with_data_dir(&data_dir).expect("boot"));
    let _ = app
        .handle_streaming_with_workspace(UserRequest::new("hello from streaming"))
        .expect("stream");

    let session = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context")
        .session_inputs();
    assert!(
        !session.permissions.entries.is_empty(),
        "handle_streaming_with_workspace must call prepare_context_session"
    );
}

#[test]
fn context_consistency_no_alternate_builder() {
    // build_context_session_inputs is the only session assembler; both paths
    // must produce identical snapshots for identical host state.
    let data_dir = temp_dir("consistency");
    let app = Arc::new(Application::boot_with_data_dir(&data_dir).expect("boot"));

    let _ = app.begin_generation("prep A");
    drain_generation(&app);
    let via_conversation = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context")
        .session_inputs();

    let _ = app.handle(UserRequest::new("prep B")).expect("handle");
    let via_handle = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context")
        .session_inputs();

    assert_eq!(via_conversation.workspace_kind, via_handle.workspace_kind);
    assert_eq!(
        via_conversation.project_open,
        via_handle.project_open
    );
    assert_eq!(
        via_conversation.permissions.entries.len(),
        via_handle.permissions.entries.len()
    );
    assert!(via_conversation
        .active_capabilities
        .capability_ids
        .is_empty());
    assert!(via_handle.active_capabilities.capability_ids.is_empty());
}
