//! Recursive filesystem walking and metadata extraction.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{JaymiError, JaymiResult};
use jaymi_knowledge::normalize_path;

/// Metadata collected for one discovered filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredItem {
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
    /// Filesystem device id for rename detection, when available.
    pub device_id: Option<u64>,
    /// Filesystem inode for rename detection, when available.
    pub inode: Option<u64>,
}

/// True when a file/directory name is treated as hidden.
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

/// Recursively walk a root and collect metadata for every reachable entry.
///
/// Includes the root itself. Does not open file contents.
pub fn walk_recursive(root: &Path) -> JaymiResult<Vec<DiscoveredItem>> {
    let root = normalize_path(root)?;
    let mut items = Vec::new();
    walk_into(&root, &mut items)?;
    Ok(items)
}

fn walk_into(path: &Path, items: &mut Vec<DiscoveredItem>) -> JaymiResult<()> {
    match collect_metadata(path) {
        Ok(item) => {
            let is_directory = item.is_directory;
            items.push(item);
            if is_directory {
                let entries = match fs::read_dir(path) {
                    Ok(entries) => entries,
                    Err(error) => {
                        jaymi_logging::warn(
                            "discovery",
                            format!("skipping unreadable directory {}: {error}", path.display()),
                        );
                        return Ok(());
                    }
                };
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            if let Err(error) = walk_into(&entry.path(), items) {
                                jaymi_logging::warn(
                                    "discovery",
                                    format!(
                                        "skipping {}: {}",
                                        entry.path().display(),
                                        error.message()
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            jaymi_logging::warn(
                                "discovery",
                                format!(
                                    "skipping unreadable entry under {}: {error}",
                                    path.display()
                                ),
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        Err(error) => {
            jaymi_logging::warn(
                "discovery",
                format!("skipping {}: {}", path.display(), error.message()),
            );
            Ok(())
        }
    }
}

fn collect_metadata(path: &Path) -> JaymiResult<DiscoveredItem> {
    let path = normalize_path(path)?;
    let meta = fs::symlink_metadata(&path).map_err(|error| {
        JaymiError::new(format!(
            "failed to read metadata for {}: {error}",
            path.display()
        ))
    })?;

    // Do not follow symlinks into arbitrary trees for Slice 1.
    if meta.file_type().is_symlink() {
        return Err(JaymiError::new(format!(
            "skipping symlink {}",
            path.display()
        )));
    }

    let is_directory = meta.is_dir();
    let filename = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let extension = if is_directory {
        None
    } else {
        path.extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
    };
    let parent = path.parent().map(|value| value.to_path_buf());
    let size = if is_directory { 0 } else { meta.len() };
    let (device_id, inode) = file_identity(&meta);

    Ok(DiscoveredItem {
        path: path.clone(),
        filename: filename.clone(),
        extension,
        size,
        created: system_time_secs(meta.created().ok()),
        modified: system_time_secs(meta.modified().ok()),
        is_directory,
        hidden: is_hidden_name(&filename),
        parent,
        device_id,
        inode,
    })
}

#[cfg(unix)]
fn file_identity(meta: &fs::Metadata) -> (Option<u64>, Option<u64>) {
    use std::os::unix::fs::MetadataExt;
    (Some(meta.dev()), Some(meta.ino()))
}

#[cfg(not(unix))]
fn file_identity(_meta: &fs::Metadata) -> (Option<u64>, Option<u64>) {
    (None, None)
}

fn system_time_secs(value: Option<SystemTime>) -> Option<i64> {
    value.and_then(|time| {
        time.duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs() as i64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_makes_absolute() {
        let dir = temp_dir("norm");
        let normalized = normalize_path(&dir).unwrap();
        assert!(normalized.is_absolute());
    }

    #[test]
    fn walk_finds_nested_files() {
        let root = temp_dir("walk");
        fs::create_dir_all(root.join("a").join("b")).unwrap();
        let mut file = fs::File::create(root.join("a").join("b").join("c.md")).unwrap();
        write!(file, "x").unwrap();

        let items = walk_recursive(&root).unwrap();
        assert!(items.iter().any(|item| item.filename == "c.md"));
        assert!(items
            .iter()
            .any(|item| item.is_directory && item.filename == "b"));
        let file = items.iter().find(|item| item.filename == "c.md").unwrap();
        assert_eq!(file.extension.as_deref(), Some("md"));
        assert_eq!(file.size, 1);
    }

    #[test]
    fn hidden_dotfiles_are_flagged() {
        assert!(is_hidden_name(".git"));
        assert!(!is_hidden_name("README.md"));
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-walk-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
