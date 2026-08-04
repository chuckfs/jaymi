//! Integration tests for Layer 6 / Architectural Integrity — Capability Engine.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{
    Capability, CapabilityAvailability, CapabilityCategory, CapabilityEngine, CapabilityEngineApi,
};
use jaymi_core::Lifecycle;

#[test]
fn capability_engine_registers_resolves_and_describes_metadata() {
    let data_dir = temp_dir("capability-engine");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let registered = app.discover_capabilities().expect("discover");
    assert_eq!(registered.len(), Capability::all().len());
    assert!(registered.contains(&Capability::Search));
    assert!(registered.contains(&Capability::ReadDocuments));
    assert!(registered.contains(&Capability::Discover));
    assert!(registered.contains(&Capability::Index));
    assert!(registered.contains(&Capability::Chat));
    assert!(registered.contains(&Capability::Internet));

    let search = app
        .resolve_capability("search")
        .expect("resolve")
        .expect("search registered");
    assert_eq!(search.id, "search");
    assert_eq!(search.name, "Search");
    assert_eq!(search.category, CapabilityCategory::Knowledge);
    assert_eq!(search.availability, CapabilityAvailability::Ready);
    assert!(search.description.contains("knowledge"));
    assert!(search.offline_capable);
    assert!(!search.requires_internet);

    let catalog = app
        .describe_capability(Capability::Internet)
        .expect("describe");
    assert_eq!(catalog.id, "internet");
    assert!(catalog.requires_internet);
    assert!(!catalog.offline_capable);
    assert_eq!(catalog.availability, CapabilityAvailability::Planned);

    // Planned stays registered; effective plan availability is Planned.
    let plan = app
        .build_capability_plan(&[Capability::Search, Capability::Internet])
        .expect("plan");
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].availability, CapabilityAvailability::Ready);
    assert_eq!(plan.steps[1].availability, CapabilityAvailability::Planned);
    assert!(!plan.is_ready());
    assert!(plan.summary().contains("incomplete"));

    let ready = app
        .build_capability_plan(&[Capability::Search, Capability::Discover])
        .expect("ready plan");
    assert!(ready.is_ready());
    assert!(ready.summary().contains("executable") || ready.summary().contains("ready"));
    assert_eq!(ready.steps[0].availability, CapabilityAvailability::Ready);

    // Unknown ids stay unknown; capabilities never execute work.
    let mut engine = CapabilityEngine::new();
    engine.initialize().unwrap();
    assert_eq!(
        engine.validate_id("not-a-real-capability"),
        CapabilityAvailability::Unknown
    );
    engine.register(Capability::Chat).unwrap();
    let chat = engine.describe(Capability::Chat);
    assert_eq!(chat.category, CapabilityCategory::Conversation);
    assert_eq!(chat.availability, CapabilityAvailability::Planned);
    assert!(engine.resolve("chat").unwrap().is_some());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-capability-engine-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
