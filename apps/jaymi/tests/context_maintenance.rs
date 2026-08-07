//! Background context maintenance must not block conversation prepare.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jaymi::{Application, MaintenanceKind};
use jaymi_context::ContextEngine;
use jaymi_core::UserRequest;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-maint-it-{}-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn prepare_merges_completed_maintenance_without_blocking_conversation() {
    let data = temp_dir("data");
    let root = temp_dir("project");
    fs::write(root.join("readme.md"), "# hello\n").unwrap();

    let app = Application::boot_with_data_dir(&data).unwrap();
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:maint".into()),
            name: "Maint".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .unwrap();
    app.open_project(project.id.as_str()).unwrap();
    app.start_coding_project().unwrap();

    // Explicit schedule in case coding-open jobs already finished / deduped.
    let _ = app.schedule_context_maintenance(MaintenanceKind::GitStatus);
    let _ = app.schedule_context_maintenance(MaintenanceKind::WorkspaceInventory);
    let _ = app.schedule_context_maintenance(MaintenanceKind::FileSummaries);

    let started = Instant::now();
    let mut saw_git = false;
    let mut saw_inventory = false;
    while started.elapsed() < Duration::from_secs(4) {
        let _ = app.pump_context_maintenance();
        let prepare_started = Instant::now();
        let _ = app.handle(UserRequest::new("hello"));
        assert!(
            prepare_started.elapsed() < Duration::from_millis(2000),
            "conversation prepare must not block on maintenance I/O"
        );

        let context = app.container().resolve::<Arc<ContextEngine>>().unwrap();
        let session = context.session_inputs();
        if !session.git_status.summary.is_empty() || session.git_status.is_repository {
            saw_git = true;
        }
        if !session.workspace_inventory.status.is_empty()
            || session.workspace_inventory.root.is_some()
        {
            saw_inventory = true;
        }
        if saw_git && saw_inventory {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(saw_git, "expected completed git snapshot merged into session");
    assert!(
        saw_inventory,
        "expected completed inventory snapshot merged into session"
    );

    let _ = fs::remove_dir_all(&data);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ambient_workspace_snapshot_feeds_conversation_without_blocking() {
    let data = temp_dir("data-ws");
    let root = temp_dir("project-ws");
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
    fs::write(root.join("lib.rs"), "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data).unwrap();
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:ws-ambient".into()),
            name: "Ambient".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .unwrap();
    app.open_project(project.id.as_str()).unwrap();
    app.start_coding_project().unwrap();

    let file = root.join("lib.rs");
    app.open_coding_file(file.to_str().unwrap()).unwrap();
    app.set_coding_tab_cursor(file.to_str().unwrap(), 0, 3)
        .unwrap();

    let started = Instant::now();
    let mut saw_snapshot = false;
    while started.elapsed() < Duration::from_secs(4) {
        let _ = app.pump_context_maintenance();
        let prepare_started = Instant::now();
        let _ = app.handle(UserRequest::new("hello"));
        assert!(
            prepare_started.elapsed() < Duration::from_millis(2000),
            "conversation prepare must not block on ambient WorkspaceSnapshot"
        );

        let context = app.container().resolve::<Arc<ContextEngine>>().unwrap();
        let session = context.session_inputs();
        if let Some(snapshot) = session.workspace_snapshot.as_ref() {
            if snapshot.current_file.path.as_deref() == file.to_str()
                && snapshot.cursor.map(|c| c.column) == Some(3)
            {
                saw_snapshot = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(
        saw_snapshot,
        "expected completed ambient WorkspaceSnapshot merged into conversation session"
    );

    let _ = fs::remove_dir_all(&data);
    let _ = fs::remove_dir_all(&root);
}
