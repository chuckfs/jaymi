//! Coding Git panel — status / stage / unstage / commit through Planner → Git Tool → Provider.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_providers::GIT_PROVIDER_ID;
use jaymi_tools::GIT_TOOL_ID;

#[test]
fn git_status_stage_unstage_and_commit_metadata() {
    let data_dir = temp_dir("git-panel-data");
    let root = temp_dir("git-panel-root");
    init_repo(&root);
    fs::write(root.join("readme.md"), "hello\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:git-panel".into()),
            name: "Git Panel".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.refresh_coding_git().expect("status");
    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git state");
    assert!(git.is_repository);
    assert!(git.branch.is_some());
    assert!(
        git.untracked.iter().any(|entry| entry.path == "readme.md"),
        "expected readme.md untracked, got {:?}",
        git.untracked
    );
    assert!(git.staged.is_empty());
    assert!(git.added.is_empty());

    let response = app
        .git_stage(&root, vec![PathBuf::from("readme.md")])
        .expect("stage via planner");
    assert!(!response.blocked);
    assert_eq!(response.tool_id.as_deref(), Some(GIT_TOOL_ID));
    assert_eq!(response.provider_id.as_deref(), Some(GIT_PROVIDER_ID));
    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("code")
    );
    assert_eq!(response.git_is_repository, Some(true));
    assert!(response
        .git_staged
        .iter()
        .any(|entry| entry.path == "readme.md"));
    assert!(response
        .git_added
        .iter()
        .any(|entry| entry.path == "readme.md"));
    assert!(!response
        .git_untracked
        .iter()
        .any(|entry| entry.path == "readme.md"));

    app.coding_git_stage(&["readme.md".into()]).expect("sync state");
    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git after stage");
    assert!(git.staged.iter().any(|entry| entry.path == "readme.md"));
    assert!(git.added.iter().any(|entry| entry.path == "readme.md"));
    assert!(!git.untracked.iter().any(|entry| entry.path == "readme.md"));

    app.coding_git_unstage(&["readme.md".into()])
        .expect("unstage");
    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git after unstage");
    assert!(!git.staged.iter().any(|entry| entry.path == "readme.md"));
    assert!(!git.added.iter().any(|entry| entry.path == "readme.md"));
    assert!(git.untracked.iter().any(|entry| entry.path == "readme.md"));

    app.coding_git_stage(&["readme.md".into()]).expect("restage");
    app.set_coding_git_commit_message("add readme".into())
        .expect("message");
    app.coding_git_commit_active().expect("commit");

    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git after commit");
    assert!(!git.staged.iter().any(|entry| entry.path == "readme.md"));
    assert!(!git.untracked.iter().any(|entry| entry.path == "readme.md"));
    assert!(git.modified.is_empty());
    assert!(git.added.is_empty());
    assert!(git.deleted.is_empty());
    assert!(git.commit_message.is_empty());

    let log = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .expect("git log");
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "add readme");
}

#[test]
fn git_detects_non_repository_and_classifies_modified_deleted() {
    let data_dir = temp_dir("git-detect-data");
    let root = temp_dir("git-detect-root");
    // Not a git repo yet.
    fs::write(root.join("orphan.txt"), "x\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:git-detect".into()),
            name: "Git Detect".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");
    app.refresh_coding_git().expect("status");

    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git");
    assert!(!git.is_repository);
    assert!(git.summary.contains("not a git repository"));

    // Initialize repo and create modified + deleted entries.
    init_repo(&root);
    fs::write(root.join("keep.txt"), "v1\n").unwrap();
    fs::write(root.join("gone.txt"), "x\n").unwrap();
    app.coding_git_stage(&["keep.txt".into(), "gone.txt".into()])
        .expect("stage init");
    app.set_coding_git_commit_message("init".into())
        .expect("msg");
    app.coding_git_commit_active().expect("commit init");

    fs::write(root.join("keep.txt"), "v2\n").unwrap();
    fs::remove_file(root.join("gone.txt")).unwrap();
    app.refresh_coding_git().expect("refresh");

    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git after edits");
    assert!(git.is_repository);
    assert!(git.modified.iter().any(|entry| entry.path == "keep.txt"));
    assert!(git.deleted.iter().any(|entry| entry.path == "gone.txt"));
}

#[test]
fn git_discard_requires_confirmation_then_restores() {
    let data_dir = temp_dir("git-discard-data");
    let root = temp_dir("git-discard-root");
    init_repo(&root);
    fs::write(root.join("tracked.txt"), "v1\n").unwrap();
    Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "user.name=Jaymi",
            "-c",
            "user.email=jaymi@local",
            "add",
            "tracked.txt",
        ])
        .output()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "user.name=Jaymi",
            "-c",
            "user.email=jaymi@local",
            "commit",
            "-m",
            "init",
        ])
        .output()
        .unwrap();
    fs::write(root.join("tracked.txt"), "v2\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:git-discard".into()),
            name: "Git Discard".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");
    app.refresh_coding_git().expect("status");

    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git");
    assert_eq!(git.modified.len(), 1);

    app.coding_git_request_discard(&["tracked.txt".into()])
        .expect("request discard");
    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("pending");
    assert_eq!(
        git.pending_discard.as_deref(),
        Some(["tracked.txt".to_string()].as_slice())
    );
    // File still dirty until confirmed.
    assert_eq!(fs::read_to_string(root.join("tracked.txt")).unwrap(), "v2\n");

    app.coding_git_cancel_discard().expect("cancel");
    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("cancelled");
    assert!(git.pending_discard.is_none());
    assert_eq!(fs::read_to_string(root.join("tracked.txt")).unwrap(), "v2\n");

    app.coding_git_request_discard(&["tracked.txt".into()])
        .expect("request again");
    app.coding_git_confirm_discard(None).expect("confirm");
    assert_eq!(fs::read_to_string(root.join("tracked.txt")).unwrap(), "v1\n");
    let git = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .git
        .clone()
        .expect("git after discard");
    assert!(git.modified.is_empty());
    assert!(git.pending_discard.is_none());
    assert!(!git.untracked.iter().any(|entry| entry.path == "tracked.txt"));
}

fn init_repo(root: &std::path::Path) {
    let init = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(root)
        .output()
        .expect("git init");
    if !init.status.success() {
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .expect("git init fallback");
        let _ = Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(root)
            .output();
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-git-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
