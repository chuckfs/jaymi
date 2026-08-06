//! Active project context provider.

use std::sync::Arc;

use jaymi_core::JaymiResult;
use jaymi_project_engine::ProjectEngineApi;

use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::{ActiveProjectSection, ContextSource};

/// Contributes open-project identity when a project is active.
///
/// Loads Project Engine workspace context for this provider's section only
/// (not a Tool). Search must not call Project Engine — index summaries are
/// host-supplied on the session.
pub struct ProjectProvider {
    projects: Arc<dyn ProjectEngineApi>,
}

impl ProjectProvider {
    /// Create a provider backed by the Project Engine.
    pub fn new(projects: Arc<dyn ProjectEngineApi>) -> Self {
        Self { projects }
    }
}

impl ContextProvider for ProjectProvider {
    fn id(&self) -> &'static str {
        "project"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::PROJECT
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        RelevanceScore::from_parts([
            15,
            if signals.has_intent(IntentTag::Project) { 55 } else { 0 },
            if matches!(signals.request_kind, RequestKind::ProjectSession) { 30 } else { 0 },
            if signals.coding_workspace() { 35 } else { 0 },
            if signals.has_capability("code") { 20 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileRead
                    | RequestKind::FileWrite
                    | RequestKind::Git
                    | RequestKind::Lsp
                    | RequestKind::Terminal
                    | RequestKind::Search
            ) {
                25
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let _ = request;
        // Open project detail can be large; over-estimate so higher-priority
        // providers keep room and fit_contribution can drop detail.
        let chars = if self.projects.open_project_id().is_some() {
            12_000
        } else {
            64
        };
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn contribute(
        &self,
        _request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        let project = match self.projects.project_context(None) {
            Ok(context) => context,
            Err(error) => {
                jaymi_logging::warn(
                    "context.provider.project",
                    format!("project context unavailable: {}", error.message()),
                );
                return Ok(None);
            }
        };

        let Some(ctx) = project else {
            return Ok(None);
        };

        Ok(Some(ContextContribution {
            sources: vec![ContextSource::ActiveProject],
            active_project: Some(ActiveProjectSection {
                project_id: Some(ctx.project.id.as_str().to_string()),
                name: Some(ctx.project.name.clone()),
                root_directory: ctx
                    .project
                    .root_directory
                    .as_ref()
                    .map(|path| path.display().to_string()),
                detail: Some(ctx),
            }),
            ..ContextContribution::default()
        }))
    }
}
