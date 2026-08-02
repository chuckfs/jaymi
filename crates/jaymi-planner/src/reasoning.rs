//! Reasoning Engine — language understanding delegated to interchangeable models.
//!
//! Used for ambiguous requests, summarization, complex planning, explanation,
//! and natural-language generation. Fully replaceable.

/// Replaceable reasoning component of the Planner.
#[derive(Debug, Default)]
pub struct ReasoningEngine;

impl ReasoningEngine {
    /// Whether a reasoning backend is wired and usable.
    pub fn is_implemented(&self) -> bool {
        false
    }

    /// Honest status label for diagnostics.
    pub fn status_label(&self) -> &'static str {
        "not_implemented"
    }
}
