//! Streaming chunk contract.

use serde::{Deserialize, Serialize};

use crate::metrics::ReasoningMetrics;
use crate::response::FinishReason;

/// Kind of streamed payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingChunkKind {
    /// Incremental visible text.
    Token,
    /// Optional intermediate thought / scratch text (not final answer).
    Thought,
    /// Stream completed successfully (may carry final metrics).
    Completed,
    /// Stream ended because the caller cancelled.
    Cancelled,
    /// Stream ended with a recoverable soft failure signal.
    Failed,
}

impl StreamingChunkKind {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Thought => "thought",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    /// True when this chunk terminates the stream.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

/// One unit of streamed reasoning output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamingChunk {
    /// Chunk kind.
    pub kind: StreamingChunkKind,
    /// Monotonic index within the stream (0-based).
    pub index: u64,
    /// Text payload when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Finish reason on terminal chunks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Metrics on terminal chunks when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ReasoningMetrics>,
}

impl StreamingChunk {
    /// Visible token chunk.
    pub fn token(index: u64, text: impl Into<String>) -> Self {
        Self {
            kind: StreamingChunkKind::Token,
            index,
            text: Some(text.into()),
            finish_reason: None,
            metrics: None,
        }
    }

    /// Intermediate thought chunk.
    pub fn thought(index: u64, text: impl Into<String>) -> Self {
        Self {
            kind: StreamingChunkKind::Thought,
            index,
            text: Some(text.into()),
            finish_reason: None,
            metrics: None,
        }
    }

    /// Successful completion marker.
    pub fn completed(index: u64, metrics: ReasoningMetrics) -> Self {
        Self {
            kind: StreamingChunkKind::Completed,
            index,
            text: None,
            finish_reason: Some(FinishReason::Completed),
            metrics: Some(metrics),
        }
    }

    /// Cancellation marker.
    pub fn cancelled(index: u64) -> Self {
        Self {
            kind: StreamingChunkKind::Cancelled,
            index,
            text: None,
            finish_reason: Some(FinishReason::Cancelled),
            metrics: Some(ReasoningMetrics {
                cancelled: true,
                ..ReasoningMetrics::default()
            }),
        }
    }

    /// Cancellation marker with reason + optional partial metrics.
    pub fn cancelled_with_reason(index: u64, metrics: ReasoningMetrics) -> Self {
        let mut metrics = metrics;
        metrics.cancelled = true;
        Self {
            kind: StreamingChunkKind::Cancelled,
            index,
            text: None,
            finish_reason: Some(FinishReason::Cancelled),
            metrics: Some(metrics),
        }
    }

    /// Soft failure marker (partial content may already have been streamed).
    pub fn failed(index: u64, metrics: ReasoningMetrics) -> Self {
        Self {
            kind: StreamingChunkKind::Failed,
            index,
            text: None,
            finish_reason: Some(FinishReason::Error),
            metrics: Some(metrics),
        }
    }

    /// True when no further chunks should follow.
    pub fn is_terminal(&self) -> bool {
        self.kind.is_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_kinds() {
        assert!(StreamingChunkKind::Completed.is_terminal());
        assert!(StreamingChunkKind::Cancelled.is_terminal());
        assert!(!StreamingChunkKind::Token.is_terminal());
        assert!(StreamingChunk::cancelled(3).is_terminal());
    }
}
