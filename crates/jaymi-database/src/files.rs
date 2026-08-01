//! Indexed file metadata for the Layer 1 knowledge engine.
//!
//! Stores filesystem metadata only — no content text, embeddings, or OCR.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{EntryType, EntityId, FileEntry};

/// One indexed filesystem entry stored in the knowledge database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    /// Stable identity derived from the absolute path.
    pub id: EntityId,
    /// Absolute path to the entry.
    pub path: PathBuf,
    /// Base file or directory name.
    pub name: String,
    /// Parent directory path.
    pub parent_path: PathBuf,
    /// Entry classification.
    pub entry_type: EntryType,
    /// Lowercase extension without dot, when present.
    pub extension: Option<String>,
    /// Size in bytes.
    pub size: u64,
    /// Last modified time as Unix epoch seconds.
    pub modified: Option<u64>,
    /// Index root label (for example `downloads`, `documents`).
    pub source_root: String,
    /// When this row was last written to the index.
    pub indexed_at: u64,
}

impl IndexedFile {
    /// Build an indexed record from a live [`FileEntry`] and root label.
    pub fn from_entry(entry: &FileEntry, source_root: impl Into<String>) -> Self {
        let extension = entry
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        let parent_path = entry
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            id: EntityId::new(format!("file:{}", entry.path.display())),
            path: entry.path.clone(),
            name: entry.name.clone(),
            parent_path,
            entry_type: entry.entry_type,
            extension,
            size: entry.size,
            modified: entry.modified,
            source_root: source_root.into(),
            indexed_at: now_unix(),
        }
    }

    /// Convert to the pipeline-facing [`FileEntry`] shape.
    pub fn to_file_entry(&self) -> FileEntry {
        FileEntry::new(
            self.name.clone(),
            self.entry_type,
            self.path.clone(),
            self.size,
            self.modified,
        )
    }
}

/// Query parameters for searching the file index.
#[derive(Debug, Clone, Default)]
pub struct IndexQuery {
    /// Optional case-insensitive substring matched against name and path.
    pub text: Option<String>,
    /// Optional source root filter (`downloads`, `documents`, …).
    pub source_root: Option<String>,
    /// Optional entry type filter.
    pub entry_type: Option<EntryType>,
    /// Maximum rows to return.
    pub limit: usize,
}

impl IndexQuery {
    /// Create a query with a default limit.
    pub fn new() -> Self {
        Self {
            text: None,
            source_root: None,
            entry_type: None,
            limit: 50,
        }
    }

    /// Filter by free-text name/path match.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        let value = text.into();
        if !value.trim().is_empty() {
            self.text = Some(value);
        }
        self
    }

    /// Filter by indexed root label.
    pub fn with_source_root(mut self, root: impl Into<String>) -> Self {
        self.source_root = Some(root.into());
        self
    }

    /// Limit result count.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }
}

/// Configured filesystem root that Jaymi indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRoot {
    /// Stable label used in queries and responses.
    pub label: String,
    /// Absolute directory path.
    pub path: PathBuf,
    /// Whether this root is included in scans.
    pub enabled: bool,
}

impl IndexRoot {
    /// Create an enabled index root.
    pub fn new(label: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            label: label.into(),
            path: path.into(),
            enabled: true,
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
