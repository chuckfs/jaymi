//! Configuration for the Jaymi personal AI environment.
//!
//! First subsystem in the deterministic boot sequence.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use jaymi_core::{HealthReport, JaymiResult, Lifecycle};
use jaymi_database::IndexRoot;

const NAME: &str = "configuration";
const DEPENDENCIES: &[&str] = &[];

/// Application configuration loaded before every other subsystem.
#[derive(Debug, Default, Clone)]
pub struct Config {
    initialized: bool,
    /// Local data directory used by later subsystems.
    pub data_dir: String,
    /// Filesystem roots included in the Layer 1 knowledge index.
    pub index_roots: Vec<IndexRoot>,
}

impl Config {
    /// Create an uninitialized configuration service.
    pub fn new() -> Self {
        Self {
            initialized: false,
            data_dir: default_data_dir(),
            index_roots: default_index_roots(),
        }
    }

    /// Returns true when configuration has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Enabled index roots only.
    pub fn enabled_index_roots(&self) -> impl Iterator<Item = &IndexRoot> {
        self.index_roots.iter().filter(|root| root.enabled)
    }
}

impl Lifecycle for Config {
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
        if self.data_dir.is_empty() {
            self.data_dir = default_data_dir();
        }
        if self.index_roots.is_empty() {
            self.index_roots = default_index_roots();
        }
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

fn default_data_dir() -> String {
    if let Ok(dir) = std::env::var("JAYMI_DATA_DIR") {
        if !dir.trim().is_empty() {
            return dir;
        }
    }
    dirs_next_data_dir().unwrap_or_else(|| "./data".to_string())
}

fn dirs_next_data_dir() -> Option<String> {
    std::env::var_os("HOME").map(|home| {
        std::path::Path::new(&home)
            .join(".local")
            .join("share")
            .join("jaymi")
            .to_string_lossy()
            .into_owned()
    })
}

fn default_index_roots() -> Vec<IndexRoot> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(IndexRoot::new("downloads", home.join("Downloads")));
        roots.push(IndexRoot::new("documents", home.join("Documents")));
    }
    if let Ok(cwd) = std::env::current_dir() {
        // Workspace / project folder — useful in development and for “what exists here?”
        roots.push(IndexRoot::new("workspace", cwd));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_marks_ready() {
        let mut config = Config::new();
        assert!(!config.health_check().initialized);
        config.initialize().unwrap();
        let health = config.health_check();
        assert!(health.initialized);
        assert!(health.healthy);
        assert!(!config.index_roots.is_empty());
        config.shutdown().unwrap();
        assert!(!config.health_check().initialized);
    }
}
