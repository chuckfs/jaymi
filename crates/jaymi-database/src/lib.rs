//! Local SQLite knowledge store for Jaymi.
//!
//! Layer 1 persists filesystem metadata so the Planner can answer
//! “what exists?” without opening Finder.

#![forbid(unsafe_code)]

pub mod entities;
pub mod events;
pub mod files;
pub mod relationships;
pub mod schema;

pub use files::{IndexQuery, IndexRoot, IndexedFile};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jaymi_core::{EntryType, HealthReport, JaymiError, JaymiResult, Lifecycle};
use rusqlite::{params, Connection, OptionalExtension};

const NAME: &str = "database";
const DEPENDENCIES: &[&str] = &["configuration", "logging"];

/// Persistent knowledge store backed by SQLite.
#[derive(Debug)]
pub struct Database {
    initialized: bool,
    connected: bool,
    path: PathBuf,
    conn: Mutex<Option<Connection>>,
}

impl Database {
    /// Create a database that will open under `data_dir/jaymi.db`.
    pub fn with_data_dir(data_dir: impl AsRef<Path>) -> Self {
        let path = data_dir.as_ref().join("jaymi.db");
        Self {
            initialized: false,
            connected: false,
            path,
            conn: Mutex::new(None),
        }
    }

    /// Create an uninitialized in-memory database (tests / default construction).
    pub fn new() -> Self {
        Self {
            initialized: false,
            connected: false,
            path: PathBuf::from(":memory:"),
            conn: Mutex::new(None),
        }
    }

    /// Absolute path to the SQLite file, or `:memory:`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns true when the database reports an active connection.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Upsert one indexed file row.
    pub fn upsert_file(&self, file: &IndexedFile) -> JaymiResult<()> {
        let guard = self.conn()?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| JaymiError::new("database is not connected"))?;
        conn.execute(
            r#"
            INSERT INTO indexed_files (
                id, path, name, parent_path, entry_type, extension,
                size_bytes, modified_unix, source_root, indexed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(path) DO UPDATE SET
                id=excluded.id,
                name=excluded.name,
                parent_path=excluded.parent_path,
                entry_type=excluded.entry_type,
                extension=excluded.extension,
                size_bytes=excluded.size_bytes,
                modified_unix=excluded.modified_unix,
                source_root=excluded.source_root,
                indexed_at=excluded.indexed_at
            "#,
            params![
                file.id.as_str(),
                file.path.display().to_string(),
                file.name,
                file.parent_path.display().to_string(),
                file.entry_type.label(),
                file.extension,
                file.size as i64,
                file.modified.map(|value| value as i64),
                file.source_root,
                file.indexed_at as i64,
            ],
        )
        .map_err(|error| JaymiError::new(format!("failed to upsert indexed file: {error}")))?;
        Ok(())
    }

    /// Replace all indexed rows for one source root with a fresh scan.
    ///
    /// This is the Layer 1 incremental strategy: rescan a root, upsert current
    /// entries, and delete paths that disappeared.
    pub fn replace_root_files(
        &self,
        source_root: &str,
        root_path: &Path,
        files: &[IndexedFile],
    ) -> JaymiResult<usize> {
        let mut guard = self.conn()?;
        let conn = guard
            .as_mut()
            .ok_or_else(|| JaymiError::new("database is not connected"))?;
        let tx = conn
            .transaction()
            .map_err(|error| JaymiError::new(format!("failed to begin index transaction: {error}")))?;

        tx.execute(
            "DELETE FROM indexed_files WHERE source_root = ?1",
            params![source_root],
        )
        .map_err(|error| JaymiError::new(format!("failed to clear root index: {error}")))?;

        for file in files {
            tx.execute(
                r#"
                INSERT INTO indexed_files (
                    id, path, name, parent_path, entry_type, extension,
                    size_bytes, modified_unix, source_root, indexed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    file.id.as_str(),
                    file.path.display().to_string(),
                    file.name,
                    file.parent_path.display().to_string(),
                    file.entry_type.label(),
                    file.extension,
                    file.size as i64,
                    file.modified.map(|value| value as i64),
                    file.source_root,
                    file.indexed_at as i64,
                ],
            )
            .map_err(|error| JaymiError::new(format!("failed to insert indexed file: {error}")))?;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        tx.execute(
            r#"
            INSERT INTO index_roots(label, path, enabled, last_scan_at)
            VALUES (?1, ?2, 1, ?3)
            ON CONFLICT(label) DO UPDATE SET
                path=excluded.path,
                enabled=1,
                last_scan_at=excluded.last_scan_at
            "#,
            params![source_root, root_path.display().to_string(), now],
        )
        .map_err(|error| JaymiError::new(format!("failed to update index root: {error}")))?;

        tx.commit()
            .map_err(|error| JaymiError::new(format!("failed to commit index transaction: {error}")))?;
        Ok(files.len())
    }

    /// Query indexed filesystem metadata.
    pub fn query_files(&self, query: &IndexQuery) -> JaymiResult<Vec<IndexedFile>> {
        let guard = self.conn()?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| JaymiError::new("database is not connected"))?;

        let mut sql = String::from(
            r#"
            SELECT id, path, name, parent_path, entry_type, extension,
                   size_bytes, modified_unix, source_root, indexed_at
            FROM indexed_files
            WHERE 1=1
            "#,
        );
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(text) = &query.text {
            sql.push_str(" AND (LOWER(name) LIKE ? OR LOWER(path) LIKE ?)");
            let pattern = format!("%{}%", text.to_ascii_lowercase());
            values.push(Box::new(pattern.clone()));
            values.push(Box::new(pattern));
        }
        if let Some(root) = &query.source_root {
            sql.push_str(" AND source_root = ?");
            values.push(Box::new(root.clone()));
        }
        if let Some(entry_type) = query.entry_type {
            sql.push_str(" AND entry_type = ?");
            values.push(Box::new(entry_type.label().to_string()));
        }
        sql.push_str(" ORDER BY name COLLATE NOCASE LIMIT ?");
        values.push(Box::new(query.limit as i64));

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|error| JaymiError::new(format!("failed to prepare index query: {error}")))?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|value| value.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), row_to_indexed_file)
            .map_err(|error| JaymiError::new(format!("index query failed: {error}")))?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row.map_err(|error| {
                JaymiError::new(format!("failed to read indexed file row: {error}"))
            })?);
        }
        Ok(files)
    }

    /// Total number of indexed entries.
    pub fn count_files(&self) -> JaymiResult<usize> {
        let guard = self.conn()?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| JaymiError::new("database is not connected"))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM indexed_files", [], |row| row.get(0))
            .map_err(|error| JaymiError::new(format!("failed to count indexed files: {error}")))?;
        Ok(count as usize)
    }

    /// Number of indexed entries for one root.
    pub fn count_root(&self, source_root: &str) -> JaymiResult<usize> {
        let guard = self.conn()?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| JaymiError::new("database is not connected"))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM indexed_files WHERE source_root = ?1",
                params![source_root],
                |row| row.get(0),
            )
            .map_err(|error| JaymiError::new(format!("failed to count root files: {error}")))?;
        Ok(count as usize)
    }

    /// Last scan timestamp for a root, if known.
    pub fn root_last_scan(&self, source_root: &str) -> JaymiResult<Option<u64>> {
        let guard = self.conn()?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| JaymiError::new("database is not connected"))?;
        let value: Option<i64> = conn
            .query_row(
                "SELECT last_scan_at FROM index_roots WHERE label = ?1",
                params![source_root],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| JaymiError::new(format!("failed to read root scan time: {error}")))?;
        Ok(value.map(|seconds| seconds as u64))
    }

    fn conn(&self) -> JaymiResult<std::sync::MutexGuard<'_, Option<Connection>>> {
        self.conn
            .lock()
            .map_err(|_| JaymiError::new("database lock poisoned"))
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
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
        if self.path != Path::new(":memory:") {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    JaymiError::new(format!(
                        "failed to create data directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }
        }

        let conn = if self.path == Path::new(":memory:") {
            Connection::open_in_memory()
        } else {
            Connection::open(&self.path)
        }
        .map_err(|error| JaymiError::new(format!("failed to open database: {error}")))?;

        schema::migrate(&conn)?;
        *self
            .conn
            .lock()
            .map_err(|_| JaymiError::new("database lock poisoned"))? = Some(conn);
        self.connected = true;
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized && self.connected,
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        if let Ok(mut guard) = self.conn.lock() {
            *guard = None;
        }
        self.connected = false;
        self.initialized = false;
        Ok(())
    }
}

fn row_to_indexed_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedFile> {
    let entry_type = match row.get::<_, String>(4)?.as_str() {
        "directory" => EntryType::Directory,
        "symlink" => EntryType::Symlink,
        "other" => EntryType::Other,
        _ => EntryType::File,
    };
    let modified: Option<i64> = row.get(7)?;
    let indexed_at: i64 = row.get(9)?;
    Ok(IndexedFile {
        id: jaymi_core::EntityId::new(row.get::<_, String>(0)?),
        path: PathBuf::from(row.get::<_, String>(1)?),
        name: row.get(2)?,
        parent_path: PathBuf::from(row.get::<_, String>(3)?),
        entry_type,
        extension: row.get(5)?,
        size: row.get::<_, i64>(6)? as u64,
        modified: modified.map(|value| value as u64),
        source_root: row.get(8)?,
        indexed_at: indexed_at as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::FileEntry;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn initialize_connects_database() {
        let mut db = Database::new();
        db.initialize().unwrap();
        assert!(db.is_connected());
        assert!(db.health_check().healthy);
        db.shutdown().unwrap();
        assert!(!db.is_connected());
    }

    #[test]
    fn indexes_and_queries_files() {
        let mut db = Database::new();
        db.initialize().unwrap();

        let entry = FileEntry::new(
            "notes.txt",
            EntryType::File,
            "/tmp/docs/notes.txt",
            12,
            Some(100),
        );
        let indexed = IndexedFile::from_entry(&entry, "documents");
        db.replace_root_files("documents", Path::new("/tmp/docs"), &[indexed])
            .unwrap();

        assert_eq!(db.count_files().unwrap(), 1);
        let results = db
            .query_files(&IndexQuery::new().with_text("notes").with_source_root("documents"))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "notes.txt");
    }

    #[test]
    fn replace_root_removes_stale_paths() {
        let mut db = Database::new();
        db.initialize().unwrap();
        let first = IndexedFile::from_entry(
            &FileEntry::new("a.txt", EntryType::File, "/tmp/a.txt", 1, None),
            "downloads",
        );
        let second = IndexedFile::from_entry(
            &FileEntry::new("b.txt", EntryType::File, "/tmp/b.txt", 1, None),
            "downloads",
        );
        db.replace_root_files("downloads", Path::new("/tmp"), &[first, second])
            .unwrap();
        assert_eq!(db.count_root("downloads").unwrap(), 2);

        let only_b = IndexedFile::from_entry(
            &FileEntry::new("b.txt", EntryType::File, "/tmp/b.txt", 1, None),
            "downloads",
        );
        db.replace_root_files("downloads", Path::new("/tmp"), &[only_b])
            .unwrap();
        assert_eq!(db.count_root("downloads").unwrap(), 1);
        assert_eq!(
            db.query_files(&IndexQuery::new().with_source_root("downloads"))
                .unwrap()[0]
                .name,
            "b.txt"
        );
        let _ = SystemTime::now().duration_since(UNIX_EPOCH);
    }
}
