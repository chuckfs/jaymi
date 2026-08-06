//! Context Validation Suite — Planner integration (A10.9).
//!
//! Verifies every user request flows through Context assemble exactly once,
//! attaches an immutable ContextBundle, and surfaces inspector explainability.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_context::{ContextEngine, ContextSource, ProviderInspectOutcome};
use jaymi_core::{Lifecycle, SearchRequest, UserRequest};
use jaymi_planner::Planner;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-validation-planner-{}-{}",
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
fn planner_integration_every_handle_assembles_once_and_attaches_bundle() {
    let app = Application::boot_with_data_dir(temp_dir("handle")).expect("boot");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    assert_eq!(context.assemble_count(), 0);

    let chat = app
        .handle(UserRequest::new("planner integration chat"))
        .expect("chat");
    assert_eq!(context.assemble_count(), 1);
    let bundle = chat.context().expect("bundle attached");
    assert!(bundle.sources().contains(&ContextSource::UserRequest));
    assert!(bundle.assemble_generation() >= 1);

    let search = app
        .handle(UserRequest::search(SearchRequest::free_text("fungi")))
        .expect("search");
    assert_eq!(context.assemble_count(), 2);
    assert!(search.context().is_some());

    let list = app
        .list_directory(temp_dir("list-target"))
        .expect("list");
    assert_eq!(context.assemble_count(), 3);
    assert!(list.context().is_some());
}

#[test]
fn planner_integration_direct_planner_handle_also_assembles() {
    let app = Application::boot_with_data_dir(temp_dir("direct")).expect("boot");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");
    let planner = app.container().resolve::<Planner>().expect("planner");

    let before = context.assemble_count();
    let response = planner
        .handle(UserRequest::new("direct planner path"))
        .expect("handle");
    assert_eq!(context.assemble_count(), before + 1);
    assert!(response.context().is_some());
}

#[test]
fn planner_integration_inspector_explains_policy_and_ordering() {
    let app = Application::boot_with_data_dir(temp_dir("inspect")).expect("boot");
    let _ = app
        .handle(UserRequest::new("explain this assemble"))
        .expect("handle");

    let report = app.inspect_context().expect("inspect").expect("report");
    assert!(!report.providers.is_empty());
    assert!(report.budget.is_some());
    assert!(report.policy.is_some());
    assert_eq!(report.cache_status(), "miss");
    assert!(report.bundle_size_characters > 0);
    assert!(
        report
            .providers
            .windows(2)
            .all(|pair| pair[0].evaluation_order <= pair[1].evaluation_order)
    );
    assert!(report.providers.iter().all(|p| !p.sensitivity.is_empty()));
    assert!(report.providers.iter().all(|p| {
        matches!(
            p.approval_status.as_str(),
            "not_required" | "approved" | "pending" | "n/a"
        )
    }));

    let rendered = report.render();
    assert!(rendered.contains("Context Inspector"));
    assert!(rendered.contains("duration_ms="));
    assert!(rendered.contains("provider_order (contributors):"));
}

#[test]
fn planner_integration_search_includes_and_chat_excludes_search_provider() {
    let app = Application::boot_with_data_dir(temp_dir("include-exclude")).expect("boot");

    let _ = app
        .handle(UserRequest::new("plain chat excludes search"))
        .expect("chat");
    let chat_report = app.inspect_context().expect("inspect").expect("report");
    let chat_search = chat_report
        .providers
        .iter()
        .find(|p| p.id == "search")
        .expect("search row");
    assert!(matches!(
        chat_search.outcome,
        ProviderInspectOutcome::SkippedPolicy { .. }
            | ProviderInspectOutcome::SkippedRelevance { .. }
            | ProviderInspectOutcome::Declined
    ));

    let _ = app
        .handle(UserRequest::search(SearchRequest::free_text("include me")))
        .expect("search");
    let search_report = app.inspect_context().expect("inspect").expect("report");
    let policy = search_report.policy.as_ref().expect("policy");
    assert!(policy
        .decisions
        .iter()
        .any(|d| d.provider_id == "search" && d.included));
}

#[test]
fn planner_integration_identical_requests_are_deterministic_after_invalidate() {
    let app = Application::boot_with_data_dir(temp_dir("det")).expect("boot");
    let context = app
        .container()
        .resolve::<Arc<ContextEngine>>()
        .expect("context");

    let first = app
        .handle(UserRequest::new("same request twice"))
        .expect("first");
    let first_bundle = first.context().expect("bundle").clone();

    context.invalidate_cache("validation suite");
    let second = app
        .handle(UserRequest::new("same request twice"))
        .expect("second");
    let second_bundle = second.context().expect("bundle");

    assert_eq!(
        first_bundle.policy().map(|p| &p.decisions),
        second_bundle.policy().map(|p| &p.decisions)
    );
    assert_eq!(
        first_bundle.active_capabilities().capability_ids,
        second_bundle.active_capabilities().capability_ids
    );
    assert_eq!(first_bundle.sources(), second_bundle.sources());
}

#[test]
fn planner_integration_diagnostics_surface_context_engine() {
    let app = Application::boot_with_data_dir(temp_dir("diag")).expect("boot");
    let _ = app
        .handle(UserRequest::new("warmup"))
        .expect("handle");
    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot
        .subsystem("Context Engine")
        .expect("context engine row");
    assert_eq!(row.status.label(), "Operational");
    assert!(row.detail.contains("sources_bound=true"));
    assert!(snapshot.render_dashboard().contains("Context Inspector"));
    assert!(context_engine_healthy(&app));
}

fn context_engine_healthy(app: &Application) -> bool {
    app.container()
        .resolve::<Arc<ContextEngine>>()
        .map(|engine| engine.health_check().healthy)
        .unwrap_or(false)
}
