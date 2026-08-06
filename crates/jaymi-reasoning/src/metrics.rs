//! Generation / latency metrics for a reasoning call.

use serde::{Deserialize, Serialize};

use crate::lifecycle::CancelReason;
use crate::model::ModelIdentifier;

/// Measurable outcome of a reasoning call (complete or stream).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ReasoningMetrics {
    /// Wall-clock duration in milliseconds (request start → terminal).
    pub latency_ms: u64,
    /// Provider-reported duration when known (e.g. Ollama `total_duration`), else TTFT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_latency_ms: Option<u64>,
    /// Time from first visible token to terminal, when streaming produced tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_duration_ms: Option<u64>,
    /// Approximate tokens/sec × 1000 (milli-tokens per second), when measurable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_sec_milli: Option<u64>,
    /// Estimated prompt / input tokens when known.
    pub input_tokens: Option<u64>,
    /// Estimated completion / output tokens when known.
    pub output_tokens: Option<u64>,
    /// Total tokens when known (`input + output` or provider-reported).
    pub total_tokens: Option<u64>,
    /// Model that produced the output, when resolved.
    pub model: Option<ModelIdentifier>,
    /// Logical provider id selected by the Reasoning Engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Attempts performed by the engine (1 = no retry).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attempts: u32,
    /// True when the caller cancelled before natural completion.
    pub cancelled: bool,
    /// Why cancellation occurred, when cancelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<CancelReason>,
    /// True when the response was truncated by parameter limits.
    pub truncated: bool,
    /// True when terminal content is a partial (cancel / disconnect / fail).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

impl ReasoningMetrics {
    /// Builder helper for a timed completion.
    pub fn timed(latency_ms: u64) -> Self {
        Self {
            latency_ms,
            ..Self::default()
        }
    }

    /// Attach model identity.
    pub fn with_model(mut self, model: ModelIdentifier) -> Self {
        self.model = Some(model);
        self
    }

    /// Attach provider id.
    pub fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = Some(provider_id.into());
        self
    }

    /// Record attempt count.
    pub fn with_attempts(mut self, attempts: u32) -> Self {
        self.attempts = attempts;
        self
    }

    /// Attach token counts (fills total when both sides present).
    pub fn with_tokens(mut self, input: Option<u64>, output: Option<u64>) -> Self {
        self.input_tokens = input;
        self.output_tokens = output;
        self.total_tokens = match (input, output) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        self
    }

    /// Attach provider-reported or time-to-first-token latency.
    pub fn with_provider_latency_ms(mut self, provider_latency_ms: u64) -> Self {
        self.provider_latency_ms = Some(provider_latency_ms);
        self
    }

    /// Attach generation duration (first token → end).
    pub fn with_generation_duration_ms(mut self, generation_duration_ms: u64) -> Self {
        self.generation_duration_ms = Some(generation_duration_ms);
        self
    }

    /// Attach approximate tokens/sec (stored as milli-tokens/sec).
    pub fn with_tokens_per_sec(mut self, tokens_per_sec: f64) -> Self {
        if tokens_per_sec.is_finite() && tokens_per_sec >= 0.0 {
            self.tokens_per_sec_milli = Some((tokens_per_sec * 1000.0).round() as u64);
        }
        self
    }

    /// Attach cancel reason (also sets `cancelled`).
    pub fn with_cancel_reason(mut self, reason: CancelReason) -> Self {
        self.cancelled = true;
        self.cancel_reason = Some(reason);
        self
    }

    /// Mark content as partial.
    pub fn with_partial(mut self, partial: bool) -> Self {
        self.partial = partial;
        self
    }

    /// Tokens/sec derived from milli field, when present.
    pub fn tokens_per_sec(&self) -> Option<f64> {
        self.tokens_per_sec_milli
            .map(|milli| milli as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelIdentifier;

    #[test]
    fn with_tokens_fills_total() {
        let metrics = ReasoningMetrics::timed(12).with_tokens(Some(10), Some(5));
        assert_eq!(metrics.total_tokens, Some(15));
        assert_eq!(metrics.latency_ms, 12);
    }

    #[test]
    fn with_model_attaches_identifier() {
        let model = ModelIdentifier::new("local", "general");
        let metrics = ReasoningMetrics::default().with_model(model.clone());
        assert_eq!(metrics.model.as_ref(), Some(&model));
    }
}
