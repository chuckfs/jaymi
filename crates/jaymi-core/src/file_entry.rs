//! Structured filesystem metadata returned through the tool pipeline.

use std::path::PathBuf;

/// Kind of filesystem entry discovered by a directory listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symbolic link.
    Symlink,
    /// Any other entry type.
    Other,
}

impl EntryType {
    /// Stable label for diagnostics and responses.
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for EntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Structured metadata for one filesystem entry.
///
/// Returned by the Filesystem Provider through the Search Files Tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Base name of the entry.
    pub name: String,
    /// Entry classification.
    pub entry_type: EntryType,
    /// Absolute or normalized path to the entry.
    pub path: PathBuf,
    /// Size in bytes. Directories may report zero depending on the platform.
    pub size: u64,
    /// Last modified time as Unix epoch seconds, when available.
    pub modified: Option<u64>,
}

impl FileEntry {
    /// Create a new file entry.
    pub fn new(
        name: impl Into<String>,
        entry_type: EntryType,
        path: impl Into<PathBuf>,
        size: u64,
        modified: Option<u64>,
    ) -> Self {
        Self {
            name: name.into(),
            entry_type,
            path: path.into(),
            size,
            modified,
        }
    }
}
