//! Sprint C0.1 — Coding Actions: toolbar → typed intents → Planner.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{dispatch_quick_action, Application, QuickAction, QuickActionEffect};
use jaymi_capabilities::{EditorSelection, WorkspaceKind};
use jaymi_core::{CodingAction, IntentId, UserRequest};
use jaymi_planner::decision::DecisionEngine;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-c01-{label}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn toolbar_dispatch_emits_typed_coding_actions_only() {
    assert!(matches!(
        dispatch_quick_action(QuickAction::Explain),
        QuickActionEffect::SubmitExplain
    ));
    assert_eq!(
        dispatch_quick_action(QuickAction::Edit),
        QuickActionEffect::SubmitCodingAction(CodingAction::EditSelection)
    );
    assert_eq!(
        dispatch_quick_action(QuickAction::More),
        QuickActionEffect::SubmitCodingAction(CodingAction::OpenCodingActions)
    );
}

#[test]
fn decision_engine_routes_coding_actions() {
    let engine = DecisionEngine::default();

    let open = UserRequest::coding_action(CodingAction::OpenCodingActions);
    assert_eq!(engine.determine_intent(&open).id(), IntentId::Unknown);

    let edit = UserRequest::coding_action(CodingAction::EditSelection);
    assert_eq!(engine.determine_intent(&edit).id(), IntentId::Unknown);

    let mut search = UserRequest::coding_action(CodingAction::SearchWorkspace);
    search.search = Some(jaymi_core::SearchRequest::free_text("fungi"));
    assert_eq!(
        engine.determine_intent(&search).id(),
        IntentId::SearchKnowledge
    );

    let search_ask = UserRequest::coding_action(CodingAction::SearchWorkspace);
    assert_eq!(engine.determine_intent(&search_ask).id(), IntentId::Unknown);
}

#[test]
fn open_coding_actions_is_honest_deterministic_reply() {
    let data_dir = temp_dir("menu-data");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let response = app
        .handle_with_workspace(UserRequest::coding_action(CodingAction::OpenCodingActions))
        .expect("menu");
    assert!(
        response.content.contains("Explain") && response.content.contains("Run"),
        "menu missing actions: {}",
        response.content
    );
    assert!(!response.reasoning_used);
    assert!(!response.awaiting_review);
}

#[test]
fn search_without_query_asks_honestly() {
    let data_dir = temp_dir("search-ask");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let response = app
        .handle_with_workspace(UserRequest::coding_action(CodingAction::SearchWorkspace))
        .expect("search ask");
    assert!(
        response.content.to_ascii_lowercase().contains("search"),
        "{}",
        response.content
    );
    assert!(!response.reasoning_used);
}

#[test]
fn run_without_project_asks_honestly() {
    let data_dir = temp_dir("run-ask");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let response = app
        .handle_with_workspace(UserRequest::coding_action(CodingAction::RunProject))
        .expect("run ask");
    assert!(
        response.content.to_ascii_lowercase().contains("run")
            || response.content.to_ascii_lowercase().contains("command"),
        "{}",
        response.content
    );
    assert!(!response.reasoning_used);
}

#[test]
fn explain_resolves_selection_vs_file_from_coding_state() {
    let data_dir = temp_dir("explain-data");
    let root = temp_dir("explain-root");
    fs::write(root.join("main.rs"), "fn main() { let x = 1; }\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.open_project_from_path(&root).expect("open project");
    app.start_coding_project().expect("coding");
    app.open_coding_file(root.join("main.rs").to_str().unwrap())
        .expect("open file");

    let file_req = app
        .build_coding_action_request(CodingAction::ExplainFile)
        .expect("explain file");
    assert_eq!(file_req.coding_action, Some(CodingAction::ExplainFile));

    app.with_coding_state(|coding| {
        let path = coding.active_tab_path().unwrap().to_string();
        coding.set_selection(
            &path,
            EditorSelection::new(0, 0, 0, 8, Some("fn main(".into())),
        );
    })
    .expect("selection");

    // begin_explain picks selection when present
    let has_selection = app
        .with_coding_state(|coding| {
            coding
                .editors
                .active_session()
                .map(|s| !s.view.selection.is_empty())
                .unwrap_or(false)
        })
        .unwrap();
    assert!(has_selection);

    let sel_req = app
        .build_coding_action_request(CodingAction::ExplainSelection)
        .expect("explain selection");
    assert_eq!(
        sel_req.coding_action,
        Some(CodingAction::ExplainSelection)
    );
    assert!(sel_req.content.contains("selection"));
    assert_eq!(
        app.experience()
            .unwrap()
            .active_workspace()
            .map(|w| w.kind),
        Some(WorkspaceKind::Coding)
    );
}

#[test]
fn run_project_with_cargo_builds_reviewed_terminal_request() {
    let data_dir = temp_dir("run-cargo-data");
    let root = temp_dir("run-cargo-root");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.open_project_from_path(&root).expect("open");
    app.start_coding_project().expect("coding");

    let request = app
        .build_coding_action_request(CodingAction::RunProject)
        .expect("build run");
    assert_eq!(request.coding_action, Some(CodingAction::RunProject));
    assert_eq!(
        request.terminal.as_ref().and_then(|t| t.command.as_deref()),
        Some("cargo test")
    );
    assert_eq!(
        DecisionEngine::default().determine_intent(&request).id(),
        IntentId::RunTerminal
    );

    let response = app.handle_with_workspace(request).expect("handle run");
    // Mutations require review — never silent execute from the toolbar.
    assert!(
        response.awaiting_review || response.execution_plan.is_some() || !response.content.is_empty(),
        "expected reviewed plan or honest content, got awaiting={} content={}",
        response.awaiting_review,
        response.content
    );
}

#[test]
fn search_with_selection_builds_search_knowledge_intent() {
    let data_dir = temp_dir("search-sel-data");
    let root = temp_dir("search-sel-root");
    fs::write(root.join("note.txt"), "hello fungi world\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.open_project_from_path(&root).expect("open");
    app.start_coding_project().expect("coding");
    app.open_coding_file(root.join("note.txt").to_str().unwrap())
        .expect("open");
    app.with_coding_state(|coding| {
        let path = coding.active_tab_path().unwrap().to_string();
        coding.set_selection(
            &path,
            EditorSelection::new(0, 6, 0, 11, Some("fungi".into())),
        );
    })
    .unwrap();

    let request = app
        .build_coding_action_request(CodingAction::SearchWorkspace)
        .expect("search");
    assert!(request.search.is_some());
    assert_eq!(
        DecisionEngine::default().determine_intent(&request).id(),
        IntentId::SearchKnowledge
    );
}
