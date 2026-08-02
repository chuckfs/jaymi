//! Integration tests for Slice 0.2 — local rotating file logging.
//!
//! Verifies:
//! - logging initializes under the configured data directory
//! - startup creates a log file
//! - planner / tool / provider activity is recorded
//! - diagnostics expose logging health

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::Lifecycle;
use jaymi_logging::Logger;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn boot_initializes_logging_and_creates_log_file() {
    let _guard = TEST_LOCK.lock().unwrap();
    let data_dir = temp_dir("logging-boot");
    let log_path = data_dir.join("logs").join("jaymi.log");
    assert!(!log_path.exists());

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    assert!(app.state().is_ready());
    assert!(log_path.exists());

    let logger = app.container().resolve::<Logger>().expect("logger");
    assert!(logger.is_initialized());
    assert!(logger.health_check().healthy);
    assert_eq!(logger.log_path(), log_path.as_path());

    let contents = fs::read_to_string(&log_path).expect("read log");
    assert!(contents.contains("[boot] Jaymi startup"));
    assert!(contents.contains("[logging] local file logging initialized"));
    assert!(contents.contains("[boot] Jaymi startup complete"));

    let snapshot = app.diagnostics().expect("diagnostics");
    assert!(snapshot.logging_healthy);
    assert_eq!(
        snapshot.logging_path.as_ref().map(PathBuf::from),
        Some(log_path)
    );
    assert_eq!(
        snapshot.logging_dir.as_ref().map(PathBuf::from),
        Some(data_dir.join("logs"))
    );
}

#[test]
fn planner_tool_and_provider_activity_is_logged() {
    let _guard = TEST_LOCK.lock().unwrap();
    let data_dir = temp_dir("logging-activity");
    let work_dir = temp_dir("logging-workdir");
    fs::write(work_dir.join("note.txt"), "hello").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.list_directory(&work_dir).expect("list");
    app.read_file(work_dir.join("note.txt")).expect("read");

    let contents = fs::read_to_string(data_dir.join("logs").join("jaymi.log")).expect("log");
    assert!(contents.contains("[planner] request received"));
    assert!(contents.contains("[tools] execute tool=search_files"));
    assert!(contents.contains("[providers] filesystem list_directory"));
    assert!(contents.contains("[tools] execute tool=read_file"));
    assert!(contents.contains("[providers] filesystem read_file"));
}

#[test]
fn shutdown_is_logged() {
    let _guard = TEST_LOCK.lock().unwrap();
    let data_dir = temp_dir("logging-shutdown");
    let mut app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.shutdown().expect("shutdown");

    let contents = fs::read_to_string(data_dir.join("logs").join("jaymi.log")).expect("log");
    assert!(contents.contains("[boot] Jaymi shutdown beginning"));
    assert!(contents.contains("[logging] local file logging shutting down"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-log-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
