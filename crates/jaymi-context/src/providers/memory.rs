//! Memory context provider.

use std::sync::Arc;

use jaymi_core::JaymiResult;
use jaymi_memory_engine::{
    AssembleContextRequest, MemoryEngineApi, PromotionAskDecision, PromotionSuggestQuery,
};

use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::{ContextSource, MemoryResultsSection};

/// Contributes relevant memories and promotion suggestions for the request.
pub struct MemoryProvider {
    memory: Arc<dyn MemoryEngineApi>,
}

impl MemoryProvider {
    /// Create a provider backed by the Memory Engine.
    pub fn new(memory: Arc<dyn MemoryEngineApi>) -> Self {
        Self { memory }
    }
}

impl ContextProvider for MemoryProvider {
    fn id(&self) -> &'static str {
        "memory"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::MEMORY
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        RelevanceScore::from_parts([
            35, // memories are broadly useful when policy allows
            if signals.has_intent(IntentTag::Chat) { 30 } else { 0 },
            if matches!(signals.request_kind, RequestKind::Chat) { 20 } else { 0 },
            if signals.has_intent(IntentTag::Project) { 20 } else { 0 },
            if signals.coding_workspace() { 15 } else { 0 },
            if signals.has_capability("chat")
                || signals.has_capability("code")
                || signals.has_capability("search")
                || signals.has_capability("read_documents")
            {
                10
            } else {
                0
            },
            // Pure mechanical discover/index still gets a modest score via baseline.
            if matches!(signals.request_kind, RequestKind::Index | RequestKind::Discover) {
                5
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let _ = request;
        // Default assemble limit is 12 memories + a few promotions.
        BudgetEstimate::flexible(BudgetUnits::from_characters(8_000, 4))
    }

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        // Memory Engine retrieve (not a Tool). Always contributes a snapshot
        // section when called so promotion suggestions can surface even when no
        // memory bodies matched the request text.
        let memory = self.memory.assemble_context(&AssembleContextRequest {
            text: request.request.content.clone(),
            conversation_id: self.memory.active_conversation_id(),
            project_id: None,
            limit: Some(12),
            ..AssembleContextRequest::default()
        })?;

        let promotion_suggestions = self.memory.suggest_promotions(&PromotionSuggestQuery {
            conversation_id: self.memory.active_conversation_id(),
            project_id: self.memory.active_project_id(),
            min_importance: None,
            limit: Some(5),
        })?;
        let promotion_ask = PromotionAskDecision::from_suggestions(&promotion_suggestions);

        // Always contribute the Memory Engine snapshot when called — including
        // empty matching sets — so promotion suggestions still surface even when
        // no memory bodies matched the request text. Decline only if both the
        // retrieve and promotion APIs failed to produce a section (impossible
        // after successful calls above); keep the section for explainability.
        let mut sources = vec![ContextSource::RetrievedMemories];
        if !promotion_suggestions.is_empty() {
            sources.push(ContextSource::PromotionSuggestions);
        }

        Ok(Some(ContextContribution {
            sources,
            memory_results: Some(MemoryResultsSection {
                memory,
                promotion_suggestions,
                promotion_ask,
            }),
            ..ContextContribution::default()
        }))
    }
}
