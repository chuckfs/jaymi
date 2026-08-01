//! Configuration for the Jaymi personal AI environment.
//!
//! First subsystem in the deterministic boot sequence.

#![forbid(unsafe_code)]

use jaymi_core::{HealthReport, JaymiResult, Lifecycle};

const NAME: &str = "configuration";
const DEPENDENCIES: &[&str] = &[];

/// Application configuration loaded before every other subsystem.
#[derive(Debug, Default, Clone)]
pub struct Config {
    initialized: bool,
    /// Local data directory used by later subsystems.
    pub data_dir: String,
}

impl Config {
    /// Create an uninitialized configuration service.
    pub fn new() -> Self {
        Self {
            initialized: false,
            data_dir: default_data_dir(),
        }
    }

    /// Returns true when configuration has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
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
        config.shutdown().unwrap();
        assert!(!config.health_check().initialized);
    }
}
