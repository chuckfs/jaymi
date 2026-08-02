//! Integration tests for Slice 0.1 — real SQLite database.
//!
//! Verifies:
//! - first boot creates the database file
//! - migrations run to the current schema version
//! - reconnect reuses the existing database
//! - health reporting exposes connected / schema / migration status

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::Lifecycle;
use jaymi_database::{Database, MigrationStatus, CURRENT_SCHEMA_VERSION};

#[test]
fn first_boot_creates_database_and_runs_migrations() {
    let data_dir = temp_dir("db-first-boot");
    let db_path = data_dir.join("jaymi.db");
    assert!(!db_path.exists());

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    assert!(app.state().is_ready());
    assert!(db_path.exists());

    let database = app.container().resolve::<Database>().expect("database");
    assert!(database.is_connected());
    assert_eq!(database.schema_version(), CURRENT_SCHEMA_VERSION);
    assert_eq!(database.migration_status(), &MigrationStatus::Applied);
    assert_eq!(database.path(), db_path.as_path());

    let health = database.health_check();
    assert!(health.healthy);
    assert_eq!(detail(&health, "connected"), "true");
    assert_eq!(
        detail(&health, "schema_version"),
        CURRENT_SCHEMA_VERSION.to_string()
    );
    assert_eq!(detail(&health, "migration_status"), "applied");
    assert_eq!(detail(&health, "path"), db_path.display().to_string());

    let snapshot = app.diagnostics().expect("diagnostics");
    assert!(snapshot.database_connected);
    assert_eq!(
        snapshot.database_path.as_ref().map(PathBuf::from),
        Some(db_path)
    );
    assert_eq!(
        snapshot.database_schema_version,
        Some(CURRENT_SCHEMA_VERSION)
    );
    assert_eq!(
        snapshot.database_migration_status.as_deref(),
        Some("applied")
    );
}

#[test]
fn reconnect_works_after_shutdown() {
    let data_dir = temp_dir("db-reconnect");
    let db_path = data_dir.join("jaymi.db");

    let mut app = Application::boot_with_data_dir(&data_dir).expect("first boot");
    assert!(db_path.exists());
    let first_version = app
        .container()
        .resolve::<Database>()
        .expect("database")
        .schema_version();
    app.shutdown().expect("shutdown");

    let app = Application::boot_with_data_dir(&data_dir).expect("second boot");
    let database = app.container().resolve::<Database>().expect("database");
    assert!(database.is_connected());
    assert_eq!(database.schema_version(), first_version);
    assert_eq!(database.schema_version(), CURRENT_SCHEMA_VERSION);
    assert_eq!(database.migration_status(), &MigrationStatus::Applied);
    assert!(database.health_check().healthy);
}

#[test]
fn health_reporting_is_accurate_before_and_after_init() {
    let data_dir = temp_dir("db-health");
    let mut database = Database::with_data_dir(&data_dir);

    let before = database.health_check();
    assert!(!before.initialized);
    assert!(!before.healthy);
    assert_eq!(detail(&before, "connected"), "false");
    assert_eq!(detail(&before, "schema_version"), "0");
    assert_eq!(detail(&before, "migration_status"), "not_started");

    database.initialize().expect("initialize");
    let after = database.health_check();
    assert!(after.initialized);
    assert!(after.healthy);
    assert_eq!(detail(&after, "connected"), "true");
    assert_eq!(
        detail(&after, "schema_version"),
        CURRENT_SCHEMA_VERSION.to_string()
    );
    assert_eq!(detail(&after, "migration_status"), "applied");

    database.shutdown().expect("shutdown");
    let shut = database.health_check();
    assert!(!shut.initialized);
    assert!(!shut.healthy);
    assert_eq!(detail(&shut, "connected"), "false");
}

fn detail(report: &jaymi_core::HealthReport, key: &str) -> String {
    report
        .details
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-db-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
