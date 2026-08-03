//! Integration tests for Capability Inspector + availability diagnostics.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{Capability, CapabilityAvailability, WorkspaceKind};
use jaymi_core::UserRequest;

#[test]
fn capability_inspector_reflects_registered_active_planned_and_requirements() {
    let data_dir = temp_dir("inspector");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let report = app.inspect_capabilities().expect("inspect");

    // Full conceptual catalog stays registered.
    assert_eq!(report.registered.len(), Capability::all().len());
    assert!(report.registered.contains(&"search".to_string()));
    assert!(report.registered.contains(&"code".to_string()));
    assert!(report.registered.contains(&"chat".to_string()));
    assert!(report.registered.contains(&"internet".to_string()));

    // Active capabilities are a subset of registered (runtime-executable).
    assert!(!report.active.is_empty());
    for id in &report.active {
        assert!(
            report.registered.contains(id),
            "active capability {id} must be registered"
        );
        let entry = report.get(id).expect("active entry");
        assert!(entry.active);
        assert!(entry.registered);
        assert!(entry.availability.is_executable_tier());
        assert!(entry.blockers.is_empty());
    }

    // Planned capabilities remain visible and registered.
    assert!(!report.planned.is_empty());
    assert!(report.planned.contains(&"chat".to_string()));
    let chat = report.get("chat").expect("chat");
    assert_eq!(chat.availability, CapabilityAvailability::Planned);
    assert!(!chat.active);

    let search = report.get("search").expect("search row");
    assert_eq!(search.availability, CapabilityAvailability::Ready);
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
    // Code is Experimental in catalog but blocked without coding tools.
    assert_eq!(code.availability, CapabilityAvailability::Unavailable);

    let rendered = report.render();
    assert!(rendered.contains("Capability Inspector"));
    assert!(rendered.contains("Availability"));
    assert!(rendered.contains("ready") || rendered.contains("planned"));
    assert!(rendered.contains("search"));
    assert!(rendered.contains("research"));
    assert!(rendered.contains("coding"));
}

#[test]
fn diagnostics_include_capability_inspector_and_availability() {
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
    assert!(!inspector.planned.is_empty());

    assert!(snapshot
        .capability_status_details
        .iter()
        .any(|line| line.contains("ready") || line.contains("planned")));

    let dashboard = snapshot.render_dashboard();
    assert!(dashboard.contains("Capability Inspector"));
    assert!(dashboard.contains("Availability") || dashboard.contains("search"));

    let caps = snapshot.subsystem("Capabilities").expect("capabilities row");
    assert!(caps.detail.contains("registered="));
    assert!(caps.detail.contains("active="));
    assert!(caps.detail.contains("planned="));
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
