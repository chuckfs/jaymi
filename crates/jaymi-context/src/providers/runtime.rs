//! Runtime intelligence context provider — reads completed RuntimeSnapshot only.
//!
//! Sprint **B2.6:** prefers ambient [`crate::RuntimeSnapshot`] on session inputs.
//! Never re-runs cargo / tests / terminal commands during assemble.

use jaymi_core::JaymiResult;

use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes runtime intelligence from the completed ambient snapshot.
pub struct RuntimeProvider;

impl ContextProvider for RuntimeProvider {
    fn id(&self) -> &'static str {
        "runtime"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::RUNTIME
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_runtime = request
            .session
            .runtime_snapshot
            .as_ref()
            .is_some_and(|snap| snap.has_intelligence());
        RelevanceScore::from_parts([
            if has_runtime { 35 } else { 0 },
            if matches!(signals.request_kind, RequestKind::Terminal) {
                50
            } else {
                0
            },
            if signals.has_intent(IntentTag::Terminal) || signals.has_intent(IntentTag::Code) {
                25
            } else {
                0
            },
            if signals.coding_workspace() { 20 } else { 0 },
            if signals.has_capability("code") || signals.has_capability("execute_terminal_commands")
            {
                15
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let chars = if let Some(snap) = request.session.runtime_snapshot.as_ref() {
            let section = snap.intelligence_section();
            let mut n = 128usize;
            n += section
                .latest_cargo_check
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            n += section
                .latest_build
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            n += section
                .latest_tests
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            n += section.output_tail.chars().count().min(640);
            n += section
                .running
                .iter()
                .map(|line| line.chars().count() + 2)
                .sum::<usize>();
            n += section
                .recent_failures
                .iter()
                .map(|line| line.chars().count() + 2)
                .sum::<usize>();
            n.max(64)
        } else {
            32
        };
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let Some(snapshot) = request.session.runtime_snapshot.as_ref() else {
            return Ok(Vec::new());
        };
        if !snapshot.has_intelligence() {
            return Ok(Vec::new());
        }
        let importance = self.relevance(request).value().saturating_add(10).min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::RuntimeIntelligence,
            ContextSource::RuntimeIntelligence,
            "runtime",
            CandidatePayload::RuntimeIntelligence(snapshot.intelligence_section()),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
