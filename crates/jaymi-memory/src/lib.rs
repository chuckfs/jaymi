//! Memory Engine for Jaymi.
//!
//! Sixth subsystem in the deterministic boot sequence.
//! Memory is intentional across conversation, project, and personal stores.

#![forbid(unsafe_code)]

pub mod conversation;
pub mod personal;
pub mod project;

use jaymi_core::{EntityId, HealthReport, JaymiResult, Lifecycle};

const NAME: &str = "memory_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
];

/// Memory type discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    /// Temporary conversation memory.
    Conversation,
    /// Project-attached memory.
    Project,
    /// Long-term personal memory.
    Personal,
}

/// Structured memory record skeleton.
#[derive(Debug, Clone)]
pub struct MemoryRecord {
    /// Unique memory identity.
    pub id: EntityId,
    /// Which memory system owns the record.
    pub memory_type: MemoryType,
}

/// Memory Engine lifecycle.
#[derive(Debug, Default)]
pub struct MemoryEngine {
    initialized: bool,
}

impl MemoryEngine {
    /// Create an uninitialized memory engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieve memories relevant to the current request.
    ///
    /// Not implemented in the boot-sequence milestone.
    pub fn retrieve(&self, _query: &str) -> JaymiResult<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    /// Evaluate whether information should be promoted into memory.
    ///
    /// Not implemented in the boot-sequence milestone.
    pub fn promote(
        &self,
        _content: &str,
        _memory_type: MemoryType,
    ) -> JaymiResult<Option<MemoryRecord>> {
        Ok(None)
    }
}

impl Lifecycle for MemoryEngine {
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
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        // Lifecycle may be initialized, but retrieval/promotion are stubs.
        // Do not report operational health until memory storage exists.
        HealthReport::new(
            NAME,
            self.initialized,
            false,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            ("status".to_string(), "stub".to_string()),
            (
                "note".to_string(),
                "retrieve/promote not implemented".to_string(),
            ),
        ])
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
    fn initialize_marks_stub_not_operational() {
        let mut engine = MemoryEngine::new();
        engine.initialize().unwrap();
        let health = engine.health_check();
        assert!(health.initialized);
        assert!(!health.healthy);
        assert!(health
            .details
            .iter()
            .any(|(key, value)| key == "status" && value == "stub"));
    }
}
