//! Optional scheduler hook for asynchronous embedding generation.
//!
//! Understanding schedules work after content upserts. The concrete queue /
//! provider lives behind Search and is invisible to the Planner.

use jaymi_core::JaymiResult;

/// Schedule embedding generation for a normalized content source.
pub trait EmbeddingScheduler: Send + Sync {
    /// Enqueue `source_id` for asynchronous embedding.
    fn schedule(&self, source_id: &str) -> JaymiResult<()>;
}
