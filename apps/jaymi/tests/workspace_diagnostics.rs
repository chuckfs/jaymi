//! Workspace Intelligence diagnostics (Sprint B2.11).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, WorkspaceDiagnosticsReport};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-b211-{label}-{nanos}"));
    std::fs::create_dir_all(&path).expect("temp dir");
    path
}

#[test]
fn diagnostics_expose_workspace_intelligence_fields() {
    let app = Application::boot_with_data_dir(temp_dir("fields")).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let report = snapshot
        .workspace_inspector
        .as_ref()
        .expect("workspace inspector");

    assert!(report.has_content());
    let labels: Vec<_> = report
        .labeled_values()
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    for required in [
        "Maintenance Generation",
        "Maintenance Jobs",
        "Candidates",
    ] {
        assert!(
            labels.iter().any(|label| label == required),
            "missing label {required:?} in {labels:?}"
        );
    }
    assert!(!report.snapshot_freshness.is_empty());
    assert!(!report.maintenance_status.is_empty());

    let rendered = report.render();
    assert!(rendered.contains("developer-only"));
    assert!(rendered.contains("never written to conversation transcript"));
    assert!(rendered.contains("Snapshot freshness"));
    assert!(rendered.contains("Maintenance status"));
}

#[test]
fn workspace_diagnostics_api_matches_snapshot() {
    let app = Application::boot_with_data_dir(temp_dir("api")).expect("boot");
    let via_api = app.workspace_diagnostics().expect("workspace_diagnostics");
    let via_snapshot = app
        .diagnostics()
        .expect("diagnostics")
        .workspace_inspector
        .expect("workspace inspector");
    assert_eq!(
        via_api.maintenance_generation,
        via_snapshot.maintenance_generation
    );
    assert_eq!(
        via_api.snapshot_freshness.len(),
        via_snapshot.snapshot_freshness.len()
    );
}

#[test]
fn dashboard_includes_workspace_section_when_present() {
    let app = Application::boot_with_data_dir(temp_dir("dashboard")).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let rendered = snapshot.render_dashboard();
    assert!(rendered.contains("Workspace Intelligence Diagnostics"));
    assert!(rendered.contains("never written to conversation transcript"));
}

#[test]
fn assemble_is_pure_observation() {
    // Empty assemble still lists freshness / maintenance rows without requiring
    // a live Application — no transcript, no maintenance schedule.
    let report = WorkspaceDiagnosticsReport::assemble(Default::default());
    assert!(!report.snapshot_freshness.is_empty());
    let text = report.render();
    assert!(text.contains("developer-only"));
}
