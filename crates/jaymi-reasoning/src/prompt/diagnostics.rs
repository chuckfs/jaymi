//! Prompt diagnostics for size, contribution, budget, and truncation.

use serde::{Deserialize, Serialize};

use super::budget::PromptBudgetUsage;
use super::section::{PromptSectionDisposition, PromptSectionId};

/// Per-section contribution to the assembled prompt.
///
/// Every section in the builder's emission order appears here — including
/// excluded / budgeted / filtered ones — so context never disappears silently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSectionContribution {
    /// Section identity.
    pub id: PromptSectionId,
    /// Characters contributed to the final prompt (`0` when not included).
    pub characters: usize,
    /// Estimated tokens for this contribution.
    pub estimated_tokens: u64,
    /// True when the section appears in the final prompt.
    pub included: bool,
    /// True when the section body was shortened to fit budget.
    pub truncated: bool,
    /// Explicit fate — included / excluded / truncated / filtered / budgeted.
    pub disposition: PromptSectionDisposition,
    /// Why the section was omitted, shortened, or filtered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `LlmSectionId` labels that feed this prompt section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_llm_sections: Vec<String>,
}

/// Coverage of one [`jaymi_context::LlmSectionId`] through PromptBuilder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptLlmCoverageEntry {
    /// `LlmSectionId::as_str()` label.
    pub llm_section: String,
    /// Prompt section that consumes it (`None` only for documented non-prompt metadata).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_section: Option<PromptSectionId>,
    /// Fate after PromptBuilder.
    pub disposition: PromptSectionDisposition,
    /// Whether the Llm section was `present` on the request context.
    pub llm_present: bool,
    /// Human-readable explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Diagnostics exposed for a finished [`super::Prompt`].
///
/// After sealing for delivery (Sprint B1.13.5), size / token fields describe the
/// prompt **actually delivered** via [`super::Prompt::to_chat_messages`] — never
/// unused or alternate framing. Assembled size is retained separately for the
/// Performance dashboard (observational only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptDiagnostics {
    /// Total characters in the **delivered** prompt (chat message contents).
    pub prompt_size_characters: usize,
    /// Estimated tokens for the **delivered** prompt.
    pub prompt_size_tokens: u64,
    /// Characters in the PromptBuilder-assembled prompt **before** delivery seal.
    ///
    /// Observational only — never used for budgeting or generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembled_prompt_size_characters: Option<usize>,
    /// Estimated tokens for the assembled prompt before delivery seal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembled_prompt_size_tokens: Option<u64>,
    /// Final token estimate for the delivered prompt (same basis as size tokens).
    #[serde(default)]
    pub final_token_estimate: u64,
    /// Prior conversation turns folded into the Conversation section (0 when none).
    #[serde(default)]
    pub conversation_turns: u64,
    /// Budget usage snapshot (used size matches delivered prompt).
    pub budget: PromptBudgetUsage,
    /// Per-section contribution breakdown (emission order, including omitted).
    ///
    /// Unused sections stay listed with disposition + `characters: 0` — their
    /// bodies are never counted toward prompt size.
    pub sections: Vec<PromptSectionContribution>,
    /// Every `LlmSectionId` traced through PromptBuilder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub llm_coverage: Vec<PromptLlmCoverageEntry>,
    /// True when any section was truncated or omitted for budget.
    pub truncated: bool,
    /// Human-readable truncation / fit notes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncation_notes: Vec<String>,
    /// Optional template id used (future templates).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    /// Optional formatter id used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatter_id: Option<String>,
    /// Optional model adapter id used (future adapters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    /// Wall-clock milliseconds spent in PromptBuilder::build (diagnostics only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_duration_ms: Option<u64>,
}

impl PromptDiagnostics {
    /// Aggregate included section count (final prompt only).
    pub fn included_section_count(&self) -> usize {
        self.sections.iter().filter(|s| s.included).count()
    }

    /// Sections with disposition [`PromptSectionDisposition::Excluded`].
    pub fn excluded_sections(&self) -> Vec<PromptSectionId> {
        self.sections
            .iter()
            .filter(|section| section.disposition == PromptSectionDisposition::Excluded)
            .map(|section| section.id)
            .collect()
    }

    /// Sections with disposition [`PromptSectionDisposition::Budgeted`].
    pub fn budgeted_sections(&self) -> Vec<PromptSectionId> {
        self.sections
            .iter()
            .filter(|section| section.disposition == PromptSectionDisposition::Budgeted)
            .map(|section| section.id)
            .collect()
    }

    /// Sections with disposition [`PromptSectionDisposition::Filtered`].
    pub fn filtered_sections(&self) -> Vec<PromptSectionId> {
        self.sections
            .iter()
            .filter(|section| section.disposition == PromptSectionDisposition::Filtered)
            .map(|section| section.id)
            .collect()
    }

    /// Section ids that were truncated or budget-omitted.
    pub fn truncated_sections(&self) -> Vec<PromptSectionId> {
        self.sections
            .iter()
            .filter(|section| {
                matches!(
                    section.disposition,
                    PromptSectionDisposition::Truncated | PromptSectionDisposition::Budgeted
                ) || section.truncated
            })
            .map(|section| section.id)
            .collect()
    }

    /// Tokens used in the final prompt.
    pub fn tokens_used(&self) -> u64 {
        self.budget.tokens_used()
    }

    /// Tokens remaining under the prompt ceiling.
    pub fn tokens_remaining(&self) -> Option<u64> {
        self.budget.tokens_remaining()
    }

    /// Context efficiency (0.0–1.0) when the budget is bounded.
    pub fn context_efficiency(&self) -> Option<f64> {
        self.budget.context_efficiency()
    }

    /// Compact disposition summary for diagnostics surfaces.
    pub fn disposition_summary(&self) -> String {
        let mut included = 0usize;
        let mut excluded = 0usize;
        let mut truncated = 0usize;
        let mut filtered = 0usize;
        let mut budgeted = 0usize;
        for section in &self.sections {
            match section.disposition {
                PromptSectionDisposition::Included => included += 1,
                PromptSectionDisposition::Excluded => excluded += 1,
                PromptSectionDisposition::Truncated => truncated += 1,
                PromptSectionDisposition::Filtered => filtered += 1,
                PromptSectionDisposition::Budgeted => budgeted += 1,
            }
        }
        format!(
            "included={included} · excluded={excluded} · truncated={truncated} · filtered={filtered} · budgeted={budgeted}"
        )
    }

    /// Characters counted only from sections that were actually delivered.
    pub fn delivered_section_characters(&self) -> usize {
        self.sections
            .iter()
            .filter(|section| section.included)
            .map(|section| section.characters)
            .sum()
    }
}
