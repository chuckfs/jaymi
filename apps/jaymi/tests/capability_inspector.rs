//! Integration tests for Layer 6 Slice 7 — Capability Inspector.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{Capability, WorkspaceKind};
use jaymi_core::UserRequest;

#[test]
fn capability_inspector_reflects_registered_active_and_requirements() {
    let data_dir = temp_dir("inspector");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let report = app.inspect_capabilities().expect("inspect");

    // Registered capabilities match boot registration.
    assert!(report.registered.contains(&"search".to_string()));
    assert!(report.registered.contains(&"code".to_string()));
    assert!(report.registered.contains(&"read_documents".to_string()));
    assert!(report.registered.contains(&"discover".to_string()));
    assert!(report.registered.contains(&"index".to_string()));
    assert_eq!(report.registered.len(), 5);

    // Active capabilities are a subset of registered (runtime-available).
    assert!(!report.active.is_empty());
    for id in &report.active {
        assert!(
            report.registered.contains(id),
            "active capability {id} must be registered"
        );
        let entry = report.get(id).expect("active entry");
        assert!(entry.active);
        assert!(entry.registered);
        assert!(entry.blockers.is_empty());
    }

    let search = report.get("search").expect("search row");
    assert_eq!(search.workspace, Some(WorkspaceKind::Research));
    assert!(
        search.required_tools.iter().any(|id| id == "search_files")
            || search
                .required_tools
                .iter()
                .any(|id| id == "search_knowledge")
    );
    assert!(search
        .required_providers
        .iter()
        .any(|id| id == "filesystem" || id == "embedding.local"));

    let code = report.get("code").expect("code row");
    assert_eq!(code.workspace, Some(WorkspaceKind::Coding));
    assert!(code.required_tools.contains(&"editor".to_string()));
    assert!(code.required_tools.contains(&"terminal".to_string()));
    assert!(code
        .required_providers
        .iter()
        .any(|id| id == "filesystem"));

    let rendered = report.render();
    assert!(rendered.contains("Capability Inspector"));
    assert!(rendered.contains("search"));
    assert!(rendered.contains("research"));
    assert!(rendered.contains("coding"));
}

#[test]
fn diagnostics_include_capability_inspector() {
    let data_dir = temp_dir("inspector-diagnostics");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let snapshot = app.diagnostics().expect("diagnostics");
    let inspector = snapshot
        .capability_inspector
        .as_ref()
        .expect("inspector in diagnostics");
    assert_eq!(inspector.registered.len(), snapshot.capability_count);
    assert_eq!(
        inspector.active.len(),
        snapshot.available_capability_ids.len()
    );
    assert_eq!(inspector.active, snapshot.available_capability_ids);

    let dashboard = snapshot.render_dashboard();
    assert!(dashboard.contains("Capability Inspector"));
    assert!(dashboard.contains("Required tools") || dashboard.contains("search"));

    let caps = snapshot.subsystem("Capabilities").expect("capabilities row");
    assert!(caps.detail.contains("registered="));
    assert!(caps.detail.contains("active="));
}

#[test]
fn inspector_attaches_session_workspace_when_expanded() {
    let data_dir = temp_dir("inspector-workspace");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    app.handle_with_workspace(UserRequest::new("Help me build an app."))
        .expect("coding");
    let report = app.inspect_capabilities().expect("inspect");
    assert_eq!(report.active_workspace, Some(WorkspaceKind::Coding));

    let code = report.get("code").expect("code");
    assert_eq!(code.capability, Capability::Code);
    assert_eq!(code.workspace, Some(WorkspaceKind::Coding));

    app.close_ui_workspace().expect("close");
    let report = app.inspect_capabilities().expect("inspect after close");
    assert_eq!(report.active_workspace, None);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-capability-inspector-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
