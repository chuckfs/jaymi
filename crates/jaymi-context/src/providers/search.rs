//! Search context provider — coordination hints only (never executes search).

use std::sync::Arc;

use jaymi_core::JaymiResult;
use jaymi_project_engine::ProjectEngineApi;
use jaymi_search::SearchEngineApi;

use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::{ContextSource, SearchContextHint, SearchResultsSection};

/// Contributes search coordination hints and any pre-attached session hits.
pub struct SearchProvider {
    search: Arc<dyn SearchEngineApi>,
    projects: Arc<dyn ProjectEngineApi>,
}

impl SearchProvider {
    /// Create a provider wired to Search + Project (for index summary only).
    pub fn new(search: Arc<dyn SearchEngineApi>, projects: Arc<dyn ProjectEngineApi>) -> Self {
        Self { search, projects }
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
            // Light boost when a coding workspace may expose index health.
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
        // Project index hint may add a small constant; detail comes from contribute.
        if self.projects.open_project_id().is_some() {
            chars += 48;
        }
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        let _ = &self.search; // keep Search Engine wired; do not call search()

        let structured = request.request.search.as_ref();
        let project_indexed = match self.projects.project_context(None) {
            Ok(Some(ctx)) => Some(ctx.search_index.indexed_file_count),
            Ok(None) => None,
            Err(error) => {
                jaymi_logging::warn(
                    "context.provider.search",
                    format!("project index unavailable: {}", error.message()),
                );
                None
            }
        };

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
            return Ok(None);
        }

        Ok(Some(ContextContribution {
            sources: vec![ContextSource::SearchResults],
            search_results: Some(SearchResultsSection { hint, hits }),
            ..ContextContribution::default()
        }))
    }
}
