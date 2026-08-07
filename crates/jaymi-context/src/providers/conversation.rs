//! Conversation context provider.

use std::sync::Arc;

use jaymi_core::JaymiResult;
use jaymi_memory_engine::MemoryEngineApi;

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::{ContextSource, ConversationSection};

/// Contributes the active conversation summary when one is selected.
pub struct ConversationProvider {
    memory: Arc<dyn MemoryEngineApi>,
}

impl ConversationProvider {
    /// Create a provider backed by the Memory Engine conversation APIs.
    pub fn new(memory: Arc<dyn MemoryEngineApi>) -> Self {
        Self { memory }
    }

    fn conversation_section(&self) -> Option<ConversationSection> {
        let id = self.memory.active_conversation_id()?;
        Some(match self.memory.load_conversation(&id) {
            Ok(Some(conversation)) => ConversationSection {
                id: Some(conversation.meta.id.as_str().to_string()),
                title: conversation.meta.title.clone(),
                status: Some(conversation.meta.status.as_str().to_string()),
                project_id: conversation.meta.project_id.clone(),
                message_count: Some(conversation.messages.len()),
            },
            Ok(None) => ConversationSection {
                id: Some(id),
                ..ConversationSection::default()
            },
            Err(error) => {
                jaymi_logging::warn(
                    "context.provider.conversation",
                    format!("conversation unavailable: {}", error.message()),
                );
                ConversationSection {
                    id: Some(id),
                    ..ConversationSection::default()
                }
            }
        })
    }
}

impl ContextProvider for ConversationProvider {
    fn id(&self) -> &'static str {
        "conversation"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::CONVERSATION
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        RelevanceScore::from_parts([
            25, // baseline conversational continuity
            if signals.has_intent(IntentTag::Chat) { 40 } else { 0 },
            if matches!(signals.request_kind, RequestKind::Chat) { 20 } else { 0 },
            if signals.has_capability("chat") { 15 } else { 0 },
            // Structured tool ops still may want transcript context, but lower.
            if matches!(
                signals.request_kind,
                RequestKind::Terminal | RequestKind::Git | RequestKind::Lsp | RequestKind::Index
            ) {
                10
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let sessionish = 96usize;
        let chars = if self.memory.active_conversation_id().is_some() {
            192
        } else {
            sessionish
        };
        let _ = request;
        BudgetEstimate::metadata(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        _request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let Some(section) = self.conversation_section() else {
            return Ok(Vec::new());
        };
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::Conversation,
            ContextSource::PreviousConversation,
            "main",
            CandidatePayload::Conversation(section),
            self.sensitivity(),
            95,
            self.priority(),
            true,
        )])
    }
}
