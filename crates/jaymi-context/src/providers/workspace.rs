//! Active workspace context provider.

use jaymi_core::JaymiResult;

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::RelevanceScore;
use crate::{ActiveCapabilitiesSection, ActiveWorkspaceSection, ContextSource};

/// Contributes the active UX workspace kind and **request-selected** capabilities.
///
/// Capability ids come from Planner [`crate::AssembleHints`] via relevance signals —
/// never from a host capability catalog.
pub struct WorkspaceProvider;

impl ContextProvider for WorkspaceProvider {
    fn id(&self) -> &'static str {
        "workspace"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::CRITICAL
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let session = request.session;
        let signals = request.relevance;
        if session.workspace_kind.is_none() && signals.active_capabilities.is_empty() {
            return RelevanceScore::NONE;
        }
        RelevanceScore::from_parts([
            if session.workspace_kind.is_some() { 70 } else { 0 },
            if !signals.active_capabilities.is_empty() { 40 } else { 0 },
            if signals.coding_workspace() { 15 } else { 0 },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let mut chars = 32usize;
        if let Some(kind) = &request.session.workspace_kind {
            chars += kind.chars().count() + 16;
        }
        chars += request
            .relevance
            .active_capabilities
            .iter()
            .map(|id| id.chars().count() + 1)
            .sum::<usize>();
        BudgetEstimate::metadata(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let kind = request.session.workspace_kind.clone();
        let capability_ids = request.relevance.active_capabilities.clone();
        if kind.is_none() && capability_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        if let Some(kind_id) = kind {
            out.push(ContextCandidate::new(
                self.id(),
                ContextCandidateKind::WorkspaceKind,
                ContextSource::ActiveWorkspace,
                kind_id.clone(),
                CandidatePayload::ActiveWorkspace(ActiveWorkspaceSection {
                    kind_id: Some(kind_id),
                }),
                self.sensitivity(),
                92,
                self.priority(),
                true,
            ));
        }
        if !capability_ids.is_empty() {
            out.push(ContextCandidate::new(
                self.id(),
                ContextCandidateKind::Capabilities,
                ContextSource::ActiveCapabilities,
                "caps",
                CandidatePayload::ActiveCapabilities(ActiveCapabilitiesSection { capability_ids }),
                self.sensitivity(),
                88,
                self.priority(),
                true,
            ));
        }
        Ok(out)
    }
}
