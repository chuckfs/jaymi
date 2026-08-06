//! Context Inspector — developer diagnostics for the latest ContextBundle.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_context::ProviderInspectOutcome;
use jaymi_core::UserRequest;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-inspector-{}-{}",
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
fn diagnostics_surface_context_inspector_after_handle() {
    let data_dir = temp_dir("diag");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    assert!(
        app.inspect_context()
            .expect("inspect")
            .is_none(),
        "no assemble yet"
    );

    let _ = app
        .handle(UserRequest::new("hello context inspector"))
        .expect("handle");

    let report = app
        .inspect_context()
        .expect("inspect")
        .expect("inspection after handle");
    assert!(report.request_preview.contains("hello context inspector"));
    assert!(!report.providers.is_empty());
    assert!(report.contributed().iter().any(|p| p.id == "memory"));
    assert!(report
        .omitted()
        .iter()
        .any(|p| matches!(
            p.outcome,
            ProviderInspectOutcome::SkippedRelevance { .. }
                | ProviderInspectOutcome::SkippedPolicy { .. }
        )));
    assert!(report.budget.is_some());

    let snapshot = app.diagnostics().expect("diagnostics");
    let inspector = snapshot
        .context_inspector
        .as_ref()
        .expect("snapshot includes context inspector");
    assert_eq!(inspector.assemble_generation, report.assemble_generation);
    assert!(snapshot
        .render_dashboard()
        .contains("Context Inspector"));
}

#[test]
fn context_inspector_does_not_reassemble() {
    let data_dir = temp_dir("no-reasm");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let context = app
        .container()
        .resolve::<std::sync::Arc<jaymi_context::ContextEngine>>()
        .expect("context");

    let _ = app.handle(UserRequest::new("one")).expect("handle");
    let before = context.assemble_count();
    let _ = app.inspect_context().expect("inspect");
    let snapshot = app.diagnostics().expect("diagnostics");
    assert!(snapshot.context_inspector.is_some());
    assert_eq!(
        context.assemble_count(),
        before,
        "inspect / diagnostics must not re-assemble"
    );
}
