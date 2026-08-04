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
                capabilities: vec![
                    Capability::Search,
                    Capability::ReadDocuments,
                    Capability::FileManagement,
                ],
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
            jaymi_logging::error(
                "providers",
                "filesystem list_directory rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!("filesystem list_directory path={}", path.display()),
        );

        let path = normalize_path(path)?;
        let metadata = fs::metadata(&path).map_err(|error| {
            let message = format!("cannot access directory {}: {error}", path.display());
            jaymi_logging::error("providers", &message);
            JaymiError::new(message)
        })?;

        if !metadata.is_dir() {
            let message = format!("path is not a directory: {}", path.display());
            jaymi_logging::warn("providers", &message);
            return Err(JaymiError::new(message));
        }

        let read_dir = fs::read_dir(&path).map_err(|error| {
            let message = format!("failed to read directory {}: {error}", path.display());
            jaymi_logging::error("providers", &message);
            JaymiError::new(message)
        })?;

        let mut entries = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|error| {
                let message = format!(
                    "failed to read directory entry in {}: {error}",
                    path.display()
                );
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
            })?;
            entries.push(file_entry_from_dir_entry(entry)?);
        }

        entries.sort_by(|left, right| left.name.cmp(&right.name));
        jaymi_logging::info(
            "providers",
            format!(
                "filesystem list_directory completed path={} entries={}",
                path.display(),
                entries.len()
            ),
        );
        Ok(entries)
    }

    /// Recursively list a directory tree for Project Explorer.
    ///
    /// Skips hidden names (leading `.`) and `.git`. Does not follow directory
    /// symlinks. Returns the canonical root plus a flat list of files and
    /// directories under it (not including the root itself). Sorting and nesting
    /// belong to callers.
    pub fn list_directory_tree(&self, path: &Path) -> JaymiResult<(PathBuf, Vec<FileEntry>)> {
        if !self.initialized {
            jaymi_logging::error(
                "providers",
                "filesystem list_directory_tree rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!("filesystem list_directory_tree path={}", path.display()),
        );

        let path = normalize_path(path)?;
        let metadata = fs::metadata(&path).map_err(|error| {
            let message = format!("cannot access directory {}: {error}", path.display());
            jaymi_logging::error("providers", &message);
            JaymiError::new(message)
        })?;

        if !metadata.is_dir() {
            let message = format!("path is not a directory: {}", path.display());
            jaymi_logging::warn("providers", &message);
            return Err(JaymiError::new(message));
        }

        let mut entries = Vec::new();
        walk_directory_tree(&path, &mut entries)?;
        jaymi_logging::info(
            "providers",
            format!(
                "filesystem list_directory_tree completed path={} entries={}",
                path.display(),
                entries.len()
            ),
        );
        Ok((path, entries))
    }

    /// Read the raw bytes of a single file.
    ///
    /// Does not parse, index, or interpret content. Parsing belongs to the
    /// parser registry invoked by the Read Tool.
    pub fn read_file(&self, path: &Path) -> JaymiResult<Vec<u8>> {
        if !self.initialized {
            jaymi_logging::error(
                "providers",
                "filesystem read_file rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!("filesystem read_file path={}", path.display()),
        );

        let path = normalize_path(path)?;
        let metadata = fs::metadata(&path).map_err(|error| {
            let message = format!("cannot access file {}: {error}", path.display());
            jaymi_logging::error("providers", &message);
            JaymiError::new(message)
        })?;

        if !metadata.is_file() {
            let message = format!("path is not a file: {}", path.display());
            jaymi_logging::warn("providers", &message);
            return Err(JaymiError::new(message));
        }

        fs::read(&path)
            .inspect(|bytes| {
                jaymi_logging::info(
                    "providers",
                    format!(
                        "filesystem read_file completed path={} bytes={}",
                        path.display(),
                        bytes.len()
                    ),
                );
            })
            .map_err(|error| {
                let message = format!("failed to read file {}: {error}", path.display());
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
            })
    }

    /// Write raw UTF-8 bytes to a file (create or overwrite).
    ///
    /// Does not create parent directories. Parent must already exist.
    pub fn write_file(&self, path: &Path, content: &[u8]) -> JaymiResult<()> {
        if !self.initialized {
            jaymi_logging::error(
                "providers",
                "filesystem write_file rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!("filesystem write_file path={}", path.display()),
        );

        let path = normalize_path_for_write(path)?;
        if path.exists() {
            let metadata = fs::metadata(&path).map_err(|error| {
                let message = format!("cannot access file {}: {error}", path.display());
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
            })?;
            if metadata.is_dir() {
                let message = format!("path is a directory, not a file: {}", path.display());
                jaymi_logging::warn("providers", &message);
                return Err(JaymiError::new(message));
            }
        }

        fs::write(&path, content)
            .map(|_| {
                jaymi_logging::info(
                    "providers",
                    format!(
                        "filesystem write_file completed path={} bytes={}",
                        path.display(),
                        content.len()
                    ),
                );
            })
            .map_err(|error| {
                let message = format!("failed to write file {}: {error}", path.display());
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
            })
    }

    /// Create a single directory (parent must already exist).
    pub fn create_directory(&self, path: &Path) -> JaymiResult<()> {
        if !self.initialized {
            jaymi_logging::error(
                "providers",
                "filesystem create_directory rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!("filesystem create_directory path={}", path.display()),
        );

        let path = normalize_path_for_write(path)?;
        if path.exists() {
            let message = format!("path already exists: {}", path.display());
            jaymi_logging::warn("providers", &message);
            return Err(JaymiError::new(message));
        }

        fs::create_dir(&path)
            .map(|_| {
                jaymi_logging::info(
                    "providers",
                    format!(
                        "filesystem create_directory completed path={}",
                        path.display()
                    ),
                );
            })
            .map_err(|error| {
                let message = format!("failed to create directory {}: {error}", path.display());
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
            })
    }

    /// Rename or move a path to a new destination.
    pub fn rename_path(&self, from: &Path, to: &Path) -> JaymiResult<()> {
        if !self.initialized {
            jaymi_logging::error(
                "providers",
                "filesystem rename_path rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!(
                "filesystem rename_path from={} to={}",
                from.display(),
                to.display()
            ),
        );

        let from = normalize_path(from)?;
        let to = normalize_path_for_write(to)?;
        if to.exists() {
            let message = format!("destination already exists: {}", to.display());
            jaymi_logging::warn("providers", &message);
            return Err(JaymiError::new(message));
        }

        fs::rename(&from, &to)
            .map(|_| {
                jaymi_logging::info(
                    "providers",
                    format!(
                        "filesystem rename_path completed from={} to={}",
                        from.display(),
                        to.display()
                    ),
                );
            })
            .map_err(|error| {
                let message = format!(
                    "failed to rename {} → {}: {error}",
                    from.display(),
                    to.display()
                );
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
            })
    }

    /// Delete a file or directory (directories removed recursively).
    pub fn delete_path(&self, path: &Path) -> JaymiResult<()> {
        if !self.initialized {
            jaymi_logging::error(
                "providers",
                "filesystem delete_path rejected: provider is not initialized",
            );
            return Err(JaymiError::new(
                "filesystem provider is not initialized".to_string(),
            ));
        }

        jaymi_logging::info(
            "providers",
            format!("filesystem delete_path path={}", path.display()),
        );

        let path = normalize_path(path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            let message = format!("cannot access path {}: {error}", path.display());
            jaymi_logging::error("providers", &message);
            JaymiError::new(message)
        })?;

        let result = if metadata.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };

        result
            .map(|_| {
                jaymi_logging::info(
                    "providers",
                    format!("filesystem delete_path completed path={}", path.display()),
                );
            })
            .map_err(|error| {
                let message = format!("failed to delete {}: {error}", path.display());
                jaymi_logging::error("providers", &message);
                JaymiError::new(message)
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

/// Resolve a write target without requiring the file to exist yet.
fn normalize_path_for_write(path: &Path) -> JaymiResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(JaymiError::new("file path must not be empty"));
    }
    if path.exists() {
        return normalize_path(path);
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let parent = normalize_path(parent)?;
            let name = path
                .file_name()
                .ok_or_else(|| JaymiError::new(format!("invalid file path: {}", path.display())))?;
            return Ok(parent.join(name));
        }
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| JaymiError::new(format!("cannot resolve path: {error}")))
    }
}

fn should_skip_explorer_name(name: &str) -> bool {
    name == ".git" || name.starts_with('.')
}

fn walk_directory_tree(dir: &Path, out: &mut Vec<FileEntry>) -> JaymiResult<()> {
    let read_dir = fs::read_dir(dir).map_err(|error| {
        JaymiError::new(format!(
            "failed to read directory {}: {error}",
            dir.display()
        ))
    })?;

    let mut children = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| {
            JaymiError::new(format!(
                "failed to read directory entry in {}: {error}",
                dir.display()
            ))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_skip_explorer_name(&name) {
            continue;
        }
        children.push(file_entry_from_dir_entry(entry)?);
    }

    children.sort_by(|left, right| left.name.cmp(&right.name));
    for child in children {
        let is_dir = child.entry_type == EntryType::Directory;
        let child_path = child.path.clone();
        out.push(child);
        if is_dir {
            // Do not follow symlinks — only real directories from metadata above.
            walk_directory_tree(&child_path, out)?;
        }
    }
    Ok(())
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
    fn list_directory_tree_skips_hidden_and_git_and_recurses() {
        let dir = tempfile_dir();
        fs::create_dir(dir.join("src")).unwrap();
        File::create(dir.join("src").join("main.rs")).unwrap();
        File::create(dir.join("Cargo.toml")).unwrap();
        File::create(dir.join(".hidden")).unwrap();
        // Prefer a file named `.git` so sandboxes that block creating a `.git`
        // directory still exercise the name filter.
        File::create(dir.join(".git")).unwrap();
        fs::create_dir(dir.join("src").join("nested")).unwrap();
        File::create(dir.join("src").join("nested").join("lib.rs")).unwrap();

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        let entries = provider.list_directory_tree(&dir).unwrap().1;
        let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();

        assert!(names.contains(&"src"));
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&"Cargo.toml"));
        assert!(names.contains(&"nested"));
        assert!(names.contains(&"lib.rs"));
        assert!(!names.iter().any(|name| name.starts_with('.')));
        assert!(!names.contains(&"config"));
    }

    #[test]
    fn write_file_creates_and_overwrites() {
        let dir = tempfile_dir();
        let path = dir.join("out.txt");

        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();
        provider.write_file(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        provider.write_file(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
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

    #[test]
    fn create_rename_delete_paths() {
        let dir = tempfile_dir();
        let mut provider = FilesystemProvider::new();
        provider.initialize().unwrap();

        let nested = dir.join("nested");
        provider.create_directory(&nested).unwrap();
        assert!(nested.is_dir());

        let file = nested.join("note.txt");
        provider.write_file(&file, b"hi").unwrap();
        let renamed = nested.join("renamed.txt");
        provider.rename_path(&file, &renamed).unwrap();
        assert!(!file.exists());
        assert_eq!(fs::read(&renamed).unwrap(), b"hi");

        provider.delete_path(&renamed).unwrap();
        assert!(!renamed.exists());
        provider.delete_path(&nested).unwrap();
        assert!(!nested.exists());
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
