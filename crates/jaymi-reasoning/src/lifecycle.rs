//! Conversation / generation streaming lifecycle.

use serde::{Deserialize, Serialize};

/// Streaming lifecycle for a conversational generation.
///
/// ```text
/// Idle → Thinking → Streaming → Cancelled | Completed | Failed
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StreamingLifecycle {
    /// No active generation.
    #[default]
    Idle,
    /// Generation started; waiting for the first visible token (may emit thoughts).
    Thinking,
    /// Visible tokens are arriving.
    Streaming,
    /// Caller cancelled; partial content may remain.
    Cancelled,
    /// Natural successful completion.
    Completed,
    /// Failed (disconnect, protocol error, provider error); partial may remain.
    Failed,
}

impl StreamingLifecycle {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Streaming => "streaming",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// True when no further chunks are expected.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }

    /// True when generation is in progress.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Thinking | Self::Streaming)
    }
}

/// Why a stream was cancelled (or aborted into a cancel-like terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    /// Explicit user / caller cancel.
    User,
    /// Engine timeout deadline.
    Timeout,
    /// Provider connection dropped mid-stream.
    ProviderDisconnect,
    /// Engine aborted for internal policy (e.g. pre-start cancel).
    Engine,
    /// Soft cancel after a failed generation that preserved partial text.
    Error,
}

impl CancelReason {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Timeout => "timeout",
            Self::ProviderDisconnect => "provider_disconnect",
            Self::Engine => "engine",
            Self::Error => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_terminal_and_active() {
        assert!(StreamingLifecycle::Completed.is_terminal());
        assert!(StreamingLifecycle::Thinking.is_active());
        assert!(!StreamingLifecycle::Idle.is_active());
        assert_eq!(CancelReason::User.as_str(), "user");
    }
}
