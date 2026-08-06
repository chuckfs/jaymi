//! Git status context provider — reads completed maintenance snapshots only.

use jaymi_core::JaymiResult;

use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes attached Git status when the session carries a completed snapshot.
///
/// Never shells out to git — Application background maintenance owns refresh.
pub struct GitStatusProvider;

impl ContextProvider for GitStatusProvider {
    fn id(&self) -> &'static str {
        "git_status"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::GIT_STATUS
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_git = request.session.git_status.is_repository
            || !request.session.git_status.summary.is_empty();
        RelevanceScore::from_parts([
            if has_git { 40 } else { 0 },
            if matches!(signals.request_kind, RequestKind::Git) {
                50
            } else {
                0
            },
            if signals.has_intent(IntentTag::Git) || signals.has_intent(IntentTag::Code) {
                25
            } else {
                0
            },
            if signals.coding_workspace() { 25 } else { 0 },
            if signals.has_capability("code") { 15 } else { 0 },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let section = &request.session.git_status;
        let chars = section.summary.chars().count()
            + section
                .sample_paths
                .iter()
                .map(|path| path.chars().count() + 1)
                .sum::<usize>()
            + 48;
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars.max(32), 4))
    }

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        let section = &request.session.git_status;
        if !section.is_repository && section.summary.is_empty() {
            return Ok(None);
        }
        Ok(Some(ContextContribution {
            sources: vec![ContextSource::GitStatus],
            git_status: Some(section.clone()),
            ..ContextContribution::default()
        }))
    }
}
