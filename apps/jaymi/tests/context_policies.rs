//! Context Policies — inclusion / exclusion / explainability integration tests.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_context::ProviderInspectOutcome;
use jaymi_core::UserRequest;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-policies-{}-{}",
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
fn diagnostics_surface_context_policy_decisions() {
    let data_dir = temp_dir("diag");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let _ = app
        .handle(UserRequest::new("hello context policies"))
        .expect("handle");

    let snapshot = app.diagnostics().expect("diagnostics");
    let inspector = snapshot
        .context_inspector
        .as_ref()
        .expect("context inspector");
    let policy = inspector.policy.as_ref().expect("policy report");
    assert!(
        !policy.active_policies.is_empty(),
        "active context policy should be recorded"
    );
    assert!(!policy.decisions.is_empty());
    assert!(policy.size_before_characters >= policy.size_after_characters);
    let dashboard = snapshot.render_dashboard();
    assert!(dashboard.contains("Context Policy"));
    assert!(dashboard.contains("Included") || dashboard.contains("Excluded"));
}

#[test]
fn search_policy_allows_structured_search_requests() {
    let data_dir = temp_dir("search");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let _ = app
        .handle(UserRequest::search(jaymi_core::SearchRequest::free_text(
            "find symbol Foo",
        )))
        .expect("handle");

    let report = app.inspect_context().expect("inspect").expect("report");
    let policy = report.policy.as_ref().expect("policy");
    assert!(
        policy
            .decisions
            .iter()
            .any(|d| d.provider_id == "search" && d.included),
        "search included when retrieval required"
    );
}

#[test]
fn plain_chat_excludes_search_via_policy_or_relevance() {
    let data_dir = temp_dir("chat");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let _ = app
        .handle(UserRequest::new("hello there"))
        .expect("handle");

    let report = app.inspect_context().expect("inspect").expect("report");
    let search = report
        .providers
        .iter()
        .find(|p| p.id == "search")
        .expect("search provider row");
    assert!(
        matches!(
            search.outcome,
            ProviderInspectOutcome::SkippedPolicy { .. }
                | ProviderInspectOutcome::SkippedRelevance { .. }
                | ProviderInspectOutcome::Declined
        ),
        "search omitted for plain chat: {:?}",
        search.outcome
    );
}
