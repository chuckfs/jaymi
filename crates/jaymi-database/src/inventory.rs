//! Persistent inventory of discovered filesystem items (Layer 1).

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension, Row};

use jaymi_core::{JaymiError, JaymiResult};

use crate::Database;

/// Metadata for one discovered file or folder. Contents are never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredItemRecord {
    /// Absolute normalized path.
    pub path: String,
    /// Final path component.
    pub filename: String,
    /// Lowercased extension without dot, when any.
    pub extension: Option<String>,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Filesystem creation time as unix seconds, when available.
    pub created: Option<i64>,
    /// Filesystem modification time as unix seconds, when available.
    pub modified: Option<i64>,
    /// True when the entry is a directory.
    pub is_directory: bool,
    /// True when the entry is hidden by platform convention.
    pub hidden: bool,
    /// Absolute parent directory path, when any.
    pub parent: Option<String>,
    /// Unix seconds when Jaymi first inventoried this path/identity.
    pub first_discovered: Option<i64>,
    /// Unix seconds when inventory metadata was last rewritten.
    pub last_indexed: Option<i64>,
    /// Last observed filesystem mtime stored for change detection.
    pub last_modified: Option<i64>,
    /// Unix seconds when a scan last confirmed the path still exists.
    pub last_verified: Option<i64>,
    /// Filesystem device id used for rename detection, when available.
    pub device_id: Option<u64>,
    /// Filesystem inode used for rename detection, when available.
    pub inode: Option<u64>,
}

/// Filter for inventory queries.
#[derive(Debug, Clone, Default)]
pub struct DiscoveredQuery {
    /// Optional path prefix (inclusive of the root itself).
    pub path_prefix: Option<String>,
    /// Exact parent directory path (immediate children only).
    pub parent: Option<String>,
    /// Optional filename substring (case-insensitive).
    pub name_contains: Option<String>,
    /// Lowercased extension without a leading dot.
    pub extension: Option<String>,
    /// Restrict results to files.
    pub files_only: bool,
    /// Restrict results to directories.
    pub directories_only: bool,
    /// Restrict results to hidden entries.
    pub hidden_only: bool,
    /// Return only directories that currently have no children in the inventory.
    pub empty_folders: bool,
    /// Result ordering.
    pub sort: DiscoverySort,
    /// Limit number of rows returned.
    pub limit: Option<usize>,
}

/// Ordering for discovery queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoverySort {
    /// Alphabetical by absolute path.
    #[default]
    Path,
    /// Newest filesystem modification time first.
    RecentlyModified,
    /// Newest filesystem creation time first.
    RecentlyCreated,
    /// Largest size first.
    Largest,
}

impl DiscoverySort {
    /// Stable label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::RecentlyModified => "recently_modified",
            Self::RecentlyCreated => "recently_created",
            Self::Largest => "largest",
        }
    }
}

/// Aggregate counts for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscoveredCounts {
    /// Number of file rows.
    pub files: u64,
    /// Number of directory rows.
    pub folders: u64,
}

/// Latest discovery scan summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryScanRecord {
    /// Scan row id.
    pub id: i64,
    /// Unix seconds when the scan started.
    pub started_at: i64,
    /// Unix seconds when the scan finished.
    pub finished_at: i64,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// JSON array of scanned root paths.
    pub roots_json: String,
    /// Filesystem entries visited during the scan.
    pub files_seen: u64,
    /// Folders visited during the scan.
    pub folders_seen: u64,
    /// Newly inserted inventory rows.
    pub files_added: u64,
    /// Existing rows whose metadata changed.
    pub files_updated: u64,
    /// Rows removed because paths disappeared.
    pub files_removed: u64,
    /// Rows confirmed unchanged (verified only).
    pub files_unchanged: u64,
    /// Status label (`completed`, `failed`, …).
    pub status: String,
}

/// Input used when recording a finished scan.
#[derive(Debug, Clone)]
pub struct DiscoveryScanInput {
    /// Unix seconds when the scan started.
    pub started_at: i64,
    /// Unix seconds when the scan finished.
    pub finished_at: i64,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Roots that were scanned.
    pub roots: Vec<String>,
    /// Filesystem entries visited during the scan.
    pub files_seen: u64,
    /// Folders visited during the scan.
    pub folders_seen: u64,
    /// Newly inserted inventory rows.
    pub files_added: u64,
    /// Existing rows whose metadata changed.
    pub files_updated: u64,
    /// Rows removed because paths disappeared.
    pub files_removed: u64,
    /// Rows confirmed unchanged (verified only).
    pub files_unchanged: u64,
    /// Status label.
    pub status: String,
}

const SELECT_ITEM_COLUMNS: &str = "path, filename, extension, size, created, modified, \
    is_directory, hidden, parent, first_discovered, last_indexed, last_modified, \
    last_verified, device_id, inode";

impl Database {
    pub(crate) fn with_connection<T>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> JaymiResult<T>,
    ) -> JaymiResult<T> {
        let guard = self
            .connection
            .as_ref()
            .ok_or_else(|| JaymiError::new("database is not connected"))?
            .lock()
            .map_err(|_| JaymiError::new("database connection lock poisoned"))?;
        f(&guard)
    }

    /// Insert a newly discovered item.
    pub fn insert_discovered_item(&self, item: &DiscoveredItemRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                r#"
                INSERT INTO discovered_items (
                    path, filename, extension, size, created, modified,
                    is_directory, hidden, parent,
                    first_discovered, last_indexed, last_modified, last_verified,
                    device_id, inode
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                )
                "#,
                params![
                    item.path,
                    item.filename,
                    item.extension,
                    item.size as i64,
                    item.created,
                    item.modified,
                    item.is_directory as i64,
                    item.hidden as i64,
                    item.parent,
                    item.first_discovered,
                    item.last_indexed,
                    item.last_modified,
                    item.last_verified,
                    item.device_id.map(|value| value as i64),
                    item.inode.map(|value| value as i64),
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Update metadata for an existing path after a change was detected.
    pub fn update_discovered_item(&self, item: &DiscoveredItemRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                r#"
                UPDATE discovered_items SET
                    filename = ?2,
                    extension = ?3,
                    size = ?4,
                    created = ?5,
                    modified = ?6,
                    is_directory = ?7,
                    hidden = ?8,
                    parent = ?9,
                    last_indexed = ?10,
                    last_modified = ?11,
                    last_verified = ?12,
                    device_id = ?13,
                    inode = ?14
                WHERE path = ?1
                "#,
                params![
                    item.path,
                    item.filename,
                    item.extension,
                    item.size as i64,
                    item.created,
                    item.modified,
                    item.is_directory as i64,
                    item.hidden as i64,
                    item.parent,
                    item.last_indexed,
                    item.last_modified,
                    item.last_verified,
                    item.device_id.map(|value| value as i64),
                    item.inode.map(|value| value as i64),
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Mark an unchanged path as verified without rewriting metadata.
    pub fn verify_discovered_item(&self, path: &str, verified_at: i64) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "UPDATE discovered_items SET last_verified = ?2 WHERE path = ?1",
                params![path, verified_at],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Move an inventory row to a new path (rename), preserving first_discovered.
    pub fn rename_discovered_item(
        &self,
        old_path: &str,
        item: &DiscoveredItemRecord,
    ) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                r#"
                UPDATE discovered_items SET
                    path = ?2,
                    filename = ?3,
                    extension = ?4,
                    size = ?5,
                    created = ?6,
                    modified = ?7,
                    is_directory = ?8,
                    hidden = ?9,
                    parent = ?10,
                    last_indexed = ?11,
                    last_modified = ?12,
                    last_verified = ?13,
                    device_id = ?14,
                    inode = ?15
                WHERE path = ?1
                "#,
                params![
                    old_path,
                    item.path,
                    item.filename,
                    item.extension,
                    item.size as i64,
                    item.created,
                    item.modified,
                    item.is_directory as i64,
                    item.hidden as i64,
                    item.parent,
                    item.last_indexed,
                    item.last_modified,
                    item.last_verified,
                    item.device_id.map(|value| value as i64),
                    item.inode.map(|value| value as i64),
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Backward-compatible upsert used by older call sites.
    pub fn upsert_discovered_item(&self, item: &DiscoveredItemRecord) -> JaymiResult<()> {
        if self.get_discovered_item(&item.path)?.is_some() {
            self.update_discovered_item(item)
        } else {
            self.insert_discovered_item(item)
        }
    }

    /// Fetch one inventory row by absolute path.
    pub fn get_discovered_item(&self, path: &str) -> JaymiResult<Option<DiscoveredItemRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                &format!("SELECT {SELECT_ITEM_COLUMNS} FROM discovered_items WHERE path = ?1"),
                params![path],
                map_discovered_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Remove one discovered path.
    pub fn remove_discovered_path(&self, path: &str) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM discovered_items WHERE path = ?1",
                params![path],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Remove all discovered items under a root (including the root itself).
    pub fn remove_under_root(&self, root: &str) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let deleted = conn
                .execute(
                    "DELETE FROM discovered_items WHERE path = ?1 OR path LIKE ?2",
                    params![root, format!("{root}/%")],
                )
                .map_err(db_error)?;
            Ok(deleted as u64)
        })
    }

    /// Query discovered items with optional filters.
    pub fn query_discovered(&self, query: &DiscoveredQuery) -> JaymiResult<Vec<DiscoveredItemRecord>> {
        self.with_connection(|conn| {
            let mut sql = format!("SELECT {SELECT_ITEM_COLUMNS} FROM discovered_items WHERE 1=1");
            let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if query.empty_folders {
                sql.push_str(
                    " AND is_directory = 1 \
                     AND NOT EXISTS ( \
                        SELECT 1 FROM discovered_items AS child \
                        WHERE child.path LIKE discovered_items.path || '/%' \
                     )",
                );
            } else {
                if let Some(prefix) = &query.path_prefix {
                    sql.push_str(" AND (path = ? OR path LIKE ?)");
                    binds.push(Box::new(prefix.clone()));
                    binds.push(Box::new(format!("{prefix}/%")));
                }
                if let Some(parent) = &query.parent {
                    sql.push_str(" AND parent = ?");
                    binds.push(Box::new(parent.clone()));
                }
                if let Some(name) = &query.name_contains {
                    sql.push_str(" AND lower(filename) LIKE ?");
                    binds.push(Box::new(format!("%{}%", name.to_ascii_lowercase())));
                }
                if let Some(extension) = &query.extension {
                    sql.push_str(" AND lower(extension) = ?");
                    binds.push(Box::new(extension.to_ascii_lowercase()));
                }
                if query.files_only {
                    sql.push_str(" AND is_directory = 0");
                }
                if query.directories_only {
                    sql.push_str(" AND is_directory = 1");
                }
                if query.hidden_only {
                    sql.push_str(" AND hidden = 1");
                }
            }

            match query.sort {
                DiscoverySort::Path => sql.push_str(" ORDER BY path ASC"),
                DiscoverySort::RecentlyModified => {
                    sql.push_str(" ORDER BY modified DESC NULLS LAST, path ASC")
                }
                DiscoverySort::RecentlyCreated => {
                    sql.push_str(" ORDER BY created DESC NULLS LAST, path ASC")
                }
                DiscoverySort::Largest => {
                    sql.push_str(" ORDER BY size DESC, path ASC")
                }
            }

            if let Some(limit) = query.limit {
                sql.push_str(" LIMIT ?");
                binds.push(Box::new(limit as i64));
            }

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                binds.iter().map(|value| value.as_ref()).collect();
            let mut stmt = conn.prepare(&sql).map_err(db_error)?;
            let rows = stmt
                .query_map(params_refs.as_slice(), map_discovered_row)
                .map_err(db_error)?;
            let mut items = Vec::new();
            for row in rows {
                items.push(row.map_err(db_error)?);
            }
            Ok(items)
        })
    }

    /// Full inventory rows under a root (including the root).
    pub fn items_under_root(&self, root: &str) -> JaymiResult<Vec<DiscoveredItemRecord>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {SELECT_ITEM_COLUMNS} FROM discovered_items \
                     WHERE path = ?1 OR path LIKE ?2 ORDER BY path"
                ))
                .map_err(db_error)?;
            let rows = stmt
                .query_map(params![root, format!("{root}/%")], map_discovered_row)
                .map_err(db_error)?;
            let mut items = Vec::new();
            for row in rows {
                items.push(row.map_err(db_error)?);
            }
            Ok(items)
        })
    }

    /// Count discovered files and folders.
    pub fn discovered_counts(&self) -> JaymiResult<DiscoveredCounts> {
        self.with_connection(|conn| {
            let files: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM discovered_items WHERE is_directory = 0",
                    [],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            let folders: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM discovered_items WHERE is_directory = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            Ok(DiscoveredCounts {
                files: files as u64,
                folders: folders as u64,
            })
        })
    }

    /// Persist a scan summary row.
    pub fn record_scan(&self, scan: &DiscoveryScanInput) -> JaymiResult<i64> {
        self.with_connection(|conn| {
            let roots_json = serde_json::to_string(&scan.roots).map_err(|error| {
                JaymiError::new(format!("failed to encode scan roots: {error}"))
            })?;
            conn.execute(
                r#"
                INSERT INTO discovery_scans (
                    started_at, finished_at, duration_ms, roots_json,
                    files_seen, folders_seen, files_added, files_updated,
                    files_removed, files_unchanged, status
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                params![
                    scan.started_at,
                    scan.finished_at,
                    scan.duration_ms as i64,
                    roots_json,
                    scan.files_seen as i64,
                    scan.folders_seen as i64,
                    scan.files_added as i64,
                    scan.files_updated as i64,
                    scan.files_removed as i64,
                    scan.files_unchanged as i64,
                    scan.status,
                ],
            )
            .map_err(db_error)?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// Fetch the most recent scan summary, when any.
    pub fn latest_scan(&self) -> JaymiResult<Option<DiscoveryScanRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                r#"
                SELECT id, started_at, finished_at, duration_ms, roots_json,
                       files_seen, folders_seen,
                       COALESCE(files_added, 0), COALESCE(files_updated, 0),
                       COALESCE(files_removed, 0), COALESCE(files_unchanged, 0),
                       status
                FROM discovery_scans
                ORDER BY id DESC
                LIMIT 1
                "#,
                [],
                |row| {
                    Ok(DiscoveryScanRecord {
                        id: row.get(0)?,
                        started_at: row.get(1)?,
                        finished_at: row.get(2)?,
                        duration_ms: row.get::<_, i64>(3)? as u64,
                        roots_json: row.get(4)?,
                        files_seen: row.get::<_, i64>(5)? as u64,
                        folders_seen: row.get::<_, i64>(6)? as u64,
                        files_added: row.get::<_, i64>(7)? as u64,
                        files_updated: row.get::<_, i64>(8)? as u64,
                        files_removed: row.get::<_, i64>(9)? as u64,
                        files_unchanged: row.get::<_, i64>(10)? as u64,
                        status: row.get(11)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// On-disk size of the SQLite database file in bytes.
    pub fn file_size_bytes(&self) -> JaymiResult<u64> {
        let meta = std::fs::metadata(&self.path).map_err(|error| {
            JaymiError::new(format!(
                "failed to read database size for {}: {error}",
                self.path.display()
            ))
        })?;
        Ok(meta.len())
    }

    /// Paths currently stored under a root (including the root).
    pub fn paths_under_root(&self, root: &str) -> JaymiResult<Vec<String>> {
        Ok(self
            .items_under_root(root)?
            .into_iter()
            .map(|item| item.path)
            .collect())
    }
}

fn map_discovered_row(row: &Row<'_>) -> rusqlite::Result<DiscoveredItemRecord> {
    Ok(DiscoveredItemRecord {
        path: row.get(0)?,
        filename: row.get(1)?,
        extension: row.get(2)?,
        size: row.get::<_, i64>(3)? as u64,
        created: row.get(4)?,
        modified: row.get(5)?,
        is_directory: row.get::<_, i64>(6)? != 0,
        hidden: row.get::<_, i64>(7)? != 0,
        parent: row.get(8)?,
        first_discovered: row.get(9)?,
        last_indexed: row.get(10)?,
        last_modified: row.get(11)?,
        last_verified: row.get(12)?,
        device_id: row
            .get::<_, Option<i64>>(13)?
            .map(|value| value as u64),
        inode: row.get::<_, Option<i64>>(14)?.map(|value| value as u64),
    })
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}

/// Convenience helper for constructing absolute path strings.
pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Normalize a path string into a [`PathBuf`] for comparisons.
pub fn path_buf(path: impl AsRef<str>) -> PathBuf {
    PathBuf::from(path.as_ref())
}
