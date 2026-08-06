//! Deliver an assembled [`Prompt`] to chat-oriented backends.
//!
//! Providers must never rebuild prompt content from `LlmContext`, history, or
//! goal. They map [`PromptChatMessage`] roles onto their wire format only.
//!
//! Sprint **B1.13.5**: [`Prompt::seal_for_delivery`] refreshes diagnostics so
//! they describe the chat messages actually sent — never unused framing.

use super::budget::{PromptBudget, PromptBudgetUsage};
use super::format::{PlainTextFormatter, PromptFormatter};
use super::section::PromptSectionId;
use super::types::{Prompt, PromptSection};

/// Chat role used when delivering a [`Prompt`] to a chat API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptChatRole {
    /// System / instruction content.
    System,
    /// User turn content.
    User,
}

/// One chat message derived from an assembled [`Prompt`].
///
/// Content is taken from PromptBuilder output — never reconstructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChatMessage {
    /// Delivery role.
    pub role: PromptChatRole,
    /// PromptBuilder-owned text.
    pub content: String,
}

impl Prompt {
    /// Deliver this prompt as chat messages for chat-oriented backends.
    ///
    /// * Non-`UserRequest` sections → one system message (PromptBuilder formatting)
    /// * `UserRequest` section body → user message
    /// * Fallback: full [`Prompt::text`] as a single user message
    ///
    /// This is transport adaptation, not prompt construction.
    pub fn to_chat_messages(&self) -> Vec<PromptChatMessage> {
        if self.sections.is_empty() {
            return full_text_as_user(self);
        }

        let system_sections: Vec<PromptSection> = self
            .sections
            .iter()
            .filter(|section| section.id != PromptSectionId::UserRequest)
            .cloned()
            .collect();
        let user_body = self
            .sections
            .iter()
            .find(|section| section.id == PromptSectionId::UserRequest)
            .map(|section| section.body.trim().to_string())
            .filter(|body| !body.is_empty());

        let mut messages = Vec::new();
        if !system_sections.is_empty() {
            let content = PlainTextFormatter.format(&system_sections);
            let content = content.trim().to_string();
            if !content.is_empty() {
                messages.push(PromptChatMessage {
                    role: PromptChatRole::System,
                    content,
                });
            }
        }
        if let Some(content) = user_body {
            messages.push(PromptChatMessage {
                role: PromptChatRole::User,
                content,
            });
        } else if messages.is_empty() {
            return full_text_as_user(self);
        }
        messages
    }

    /// Total characters across delivered chat message contents.
    pub fn delivered_character_count(&self) -> usize {
        self.to_chat_messages()
            .iter()
            .map(|message| message.content.chars().count())
            .sum()
    }

    /// Refresh diagnostics so they describe the prompt actually delivered.
    ///
    /// * Size / tokens / budget usage ← `to_chat_messages()` contents
    /// * Included section chars ← delivery framing (user body without heading)
    /// * Unused sections stay `characters: 0` (never counted as sent content)
    /// * `conversation_turns` / `final_token_estimate` recorded explicitly
    pub fn seal_for_delivery(&mut self, budget: &PromptBudget, conversation_turns: usize) {
        let messages = self.to_chat_messages();
        let delivered_chars: usize = messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum();
        let delivered_tokens = budget.estimate_tokens(delivered_chars);

        let user_delivered = messages
            .iter()
            .find(|message| message.role == PromptChatRole::User)
            .map(|message| message.content.chars().count())
            .unwrap_or(0);

        let formatter = PlainTextFormatter;

        for contribution in &mut self.diagnostics.sections {
            if !contribution.included {
                // Never describe unused prompt content as sent.
                contribution.characters = 0;
                contribution.estimated_tokens = 0;
                continue;
            }
            let characters = if contribution.id == PromptSectionId::UserRequest {
                user_delivered
            } else if let Some(section) = self
                .sections
                .iter()
                .find(|section| section.id == contribution.id)
            {
                // Match delivery: system message uses PlainTextFormatter framing.
                let alone = formatter.format(std::slice::from_ref(section));
                alone.trim().chars().count()
            } else {
                0
            };
            contribution.characters = characters;
            contribution.estimated_tokens = budget.estimate_tokens(characters);
        }

        // Preserve assembled size when the builder already recorded it.
        if self.diagnostics.assembled_prompt_size_characters.is_none() {
            self.diagnostics.assembled_prompt_size_characters =
                Some(self.diagnostics.prompt_size_characters);
            self.diagnostics.assembled_prompt_size_tokens =
                Some(self.diagnostics.prompt_size_tokens);
        }
        self.diagnostics.prompt_size_characters = delivered_chars;
        self.diagnostics.prompt_size_tokens = delivered_tokens;
        self.diagnostics.final_token_estimate = delivered_tokens;
        self.diagnostics.conversation_turns = conversation_turns as u64;
        self.diagnostics.budget =
            PromptBudgetUsage::from_budget(budget, delivered_chars, self.diagnostics.truncated);
    }
}

fn full_text_as_user(prompt: &Prompt) -> Vec<PromptChatMessage> {
    let content = prompt.text.trim();
    if content.is_empty() {
        return Vec::new();
    }
    vec![PromptChatMessage {
        role: PromptChatRole::User,
        content: content.to_string(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::builder::PromptBuilder;
    use crate::request::ReasoningRequest;
    use crate::types::ConversationTurn;
    use jaymi_context::{
        ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
        UserRequestMetadataSection,
    };

    fn sample_context(preview: &str) -> LlmContext {
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
            })
            .build();
        LlmContext::from_bundle(&bundle)
    }

    #[test]
    fn splits_system_context_and_user_request() {
        let request =
            ReasoningRequest::new("Summarize the repo.", sample_context("Summarize the repo."))
                .with_history(vec![ConversationTurn::system("Be concise.")]);
        let prompt = PromptBuilder::new()
            .with_system_instructions("Be concise.")
            .build_from_request(&request);
        let messages = prompt.to_chat_messages();
        assert!(messages.len() >= 2);
        assert_eq!(messages[0].role, PromptChatRole::System);
        assert!(messages[0].content.contains("Be concise."));
        let user = messages
            .iter()
            .find(|message| message.role == PromptChatRole::User)
            .expect("user message");
        assert!(user.content.contains("Summarize the repo."));
    }

    #[test]
    fn empty_sections_fall_back_to_full_text() {
        let prompt = Prompt {
            schema_version: super::super::types::PROMPT_SCHEMA_VERSION,
            sections: vec![],
            text: "## User Request\nhello\n".into(),
            diagnostics: crate::prompt::diagnostics::PromptDiagnostics {
                prompt_size_characters: 20,
                prompt_size_tokens: 5,
                assembled_prompt_size_characters: Some(20),
                assembled_prompt_size_tokens: Some(5),
                final_token_estimate: 5,
                conversation_turns: 0,
                budget: crate::prompt::budget::PromptBudgetUsage {
                    used_characters: 20,
                    estimated_tokens: 5,
                    max_characters: None,
                    max_tokens: None,
                    remaining_characters: None,
                    remaining_tokens: None,
                    reserved_completion_tokens: 0,
                    context_window_tokens: None,
                    context_efficiency_bps: None,
                    truncated: false,
                },
                sections: vec![],
                llm_coverage: vec![],
                truncated: false,
                truncation_notes: vec![],
                template_id: None,
                formatter_id: None,
                adapter_id: None,
                build_duration_ms: None,
            },
        };
        let messages = prompt.to_chat_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, PromptChatRole::User);
        assert!(messages[0].content.contains("hello"));
    }

    #[test]
    fn seal_matches_delivered_chat_messages() {
        let request = ReasoningRequest::new("goal text", sample_context("goal text")).with_history(
            vec![
                ConversationTurn::user("earlier"),
                ConversationTurn::assistant("reply"),
            ],
        );
        let mut prompt = PromptBuilder::new()
            .with_system_instructions("sys")
            .build_from_request(&request);
        // Builder already seals; re-seal is idempotent.
        let budget = PromptBudget::default();
        prompt.seal_for_delivery(&budget, 2);
        let delivered = prompt.delivered_character_count();
        assert_eq!(prompt.diagnostics.prompt_size_characters, delivered);
        assert!(
            prompt
                .diagnostics
                .assembled_prompt_size_characters
                .is_some(),
            "assembled size retained for Performance dashboard"
        );
        assert_eq!(
            prompt.diagnostics.prompt_size_tokens,
            budget.estimate_tokens(delivered)
        );
        assert_eq!(
            prompt.diagnostics.final_token_estimate,
            prompt.diagnostics.prompt_size_tokens
        );
        assert_eq!(prompt.diagnostics.conversation_turns, 2);
        assert_eq!(prompt.diagnostics.budget.used_characters, delivered);
        for section in &prompt.diagnostics.sections {
            if !section.included {
                assert_eq!(section.characters, 0);
                assert_eq!(section.estimated_tokens, 0);
            }
        }
        // Delivered size must not equal flat prompt.text when framing differs.
        let text_chars = prompt.text.chars().count();
        assert_ne!(
            delivered, text_chars,
            "delivery framing should differ from flat prompt.text"
        );
    }
}
