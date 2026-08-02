//! Knowledge domain types — independent of SQLite row layout.

use std::path::PathBuf;

/// One indexed knowledge item (file or folder metadata only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeItem {
    /// Absolute normalized path.
    pub path: PathBuf,
    /// Final path component.
    pub filename: String,
    /// Lowercased extension without the leading dot.
    pub extension: Option<String>,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Creation time as unix seconds, when available.
    pub created: Option<i64>,
    /// Modification time as unix seconds, when available.
    pub modified: Option<i64>,
    /// True when the entry is a directory.
    pub is_directory: bool,
    /// True when the entry is hidden by platform convention.
    pub hidden: bool,
    /// Absolute parent directory path, when any.
    pub parent: Option<PathBuf>,
    /// Unix seconds when Jaymi first inventoried this path/identity.
    pub first_discovered: Option<i64>,
    /// Unix seconds when inventory metadata was last rewritten.
    pub last_indexed: Option<i64>,
    /// Last observed filesystem mtime stored for change detection.
    pub last_modified: Option<i64>,
    /// Unix seconds when a scan last confirmed the path still exists.
    pub last_verified: Option<i64>,
    /// Filesystem device id for rename detection, when available.
    pub device_id: Option<u64>,
    /// Filesystem inode for rename detection, when available.
    pub inode: Option<u64>,
}

/// Ordering for knowledge queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KnowledgeSort {
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

/// Filter for knowledge inventory queries.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeQuery {
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
    pub sort: KnowledgeSort,
    /// Limit number of rows returned.
    pub limit: Option<usize>,
}

/// Kind of "recent items" listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentKind {
    /// Order by filesystem modification time.
    Modified,
    /// Order by filesystem creation time.
    Created,
}

/// Outcome of publishing one knowledge item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    /// New inventory row inserted.
    Inserted,
    /// Existing row metadata rewritten.
    Updated,
    /// Existing row confirmed unchanged.
    Verified,
}

/// Summary recorded after an indexing scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    /// Unix seconds when the scan started.
    pub started_at: i64,
    /// Unix seconds when the scan finished.
    pub finished_at: i64,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Scanned root paths.
    pub roots: Vec<String>,
    /// Filesystem entries visited.
    pub files_seen: u64,
    /// Folders visited.
    pub folders_seen: u64,
    /// Newly inserted rows.
    pub files_added: u64,
    /// Updated rows.
    pub files_updated: u64,
    /// Removed rows.
    pub files_removed: u64,
    /// Unchanged verified rows.
    pub files_unchanged: u64,
    /// Scan status label.
    pub status: String,
}
