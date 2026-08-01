//! Local SQLite knowledge store for Jaymi.
//!
//! Third subsystem in the deterministic boot sequence. This milestone only
//! manages connection lifecycle — no schema or query logic.

#![forbid(unsafe_code)]

pub mod entities;
pub mod events;
pub mod relationships;

use jaymi_core::{HealthReport, JaymiResult, Lifecycle};

const NAME: &str = "database";
const DEPENDENCIES: &[&str] = &["configuration", "logging"];

/// Persistent knowledge store connection lifecycle.
#[derive(Debug, Default)]
pub struct Database {
    initialized: bool,
    connected: bool,
}

impl Database {
    /// Create an uninitialized database service.
    pub fn new() -> Self {
        Self {
            initialized: false,
            connected: false,
        }
    }

    /// Returns true when the database reports an active connection.
    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

impl Lifecycle for Database {
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
        // Lifecycle only: mark the store as connected without schema work.
        self.connected = true;
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized && self.connected,
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.connected = false;
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_connects_database() {
        let mut db = Database::new();
        db.initialize().unwrap();
        assert!(db.is_connected());
        assert!(db.health_check().healthy);
        db.shutdown().unwrap();
        assert!(!db.is_connected());
    }
}
