//! Context History — recent ContextBundles retained for inspection.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-history-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn context_history_records_after_handle_and_surfaces_in_diagnostics() {
    let data_dir = temp_dir("history");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    assert!(
        app.context_history().expect("history").is_empty(),
        "no assemble yet"
    );

    let _ = app
        .handle(UserRequest::new("hello context history"))
        .expect("handle");

    let history = app.context_history().expect("history");
    assert_eq!(history.len(), 1);
    let entry = &history[0];
    assert!(entry.timestamp_unix_ms > 0);
    assert!(entry.request.contains("hello context history"));
    assert!(entry.providers_used.iter().any(|id| {
        id == "conversation" || id == "permission" || id == "workspace"
    }));
    assert!(entry.bundle_size_characters > 0);
    assert_eq!(entry.bundle.user_request().content_preview, entry.request);

    let snapshot = app.diagnostics().expect("diagnostics");
    assert_eq!(snapshot.context_history.len(), 1);
    assert!(snapshot
        .render_dashboard()
        .contains("Context History"));
    assert!(snapshot
        .render_dashboard()
        .contains("hello context history"));
}

#[test]
fn context_history_read_does_not_reassemble() {
    let data_dir = temp_dir("no-reasm");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let context = app
        .container()
        .resolve::<std::sync::Arc<jaymi_context::ContextEngine>>()
        .expect("context");

    let _ = app.handle(UserRequest::new("one")).expect("handle");
    let before = context.assemble_count();
    let _ = app.context_history().expect("history");
    let _ = app.diagnostics().expect("diagnostics");
    assert_eq!(
        context.assemble_count(),
        before,
        "history / diagnostics must not re-assemble"
    );
}
