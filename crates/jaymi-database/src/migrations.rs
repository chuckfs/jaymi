//! Schema migrations for the local SQLite store.

use rusqlite::{params, Connection};

use jaymi_core::{JaymiError, JaymiResult};

/// Latest schema version applied by this build.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
        CREATE TABLE schema_version (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE system_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            payload TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE settings (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
    "#,
}];

/// Read the highest applied schema version, or `0` when uninitialized.
pub fn current_version(connection: &Connection) -> JaymiResult<u32> {
    let table_exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;

    if table_exists == 0 {
        return Ok(0);
    }

    let version: u32 = connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    Ok(version)
}

/// Apply all pending migrations inside transactions.
///
/// Returns the schema version after migrations complete.
pub fn migrate(connection: &Connection) -> JaymiResult<u32> {
    let mut version = current_version(connection)?;

    for migration in MIGRATIONS {
        if migration.version <= version {
            continue;
        }

        let tx = connection
            .unchecked_transaction()
            .map_err(db_error)?;
        tx.execute_batch(migration.sql).map_err(db_error)?;
        tx.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
            params![migration.version],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        version = migration.version;
    }

    if version != CURRENT_SCHEMA_VERSION {
        return Err(JaymiError::new(format!(
            "schema migration incomplete: at version {version}, expected {CURRENT_SCHEMA_VERSION}"
        )));
    }

    Ok(version)
}

/// Confirm required Slice 0.1 tables exist after migration.
pub fn verify_schema(connection: &Connection) -> JaymiResult<()> {
    for table in ["schema_version", "system_events", "settings"] {
        let exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                params![table],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if exists == 0 {
            return Err(JaymiError::new(format!(
                "required table '{table}' is missing after migration"
            )));
        }
    }
    Ok(())
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
