//! Integration tests for Architectural Integrity Slice 7 — Capability Availability.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{Capability, CapabilityAvailability};

#[test]
fn boot_keeps_full_catalog_registered_with_availability() {
    let data_dir = temp_dir("availability-boot");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let registered = app.discover_capabilities().expect("list");
    assert_eq!(registered.len(), Capability::all().len());

    let discovery = app.discover_capability_status().expect("discover");
    let search = discovery.get("search").expect("search");
    assert_eq!(search.availability, CapabilityAvailability::Ready);
    assert!(search.is_available());

    let chat = discovery.get("chat").expect("chat");
    assert_eq!(chat.availability, CapabilityAvailability::Planned);
    assert!(!chat.is_available());
    assert!(chat.registered);
    assert!(chat.blockers.is_empty());

    let code = discovery.get("code").expect("code");
    assert_eq!(code.availability, CapabilityAvailability::Unavailable);
    assert!(!code.is_available());
    assert!(code.registered);

    assert!(discovery.planned_count() > 0);
    assert!(!discovery.available.is_empty());
}

#[test]
fn planning_marks_planned_steps_without_unregistering() {
    let data_dir = temp_dir("availability-plan");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let plan = app
        .build_capability_plan(&[
            Capability::Search,
            Capability::GenerateImages,
            Capability::Internet,
        ])
        .expect("plan");

    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.steps[0].availability, CapabilityAvailability::Ready);
    assert_eq!(
        plan.steps[1].availability,
        CapabilityAvailability::Planned
    );
    assert_eq!(
        plan.steps[2].availability,
        CapabilityAvailability::Planned
    );
    assert!(!plan.is_ready());
    assert!(!plan.is_executable());
    assert_eq!(plan.unavailable().len(), 2);

    // Catalog still lists Planned capabilities as registered.
    let ids = app.discover_capabilities().expect("ids");
    assert!(ids.contains(&Capability::GenerateImages));
    assert!(ids.contains(&Capability::Internet));
}

#[test]
fn diagnostics_surface_availability_labels() {
    let data_dir = temp_dir("availability-diagnostics");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");

    assert_eq!(snapshot.capability_count, Capability::all().len());
    assert!(snapshot.capability_ids.contains(&"chat".to_string()));
    assert!(snapshot.capability_ids.contains(&"ocr".to_string()));

    assert!(snapshot
        .capability_status_details
        .iter()
        .any(|line| line.contains("search") && line.contains("ready")));
    assert!(snapshot
        .capability_status_details
        .iter()
        .any(|line| line.contains("chat") && line.contains("planned")));

    let inspector = snapshot
        .capability_inspector
        .as_ref()
        .expect("inspector");
    assert!(!inspector.planned.is_empty());
    let rendered = inspector.render();
    assert!(rendered.contains("planned"));
    assert!(rendered.contains("Availability"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-capability-availability-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
