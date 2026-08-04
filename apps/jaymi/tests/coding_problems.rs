//! Problems panel — `ProblemsRegistry` aggregation, live refresh, and click-to-jump.
//!
//! Mirrors the Find in Files / Quick Open jump tests (`project_search.rs`):
//! `OpenProblem` reuses `Application::open_search_result` to land the cursor
//! in Monaco, so a Problems row behaves exactly like any other locatable hit.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{
    DiagnosticState, ExplorerStatus, GitStatusState, ProblemSeverity, ProblemsCollectContext,
};
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

fn open_project(app: &Application, root: PathBuf, project_id: &str) {
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some(project_id.into()),
            name: project_id.into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create project");
    app.open_project(project.id.as_str()).expect("open project");
    app.start_coding_project().expect("start coding");
}

#[test]
fn boot_registers_all_builtin_problem_providers() {
    let data_dir = temp_dir("problems-boot");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let registry = app.problems_registry().expect("problems registry");
    let ids = registry.list_ids().expect("list ids");

    let mut expected = vec!["lsp", "planner", "workspace", "permissions", "search", "memory"];
    expected.sort();
    assert_eq!(ids, expected, "boot must register every built-in Problems provider");
}

#[test]
fn lsp_diagnostic_appears_in_problems_after_refresh() {
    let data_dir = temp_dir("problems-lsp-data");
    let root = temp_dir("problems-lsp-root");
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    open_project(&app, root.clone(), "project:problems-lsp");

    let path = root.join("main.rs").to_string_lossy().into_owned();
    app.with_coding_state(|coding| {
        coding.diagnostics.push(DiagnosticState {
            message: "unused variable `x`".into(),
            path: Some(path.clone()),
            severity: "warning".into(),
            source: "rust-analyzer".into(),
            line: Some(2),
            character: Some(4),
            end_line: Some(2),
            end_character: Some(9),
        });
    })
    .expect("seed lsp diagnostic");

    // No problems yet — CodingState.problems only reflects the last refresh.
    let before = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .problems
        .clone();
    assert!(before.iter().all(|issue| issue.source != "lsp"));

    app.refresh_coding_problems().expect("refresh problems");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let issue = coding
        .problems
        .iter()
        .find(|issue| issue.source == "lsp")
        .unwrap_or_else(|| panic!("expected an lsp problem, got {:?}", coding.problems));
    assert_eq!(issue.source_label, "rust-analyzer");
    assert_eq!(issue.severity, ProblemSeverity::Warning);
    assert_eq!(issue.path.as_deref(), Some(path.as_str()));
    assert_eq!(issue.line, Some(2));
    assert_eq!(issue.column, Some(4));
    assert!(issue.message.contains("unused variable"));
    assert!(issue.can_jump());
}

#[test]
fn workspace_and_git_errors_appear_as_problems_after_refresh() {
    let data_dir = temp_dir("problems-workspace-data");
    let root = temp_dir("problems-workspace-root");
    fs::create_dir_all(&root).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    open_project(&app, root.clone(), "project:problems-workspace");

    app.with_coding_state(|coding| {
        coding.explorer.status = ExplorerStatus::Error("permission denied listing tree".into());
        coding.git = Some(GitStatusState {
            is_repository: true,
            last_error: Some("git status failed".into()),
            ..GitStatusState::default()
        });
    })
    .expect("seed workspace/git errors");

    app.refresh_coding_problems().expect("refresh problems");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let workspace_issues: Vec<_> = coding
        .problems
        .iter()
        .filter(|issue| issue.source == "workspace")
        .collect();
    assert!(
        workspace_issues
            .iter()
            .any(|issue| issue.message.contains("permission denied listing tree")),
        "missing explorer error problem: {:?}",
        coding.problems
    );
    assert!(
        workspace_issues
            .iter()
            .any(|issue| issue.message.contains("git status failed")),
        "missing git error problem: {:?}",
        coding.problems
    );
}

/// Planner / Permissions / Search / Memory providers are hard to trigger
/// end-to-end deterministically (they depend on live policy denials, index
/// state, and store health). Exercise them the way the task recommends when
/// that's the case: through the boot-registered registry with a hand-built
/// `ProblemsCollectContext`, which still proves the *installed* providers
/// (registered at boot, in boot order) behave correctly — not a standalone
/// copy.
#[test]
fn registered_providers_cover_planner_permissions_search_and_memory() {
    let data_dir = temp_dir("problems-context-data");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let registry = app.problems_registry().expect("problems registry");

    let ctx = ProblemsCollectContext {
        planner_blocked: true,
        planner_summary: Some("blocked by policy".into()),
        permission_decision: Some("Denied".into()),
        permission_denied: true,
        index_status: Some("Disabled".into()),
        index_detail: Some("indexing_enabled=false".into()),
        search_unhealthy: Some("no embeddings available".into()),
        understanding_failure: Some("parse failed for notes.pdf".into()),
        memory_unhealthy: true,
        memory_detail: Some("memory store unreachable".into()),
        ..ProblemsCollectContext::default()
    };
    let issues = registry.collect_all(&ctx).expect("collect_all");

    for (source, needle) in [
        ("planner", "blocked by policy"),
        ("permissions", "Denied"),
        ("search", "no embeddings available"),
        ("search", "parse failed for notes.pdf"),
        ("memory", "memory store unreachable"),
    ] {
        assert!(
            issues
                .iter()
                .any(|issue| issue.source == source && issue.message.contains(needle)),
            "missing {source} issue containing {needle:?}; got {issues:?}"
        );
    }

    // A permission-denied planner turn escalates from Warning to Error.
    let planner_issue = issues
        .iter()
        .find(|issue| issue.source == "planner")
        .expect("planner issue");
    assert_eq!(planner_issue.severity, ProblemSeverity::Error);
}

#[test]
fn open_problem_jumps_cursor_like_open_search_result() {
    let data_dir = temp_dir("problems-open-data");
    let root = temp_dir("problems-open-root");
    let path = root.join("main.rs");
    fs::write(&path, "fn main() {\n    let x = 1;\n}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    open_project(&app, root.clone(), "project:problems-open");

    let path_str = path.to_string_lossy().into_owned();
    app.with_coding_state(|coding| {
        coding.diagnostics.push(DiagnosticState {
            message: "unused variable `x`".into(),
            path: Some(path_str.clone()),
            severity: "warning".into(),
            source: "rust-analyzer".into(),
            line: Some(1),
            character: Some(8),
            end_line: Some(1),
            end_character: Some(9),
        });
    })
    .expect("seed lsp diagnostic");
    app.refresh_coding_problems().expect("refresh problems");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let issue = coding
        .problems
        .iter()
        .find(|issue| issue.source == "lsp")
        .expect("lsp issue");
    assert!(issue.can_jump());

    // `OpenProblem` (UI event) resolves to this exact call — see
    // `apps/jaymi/src/ui/mod.rs` handling of `CodingShellEvent::OpenProblem`.
    app.open_search_result(issue.path.as_deref().unwrap(), issue.line, issue.column)
        .expect("open problem jump");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.active_tab_path(), Some(path_str.as_str()));
    let session = coding
        .editors
        .session_by_path(&path_str)
        .expect("open session");
    assert_eq!(session.view.cursor.line, 1);
    assert_eq!(session.view.cursor.column, 8);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-problems-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap_or(dir)
}
