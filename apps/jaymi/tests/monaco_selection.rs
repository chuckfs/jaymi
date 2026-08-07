//! Monaco selection intelligence — CodingState → session selection text.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::EditorSelection;
use jaymi_context::ContextEngine;
use jaymi_core::UserRequest;
use std::sync::Arc;

#[test]
fn coding_selection_reaches_context_session_inputs() {
    let data_dir = temp_dir("monaco-selection");
    let root = data_dir.join("proj");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("main.rs");
    fs::write(&file, "fn main() {\n    let x = 1;\n}\n").unwrap();
    let path = file.to_string_lossy().into_owned();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let _ = app
        .handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("open coding");

    app.with_coding_state(|coding| {
        coding.open_permanent(&path, "fn main() {\n    let x = 1;\n}\n".into());
    })
    .expect("open file");

    app.set_coding_tab_selection(
        &path,
        EditorSelection::new(1, 4, 1, 9, Some("x = 1".into())),
    )
    .expect("selection");

    let _ = app
        .handle(UserRequest::new("Explain this."))
        .expect("handle");

    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    let session = context.session_inputs();

    assert_eq!(session.current_selection.path.as_deref(), Some(path.as_str()));
    assert_eq!(session.current_selection.start_line, 1);
    assert_eq!(session.current_selection.start_column, 4);
    assert_eq!(session.current_selection.end_line, 1);
    assert_eq!(session.current_selection.end_column, 9);
    assert_eq!(
        session.current_selection.text.as_deref(),
        Some("x = 1"),
        "selected text must flow into ContextSessionInputs for Environmental Resolution"
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-monaco-selection-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
