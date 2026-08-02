//! Local SQLite knowledge store for Jaymi.
//!
//! Third subsystem in the deterministic boot sequence. Opens an embedded
//! SQLite database under the configured data directory, runs schema
//! migrations, and reports connection health.

#![forbid(unsafe_code)]

pub mod content;
pub mod conversations;
pub mod embeddings;
pub mod entities;
pub mod events;
pub mod inventory;
pub mod memory;
pub mod migrations;
pub mod projects;
pub mod relationships;

pub use content::{
    build_fts_and_query, build_fts_match_query, ContentCounts, ContentFtsHit, ContentMetadataHit,
    ContentMetadataQuery, ContentRecord,
};
pub use conversations::{
    ConversationAttachmentRecord, ConversationMessageRecord, ConversationRecord,
    ConversationReferenceRecord, LoadedConversationRecord, LoadedMessageRecord,
};
pub use embeddings::{
    blob_to_vector, content_embedding_hash, vector_to_blob, EmbeddingCounts, EmbeddingQueueItem,
    EmbeddingRecord, EmbeddingSimilarityHit,
};
pub use inventory::{
    DiscoveredCounts, DiscoveredItemRecord, DiscoveredQuery, DiscoveryScanInput,
    DiscoveryScanRecord, DiscoverySort,
};
pub use memory::{ConversationArchiveRecord, MemoryRecord, MemorySearchQuery};
pub use projects::ProjectRecord;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
pub use migrations::CURRENT_SCHEMA_VERSION;
use migrations::{migrate, verify_schema};

const NAME: &str = "database";
const DEPENDENCIES: &[&str] = &["configuration", "logging"];
const DATABASE_FILE_NAME: &str = "jaymi.db";

/// Status of schema migrations for diagnostics and health reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    /// Migrations have not been attempted yet.
    NotStarted,
    /// Schema is at the expected version.
    Applied,
    /// The last migration attempt failed.
    Failed(String),
}

impl MigrationStatus {
    /// Stable short label for diagnostics.
    pub fn label(&self) -> &str {
        match self {
            Self::NotStarted => "not_started",
            Self::Applied => "applied",
            Self::Failed(_) => "failed",
        }
    }

    /// Human-readable status including failure detail when present.
    pub fn display(&self) -> String {
        match self {
            Self::NotStarted => "not_started".to_string(),
            Self::Applied => "applied".to_string(),
            Self::Failed(message) => format!("failed: {message}"),
        }
    }
}

/// Persistent knowledge store connection lifecycle.
pub struct Database {
    initialized: bool,
    path: PathBuf,
    /// Open SQLite connection. Wrapped in a mutex so [`Database`] stays `Sync`
    /// for the shared [`Lifecycle`] bound (`Connection` itself is not `Sync`).
    connection: Option<Mutex<Connection>>,
    schema_version: u32,
    migration_status: MigrationStatus,
}

impl Database {
    /// Create an uninitialized database using the default data directory.
    ///
    /// Prefer [`Database::with_data_dir`] when the configured data directory is
    /// available so the file is stored in the correct location.
    pub fn new() -> Self {
        Self::with_data_dir(default_data_dir())
    }

    /// Create an uninitialized database stored under `data_dir/jaymi.db`.
    pub fn with_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self {
            initialized: false,
            path: data_dir.as_ref().join(DATABASE_FILE_NAME),
            connection: None,
            schema_version: 0,
            migration_status: MigrationStatus::NotStarted,
        }
    }

    /// Absolute path to the SQLite database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns true when the database reports an active connection.
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    /// Highest applied schema version, or `0` before migrations succeed.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Latest migration outcome.
    pub fn migration_status(&self) -> &MigrationStatus {
        &self.migration_status
    }

    fn open_and_migrate(&mut self) -> JaymiResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                JaymiError::new(format!(
                    "failed to create database directory {}: {error}",
                    parent.display()
                ))
            })?;
        }

        let connection = Connection::open(&self.path).map_err(|error| {
            JaymiError::new(format!(
                "failed to open database {}: {error}",
                self.path.display()
            ))
        })?;

        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| JaymiError::new(format!("failed to enable foreign keys: {error}")))?;

        let version = migrate(&connection).map_err(|error| {
            JaymiError::new(format!(
                "schema migration failed for {}: {}",
                self.path.display(),
                error.message()
            ))
        })?;
        verify_schema(&connection).map_err(|error| {
            JaymiError::new(format!(
                "schema verification failed for {}: {}",
                self.path.display(),
                error.message()
            ))
        })?;

        self.schema_version = version;
        self.migration_status = MigrationStatus::Applied;
        self.connection = Some(Mutex::new(connection));
        Ok(())
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database")
            .field("initialized", &self.initialized)
            .field("path", &self.path)
            .field("connected", &self.connection.is_some())
            .field("schema_version", &self.schema_version)
            .field("migration_status", &self.migration_status)
            .finish()
    }
}

impl Lifecycle for Database {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        match self.open_and_migrate() {
            Ok(()) => {
                self.initialized = true;
                Ok(())
            }
            Err(error) => {
                self.initialized = false;
                self.connection = None;
                self.schema_version = 0;
                self.migration_status = MigrationStatus::Failed(error.message().to_string());
                Err(error)
            }
        }
    }

    fn health_check(&self) -> HealthReport {
        let connected = self.is_connected();
        let migrations_ok = matches!(self.migration_status, MigrationStatus::Applied);
        let schema_ok = self.schema_version == CURRENT_SCHEMA_VERSION;
        let healthy = self.initialized && connected && migrations_ok && schema_ok;

        HealthReport::new(
            NAME,
            self.initialized,
            healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            ("connected".to_string(), connected.to_string()),
            (
                "schema_version".to_string(),
                self.schema_version.to_string(),
            ),
            (
                "migration_status".to_string(),
                self.migration_status.display(),
            ),
            ("path".to_string(), self.path.display().to_string()),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.connection = None;
        self.initialized = false;
        // Retain path, schema_version, and last migration_status for diagnostics
        // after a clean shutdown until the next initialize.
        Ok(())
    }
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local").join("share").join("jaymi"))
        .unwrap_or_else(|| PathBuf::from("./data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn initialize_creates_sqlite_file_and_schema() {
        let dir = temp_dir("init");
        let mut db = Database::with_data_dir(&dir);
        db.initialize().unwrap();

        assert!(db.is_connected());
        assert!(db.path().exists());
        assert_eq!(db.schema_version(), CURRENT_SCHEMA_VERSION);
        assert_eq!(db.migration_status(), &MigrationStatus::Applied);

        let health = db.health_check();
        assert!(health.healthy);
        assert!(health.initialized);
        assert!(detail(&health, "connected") == "true");
        assert_eq!(
            detail(&health, "schema_version"),
            CURRENT_SCHEMA_VERSION.to_string()
        );
        assert_eq!(detail(&health, "migration_status"), "applied");

        db.shutdown().unwrap();
        assert!(!db.is_connected());
        assert!(!db.health_check().healthy);
    }

    #[test]
    fn reconnect_reuses_existing_database() {
        let dir = temp_dir("reconnect");
        let mut db = Database::with_data_dir(&dir);
        db.initialize().unwrap();
        let path = db.path().to_path_buf();
        db.shutdown().unwrap();

        let mut db = Database::with_data_dir(&dir);
        db.initialize().unwrap();
        assert_eq!(db.path(), path.as_path());
        assert_eq!(db.schema_version(), CURRENT_SCHEMA_VERSION);
        assert!(db.health_check().healthy);
    }

    #[test]
    fn second_initialize_is_idempotent() {
        let dir = temp_dir("idempotent");
        let mut db = Database::with_data_dir(&dir);
        db.initialize().unwrap();
        db.shutdown().unwrap();
        db.initialize().unwrap();
        assert_eq!(db.schema_version(), CURRENT_SCHEMA_VERSION);
        assert_eq!(db.migration_status(), &MigrationStatus::Applied);
    }

    fn detail(report: &HealthReport, key: &str) -> String {
        report
            .details
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-db-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
