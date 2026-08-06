//! Prompt token / character budgets.

use serde::{Deserialize, Serialize};

use jaymi_context::DEFAULT_CHARS_PER_TOKEN;

use crate::model::{ModelLimits, DEFAULT_RESERVED_COMPLETION_TOKENS};

/// Default character budget for one assembled prompt (when no model window is known).
pub const DEFAULT_PROMPT_MAX_CHARACTERS: usize = 24_000;

/// Limits applied while assembling a [`super::Prompt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBudget {
    /// Soft character ceiling (`None` = unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<usize>,
    /// Soft estimated-token ceiling for the **prompt** (`None` = unlimited).
    ///
    /// When derived from a model, this is `context_window − reserved_completion`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Characters-per-token estimate for budgeting (provider-independent).
    pub chars_per_token: usize,
    /// Tokens reserved for the model completion (not available to the prompt).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub reserved_completion_tokens: u64,
    /// Full model context window when the budget was derived from model limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u64>,
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

impl Default for PromptBudget {
    fn default() -> Self {
        Self {
            max_characters: Some(DEFAULT_PROMPT_MAX_CHARACTERS),
            max_tokens: None,
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
            reserved_completion_tokens: DEFAULT_RESERVED_COMPLETION_TOKENS,
            context_window_tokens: None,
        }
    }
}

impl PromptBudget {
    /// Unlimited budget (still reports estimated tokens).
    pub fn unlimited() -> Self {
        Self {
            max_characters: None,
            max_tokens: None,
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
            reserved_completion_tokens: 0,
            context_window_tokens: None,
        }
    }

    /// Character-limited budget.
    pub fn characters(max_characters: usize) -> Self {
        Self {
            max_characters: Some(max_characters),
            max_tokens: None,
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
            reserved_completion_tokens: 0,
            context_window_tokens: None,
        }
    }

    /// Token-limited prompt budget (no model window metadata).
    pub fn tokens(max_tokens: u64) -> Self {
        Self {
            max_characters: None,
            max_tokens: Some(max_tokens),
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
            reserved_completion_tokens: 0,
            context_window_tokens: None,
        }
    }

    /// Derive a prompt budget from model limits.
    ///
    /// ```text
    /// prompt_tokens = context_window.saturating_sub(reserved_completion)
    /// ```
    ///
    /// Scales automatically for long-context models when `context_tokens` is set.
    /// When the window is unknown, falls back to [`DEFAULT_PROMPT_MAX_CHARACTERS`]
    /// while still recording the reserved completion for diagnostics.
    pub fn from_model_limits(limits: &ModelLimits, reserved_completion: u64) -> Self {
        let reserved = if reserved_completion == 0 {
            limits.reserved_completion_tokens()
        } else {
            reserved_completion
        };
        let chars_per_token = DEFAULT_CHARS_PER_TOKEN;
        match limits.context_tokens {
            Some(window) => {
                let prompt_tokens = window.saturating_sub(reserved).max(1);
                Self {
                    max_characters: None,
                    max_tokens: Some(prompt_tokens),
                    chars_per_token,
                    reserved_completion_tokens: reserved,
                    context_window_tokens: Some(window),
                }
            }
            None => Self {
                max_characters: Some(DEFAULT_PROMPT_MAX_CHARACTERS),
                max_tokens: None,
                chars_per_token,
                reserved_completion_tokens: reserved,
                context_window_tokens: None,
            },
        }
    }

    /// Override reserved completion tokens and recompute prompt ceiling when a window is known.
    pub fn with_reserved_completion(mut self, reserved: u64) -> Self {
        self.reserved_completion_tokens = reserved;
        if let Some(window) = self.context_window_tokens {
            self.max_tokens = Some(window.saturating_sub(reserved).max(1));
            self.max_characters = None;
        }
        self
    }

    /// Estimate tokens from a character count.
    pub fn estimate_tokens(&self, characters: usize) -> u64 {
        let divisor = self.chars_per_token.max(1);
        characters.div_ceil(divisor) as u64
    }

    /// Effective character ceiling considering both char and token limits.
    pub fn effective_max_characters(&self) -> Option<usize> {
        let from_tokens = self.max_tokens.map(|tokens| {
            let cpt = self.chars_per_token.max(1);
            (tokens as usize).saturating_mul(cpt)
        });
        match (self.max_characters, from_tokens) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// Effective prompt token ceiling.
    pub fn effective_max_tokens(&self) -> Option<u64> {
        match (self.max_tokens, self.max_characters) {
            (Some(tokens), Some(chars)) => {
                let from_chars = self.estimate_tokens(chars);
                Some(tokens.min(from_chars))
            }
            (Some(tokens), None) => Some(tokens),
            (None, Some(chars)) => Some(self.estimate_tokens(chars)),
            (None, None) => None,
        }
    }
}

/// How much of the budget a finished prompt consumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBudgetUsage {
    /// Characters used in the **delivered** prompt (chat message contents).
    pub used_characters: usize,
    /// Estimated tokens for the **delivered** prompt.
    pub estimated_tokens: u64,
    /// Configured character ceiling when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<usize>,
    /// Configured prompt token ceiling when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Remaining characters under the effective ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_characters: Option<usize>,
    /// Remaining prompt tokens under the effective ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_tokens: Option<u64>,
    /// Tokens reserved for completion (not available to the prompt).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub reserved_completion_tokens: u64,
    /// Full model context window when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u64>,
    /// Context efficiency in basis points (`used / ceiling * 10_000`), when bounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_efficiency_bps: Option<u16>,
    /// True when truncation was required to fit the budget.
    pub truncated: bool,
}

impl PromptBudgetUsage {
    /// Build usage from budget + final size.
    pub fn from_budget(budget: &PromptBudget, used_characters: usize, truncated: bool) -> Self {
        let max_characters = budget.effective_max_characters();
        let estimated_tokens = budget.estimate_tokens(used_characters);
        let max_tokens = budget.effective_max_tokens();
        let remaining_tokens = max_tokens.map(|max| max.saturating_sub(estimated_tokens));
        let context_efficiency_bps = max_tokens.map(|max| {
            if max == 0 {
                0
            } else {
                ((estimated_tokens.saturating_mul(10_000)) / max).min(10_000) as u16
            }
        });
        Self {
            used_characters,
            estimated_tokens,
            max_characters,
            max_tokens,
            remaining_characters: max_characters.map(|max| max.saturating_sub(used_characters)),
            remaining_tokens,
            reserved_completion_tokens: budget.reserved_completion_tokens,
            context_window_tokens: budget.context_window_tokens,
            context_efficiency_bps,
            truncated,
        }
    }

    /// Tokens used (alias of [`Self::estimated_tokens`] for diagnostics naming).
    pub fn tokens_used(&self) -> u64 {
        self.estimated_tokens
    }

    /// Tokens remaining under the prompt ceiling.
    pub fn tokens_remaining(&self) -> Option<u64> {
        self.remaining_tokens
    }

    /// Context efficiency as a 0.0–1.0 ratio when bounded.
    pub fn context_efficiency(&self) -> Option<f64> {
        self.context_efficiency_bps
            .map(|bps| bps as f64 / 10_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelLimits;

    #[test]
    fn from_model_limits_reserves_completion() {
        let limits = ModelLimits::new(8_192).with_max_output_tokens(1_024);
        let budget = PromptBudget::from_model_limits(&limits, 1_024);
        assert_eq!(budget.context_window_tokens, Some(8_192));
        assert_eq!(budget.max_tokens, Some(7_168));
        assert_eq!(budget.reserved_completion_tokens, 1_024);
        assert_eq!(budget.effective_max_characters(), Some(7_168 * DEFAULT_CHARS_PER_TOKEN));
    }

    #[test]
    fn long_context_models_scale_budget() {
        let limits = ModelLimits::new(131_072);
        let budget = PromptBudget::from_model_limits(&limits, 4_096);
        assert_eq!(budget.max_tokens, Some(131_072 - 4_096));
    }

    #[test]
    fn usage_reports_remaining_and_efficiency() {
        let budget = PromptBudget::tokens(100);
        let usage = PromptBudgetUsage::from_budget(&budget, 200, false); // 200 chars ≈ 50 tokens
        assert_eq!(usage.estimated_tokens, 50);
        assert_eq!(usage.remaining_tokens, Some(50));
        assert_eq!(usage.context_efficiency_bps, Some(5_000));
    }
}
