//! SQLite schema migrations for the knowledge store.

use rusqlite::Connection;

use jaymi_core::{JaymiError, JaymiResult};

/// Apply the Layer 1 schema if it is not already present.
pub fn migrate(conn: &Connection) -> JaymiResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS index_roots (
            label TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_scan_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS indexed_files (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            parent_path TEXT NOT NULL,
            entry_type TEXT NOT NULL,
            extension TEXT,
            size_bytes INTEGER NOT NULL,
            modified_unix INTEGER,
            source_root TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_indexed_files_name
            ON indexed_files(name);
        CREATE INDEX IF NOT EXISTS idx_indexed_files_root
            ON indexed_files(source_root);
        CREATE INDEX IF NOT EXISTS idx_indexed_files_parent
            ON indexed_files(parent_path);
        CREATE INDEX IF NOT EXISTS idx_indexed_files_extension
            ON indexed_files(extension);
        "#,
    )
    .map_err(|error| JaymiError::new(format!("schema migration failed: {error}")))?;

    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, strftime('%s','now'))",
        [],
    )
    .map_err(|error| JaymiError::new(format!("failed to record migration: {error}")))?;

    Ok(())
}
