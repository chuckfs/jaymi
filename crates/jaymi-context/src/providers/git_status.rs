//! Git status context provider — reads completed maintenance snapshots only.
//!
//! Sprint **B2.5:** prefers ambient [`crate::GitSnapshot`] on session inputs and
//! exposes a capped [`crate::GitStatusSection`] summary. Never shells out to git
//! — Application background maintenance owns refresh. Reasoning never runs git.

use jaymi_core::JaymiResult;

use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes attached Git status when the session carries a completed snapshot.
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
        let has_git = request
            .session
            .git_snapshot
            .as_ref()
            .is_some_and(|snap| snap.has_intelligence())
            || request.session.git_status.is_repository
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
        let section = request
            .session
            .git_snapshot
            .as_ref()
            .map(|snap| snap.status_section())
            .unwrap_or_else(|| request.session.git_status.clone());
        let chars = section.summary.chars().count()
            + section
                .sample_paths
                .iter()
                .map(|path| path.chars().count() + 1)
                .sum::<usize>()
            + section
                .recent_commits
                .iter()
                .map(|commit| commit.subject.chars().count() + commit.short_sha.len() + 8)
                .sum::<usize>()
            + 96;
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars.max(32), 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let section = if let Some(snapshot) = request.session.git_snapshot.as_ref() {
            if snapshot.has_intelligence() {
                snapshot.status_section()
            } else {
                return Ok(Vec::new());
            }
        } else {
            let section = &request.session.git_status;
            if !section.is_repository && section.summary.is_empty() {
                return Ok(Vec::new());
            }
            section.clone()
        };
        let importance = self.relevance(request).value().saturating_add(10).min(100);
        Ok(vec![ContextCandidate::new(
            self.id(),
            ContextCandidateKind::GitStatus,
            ContextSource::GitStatus,
            "status",
            CandidatePayload::GitStatus(section),
            self.sensitivity(),
            importance,
            self.priority(),
            false,
        )])
    }
}
