//! Workspace Memory context provider — session WorkspaceMemorySnapshot only.
//!
//! Sprint **B2.9:** proposes Coding workspace activity candidates. Distinct from
//! Conversation Memory (`MemoryProvider`). Context Policy decides inclusion.

use jaymi_core::JaymiResult;

use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes workspace activity memory from the session snapshot.
pub struct WorkspaceMemoryProvider;

impl ContextProvider for WorkspaceMemoryProvider {
    fn id(&self) -> &'static str {
        "workspace_memory"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::WORKSPACE_MEMORY
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_memory = request
            .session
            .workspace_memory_snapshot
            .as_ref()
            .is_some_and(|snap| snap.has_memory());
        RelevanceScore::from_parts([
            if has_memory { 40 } else { 0 },
            if signals.coding_workspace() { 25 } else { 0 },
            if signals.has_intent(IntentTag::Code) { 20 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::Terminal | RequestKind::FileWrite | RequestKind::FileRead | RequestKind::Lsp
            ) {
                15
            } else {
                0
            },
            if signals.has_capability("code") || signals.has_capability("execute_terminal_commands")
            {
                10
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let chars = if let Some(snap) = request.session.workspace_memory_snapshot.as_ref() {
            let section = snap.memory_section();
            let mut n = 96usize;
            n += section
                .coding_objective
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            n += section
                .recent_edits
                .iter()
                .map(|p| p.chars().count() + 2)
                .sum::<usize>();
            n += section
                .recently_opened
                .iter()
                .map(|p| p.chars().count() + 2)
                .sum::<usize>();
            n += section
                .recent_builds
                .iter()
                .map(|p| p.chars().count() + 2)
                .sum::<usize>();
            n += section
                .recent_failures
                .iter()
                .map(|p| p.chars().count() + 2)
                .sum::<usize>();
            n.max(48)
        } else {
            32
        };
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let Some(snapshot) = request.session.workspace_memory_snapshot.as_ref() else {
            return Ok(Vec::new());
        };
        if !snapshot.has_memory() {
            return Ok(Vec::new());
        }
        let importance = self
            .relevance(request)
            .value()
            .saturating_add(15)
            .min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::WorkspaceMemory,
            ContextSource::WorkspaceMemory,
            "activity",
            CandidatePayload::WorkspaceMemory(snapshot.memory_section()),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
