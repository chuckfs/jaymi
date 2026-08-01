//! On-disk project structure, including the hidden `.jaymi` directory.

use std::path::{Path, PathBuf};

/// Layout of project-local Jaymi metadata.
///
/// ```text
/// MyProject/
/// ├── src/
/// ├── docs/
/// ├── assets/
/// └── .jaymi/
///     ├── project.json
///     ├── conversations/
///     ├── memories/
///     ├── tasks/
///     ├── artifacts/
///     └── cache/
/// ```
#[derive(Debug, Clone)]
pub struct JaymiProjectLayout {
    pub jaymi_dir: PathBuf,
    pub project_json: PathBuf,
    pub conversations: PathBuf,
    pub memories: PathBuf,
    pub tasks: PathBuf,
    pub artifacts: PathBuf,
    pub cache: PathBuf,
}

impl JaymiProjectLayout {
    /// Derive the `.jaymi` layout for a project root.
    pub fn for_root(root: &Path) -> Self {
        let jaymi_dir = root.join(".jaymi");
        Self {
            project_json: jaymi_dir.join("project.json"),
            conversations: jaymi_dir.join("conversations"),
            memories: jaymi_dir.join("memories"),
            tasks: jaymi_dir.join("tasks"),
            artifacts: jaymi_dir.join("artifacts"),
            cache: jaymi_dir.join("cache"),
            jaymi_dir,
        }
    }
}
