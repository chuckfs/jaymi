//! Integration tests for Layer 6 Slice 6 — Capability Composition.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{
    research_coding_creation, Capability, CapabilityComposition, WorkspaceKind,
};
use jaymi_core::UserRequest;
use jaymi_planner::Planner;

#[test]
fn planner_composes_research_coding_creation_into_one_plan() {
    let data_dir = temp_dir("composition-pipeline");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let planner = app.container().resolve::<Planner>().expect("planner");

    let goal = "research then code then create";
    let first = planner.handle(UserRequest::new(goal)).expect("first");
    let second = planner.handle(UserRequest::new(goal)).expect("second");

    assert_eq!(first.capability, Some(Capability::Search));
    assert_eq!(first.execution_plan, second.execution_plan);

    let plan = first.execution_plan.expect("execution plan");
    assert_eq!(plan.goal.as_deref(), Some(goal));
    assert_eq!(plan.steps.len(), 3);
    assert_eq!(
        plan.capabilities(),
        vec![
            Capability::Search,
            Capability::Code,
            Capability::GenerateImages
        ]
    );

    // Capabilities remain independent steps — each keeps its own requirements.
    assert_eq!(plan.steps[0].capability, Capability::Search);
    assert!(
        plan.steps[0]
            .required_tools
            .iter()
            .any(|id| id == "search_files" || id == "search_knowledge")
            || !plan.steps[0].required_tools.is_empty()
    );
    assert_eq!(plan.steps[1].capability, Capability::Code);
    assert!(plan.steps[1]
        .required_tools
        .iter()
        .any(|id| id == "editor" || id == "terminal"));
    assert_eq!(plan.steps[2].capability, Capability::GenerateImages);
    assert!(plan.steps[2]
        .required_permissions
        .iter()
        .any(|permission| permission.label() == "ai_providers:execute"));

    // Composition is planning only.
    assert!(first.tool_id.is_none());
    assert!(first.content.contains("Composed 3 independent capabilities"));
    assert!(first.content.contains("search → code → generate_images"));
    assert!(!first.blocked);

    let workspace = first.workspace.expect("workspace from primary");
    assert_eq!(workspace.kind, WorkspaceKind::Research);
}

#[test]
fn direct_composition_api_keeps_capabilities_independent() {
    let data_dir = temp_dir("composition-api");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let composition = CapabilityComposition::new(research_coding_creation())
        .expect("composition")
        .with_goal("illustrate a researched prototype");
    let plan = app
        .compose_capability_plan(&composition)
        .expect("compose plan");

    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.capabilities(), research_coding_creation());
    assert_eq!(
        plan.goal.as_deref(),
        Some("illustrate a researched prototype")
    );

    // Aggregated requirements span steps without merging capabilities.
    let tools = plan.required_tools();
    let permissions = plan.required_permissions();
    assert!(tools.iter().any(|id| id.contains("search") || id == "editor"));
    assert!(permissions
        .iter()
        .any(|permission| permission.label() == "filesystem:read"));
    assert!(permissions
        .iter()
        .any(|permission| permission.label() == "terminal:execute")
        || permissions
            .iter()
            .any(|permission| permission.label() == "ai_providers:execute"));

    // Steps stay distinct objects — mutating one list must not alter another.
    assert_ne!(
        plan.steps[0].required_tools,
        plan.steps[1].required_tools
    );
    assert_ne!(
        plan.steps[1].required_permissions,
        plan.steps[2].required_permissions
    );
}

#[test]
fn plan_capabilities_accepts_custom_sequences() {
    let data_dir = temp_dir("composition-custom");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let plan = app
        .plan_capabilities(
            &[Capability::Discover, Capability::ReadDocuments, Capability::Code],
            Some("inventory then read then implement"),
        )
        .expect("custom compose");

    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.steps[0].capability, Capability::Discover);
    assert_eq!(plan.steps[1].capability, Capability::ReadDocuments);
    assert_eq!(plan.steps[2].capability, Capability::Code);
    assert!(app
        .container()
        .resolve::<Planner>()
        .expect("planner")
        .handle(UserRequest::new("compose discover read code"))
        .expect("handle compose")
        .execution_plan
        .expect("plan")
        .capabilities()
        .contains(&Capability::Discover));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-capability-composition-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
