//! Assembled prompt types.

use serde::{Deserialize, Serialize};

use super::diagnostics::PromptDiagnostics;
use super::section::PromptSectionId;

/// Schema version for [`Prompt`] layout.
pub const PROMPT_SCHEMA_VERSION: u32 = 1;

/// One included section in an assembled prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSection {
    /// Section identity.
    pub id: PromptSectionId,
    /// Display heading.
    pub heading: String,
    /// Section body text (provider-independent plain text).
    pub body: String,
}

impl PromptSection {
    /// Character length of heading + body contribution (approx for diagnostics).
    pub fn character_len(&self) -> usize {
        self.heading.chars().count() + self.body.chars().count() + 4
    }
}

/// Immutable assembled prompt ready for a reasoning backend.
///
/// Equality is structural: same sections, text, and diagnostics fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    /// Schema version.
    pub schema_version: u32,
    /// Included sections in emission order.
    pub sections: Vec<PromptSection>,
    /// Fully formatted prompt text (deterministic for identical inputs).
    pub text: String,
    /// Size / budget / truncation diagnostics.
    pub diagnostics: PromptDiagnostics,
}

impl Prompt {
    /// Prompt character size (same as diagnostics).
    pub fn size_characters(&self) -> usize {
        self.diagnostics.prompt_size_characters
    }

    /// Estimated token size.
    pub fn size_tokens(&self) -> u64 {
        self.diagnostics.prompt_size_tokens
    }

    /// True when budget truncation occurred.
    pub fn was_truncated(&self) -> bool {
        self.diagnostics.truncated
    }

    /// Section ids present in the final prompt (emission order).
    pub fn section_ids(&self) -> Vec<PromptSectionId> {
        self.sections.iter().map(|section| section.id).collect()
    }
}
