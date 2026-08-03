//! Integration tests for Layer 6 Slice 3 — Capability Planning.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::Capability;
use jaymi_core::UserRequest;
use jaymi_planner::Planner;

#[test]
fn coding_request_builds_deterministic_execution_plan_without_tools() {
    let data_dir = temp_dir("capability-planning");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let planner = app.container().resolve::<Planner>().expect("planner");

    let goal = "Help me build an app.";
    let first = planner
        .handle(UserRequest::new(goal))
        .expect("first plan");
    let second = planner
        .handle(UserRequest::new(goal))
        .expect("second plan");

    assert_eq!(first.capability, Some(Capability::Code));
    assert_eq!(second.capability, Some(Capability::Code));

    let plan = first.execution_plan.expect("execution plan");
    let plan_again = second.execution_plan.expect("execution plan again");
    assert_eq!(plan, plan_again, "plan generation must be deterministic");

    assert_eq!(plan.goal.as_deref(), Some(goal));
    assert_eq!(plan.steps.len(), 1);
    let step = &plan.steps[0];
    assert_eq!(step.capability, Capability::Code);
    assert!(
        step.required_tools.iter().any(|id| id == "terminal"),
        "live terminal tool should appear in the code plan: {:?}",
        step.required_tools
    );
    assert!(step
        .required_providers
        .iter()
        .any(|id| id == "filesystem" || id == "terminal"));
    assert!(step
        .required_permissions
        .iter()
        .any(|permission| permission.label() == "filesystem:read"));
    assert!(step
        .required_permissions
        .iter()
        .any(|permission| permission.label() == "filesystem:write"));
    assert!(step
        .required_permissions
        .iter()
        .any(|permission| permission.label() == "terminal:execute"));

    // Planning for "help me build an app" still does not execute tools.
    assert!(first.tool_id.is_none());
    assert!(plan.is_executable());
    assert!(first.content.contains("Execution plan"));
    assert!(first.content.contains("code"));
    assert!(!first.blocked);

    let direct = app
        .plan_capability(Capability::Code, Some(goal))
        .expect("direct plan");
    assert_eq!(direct.steps[0].required_tools, plan.steps[0].required_tools);
    assert_eq!(
        direct.required_permissions(),
        plan.required_permissions()
    );
}

#[test]
fn search_plan_includes_live_tools_and_providers() {
    let data_dir = temp_dir("capability-planning-search");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let plan = app
        .build_capability_plan(&[Capability::Search])
        .expect("search plan");
    assert_eq!(plan.steps.len(), 1);
    let step = &plan.steps[0];
    assert_eq!(step.capability, Capability::Search);
    assert!(
        step.required_tools.iter().any(|id| id == "search_files")
            || step
                .required_tools
                .iter()
                .any(|id| id == "search_knowledge"),
        "expected live search tools; got {:?}",
        step.required_tools
    );
    assert!(step
        .required_providers
        .iter()
        .any(|id| id == "filesystem" || id == "embedding.local"));
    assert!(step
        .required_permissions
        .iter()
        .any(|permission| permission.label() == "filesystem:read"));
    assert!(plan.is_ready());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-capability-planning-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
