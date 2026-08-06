//! ReasoningProvider — the only backend abstraction the engine knows.

use crate::error::{ReasoningError, ReasoningResult};
use crate::model::ReasoningModelInfo;
use crate::request::ReasoningRequest;
use crate::response::ReasoningResponse;
use crate::stream::StreamingChunk;

/// What a reasoning backend can do.
///
/// Capabilities are semantic flags — never transport or vendor names.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ReasoningCapabilities {
    /// Can produce a complete response in one call.
    pub complete: bool,
    /// Can deliver incremental [`StreamingChunk`]s.
    pub stream: bool,
    /// Honors cooperative cancellation mid-generation.
    pub cancellation: bool,
    /// Can list discoverable models.
    pub list_models: bool,
    /// Reports live health.
    pub health: bool,
    /// Accepts multi-turn conversation history.
    pub multi_turn: bool,
    /// Request carries structured [`jaymi_context::LlmContext`] (via PromptBuilder).
    pub structured_context: bool,
    /// Consumes assembled [`crate::Prompt`] — never rebuilds prompts itself.
    pub assembled_prompt: bool,
}

impl ReasoningCapabilities {
    /// Full contract surface (typical for a mature backend).
    pub fn full() -> Self {
        Self {
            complete: true,
            stream: true,
            cancellation: true,
            list_models: true,
            health: true,
            multi_turn: true,
            structured_context: true,
            assembled_prompt: true,
        }
    }
}

/// Live readiness of a reasoning backend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningHealth {
    /// Ready to accept requests.
    Ready,
    /// Registered but temporarily unable to serve.
    Degraded {
        /// Human-readable explanation.
        reason: String,
    },
    /// Not usable.
    Unavailable {
        /// Human-readable explanation.
        reason: String,
    },
}

impl ReasoningHealth {
    /// True when new requests may be accepted.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Ready | Self::Degraded { .. })
    }

    /// Stable label.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded { .. } => "degraded",
            Self::Unavailable { .. } => "unavailable",
        }
    }
}

/// Incremental stream handle returned by [`ReasoningProvider::stream`].
///
/// Callers pull chunks until a terminal chunk or [`Self::cancel`].
pub trait ReasoningStream: Send {
    /// Pull the next chunk, or `Ok(None)` when the stream is exhausted
    /// without a terminal marker (providers should prefer an explicit
    /// terminal [`StreamingChunk`]).
    fn next_chunk(&mut self) -> ReasoningResult<Option<StreamingChunk>>;

    /// Request cooperative cancellation; subsequent chunks should end with
    /// a cancelled terminal chunk or [`ReasoningError::Cancelled`].
    fn cancel(&mut self);
}

/// Interchangeable reasoning backend.
///
/// The Reasoning Engine depends only on this trait. Concrete backends
/// (local runtimes, remote APIs, test doubles) implement it outside this crate.
pub trait ReasoningProvider: Send + Sync {
    /// Logical registration id for this backend (not a wire protocol name).
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Declared capabilities.
    fn capabilities(&self) -> ReasoningCapabilities;

    /// Current health.
    fn health(&self) -> ReasoningHealth;

    /// Discoverable models when [`ReasoningCapabilities::list_models`] is true.
    fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
        if !self.capabilities().list_models {
            return Err(ReasoningError::Unavailable {
                reason: "list_models is not supported by this provider".into(),
            });
        }
        Ok(Vec::new())
    }

    /// Model context / generation limits for PromptBuilder budgeting.
    ///
    /// Default implementation looks up [`Self::list_models`]. Providers should
    /// override when they can report limits without a full model listing.
    fn model_limits(
        &self,
        model: Option<&crate::model::ModelIdentifier>,
    ) -> ReasoningResult<crate::model::ModelLimits> {
        let models = self.list_models()?;
        if let Some(wanted) = model {
            if let Some(info) = models.iter().find(|info| {
                info.id.provider == wanted.provider && info.id.name == wanted.name
            }) {
                return Ok(info.limits());
            }
            return Err(ReasoningError::ModelNotFound {
                model: wanted.display(),
            });
        }
        Ok(models
            .first()
            .map(ReasoningModelInfo::limits)
            .unwrap_or_default())
    }

    /// Produce a complete response.
    fn complete(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse>;

    /// Begin a streaming generation.
    fn stream(&self, request: ReasoningRequest) -> ReasoningResult<Box<dyn ReasoningStream>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_usability() {
        assert!(ReasoningHealth::Ready.is_usable());
        assert!(ReasoningHealth::Degraded {
            reason: "warm".into()
        }
        .is_usable());
        assert!(!ReasoningHealth::Unavailable {
            reason: "down".into()
        }
        .is_usable());
    }

    #[test]
    fn full_capabilities_enable_surface() {
        let caps = ReasoningCapabilities::full();
        assert!(caps.complete && caps.stream && caps.cancellation);
    }
}
