//! Search context provider — coordination hints only (never executes search).

use std::sync::Arc;

use jaymi_core::JaymiResult;
use jaymi_search::SearchEngineApi;

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::{ContextSource, SearchContextHint, SearchResultsSection};

/// Contributes search coordination hints and any pre-attached session hits.
pub struct SearchProvider {
    search: Arc<dyn SearchEngineApi>,
}

impl SearchProvider {
    /// Create a provider wired to Search Engine (kept for future scoped use).
    pub fn new(search: Arc<dyn SearchEngineApi>) -> Self {
        Self { search }
    }
}

impl ContextProvider for SearchProvider {
    fn id(&self) -> &'static str {
        "search"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::SEARCH
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_hits = !request.session.search_hits.is_empty();
        RelevanceScore::from_parts([
            if matches!(
                signals.request_kind,
                RequestKind::Search | RequestKind::Discover
            ) {
                60
            } else {
                0
            },
            if signals.has_intent(IntentTag::Search) { 35 } else { 0 },
            if signals.has_capability("search") { 25 } else { 0 },
            if has_hits { 40 } else { 0 },
            if request.request.project_knowledge.is_some() { 30 } else { 0 },
            if signals.coding_workspace() { 15 } else { 0 },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let mut chars = 64usize;
        if request.request.search.is_some() || request.request.project_knowledge.is_some() {
            chars += 128;
        }
        for hit in &request.session.search_hits {
            chars += hit.title.chars().count()
                + hit.item_id.chars().count()
                + hit.preview.as_ref().map(|p| p.chars().count()).unwrap_or(0)
                + 32;
        }
        if request.session.project_indexed_documents.is_some() {
            chars += 48;
        }
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let _ = &self.search;

        let structured = request.request.search.as_ref();
        let project_indexed = request.session.project_indexed_documents;
        let hits = request.session.search_hits.clone();
        let hint = if structured.is_some() || project_indexed.is_some() {
            Some(SearchContextHint {
                structured_query_pending: structured.is_some(),
                query_preview: structured
                    .and_then(|search| search.free_text.clone().or_else(|| search.filename.clone())),
                project_indexed_documents: project_indexed,
            })
        } else {
            None
        };

        if hint.is_none() && hits.is_empty() {
            return Ok(Vec::new());
        }

        let importance = self.relevance(request).value().saturating_add(10).min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::SearchResults,
            ContextSource::SearchResults,
            "search",
            CandidatePayload::SearchResults(SearchResultsSection { hint, hits }),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
