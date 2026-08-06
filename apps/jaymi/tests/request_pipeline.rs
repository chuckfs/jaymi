//! Canonical request pipeline — Intent → Capability → Context assemble.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::{SearchRequest, UserRequest};
use jaymi_planner::request_lifecycle::RequestStage;

#[test]
fn request_stage_order_matches_canonical_pipeline() {
    let stages = [
        RequestStage::ReceiveRequest,
        RequestStage::DetermineIntent,
        RequestStage::ResolveCapability,
        RequestStage::EvaluateContextPolicy,
        RequestStage::CollectFromProviders,
        RequestStage::AssembleContextBundle,
        RequestStage::RunBehavior, // Planned
        RequestStage::EvaluateActionPolicy,
        RequestStage::CheckPermissions,
        RequestStage::ExecuteTool,
        RequestStage::InvokeProviders,
        RequestStage::Respond,
    ];
    // Discriminant order is the documented lifecycle.
    for window in stages.windows(2) {
        assert!(
            (window[0] as u8) < (window[1] as u8),
            "{:?} must precede {:?}",
            window[0],
            window[1]
        );
    }
}

#[test]
fn handle_assembles_after_intent_with_capability_hints() {
    let data_dir = temp_dir("pipeline-hints");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let _ = app
        .handle(UserRequest::search(SearchRequest::free_text("fungi")))
        .expect("search handle");

    let report = app
        .inspect_context()
        .expect("inspect")
        .expect("context inspector after handle");
    let notes = report.notes.join("\n");
    assert!(
        notes.contains("pipeline intent=search_knowledge"),
        "assemble notes must record Planner intent label; got:\n{notes}"
    );
    assert!(
        notes.contains("capabilities=[") && notes.contains("search"),
        "assemble notes must record selected capability ids; got:\n{notes}"
    );
    let policy = report.policy.as_ref().expect("policy report");
    assert!(
        policy
            .decisions
            .iter()
            .any(|d| d.provider_id == "search" && d.included),
        "search capability hint should allow search provider participation"
    );
}

#[test]
fn unsupported_chat_still_assembles_with_intent_label() {
    let data_dir = temp_dir("pipeline-unknown");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let _ = app
        .handle(UserRequest::new("hello canonical pipeline"))
        .expect("handle");
    let report = app.inspect_context().expect("inspect").expect("report");
    let notes = report.notes.join("\n");
    assert!(
        notes.contains("pipeline intent="),
        "every handle must stamp pipeline intent into assemble notes; got:\n{notes}"
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-request-pipeline-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
