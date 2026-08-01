//! Memory Engine for Jaymi.
//!
//! Memory is intentional. Jaymi maintains three independent memory systems and
//! never remembers everything automatically.

#![forbid(unsafe_code)]

pub mod conversation;
pub mod personal;
pub mod project;

use jaymi_core::{EntityId, JaymiResult};

/// Memory type discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    Conversation,
    Project,
    Personal,
}

/// Structured memory record skeleton.
#[derive(Debug, Clone)]
pub struct MemoryRecord {
    pub id: EntityId,
    pub memory_type: MemoryType,
}

/// Memory Engine skeleton.
#[derive(Debug, Default)]
pub struct MemoryEngine;

impl MemoryEngine {
    /// Retrieve memories relevant to the current request.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn retrieve(&self, _query: &str) -> JaymiResult<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    /// Evaluate whether information should be promoted into memory.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn promote(&self, _content: &str, _memory_type: MemoryType) -> JaymiResult<Option<MemoryRecord>> {
        Ok(None)
    }
}
