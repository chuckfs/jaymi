//! Map assembled [`Prompt`] → Ollama chat messages (transport only).
//!
//! PromptBuilder is the single source of truth. This module never reads
//! `goal`, `history`, or `LlmContext` for generation content.

use jaymi_reasoning::{Prompt, PromptChatRole, ReasoningRequest, ReasoningResult};

use crate::types::ChatMessage;

/// Build chat messages from the assembled prompt on the request.
///
/// Fails when PromptBuilder output was not attached (call through
/// [`jaymi_reasoning::ReasoningEngine`]).
pub fn messages_from_request(request: &ReasoningRequest) -> ReasoningResult<Vec<ChatMessage>> {
    let prompt = request.require_prompt()?;
    Ok(messages_from_prompt(prompt))
}

/// Transport mapping: PromptBuilder output → Ollama `/api/chat` messages.
pub fn messages_from_prompt(prompt: &Prompt) -> Vec<ChatMessage> {
    prompt
        .to_chat_messages()
        .into_iter()
        .filter_map(|message| {
            let content = message.content.trim();
            if content.is_empty() {
                return None;
            }
            let role = match message.role {
                PromptChatRole::System => "system",
                PromptChatRole::User => "user",
            };
            Some(ChatMessage {
                role: role.into(),
                content: content.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
        UserRequestMetadataSection,
    };
    use jaymi_reasoning::{PromptBuilder, ReasoningRequest};

    fn ctx(preview: &str) -> LlmContext {
        let bundle = ContextBundleBuilder::new()
            .user_request(UserRequestMetadataSection {
                content_preview: preview.into(),
                ..UserRequestMetadataSection::default()
            })
            .planner_metadata(PlannerMetadataSection {
                assemble_generation: 1,
                sources: vec![ContextSource::UserRequest],
                notes: vec![],
                budget: None,
                policy: None,
                            environmental: None,
            })
            .build();
        LlmContext::from_bundle(&bundle)
    }

    #[test]
    fn requires_assembled_prompt() {
        let request = ReasoningRequest::new("hello", ctx("hello"));
        let err = messages_from_request(&request).unwrap_err();
        assert!(matches!(
            err,
            jaymi_reasoning::ReasoningError::InvalidRequest { .. }
        ));
    }

    #[test]
    fn maps_prompt_builder_output_not_raw_goal() {
        let request = ReasoningRequest::new("hello", ctx("hello"));
        let prompt = PromptBuilder::new()
            .with_system_instructions("Stay offline-first.")
            .build_from_request(&request);
        let request = request.with_prompt(prompt.clone());
        let messages = messages_from_request(&request).unwrap();
        assert!(!messages.is_empty());
        let blob: String = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(blob.contains("Stay offline-first."));
        assert!(blob.contains("hello"));
        // Must not invent content outside the assembled prompt.
        assert!(prompt.text.contains("Stay offline-first."));
    }
}
