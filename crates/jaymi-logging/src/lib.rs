//! Logging for the Jaymi personal AI environment.
//!
//! Second subsystem in the deterministic boot sequence.

#![forbid(unsafe_code)]

use jaymi_core::{HealthReport, JaymiResult, Lifecycle};

const NAME: &str = "logging";
const DEPENDENCIES: &[&str] = &["configuration"];

/// Logging subsystem responsible for process-wide diagnostic output.
#[derive(Debug, Default)]
pub struct Logger {
    initialized: bool,
}

impl Logger {
    /// Create an uninitialized logger.
    pub fn new() -> Self {
        Self {
            initialized: false,
        }
    }

    /// Returns true when logging has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Lifecycle for Logger {
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
        // Lifecycle only: establish that logging is available.
        // Concrete sinks/frameworks can be introduced later.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_round_trip() {
        let mut logger = Logger::new();
        logger.initialize().unwrap();
        assert!(logger.health_check().healthy);
        logger.shutdown().unwrap();
        assert!(!logger.is_initialized());
    }
}
