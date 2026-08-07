//! Active project context provider.
//!
//! Sprint **B2.4:** prefers the ambient [`crate::ProjectSnapshot`] on session
//! inputs for project intelligence. Never filesystem-scans during assemble for
//! intelligence. Planner never scans projects — observation is Application
//! maintenance only.
//!
//! Heavy [`jaymi_project_engine::ProjectContext`] detail is attached only for
//! [`RequestKind::ProjectSession`] (open / continue) so Application /
//! PlannerResponse accessors keep a single ContextBundle contract. Ordinary
//! chat and coding requests get identity (+ intelligence from the snapshot)
//! without calling `assemble_context`.

use std::sync::Arc;

use jaymi_core::{IntentId, JaymiResult};
use jaymi_project_engine::ProjectEngineApi;

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::{ActiveProjectSection, ContextSource};

/// Contributes open-project identity (+ intelligence from ProjectSnapshot).
pub struct ProjectProvider {
    projects: Arc<dyn ProjectEngineApi>,
}

impl ProjectProvider {
    /// Create a provider backed by the Project Engine.
    pub fn new(projects: Arc<dyn ProjectEngineApi>) -> Self {
        Self { projects }
    }

    fn should_attach_project_detail(request: &ProviderRequest<'_>) -> bool {
        matches!(request.relevance.request_kind, RequestKind::ProjectSession)
            && !matches!(request.relevance.intent, IntentId::CloseProject)
    }

    fn project_detail(
        &self,
        request: &ProviderRequest<'_>,
    ) -> Option<jaymi_project_engine::ProjectContext> {
        if !Self::should_attach_project_detail(request) {
            return None;
        }
        match self.projects.project_context(None) {
            Ok(context) => context,
            Err(error) => {
                jaymi_logging::warn(
                    "context.provider.project",
                    format!("project context unavailable: {}", error.message()),
                );
                None
            }
        }
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
        let has_snapshot = request
            .session
            .project_snapshot
            .as_ref()
            .is_some_and(|snap| snap.has_project());
        RelevanceScore::from_parts([
            if has_snapshot || self.projects.open_project_id().is_some() {
                15
            } else {
                0
            },
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
        let chars = if let Some(snap) = request.session.project_snapshot.as_ref() {
            let mut n = 256usize;
            n += snap.languages.len().saturating_mul(12);
            n += snap.frameworks.len().saturating_mul(12);
            n += snap
                .dependency_summary
                .top_level
                .iter()
                .map(|dep| dep.chars().count() + 4)
                .sum::<usize>();
            n += snap
                .workspace_layout
                .top_level_dirs
                .iter()
                .map(|dir| dir.chars().count() + 4)
                .sum::<usize>();
            if Self::should_attach_project_detail(request) {
                n = n.saturating_add(2_048);
            }
            n.max(128)
        } else if Self::should_attach_project_detail(request) {
            2_048
        } else if self.projects.open_project_id().is_some() {
            256
        } else {
            64
        };
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let detail = self.project_detail(request);
        let sensitivity = self.sensitivity();
        let priority = self.priority();
        let mut out = Vec::new();

        if let Some(snapshot) = request.session.project_snapshot.as_ref() {
            if snapshot.has_project() || snapshot.has_intelligence() {
                let project_id = snapshot
                    .metadata
                    .project_id
                    .clone()
                    .unwrap_or_else(|| "active".into());
                out.push(ContextCandidate::new(
                    self.id(),
                    ContextCandidateKind::ProjectIdentity,
                    ContextSource::ActiveProject,
                    project_id,
                    CandidatePayload::ActiveProject(ActiveProjectSection {
                        project_id: snapshot.metadata.project_id.clone(),
                        name: snapshot.metadata.name.clone(),
                        root_directory: snapshot.metadata.root_directory.clone(),
                        detail,
                    }),
                    sensitivity,
                    90,
                    priority,
                    true,
                ));
                if snapshot.has_intelligence() {
                    out.push(ContextCandidate::new(
                        self.id(),
                        ContextCandidateKind::ProjectIntelligence,
                        ContextSource::ProjectIntelligence,
                        "intel",
                        CandidatePayload::ProjectIntelligence(snapshot.intelligence_section()),
                        sensitivity,
                        70,
                        priority,
                        false,
                    ));
                }
                return Ok(out);
            }
        }

        let Some(project_id) = self.projects.open_project_id() else {
            return Ok(Vec::new());
        };
        let project = match self.projects.get(&project_id) {
            Ok(project) => project,
            Err(error) => {
                jaymi_logging::warn(
                    "context.provider.project",
                    format!("project identity unavailable: {}", error.message()),
                );
                return Ok(Vec::new());
            }
        };
        let Some(project) = project else {
            return Ok(Vec::new());
        };

        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::ProjectIdentity,
            ContextSource::ActiveProject,
            project.id.as_str(),
            CandidatePayload::ActiveProject(ActiveProjectSection {
                project_id: Some(project.id.as_str().to_string()),
                name: Some(project.name.clone()),
                root_directory: project
                    .root_directory
                    .as_ref()
                    .map(|path| path.display().to_string()),
                detail,
            }),
            sensitivity,
            90,
            priority,
            true,
        )])
    }
}
