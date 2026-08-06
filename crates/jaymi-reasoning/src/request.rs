//! Reasoning request envelope.

use jaymi_context::LlmContext;
use serde::Serialize;

use crate::cancellation::CancellationToken;
use crate::error::{ReasoningError, ReasoningResult};
use crate::model::ModelIdentifier;
use crate::parameters::GenerationParameters;
use crate::prompt::Prompt;
use crate::types::ConversationTurn;

/// Input to a reasoning provider.
///
/// Carries structured [`LlmContext`] (never raw subsystem queries), optional
/// conversation history, generation parameters, and — once the Reasoning Engine
/// has run PromptBuilder — an assembled [`Prompt`].
///
/// Providers must consume [`Self::prompt`] for generation content. They must not
/// rebuild prompts from `goal`, `history`, or `context`.
///
/// Serialize-only: [`LlmContext`] is currently serialize-only; round-trip
/// deserialization of the full request is deferred until that changes.
#[derive(Debug, Clone, Serialize)]
pub struct ReasoningRequest {
    /// Primary user goal / latest utterance.
    pub goal: String,
    /// Prior turns (oldest first), excluding the latest goal when duplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<ConversationTurn>,
    /// Structured request context from the Context Engine.
    pub context: LlmContext,
    /// Assembled prompt from PromptBuilder (attached by ReasoningEngine).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Prompt>,
    /// Sampling / limit parameters.
    #[serde(default)]
    pub parameters: GenerationParameters,
    /// Preferred model when the caller has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelIdentifier>,
    /// Optional correlation id for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Live cancellation handle (not serialized).
    #[serde(skip)]
    pub cancellation: CancellationToken,
}

impl ReasoningRequest {
    /// Build a request from a goal and assembled LLM context.
    pub fn new(goal: impl Into<String>, context: LlmContext) -> Self {
        Self {
            goal: goal.into(),
            history: Vec::new(),
            context,
            prompt: None,
            parameters: GenerationParameters::default(),
            model: None,
            request_id: None,
            cancellation: CancellationToken::new(),
        }
    }

    /// Attach conversation history.
    pub fn with_history(mut self, history: Vec<ConversationTurn>) -> Self {
        self.history = history;
        self
    }

    /// Attach an assembled [`Prompt`] (PromptBuilder output).
    pub fn with_prompt(mut self, prompt: Prompt) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// Attach generation parameters.
    pub fn with_parameters(mut self, parameters: GenerationParameters) -> Self {
        self.parameters = parameters;
        self
    }

    /// Prefer a specific model.
    pub fn with_model(mut self, model: ModelIdentifier) -> Self {
        self.model = Some(model);
        self
    }

    /// Attach a correlation id.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Replace the cancellation token.
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// True when the caller has requested cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// True when PromptBuilder output is attached.
    pub fn has_assembled_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// Borrow the assembled prompt, or fail if the engine did not attach one.
    pub fn require_prompt(&self) -> ReasoningResult<&Prompt> {
        self.prompt.as_ref().ok_or_else(|| ReasoningError::InvalidRequest {
            reason: "assembled prompt required — providers consume PromptBuilder output only"
                .into(),
        })
    }

    /// Latest user content: goal, or last user turn if goal is empty.
    pub fn latest_user_text(&self) -> &str {
        if !self.goal.trim().is_empty() {
            return self.goal.as_str();
        }
        self.history
            .iter()
            .rev()
            .find(|turn| matches!(turn.role, crate::types::ConversationRole::User))
            .map(|turn| turn.content.as_str())
            .unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
        UserRequestMetadataSection,
    };

    fn sample_context() -> LlmContext {
        let bundle = ContextBundleBuilder::new()
            .user_request(UserRequestMetadataSection {
                content_preview: "hello".into(),
                ..UserRequestMetadataSection::default()
            })
            .planner_metadata(PlannerMetadataSection {
                assemble_generation: 1,
                sources: vec![ContextSource::UserRequest],
                notes: vec![],
                budget: None,
                policy: None,
            })
            .build();
        LlmContext::from_bundle(&bundle)
    }

    #[test]
    fn new_request_carries_goal_and_context() {
        let request = ReasoningRequest::new("summarize this", sample_context());
        assert_eq!(request.goal, "summarize this");
        assert_eq!(request.context.schema_version, 1);
        assert!(!request.is_cancelled());
        assert!(!request.has_assembled_prompt());
        assert!(request.require_prompt().is_err());
    }

    #[test]
    fn cancellation_is_visible_on_request() {
        let token = CancellationToken::new();
        let request = ReasoningRequest::new("x", sample_context()).with_cancellation(token.clone());
        token.cancel();
        assert!(request.is_cancelled());
    }
}
