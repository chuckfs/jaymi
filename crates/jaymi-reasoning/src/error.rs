//! Reasoning error vocabulary (provider-independent).

use jaymi_core::{JaymiError, JaymiResult};
use serde::{Deserialize, Serialize};

/// Result alias for reasoning operations.
pub type ReasoningResult<T> = Result<T, ReasoningError>;

/// Failures that may occur while preparing or running reasoning.
///
/// Variants describe *what* failed, never *which transport or vendor* did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ReasoningError {
    /// No reasoning backend is wired (contract-only / stub engine).
    NotImplemented,
    /// Backend is registered but currently unavailable.
    Unavailable {
        /// Human-readable explanation.
        reason: String,
    },
    /// Caller cancelled the request (token or stream abort).
    Cancelled,
    /// Request failed validation before generation.
    InvalidRequest {
        /// Validation explanation.
        reason: String,
    },
    /// Requested model is not known to the provider.
    ModelNotFound {
        /// Requested model id (as text).
        model: String,
    },
    /// Generation started but failed.
    GenerationFailed {
        /// Failure explanation.
        reason: String,
    },
    /// Generation exceeded an allowed duration.
    TimedOut {
        /// Allowed milliseconds when known.
        limit_ms: Option<u64>,
    },
    /// Streaming protocol violation or incomplete stream.
    StreamFailed {
        /// Failure explanation.
        reason: String,
    },
}

impl ReasoningError {
    /// Stable label for diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::Unavailable { .. } => "unavailable",
            Self::Cancelled => "cancelled",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::ModelNotFound { .. } => "model_not_found",
            Self::GenerationFailed { .. } => "generation_failed",
            Self::TimedOut { .. } => "timed_out",
            Self::StreamFailed { .. } => "stream_failed",
        }
    }

    /// Human-readable message.
    pub fn message(&self) -> String {
        match self {
            Self::NotImplemented => "reasoning backend is not implemented".into(),
            Self::Unavailable { reason } => format!("reasoning unavailable: {reason}"),
            Self::Cancelled => "reasoning cancelled".into(),
            Self::InvalidRequest { reason } => format!("invalid reasoning request: {reason}"),
            Self::ModelNotFound { model } => format!("model not found: {model}"),
            Self::GenerationFailed { reason } => format!("generation failed: {reason}"),
            Self::TimedOut { limit_ms } => match limit_ms {
                Some(ms) => format!("reasoning timed out after {ms}ms"),
                None => "reasoning timed out".into(),
            },
            Self::StreamFailed { reason } => format!("reasoning stream failed: {reason}"),
        }
    }
}

impl std::fmt::Display for ReasoningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ReasoningError {}

impl From<ReasoningError> for JaymiError {
    fn from(error: ReasoningError) -> Self {
        JaymiError::new(error.message())
    }
}

impl ReasoningError {
    /// Convert into a [`JaymiResult`] error.
    pub fn into_jaymi_result<T>(self) -> JaymiResult<T> {
        Err(self.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_messages_are_stable() {
        assert_eq!(ReasoningError::NotImplemented.as_str(), "not_implemented");
        assert_eq!(ReasoningError::Cancelled.as_str(), "cancelled");
        assert!(ReasoningError::NotImplemented
            .message()
            .contains("not implemented"));
    }

    #[test]
    fn converts_to_jaymi_error() {
        let jaymi: JaymiError = ReasoningError::Cancelled.into();
        assert!(jaymi.message().contains("cancelled"));
    }
}
