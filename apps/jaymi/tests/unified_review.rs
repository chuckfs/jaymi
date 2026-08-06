//! Unified Review lifecycle — every entry point shares one approval path.
//!
//! ExecutionPlan → Review → Planner → Approved → Execution
//!
//! Conversation cards and Coding / Git / Terminal / LSP gestures may differ in
//! UI, but all emit [`ReviewIntent`] through [`Application::submit_review`].
//! Tools never execute outside an Approved plan.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_planner::{
    ApprovalDecision, ApprovalHistoryAccess, ApprovalHistoryQuery, ReviewIntent,
};
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_providers::DEFAULT_TERMINAL_SESSION_ID;
use jaymi_tools::{GIT_TOOL_ID, LANGUAGE_SERVER_TOOL_ID, TERMINAL_TOOL_ID, WRITE_FILE_TOOL_ID};

#[test]
fn conversation_review() {
    let data_dir = temp_dir("conversation-review-data");
    let root = temp_dir("conversation-review-root");
    let path = root.join("notes.md");
    fs::write(&path, "before\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:conversation-review".into()),
            name: "Conversation Review".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");

    let paused = app
        .handle_with_workspace(UserRequest::write_file(&path, "after\n"))
        .expect("pause for review");
    assert!(paused.awaiting_review);
    assert_eq!(paused.tool_id.as_deref(), Some(WRITE_FILE_TOOL_ID));
    assert_eq!(fs::read_to_string(&path).unwrap(), "before\n");

    let experience = app.experience().expect("experience");
    let card = experience
        .conversation()
        .iter()
        .rev()
        .find_map(|turn| turn.review.as_ref())
        .expect("conversation Review Card");
    assert!(card.state.is_pending());
    let plan_id = card.plan_id.clone();

    let resumed = app
        .submit_review(ReviewIntent::Approve {
            plan_id: plan_id.clone(),
        })
        .expect("approve via submit_review");
    assert!(!resumed.awaiting_review);
    assert!(!resumed.blocked);
    assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");

    assert_latest_approve(&app, &plan_id);
    let intent = app
        .experience()
        .expect("experience")
        .last_review_intent()
        .cloned();
    assert_eq!(intent.map(|i| i.as_str()), Some("approve"));
}

#[test]
fn coding_review() {
    let data_dir = temp_dir("coding-review-data");
    let root = temp_dir("coding-review-root");
    let path = root.join("main.rs");
    fs::write(&path, "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:coding-review".into()),
            name: "Coding Review".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let path_str = path.to_string_lossy().into_owned();
    app.open_coding_file(&path_str).expect("open");
    app.set_coding_tab_content(&path_str, "fn main() { /* saved */ }\n".into())
        .expect("edit");

    let paused = app
        .write_file(&path, "fn main() { /* saved */ }\n")
        .expect("write pauses");
    assert!(
        paused.awaiting_review,
        "Modify risk must pause before gesture approval"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "fn main() {}\n");
    let plan_id = paused
        .execution_plan
        .as_ref()
        .expect("plan")
        .id()
        .clone();

    let resumed = app
        .complete_user_initiated(paused)
        .expect("Save gesture auto-submits Approve");
    assert!(!resumed.awaiting_review);
    assert!(!resumed.blocked);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn main() { /* saved */ }\n"
    );

    assert_latest_approve(&app, &plan_id);
    assert_eq!(
        app.experience()
            .expect("experience")
            .last_review_intent()
            .map(ReviewIntent::as_str),
        Some("approve"),
        "gesture path must still record ReviewIntent::Approve"
    );
}

#[test]
fn terminal_review() {
    let data_dir = temp_dir("terminal-review-data");
    let root = temp_dir("terminal-review-root");
    fs::write(root.join("marker.txt"), "present").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:terminal-review".into()),
            name: "Terminal Review".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let paused = app
        .run_terminal(DEFAULT_TERMINAL_SESSION_ID, &root, "ls")
        .expect("run pauses");
    assert!(paused.awaiting_review);
    assert_eq!(paused.tool_id.as_deref(), Some(TERMINAL_TOOL_ID));
    let plan_id = paused
        .execution_plan
        .as_ref()
        .expect("plan")
        .id()
        .clone();

    let resumed = app
        .complete_user_initiated(paused)
        .expect("Run gesture auto-submits Approve");
    assert!(!resumed.awaiting_review);
    assert!(!resumed.blocked);
    let stdout = format!(
        "{}{}",
        resumed.terminal_output.as_deref().unwrap_or(""),
        resumed.terminal_scrollback.as_deref().unwrap_or("")
    );
    assert!(
        stdout.contains("marker.txt"),
        "stdout missing marker.txt: {stdout}"
    );

    assert_latest_approve(&app, &plan_id);
    assert_eq!(
        app.experience()
            .expect("experience")
            .last_review_intent()
            .map(ReviewIntent::as_str),
        Some("approve")
    );
}

#[test]
fn git_review() {
    let data_dir = temp_dir("git-review-data");
    let root = temp_dir("git-review-root");
    init_repo(&root);
    fs::write(root.join("readme.md"), "hello\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:git-review".into()),
            name: "Git Review".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let paused = app
        .git_stage(&root, vec![PathBuf::from("readme.md")])
        .expect("stage pauses");
    assert!(paused.awaiting_review);
    assert_eq!(paused.tool_id.as_deref(), Some(GIT_TOOL_ID));
    let plan_id = paused
        .execution_plan
        .as_ref()
        .expect("plan")
        .id()
        .clone();

    let resumed = app
        .complete_user_initiated(paused)
        .expect("Git gesture auto-submits Approve");
    assert!(!resumed.awaiting_review);
    assert!(!resumed.blocked);
    assert!(resumed
        .git_staged
        .iter()
        .any(|entry| entry.path == "readme.md"));

    assert_latest_approve(&app, &plan_id);
    assert_eq!(
        app.experience()
            .expect("experience")
            .last_review_intent()
            .map(ReviewIntent::as_str),
        Some("approve")
    );
}

#[test]
fn rename_review() {
    let data_dir = temp_dir("rename-review-data");
    let root = temp_dir("rename-review-root");
    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/main.rs");
    let content = "fn greet() {}\nfn main() { greet(); }\n";
    fs::write(&file, content).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:rename-review".into()),
            name: "Rename Review".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");
    let path = file.to_str().unwrap();
    app.open_coding_file(path).expect("open");

    // Pause without auto-approve to prove rename requires the same Review gate.
    let request = {
        let coding = app
            .capability_state()
            .unwrap()
            .unwrap()
            .coding()
            .unwrap()
            .clone();
        let workspace = coding
            .explorer
            .project_root
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        jaymi_core::LspRequest {
            operation: jaymi_core::LspOperation::Rename,
            workspace_root: workspace,
            path: Some(file.clone()),
            content: None,
            language: Some("rust".into()),
            line: Some(1),
            character: Some(13),
            new_name: Some("say_hello".into()),
            version: None,
        }
    };
    let paused = app.lsp(request).expect("rename pauses");
    assert!(
        paused.awaiting_review,
        "LSP rename must pause for Review like other mutating tools"
    );
    assert_eq!(paused.tool_id.as_deref(), Some(LANGUAGE_SERVER_TOOL_ID));
    let plan_id = paused
        .execution_plan
        .as_ref()
        .expect("plan")
        .id()
        .clone();

    let resumed = app
        .complete_user_initiated(paused)
        .expect("rename gesture auto-submits Approve");
    assert!(!resumed.awaiting_review);
    assert!(!resumed.blocked);
    assert!(
        resumed
            .lsp_edits
            .iter()
            .any(|edit| edit.new_text == "say_hello"),
        "expected rename edits after Approve, got {:?}",
        resumed.lsp_edits
    );

    assert_latest_approve(&app, &plan_id);
    assert_eq!(
        app.experience()
            .expect("experience")
            .last_review_intent()
            .map(ReviewIntent::as_str),
        Some("approve")
    );

    // Public Coding entry point must use the same auto-Approve path.
    let via_coding = app
        .coding_lsp_rename(path, 1, 13, "greet_again")
        .expect("coding_lsp_rename");
    assert!(!via_coding.awaiting_review);
    assert!(!via_coding.blocked);
    assert_eq!(
        app.experience()
            .expect("experience")
            .last_review_intent()
            .map(ReviewIntent::as_str),
        Some("approve")
    );
}

fn assert_latest_approve(app: &Application, plan_id: &jaymi_planner::ExecutionPlanId) {
    let history = app
        .search_approval_history(
            &ApprovalHistoryQuery {
                plan_id: Some(plan_id.clone()),
                limit: Some(5),
                ..ApprovalHistoryQuery::default()
            },
            ApprovalHistoryAccess::Full,
        )
        .expect("approval history");
    assert!(
        history
            .iter()
            .any(|entry| entry.decision == ApprovalDecision::Approve.as_str()),
        "expected Approve in approval history for {plan_id}, got {history:?}"
    );
}

fn init_repo(root: &std::path::Path) {
    let status = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .status()
        .expect("git init");
    assert!(status.success());
    let _ = Command::new("git")
        .args(["config", "user.email", "jaymi@test.local"])
        .current_dir(root)
        .status();
    let _ = Command::new("git")
        .args(["config", "user.name", "Jaymi Test"])
        .current_dir(root)
        .status();
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-unified-review-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
