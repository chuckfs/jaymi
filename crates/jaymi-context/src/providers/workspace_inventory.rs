//! Workspace inventory context provider — reads completed maintenance snapshots.

use jaymi_core::JaymiResult;

use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes workspace inventory when the session carries a completed snapshot.
pub struct WorkspaceInventoryProvider;

impl ContextProvider for WorkspaceInventoryProvider {
    fn id(&self) -> &'static str {
        "workspace_inventory"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::WORKSPACE_INVENTORY
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_inventory = request.session.workspace_inventory.root.is_some()
            || request.session.workspace_inventory.file_count > 0
            || !request.session.workspace_inventory.status.is_empty();
        RelevanceScore::from_parts([
            if has_inventory { 35 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::Search | RequestKind::Discover | RequestKind::Index
            ) {
                40
            } else {
                0
            },
            if signals.has_intent(IntentTag::Search) || signals.has_intent(IntentTag::Code) {
                20
            } else {
                0
            },
            if signals.coding_workspace() { 25 } else { 0 },
            if signals.has_capability("code") || signals.has_capability("search") {
                15
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let section = &request.session.workspace_inventory;
        let chars = section
            .root
            .as_ref()
            .map(|root| root.chars().count())
            .unwrap_or(0)
            + section.status.chars().count()
            + section
                .sample_paths
                .iter()
                .map(|path| path.chars().count() + 1)
                .sum::<usize>()
            + 48;
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars.max(32), 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let section = &request.session.workspace_inventory;
        if section.root.is_none()
            && section.file_count == 0
            && section.directory_count == 0
            && section.status.is_empty()
        {
            return Ok(Vec::new());
        }
        let importance = self.relevance(request).value().saturating_add(10).min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::WorkspaceInventory,
            ContextSource::WorkspaceInventory,
            "inventory",
            CandidatePayload::WorkspaceInventory(section.clone()),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
