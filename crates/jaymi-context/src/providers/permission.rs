//! Permission context provider.

use jaymi_core::JaymiResult;

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes attached permission grants when the session carries any.
pub struct PermissionProvider;

impl ContextProvider for PermissionProvider {
    fn id(&self) -> &'static str {
        "permission"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::PERMISSION
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_grants = !request.session.permissions.entries.is_empty();
        RelevanceScore::from_parts([
            if has_grants { 45 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileWrite
                    | RequestKind::Terminal
                    | RequestKind::Git
                    | RequestKind::Index
            ) {
                50
            } else {
                0
            },
            if signals.has_intent(IntentTag::Write)
                || signals.has_intent(IntentTag::Terminal)
                || signals.has_intent(IntentTag::Git)
            {
                25
            } else {
                0
            },
            if signals.coding_workspace() { 15 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileRead | RequestKind::Search | RequestKind::Discover
            ) {
                20
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let chars = request
            .session
            .permissions
            .entries
            .iter()
            .map(|entry| {
                entry.category.chars().count()
                    + entry.action.chars().count()
                    + entry.decision.chars().count()
                    + entry
                        .explanation
                        .as_ref()
                        .map(|value| value.chars().count())
                        .unwrap_or(0)
                    + 16
            })
            .sum::<usize>()
            .max(48);
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        if request.session.permissions.entries.is_empty() {
            return Ok(Vec::new());
        }
        let importance = self.relevance(request).value().saturating_add(10).min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::Permissions,
            ContextSource::Permissions,
            "grants",
            CandidatePayload::Permissions(request.session.permissions.clone()),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
