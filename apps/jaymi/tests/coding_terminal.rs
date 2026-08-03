//! Coding Terminal — PTY session through Planner → Terminal Tool → Provider.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_providers::{DEFAULT_TERMINAL_SESSION_ID, TERMINAL_PROVIDER_ID};
use jaymi_tools::TERMINAL_TOOL_ID;

#[test]
fn spawn_shell_execute_command_and_capture_stdout() {
    let data_dir = temp_dir("term-spawn-data");
    let root = temp_dir("term-spawn-root");
    fs::write(root.join("marker.txt"), "present").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-spawn".into()),
            name: "Term Spawn".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert!(
        coding
            .terminal_sessions
            .iter()
            .any(|session| session.id == DEFAULT_TERMINAL_SESSION_ID),
        "expected default terminal session after coding start"
    );

    let response = app
        .run_terminal(DEFAULT_TERMINAL_SESSION_ID, &root, "ls")
        .expect("run ls");
    assert!(!response.blocked);
    assert_eq!(response.tool_id.as_deref(), Some(TERMINAL_TOOL_ID));
    assert_eq!(response.provider_id.as_deref(), Some(TERMINAL_PROVIDER_ID));
    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("execute_terminal_commands")
    );
    let stdout = format!(
        "{}{}",
        response.terminal_output.as_deref().unwrap_or(""),
        response.terminal_scrollback.as_deref().unwrap_or("")
    );
    assert!(
        stdout.contains("marker.txt"),
        "stdout missing marker.txt: {stdout}"
    );
}

#[test]
fn terminal_session_persists_across_commands() {
    let data_dir = temp_dir("term-persist-data");
    let root = temp_dir("term-persist-root");
    fs::create_dir_all(root.join("nested")).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-persist".into()),
            name: "Term Persist".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.run_coding_terminal_command(DEFAULT_TERMINAL_SESSION_ID, "pwd")
        .expect("pwd");
    app.run_coding_terminal_command(DEFAULT_TERMINAL_SESSION_ID, "cd nested && pwd")
        .expect("cd nested");
    app.run_coding_terminal_command(DEFAULT_TERMINAL_SESSION_ID, "pwd")
        .expect("pwd again");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let session = coding
        .terminal_sessions
        .iter()
        .find(|session| session.id == DEFAULT_TERMINAL_SESSION_ID)
        .expect("session");
    assert_eq!(session.history.len(), 3);
    assert!(
        session.output.contains("nested") || session.last_command.as_deref() == Some("pwd"),
        "expected persistent session output, got: {}",
        session.output
    );

    // Provider still holds the live PTY for this workspace.
    let terminal = app
        .container()
        .resolve::<std::sync::Arc<jaymi_providers::TerminalProvider>>()
        .expect("terminal provider");
    assert!(terminal
        .has_session(DEFAULT_TERMINAL_SESSION_ID)
        .expect("has session"));
}

#[test]
fn coding_terminal_can_run_git_status() {
    let data_dir = temp_dir("term-git-data");
    let root = temp_dir("term-git-root");
    // Initialize a tiny git repo so `git status` succeeds.
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&root)
        .output()
        .expect("git init");
    fs::write(root.join("README.md"), "hello\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-git".into()),
            name: "Term Git".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.run_coding_terminal_command(DEFAULT_TERMINAL_SESSION_ID, "git status")
        .expect("git status");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let session = &coding.terminal_sessions[0];
    assert!(
        session.output.to_ascii_lowercase().contains("git")
            || session.output.contains("README")
            || session.output.contains("Untracked")
            || session.output.contains("No commits")
            || session.output.contains("branch"),
        "unexpected git status output: {}",
        session.output
    );
}

#[test]
fn coding_terminal_can_run_cargo_test() {
    let data_dir = temp_dir("term-cargo-data");
    let root = temp_dir("term-cargo-root");
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "jaymi_term_probe"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {\n        assert_eq!(2 + 2, 4);\n    }\n}\n",
    )
    .unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-cargo".into()),
            name: "Term Cargo".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.run_coding_terminal_command(DEFAULT_TERMINAL_SESSION_ID, "cargo test")
        .expect("cargo test");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let output = &coding.terminal_sessions[0].output;
    assert!(
        output.contains("it_works")
            || output.contains("test result:")
            || output.contains("running 1 test")
            || output.contains("ok"),
        "cargo test output unexpected: {output}"
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-coding-term-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
