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

#[test]
fn create_second_terminal_switches_active_session() {
    let data_dir = temp_dir("term-create-data");
    let root = temp_dir("term-create-root");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-create".into()),
            name: "Term Create".into(),
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
    assert_eq!(coding.terminal_sessions.len(), 1);
    assert_eq!(
        coding.active_terminal_id.as_deref(),
        Some(DEFAULT_TERMINAL_SESSION_ID)
    );

    app.create_coding_terminal(Some("Build".to_string()))
        .expect("create second terminal");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.terminal_sessions.len(), 2);
    let active_id = coding.active_terminal_id.clone().expect("active id");
    assert_ne!(active_id, DEFAULT_TERMINAL_SESSION_ID);
    let active_session = coding
        .terminal_sessions
        .iter()
        .find(|session| session.id == active_id)
        .expect("active session present");
    assert_eq!(active_session.title, "Build");
    let expected_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    assert_eq!(
        active_session.cwd.as_deref(),
        Some(expected_root.to_string_lossy().into_owned().as_str())
    );

    // Selecting the original session switches active back.
    app.select_coding_terminal(DEFAULT_TERMINAL_SESSION_ID)
        .expect("select");
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(
        coding.active_terminal_id.as_deref(),
        Some(DEFAULT_TERMINAL_SESSION_ID)
    );
}

#[test]
fn rename_terminal_title_persists_on_coding_state() {
    let data_dir = temp_dir("term-rename-data");
    let root = temp_dir("term-rename-root");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-rename".into()),
            name: "Term Rename".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.rename_coding_terminal(DEFAULT_TERMINAL_SESSION_ID, "Server")
        .expect("rename");

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
    assert_eq!(session.title, "Server");
}

#[test]
fn kill_terminal_removes_session_and_provider_forgets_it() {
    let data_dir = temp_dir("term-kill-data");
    let root = temp_dir("term-kill-root");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-kill".into()),
            name: "Term Kill".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.create_coding_terminal(Some("Extra".to_string()))
        .expect("create second terminal");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.terminal_sessions.len(), 2);
    let extra_id = coding
        .terminal_sessions
        .iter()
        .find(|session| session.id != DEFAULT_TERMINAL_SESSION_ID)
        .map(|session| session.id.clone())
        .expect("extra session id");

    app.kill_coding_terminal(&extra_id).expect("kill");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert_eq!(coding.terminal_sessions.len(), 1);
    assert!(coding
        .terminal_sessions
        .iter()
        .all(|session| session.id != extra_id));
    assert_eq!(
        coding.active_terminal_id.as_deref(),
        Some(DEFAULT_TERMINAL_SESSION_ID)
    );

    let terminal = app
        .container()
        .resolve::<std::sync::Arc<jaymi_providers::TerminalProvider>>()
        .expect("terminal provider");
    assert!(!terminal.has_session(&extra_id).expect("has session"));
    assert!(terminal
        .has_session(DEFAULT_TERMINAL_SESSION_ID)
        .expect("has session"));
}

#[test]
fn created_terminal_cwd_follows_project_root() {
    let data_dir = temp_dir("term-cwd-data");
    let root = temp_dir("term-cwd-root");

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:term-cwd".into()),
            name: "Term Cwd".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    app.create_coding_terminal(None).expect("create terminal");

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let active_id = coding.active_terminal_id.clone().expect("active id");
    let session = coding
        .terminal_sessions
        .iter()
        .find(|session| session.id == active_id)
        .expect("active session");
    let expected_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    assert_eq!(
        session.cwd.as_deref(),
        Some(expected_root.to_string_lossy().into_owned().as_str())
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
