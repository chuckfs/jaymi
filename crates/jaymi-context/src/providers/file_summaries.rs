//! File summaries context provider — reads completed maintenance snapshots.

use jaymi_core::JaymiResult;

use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes file summaries when the session carries completed maintenance data.
///
/// Never reads files during assemble — Application background maintenance owns refresh.
pub struct FileSummariesProvider;

impl ContextProvider for FileSummariesProvider {
    fn id(&self) -> &'static str {
        "file_summaries"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::FILE_SUMMARIES
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_summaries = !request.session.file_summaries.entries.is_empty();
        RelevanceScore::from_parts([
            if has_summaries { 40 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileRead | RequestKind::FileWrite | RequestKind::Chat
            ) {
                25
            } else {
                0
            },
            if signals.has_intent(IntentTag::Code) || signals.has_intent(IntentTag::Read) {
                25
            } else {
                0
            },
            if signals.coding_workspace() { 20 } else { 0 },
            if signals.has_capability("code") { 15 } else { 0 },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let chars = request
            .session
            .file_summaries
            .entries
            .iter()
            .map(|entry| {
                entry.path.chars().count()
                    + entry.summary.chars().count()
                    + entry
                        .language
                        .as_ref()
                        .map(|language| language.chars().count())
                        .unwrap_or(0)
                    + 24
            })
            .sum::<usize>()
            .max(32);
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        if request.session.file_summaries.entries.is_empty() {
            return Ok(None);
        }
        Ok(Some(ContextContribution {
            sources: vec![ContextSource::FileSummaries],
            file_summaries: Some(request.session.file_summaries.clone()),
            ..ContextContribution::default()
        }))
    }
}
