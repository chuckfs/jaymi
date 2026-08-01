//! Filesystem Provider — local directory listing.
//!
//! Lists a single directory. Does not recurse, index, or interpret content.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_capabilities::Capability;
use jaymi_core::{EntryType, FileEntry, JaymiError, JaymiResult};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Provider ID used for registration and tool metadata.
pub const FILESYSTEM_PROVIDER_ID: &str = "filesystem";

/// Local filesystem provider.
///
/// Exposes directory listing and raw file reads. The Planner never calls this
/// type directly — tools mediate all access.
#[derive(Debug)]
pub struct FilesystemProvider {
    identity: ProviderIdentity,
    initialized: bool,
}

impl FilesystemProvider {
    /// Create an uninitialized filesystem provider.
    pub fn new() -> Self {
        Self {
            identity: ProviderIdentity {
                id: FILESYSTEM_PROVIDER_ID.to_string(),
                name: "Filesystem".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Local filesystem access for listing and reading files".to_string(),
                category: ProviderCategory::Local,
                author: "jaymi".to_string(),
                capabilities: vec![Capability::Search, Capability::ReadContent],
            },
            initialized: false,
        }
    }

    /// Returns true after [`Provider::initialize`] succeeds.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// List the immediate contents of a single directory.
    ///
    /// This operation is non-recursive. Symbolic links are reported as
    /// [`EntryType::Symlink`] without being followed for type detection.
    pub fn list_directory(&self, path: &Path) -> JaymiResult<Vec<FileEntry>> {
        if !self.initialized {
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        let path = normalize_path(path)?;
        let metadata = fs::metadata(&path).map_err(|error| {
            JaymiError::new(format!(
                "cannot access directory {}: {error}",
                path.display()
            ))
        })?;

        if !metadata.is_dir() {
            return Err(JaymiError::new(format!(
                "path is not a directory: {}",
                path.display()
            )));
        }

        let read_dir = fs::read_dir(&path).map_err(|error| {
            JaymiError::new(format!(
                "failed to read directory {}: {error}",
                path.display()
            ))
        })?;

        let mut entries = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|error| {
                JaymiError::new(format!(
                    "failed to read directory entry in {}: {error}",
                    path.display()
                ))
            })?;
            entries.push(file_entry_from_dir_entry(entry)?);
        }

        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    /// Read the raw bytes of a single file.
    ///
    /// Does not parse, index, or interpret content. Parsing belongs to the
    /// content registry invoked by the Content Tool.
    pub fn read_file(&self, path: &Path) -> JaymiResult<Vec<u8>> {
        if !self.initialized {
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        let path = normalize_path(path)?;
        let metadata = fs::metadata(&path).map_err(|error| {
            JaymiError::new(format!("cannot access file {}: {error}", path.display()))
        })?;

        if !metadata.is_file() {
            return Err(JaymiError::new(format!(
                "path is not a file: {}",
                path.display()
            )));
        }

        fs::read(&path).map_err(|error| {
            JaymiError::new(format!("failed to read file {}: {error}", path.display()))
        })
    }
}

impl Default for FilesystemProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for FilesystemProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ))
        }
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

fn normalize_path(path: &Path) -> JaymiResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(JaymiError::new("directory path must not be empty"));
    }
    fs::canonicalize(path).or_else(|_| {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .map_err(|error| JaymiError::new(format!("cannot resolve path: {error}")))
        }
    })
}

fn file_entry_from_dir_entry(entry: fs::DirEntry) -> JaymiResult<FileEntry> {
    let path = entry.path();
    let name = entry.file_name().to_string_lossy().into_owned();
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        JaymiError::new(format!(
            "failed to read metadata for {}: {error}",
            path.display()
        ))
    })?;

    let entry_type = if metadata.file_type().is_symlink() {
        EntryType::Symlink
    } else if metadata.is_dir() {
        EntryType::Directory
    } else if metadata.is_file() {
        EntryType::File
    } else {
        EntryType::Other
    };

    Ok(FileEntry::new(
        name,
        entry_type,
        path,
        metadata.len(),
        system_time_to_unix(metadata.modified().ok()),
    ))
}

fn system_time_to_unix(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn list_directory_returns_structured_entries() {
        let dir = tempfile_dir();
        let file_path = dir.join("note.txt");
        let mut file = File::create(&file_path).unwrap();
        write!(file, "hello").unwrap();
        fs::create_dir(dir.join("subdir")).unwrap();

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let entries = provider.list_directory(&dir).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "note.txt");
        assert_eq!(entries[0].entry_type, EntryType::File);
        assert_eq!(entries[0].size, 5);
        assert!(entries[0].modified.is_some());
        assert_eq!(entries[1].name, "subdir");
        assert_eq!(entries[1].entry_type, EntryType::Directory);
    }

    #[test]
    fn list_directory_does_not_recurse() {
        let dir = tempfile_dir();
        fs::create_dir(dir.join("outer")).unwrap();
        File::create(dir.join("outer").join("nested.txt")).unwrap();

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let entries = provider.list_directory(&dir).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "outer");
        assert!(entries.iter().all(|entry| entry.name != "nested.txt"));
    }

    #[test]
    fn list_directory_requires_initialization() {
        let provider = FilesystemProvider::new();
        let error = provider.list_directory(Path::new(".")).unwrap_err();
        assert!(error.message().contains("not initialized"));
    }

    #[test]
    fn read_file_returns_bytes() {
        let dir = tempfile_dir();
        let path = dir.join("hello.txt");
        let mut file = File::create(&path).unwrap();
        write!(file, "hello").unwrap();

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let bytes = provider.read_file(&path).unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn read_file_rejects_directories() {
        let dir = tempfile_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let error = provider.read_file(&dir).unwrap_err();
        assert!(error.message().contains("not a file"));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-fs-test-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
