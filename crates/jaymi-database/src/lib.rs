//! Local SQLite knowledge store for Jaymi.
//!
//! The database organizes relationships between conversations, memories,
//! projects, files, providers, and user knowledge. It stores knowledge;
//! the Planner creates intelligence.

#![forbid(unsafe_code)]

pub mod entities;
pub mod events;
pub mod relationships;

use jaymi_core::JaymiResult;

/// Persistent knowledge store skeleton.
#[derive(Debug, Default)]
pub struct Database;

impl Database {
    /// Open or create the local database.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn open() -> JaymiResult<Self> {
        Ok(Self)
    }
}
