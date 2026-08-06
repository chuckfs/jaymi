//! Diagnostics context provider.

use jaymi_core::JaymiResult;

use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes attached diagnostics when the session carries any.
pub struct DiagnosticsProvider;

impl ContextProvider for DiagnosticsProvider {
    fn id(&self) -> &'static str {
        "diagnostics"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::DIAGNOSTICS
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_diagnostics = !request.session.diagnostics.diagnostics.is_empty();
        RelevanceScore::from_parts([
            if has_diagnostics { 45 } else { 0 },
            if matches!(signals.request_kind, RequestKind::Lsp) { 50 } else { 0 },
            if signals.has_intent(IntentTag::Lsp) || signals.has_intent(IntentTag::Code) {
                25
            } else {
                0
            },
            if signals.coding_workspace() { 30 } else { 0 },
            if signals.has_capability("code") { 20 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileRead | RequestKind::FileWrite
            ) {
                15
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let chars = request
            .session
            .diagnostics
            .diagnostics
            .iter()
            .map(|diag| {
                diag.message.chars().count()
                    + diag.severity.chars().count()
                    + diag.path.as_ref().map(|p| p.chars().count()).unwrap_or(0)
                    + 16
            })
            .sum::<usize>()
            .max(32);
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        if request.session.diagnostics.diagnostics.is_empty() {
            return Ok(None);
        }
        Ok(Some(ContextContribution {
            sources: vec![ContextSource::Diagnostics],
            diagnostics: Some(request.session.diagnostics.clone()),
            ..ContextContribution::default()
        }))
    }
}
