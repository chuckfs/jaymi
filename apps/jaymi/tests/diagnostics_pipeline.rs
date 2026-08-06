//! Integration tests for Slice 0.5 — diagnostics developer dashboard.
//!
//! Status vocabulary: Operational / Experimental / Stub / Disabled.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_config::Config;
use jaymi_core::Lifecycle;

#[test]
fn diagnostics_dashboard_reports_honest_subsystem_states() {
    let data_dir = temp_dir("diagnostics-dashboard");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");

    let expected = [
        ("Planner", OperationalStatus::Operational),
        ("Database", OperationalStatus::Operational),
        ("Configuration", OperationalStatus::Operational),
        ("Session Cache", OperationalStatus::Operational),
        ("Logging", OperationalStatus::Operational),
        ("Permissions", OperationalStatus::Operational),
        ("Policies", OperationalStatus::Experimental),
        ("Providers", OperationalStatus::Experimental),
        ("OCR Provider", OperationalStatus::Stub),
        ("Embedding Provider", OperationalStatus::Experimental),
        ("Embedding Queue", OperationalStatus::Operational),
        ("Capabilities", OperationalStatus::Operational),
        ("Tools", OperationalStatus::Operational),
        ("Parser Registry", OperationalStatus::Operational),
        ("Index Status", OperationalStatus::Operational),
        ("Discovery Queries", OperationalStatus::Operational),
        ("Collections", OperationalStatus::Operational),
        ("Understanding", OperationalStatus::Operational),
        ("Content Intelligence", OperationalStatus::Operational),
        ("Search Engine", OperationalStatus::Operational),
        ("Watcher Status", OperationalStatus::Operational),
        ("Memory Status", OperationalStatus::Operational),
        ("Context Engine", OperationalStatus::Operational),
        ("Project Status", OperationalStatus::Operational),
        (
            "Reasoning Status",
            // Ollama may be offline in CI; registry is always wired → never Stub.
            OperationalStatus::Disabled,
        ),
    ];

    assert_eq!(snapshot.subsystems.len(), expected.len());
    for (name, status) in expected {
        let row = snapshot
            .subsystem(name)
            .unwrap_or_else(|| panic!("missing subsystem row: {name}"));
        if name == "Reasoning Status" {
            assert_ne!(row.status, OperationalStatus::Stub, "Reasoning Status");
            assert!(matches!(
                row.status,
                OperationalStatus::Disabled | OperationalStatus::Operational
            ));
        } else {
            assert_eq!(row.status, status, "unexpected status for {name}");
        }
        assert!(!row.detail.is_empty(), "empty detail for {name}");
    }

    let reasoning = snapshot
        .reasoning_inspector
        .as_ref()
        .expect("reasoning inspector");
    assert!(!reasoning.reasoning_health.is_empty());
    assert!(!reasoning.conversation_runtime_state.is_empty());
    assert!(snapshot
        .render_dashboard()
        .contains("Conversational Reasoning"));

    let rendered = snapshot.render_dashboard();
    assert!(rendered.contains("Jaymi Diagnostics"));
    assert!(rendered.contains("Memory Status"));
    assert!(rendered.contains("active="));
    assert!(rendered.contains("Stub"));
    assert!(rendered.contains("Experimental"));
    assert!(
        !rendered.contains("Healthy"),
        "dashboard must not claim Healthy for any subsystem"
    );
    assert!(
        !rendered.contains("Degraded"),
        "dashboard must use Experimental instead of Degraded"
    );
    assert!(
        !rendered.contains("Not implemented"),
        "dashboard must use Stub instead of Not implemented"
    );

    let policies = snapshot.subsystem("Policies").unwrap();
    assert!(policies.detail.contains("enforced: Offline First"));
    assert!(policies.detail.contains("other builtins declared"));

    let providers = snapshot.subsystem("Providers").unwrap();
    assert!(providers.detail.contains("stub"));
    assert!(providers.detail.contains("ready"));

    assert!(snapshot.planner_healthy);
    assert!(snapshot.database_connected);
    assert!(snapshot.logging_healthy);
    assert_eq!(snapshot.parser_count, snapshot.parser_ids.len());
    assert!(!snapshot.parser_ids.is_empty());
    assert!(snapshot.parser_ids.iter().any(|id| id == "image"));
    assert!(snapshot.parser_ids.iter().any(|id| id == "pdf"));
    assert!(snapshot.parser_ids.iter().any(|id| id == "docx"));
    assert!(snapshot.parser_ids.iter().any(|id| id == "markdown"));
    assert!(snapshot.parser_ids.iter().any(|id| id == "json"));
    assert!(snapshot.parser_ids.iter().any(|id| id == "plain_text"));
    assert!(snapshot
        .subsystem("Parser Registry")
        .unwrap()
        .detail
        .contains("registered="));
    assert!(snapshot
        .subsystem("Parser Registry")
        .unwrap()
        .detail
        .contains("usage="));
    assert!(snapshot
        .subsystem("OCR Provider")
        .unwrap()
        .detail
        .contains("engine=none"));
    assert_eq!(
        snapshot.subsystem("OCR Provider").unwrap().status,
        OperationalStatus::Stub
    );
    assert!(snapshot
        .active_policies
        .iter()
        .any(|name| name == "Offline First"));
    assert_eq!(snapshot.config_indexing_enabled, Some(true));
    assert!(snapshot
        .subsystem("Content Intelligence")
        .unwrap()
        .detail
        .contains("documents="));
    assert!(snapshot
        .subsystem("Search Engine")
        .unwrap()
        .detail
        .contains("searches="));
    assert!(snapshot
        .subsystem("Index Status")
        .unwrap()
        .detail
        .contains("files="));
    assert!(snapshot
        .subsystem("Discovery Queries")
        .unwrap()
        .detail
        .contains("query_count="));
    assert!(snapshot
        .subsystem("Collections")
        .unwrap()
        .detail
        .contains("collections="));

    // OCR capability stays registered but Planned — not executable via placeholder.
    assert!(snapshot.capability_ids.iter().any(|id| id == "ocr"));
    assert!(!snapshot
        .available_capability_ids
        .iter()
        .any(|id| id == "ocr"));
}

#[test]
fn diagnostics_reflect_config_indexing_flag_without_claiming_content_understanding() {
    let data_dir = temp_dir("diagnostics-index-flag");
    let mut config = Config::with_data_dir(&data_dir);
    config.initialize().unwrap();
    config.settings_mut().indexing_enabled = false;
    config.save().unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    assert_eq!(snapshot.config_indexing_enabled, Some(false));
    let index = snapshot.subsystem("Index Status").unwrap();
    assert_eq!(index.status, OperationalStatus::Disabled);
    assert!(index.detail.contains("indexing_enabled=false"));

    let watcher = snapshot.subsystem("Watcher Status").unwrap();
    assert_eq!(watcher.status, OperationalStatus::Disabled);
    assert!(watcher.detail.contains("status=disabled"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-diagnostics-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
