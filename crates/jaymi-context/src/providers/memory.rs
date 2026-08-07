//! Memory context provider.

use std::sync::Arc;

use jaymi_core::JaymiResult;
use jaymi_memory_engine::{
    AssembleContextRequest, MemoryEngineApi, PromotionAskDecision, PromotionSuggestQuery,
};

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
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
            if matches!(signals.request_kind, RequestKind::Index | RequestKind::Discover) {
                5
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let _ = request;
        BudgetEstimate::flexible(BudgetUnits::from_characters(8_000, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
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

        let importance = self.relevance(request).value().saturating_add(10).min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::MemoryResults,
            ContextSource::RetrievedMemories,
            "memory",
            CandidatePayload::MemoryResults(MemoryResultsSection {
                memory,
                promotion_suggestions,
                promotion_ask,
            }),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
