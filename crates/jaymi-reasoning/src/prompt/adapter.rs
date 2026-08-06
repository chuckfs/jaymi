//! Future model-specific prompt adapters.
//!
//! Adapters may reshape an assembled [`super::Prompt`] for a particular model
//! family. They must not contain transport or vendor SDK logic — only text /
//! section reshaping. Sprint B1.2 ships a no-op adapter.

use super::types::Prompt;

/// Optional post-assembly reshape hook for model-specific needs.
pub trait ModelPromptAdapter: Send + Sync {
    /// Stable adapter id (`null` when unused).
    fn id(&self) -> &str;

    /// Adapt a fully built prompt. Default implementations return it unchanged.
    fn adapt(&self, prompt: Prompt) -> Prompt;
}

/// Identity adapter — no model-specific reshaping.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullPromptAdapter;

impl ModelPromptAdapter for NullPromptAdapter {
    fn id(&self) -> &str {
        "null"
    }

    fn adapt(&self, prompt: Prompt) -> Prompt {
        prompt
    }
}
