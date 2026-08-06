//! Prompt construction — provider-independent formatting of [`LlmContext`].
//!
//! Architectural path:
//!
//! ```text
//! ContextBundle → LlmContext → PromptBuilder → Prompt → ReasoningRequest
//! ```
//!
//! Providers never assemble prompts. The Planner never concatenates prompt
//! strings. The Reasoning Engine delegates construction here and attaches the
//! assembled [`Prompt`] onto [`crate::ReasoningRequest`] before provider calls.

mod adapter;
mod budget;
mod builder;
mod delivery;
mod diagnostics;
mod format;
mod section;
mod template;
mod types;

pub use adapter::{ModelPromptAdapter, NullPromptAdapter};
pub use budget::{PromptBudget, PromptBudgetUsage, DEFAULT_PROMPT_MAX_CHARACTERS};
pub use builder::PromptBuilder;
pub use delivery::{PromptChatMessage, PromptChatRole};
pub use diagnostics::{
    PromptDiagnostics, PromptLlmCoverageEntry, PromptSectionContribution,
};
pub use format::{PlainTextFormatter, PromptFormatter};
pub use section::{PromptSectionDisposition, PromptSectionId};
pub use template::{DefaultPromptTemplate, PromptTemplate};
pub use types::{Prompt, PromptSection, PROMPT_SCHEMA_VERSION};
