//! Schema migrations for the local SQLite store.

use rusqlite::{params, Connection};

use jaymi_core::{JaymiError, JaymiResult};

/// Latest schema version applied by this build.
pub const CURRENT_SCHEMA_VERSION: u32 = 9;

struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
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
    },
    Migration {
        version: 2,
        sql: r#"
        CREATE TABLE discovered_items (
            path TEXT PRIMARY KEY NOT NULL,
            filename TEXT NOT NULL,
            extension TEXT,
            size INTEGER NOT NULL,
            created INTEGER,
            modified INTEGER,
            is_directory INTEGER NOT NULL,
            hidden INTEGER NOT NULL,
            parent TEXT
        );

        CREATE INDEX idx_discovered_items_parent ON discovered_items(parent);
        CREATE INDEX idx_discovered_items_extension ON discovered_items(extension);
        CREATE INDEX idx_discovered_items_is_directory ON discovered_items(is_directory);

        CREATE TABLE discovery_scans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at INTEGER NOT NULL,
            finished_at INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            roots_json TEXT NOT NULL,
            files_seen INTEGER NOT NULL,
            folders_seen INTEGER NOT NULL,
            status TEXT NOT NULL
        );
    "#,
    },
    Migration {
        version: 3,
        sql: r#"
        ALTER TABLE discovered_items ADD COLUMN first_discovered INTEGER;
        ALTER TABLE discovered_items ADD COLUMN last_indexed INTEGER;
        ALTER TABLE discovered_items ADD COLUMN last_modified INTEGER;
        ALTER TABLE discovered_items ADD COLUMN last_verified INTEGER;
        ALTER TABLE discovered_items ADD COLUMN device_id INTEGER;
        ALTER TABLE discovered_items ADD COLUMN inode INTEGER;

        UPDATE discovered_items SET
            first_discovered = COALESCE(created, modified, CAST(strftime('%s','now') AS INTEGER)),
            last_indexed = COALESCE(modified, created, CAST(strftime('%s','now') AS INTEGER)),
            last_modified = modified,
            last_verified = COALESCE(modified, created, CAST(strftime('%s','now') AS INTEGER))
        WHERE first_discovered IS NULL;

        CREATE INDEX IF NOT EXISTS idx_discovered_items_inode
            ON discovered_items(device_id, inode);

        ALTER TABLE discovery_scans ADD COLUMN files_added INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE discovery_scans ADD COLUMN files_updated INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE discovery_scans ADD COLUMN files_removed INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE discovery_scans ADD COLUMN files_unchanged INTEGER NOT NULL DEFAULT 0;
    "#,
    },
    Migration {
        version: 4,
        sql: r#"
        CREATE TABLE content (
            content_id TEXT PRIMARY KEY NOT NULL,
            source_id TEXT NOT NULL UNIQUE,
            content_type TEXT NOT NULL,
            plain_text TEXT NOT NULL,
            title TEXT,
            language TEXT,
            parser_used TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            extraction_timestamp INTEGER NOT NULL,
            FOREIGN KEY (source_id) REFERENCES discovered_items(path) ON DELETE CASCADE
        );

        CREATE INDEX idx_content_source_id ON content(source_id);
        CREATE INDEX idx_content_content_type ON content(content_type);
        CREATE INDEX idx_content_parser_used ON content(parser_used);
    "#,
    },
    Migration {
        version: 5,
        sql: r#"
        ALTER TABLE content ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE content ADD COLUMN character_count INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE content ADD COLUMN reading_time_seconds INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE content ADD COLUMN headings_json TEXT NOT NULL DEFAULT '[]';
        ALTER TABLE content ADD COLUMN sections_json TEXT NOT NULL DEFAULT '[]';
        ALTER TABLE content ADD COLUMN internal_links_json TEXT NOT NULL DEFAULT '[]';
        ALTER TABLE content ADD COLUMN external_links_json TEXT NOT NULL DEFAULT '[]';
        ALTER TABLE content ADD COLUMN enrichment_version TEXT NOT NULL DEFAULT '1';
    "#,
    },
    Migration {
        version: 6,
        sql: r#"
        ALTER TABLE content ADD COLUMN image_metadata_json TEXT;
    "#,
    },
    Migration {
        version: 7,
        sql: r#"
        CREATE VIRTUAL TABLE content_fts USING fts5(
            source_id UNINDEXED,
            title,
            plain_text,
            tokenize = 'unicode61'
        );

        INSERT INTO content_fts(source_id, title, plain_text)
        SELECT source_id, COALESCE(title, ''), plain_text FROM content;

        CREATE TRIGGER content_fts_ad AFTER DELETE ON content BEGIN
            DELETE FROM content_fts WHERE source_id = old.source_id;
        END;
    "#,
    },
    Migration {
        version: 8,
        sql: r#"
        ALTER TABLE content ADD COLUMN author TEXT;
        ALTER TABLE content ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';

        CREATE INDEX IF NOT EXISTS idx_content_language ON content(language);
        CREATE INDEX IF NOT EXISTS idx_content_author ON content(author);
    "#,
    },
    Migration {
        version: 9,
        sql: r#"
        CREATE TABLE content_embeddings (
            source_id TEXT PRIMARY KEY NOT NULL,
            model_id TEXT NOT NULL,
            dims INTEGER NOT NULL,
            vector BLOB NOT NULL,
            content_hash TEXT NOT NULL,
            embedded_at INTEGER NOT NULL,
            FOREIGN KEY (source_id) REFERENCES content(source_id) ON DELETE CASCADE
        );

        CREATE INDEX idx_content_embeddings_model ON content_embeddings(model_id);

        CREATE TABLE embedding_queue (
            source_id TEXT PRIMARY KEY NOT NULL,
            enqueued_at INTEGER NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            FOREIGN KEY (source_id) REFERENCES content(source_id) ON DELETE CASCADE
        );
    "#,
    },
];

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

        let tx = connection.unchecked_transaction().map_err(db_error)?;
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

/// Confirm required tables exist after migration.
pub fn verify_schema(connection: &Connection) -> JaymiResult<()> {
    for table in [
        "schema_version",
        "system_events",
        "settings",
        "discovered_items",
        "discovery_scans",
        "content",
        "content_fts",
        "content_embeddings",
        "embedding_queue",
    ] {
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

    // Slice 2 columns must exist after v3.
    for column in [
        "first_discovered",
        "last_indexed",
        "last_modified",
        "last_verified",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('discovered_items') WHERE name = ?1",
                params![column],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if exists == 0 {
            return Err(JaymiError::new(format!(
                "required column '{column}' is missing from discovered_items"
            )));
        }
    }

    Ok(())
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
