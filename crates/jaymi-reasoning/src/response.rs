//! Reasoning response envelope.

use serde::{Deserialize, Serialize};

use crate::metrics::ReasoningMetrics;
use crate::model::ModelIdentifier;
use crate::types::ConversationTurn;

/// Why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural completion.
    Completed,
    /// Hit max output tokens / length limit.
    Length,
    /// Matched a stop sequence.
    StopSequence,
    /// Caller cancelled.
    Cancelled,
    /// Provider aborted for content / safety policy.
    ContentFilter,
    /// Error path (details on [`crate::ReasoningError`] when returned as Err).
    Error,
}

impl FinishReason {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Length => "length",
            Self::StopSequence => "stop_sequence",
            Self::Cancelled => "cancelled",
            Self::ContentFilter => "content_filter",
            Self::Error => "error",
        }
    }
}

/// Successful (or soft-failed) reasoning outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningResponse {
    /// Primary textual answer.
    pub content: String,
    /// Why generation stopped.
    pub finish_reason: FinishReason,
    /// Model that produced the answer when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelIdentifier>,
    /// Timing / token metrics.
    pub metrics: ReasoningMetrics,
    /// Assistant turn ready to append to conversation history.
    pub assistant_turn: ConversationTurn,
    /// Optional structured notes for diagnostics (never vendor payloads).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl ReasoningResponse {
    /// Build a completed response from content.
    pub fn completed(content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            assistant_turn: ConversationTurn::assistant(content.clone()),
            content,
            finish_reason: FinishReason::Completed,
            model: None,
            metrics: ReasoningMetrics::default(),
            notes: Vec::new(),
        }
    }

    /// Attach metrics.
    pub fn with_metrics(mut self, metrics: ReasoningMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Attach model identity.
    pub fn with_model(mut self, model: ModelIdentifier) -> Self {
        self.model = Some(model);
        self
    }

    /// Override finish reason.
    pub fn with_finish_reason(mut self, reason: FinishReason) -> Self {
        self.finish_reason = reason;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_sets_assistant_turn() {
        let response = ReasoningResponse::completed("hello");
        assert_eq!(response.content, "hello");
        assert_eq!(response.finish_reason, FinishReason::Completed);
        assert_eq!(response.assistant_turn.content, "hello");
    }
}
