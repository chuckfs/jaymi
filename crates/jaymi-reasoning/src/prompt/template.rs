//! Future prompt templates — section order and system copy hooks.

use super::section::PromptSectionId;

/// Declares which sections to emit and in what order.
///
/// Sprint B1.2 ships [`DefaultPromptTemplate`]. Future templates can swap
/// ordering / system instructions without touching providers.
pub trait PromptTemplate: Send + Sync {
    /// Stable template id.
    fn id(&self) -> &str;

    /// Section emission order.
    fn section_order(&self) -> &[PromptSectionId];

    /// Default system instructions body (may be overridden on the builder).
    fn default_system_instructions(&self) -> &str;
}

/// Canonical Jaymi conversational reasoning template.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPromptTemplate;

impl PromptTemplate for DefaultPromptTemplate {
    fn id(&self) -> &str {
        "jaymi.default.v1"
    }

    fn section_order(&self) -> &[PromptSectionId] {
        PromptSectionId::ORDER
    }

    fn default_system_instructions(&self) -> &str {
        "You are Jaymi, a local-first personal AI environment. \
Reason over the structured context below. Prefer project and memory facts \
when present. Do not invent file paths, permissions, or tool results."
    }
}
