//! Active workspace context provider.

use jaymi_core::JaymiResult;

use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
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

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        let kind = request.session.workspace_kind.clone();
        let capability_ids = request.relevance.active_capabilities.clone();
        if kind.is_none() && capability_ids.is_empty() {
            return Ok(None);
        }

        let mut sources = Vec::new();
        let active_workspace = kind.map(|kind_id| {
            sources.push(ContextSource::ActiveWorkspace);
            ActiveWorkspaceSection {
                kind_id: Some(kind_id),
            }
        });
        let active_capabilities = if capability_ids.is_empty() {
            None
        } else {
            sources.push(ContextSource::ActiveCapabilities);
            Some(ActiveCapabilitiesSection { capability_ids })
        };

        Ok(Some(ContextContribution {
            sources,
            active_workspace,
            active_capabilities,
            ..ContextContribution::default()
        }))
    }
}
