//! Path normalization shared by knowledge reads and discovery indexing.

use std::path::{Path, PathBuf};

use jaymi_core::{JaymiError, JaymiResult};

/// Normalize a path to an absolute cleaned form without reading contents.
///
/// When the path exists, it is canonicalized so inventory keys stay stable
/// across platforms that expose symlinked temp roots. When missing, the
/// longest existing prefix is canonicalized.
pub fn normalize_path(path: impl AsRef<Path>) -> JaymiResult<PathBuf> {
    let path = path.as_ref();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                JaymiError::new(format!("failed to resolve current directory: {error}"))
            })?
            .join(path)
    };
    let cleaned = clean_path(&absolute);
    if cleaned.exists() {
        Ok(cleaned.canonicalize().unwrap_or(cleaned))
    } else {
        Ok(canonicalize_missing(&cleaned))
    }
}

fn canonicalize_missing(path: &Path) -> PathBuf {
    let mut suffix = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            let mut base = current.canonicalize().unwrap_or_else(|_| current.clone());
            for part in suffix.iter().rev() {
                base.push(part);
            }
            return base;
        }
        match current.file_name() {
            Some(name) => {
                suffix.push(name.to_os_string());
                if !current.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    path.to_path_buf()
}

fn clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}
