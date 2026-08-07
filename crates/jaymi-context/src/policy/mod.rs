//! Context Policies — decide which providers / candidates may contribute.
//!
//! The [`ContextPolicyEngine`] filters and prioritizes [`crate::ContextProvider`]s
//! before assembly, and (Sprint B2.7) scores individual [`crate::ContextCandidate`]
//! nodes by relevance, recency, importance, privacy, and budget. Sprint **B2.8**
//! adds deterministic [`ContextSelectionProfile`] allowlists so workspace
//! context is chosen intelligently without AI scoring. Policies never gather
//! context and never mutate providers.
//!
//! Independent of the LLM and of the action-oriented `jaymi-policies` Policy Engine.

mod builtin;
mod decision;
mod selection;
mod sensitivity;

pub use builtin::{default_context_policies, JaymiDefaultContextPolicy, DEFAULT_CONTEXT_POLICY_ID};
pub use decision::{
    apply_contribution_constraints, apply_policy_to_contribution, ContextPolicyCandidate,
    ContextPolicyDecision, ContextPolicyDecisionRecord, ContextPolicyInputs,
    ContributionConstraints, PolicyDecisionSummary, PolicyReport,
};
pub use selection::{
    assess_context_selection, ContextSelectionAssessment, ContextSelectionProfile,
};
pub use sensitivity::Sensitivity;

use std::sync::Arc;

use crate::budget::BudgetEstimate;
use crate::candidate::{
    score_candidate, CandidateItemDecision, CandidateScores, ContextCandidate,
};
use crate::provider::ContextProvider;
use crate::relevance::RelevanceScore;

/// Trait for a deterministic context policy rule set.
///
/// Future policies (local-only, cloud restrictions, trust levels, enterprise,
/// project-scoped privacy, user-defined rules) implement this trait. Do not
/// gather context here — only decide participation.
pub trait ContextPolicy: Send + Sync {
    /// Stable policy identity for diagnostics / explainability.
    fn id(&self) -> &'static str;

    /// Evaluate whether a provider may participate and under what constraints.
    fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision;

    /// Evaluate one [`ContextCandidate`] item (Sprint B2.7).
    ///
    /// Default scores relevance / recency / importance / privacy. Override for
    /// specialized rules. Never gathers context.
    fn evaluate_candidate_item(
        &self,
        candidate: &ContextCandidate,
        provider_relevance: u8,
        now_unix: i64,
        inputs: &ContextPolicyInputs<'_>,
    ) -> CandidateItemDecision {
        let scores = score_candidate(
            candidate,
            provider_relevance,
            now_unix,
            inputs.max_sensitivity,
        );
        if !scores.privacy_ok {
            return CandidateItemDecision::deny(
                format!(
                    "Candidate privacy '{}' exceeds request maximum '{}'",
                    candidate.sensitivity.as_str(),
                    inputs.max_sensitivity.as_str()
                ),
                scores,
            );
        }
        if !candidate.required && scores.relevance < 20 && scores.importance < 40 {
            return CandidateItemDecision::deny(
                "Candidate relevance/importance below inclusion floor",
                scores,
            );
        }
        CandidateItemDecision::allow(
            format!(
                "Selected · relevance={} recency={} importance={}",
                scores.relevance, scores.recency, scores.importance
            ),
            scores,
        )
    }
}

/// Engine that evaluates registered [`ContextPolicy`]s for each provider.
///
/// Merge rules (deterministic):
/// * Any deny → provider excluded (first deny reason wins)
/// * Priority = minimum of allowed priority overrides (more restrictive)
/// * `can_truncate` = AND across allows
/// * `requires_user_approval` = OR across allows
/// * `exclude_sensitive` = OR across allows
/// * `bypass_relevance` = OR across allows
/// * Constraints merge with OR for exclusions
pub struct ContextPolicyEngine {
    policies: Vec<Arc<dyn ContextPolicy>>,
}

impl Default for ContextPolicyEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ContextPolicyEngine {
    /// Empty engine (no policies — everything allowed at provider priority).
    pub fn empty() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Engine with the Jaymi default context policy set.
    pub fn with_defaults() -> Self {
        Self {
            policies: default_context_policies(),
        }
    }

    /// Replace the policy set.
    pub fn set_policies(&mut self, policies: Vec<Arc<dyn ContextPolicy>>) {
        self.policies = policies;
    }

    /// Append a policy (for future enterprise / user rules).
    pub fn register_policy(&mut self, policy: Arc<dyn ContextPolicy>) {
        self.policies.push(policy);
    }

    /// Active policy ids in registration order.
    pub fn active_policy_ids(&self) -> Vec<&'static str> {
        self.policies.iter().map(|policy| policy.id()).collect()
    }

    /// Fingerprint of active policy ids (cache correctness).
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for id in self.active_policy_ids() {
            id.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Evaluate all policies for one provider candidate.
    pub fn evaluate_candidate(
        &self,
        candidate: &ContextPolicyCandidate<'_>,
    ) -> ContextPolicyDecisionRecord {
        if self.policies.is_empty() {
            return ContextPolicyDecisionRecord {
                provider_id: candidate.provider_id.to_string(),
                sensitivity: candidate.sensitivity,
                relevance: candidate.relevance.value(),
                estimate_characters: candidate.estimate.units.characters,
                decision: ContextPolicyDecision::allow(
                    format!(
                        "No context policies registered; provider '{}' retained at priority {}",
                        candidate.provider_id,
                        candidate.provider_priority.value()
                    ),
                    candidate.provider_priority,
                ),
                applied_policies: Vec::new(),
            };
        }

        let mut allowed = true;
        let mut deny_reason: Option<String> = None;
        let mut priority = candidate.provider_priority;
        let mut can_truncate = true;
        let mut requires_user_approval = false;
        let mut exclude_sensitive = false;
        let mut bypass_relevance = false;
        let mut constraints = ContributionConstraints::default();
        let mut allow_reasons: Vec<String> = Vec::new();
        let mut applied: Vec<String> = Vec::new();

        for policy in &self.policies {
            let decision = policy.evaluate(candidate);
            applied.push(policy.id().to_string());
            if !decision.participate {
                allowed = false;
                if deny_reason.is_none() {
                    deny_reason = Some(format!("{}: {}", policy.id(), decision.reason));
                }
                continue;
            }
            allow_reasons.push(format!("{}: {}", policy.id(), decision.reason));
            if decision.priority.value() < priority.value() {
                priority = decision.priority;
            }
            can_truncate = can_truncate && decision.can_truncate;
            requires_user_approval = requires_user_approval || decision.requires_user_approval;
            exclude_sensitive = exclude_sensitive || decision.exclude_sensitive;
            bypass_relevance = bypass_relevance || decision.bypass_relevance;
            constraints.merge(&decision.constraints);
        }

        let decision = if allowed {
            ContextPolicyDecision {
                participate: true,
                reason: allow_reasons.join("; "),
                priority,
                can_truncate,
                requires_user_approval,
                exclude_sensitive,
                bypass_relevance,
                constraints,
            }
        } else {
            ContextPolicyDecision::deny(
                deny_reason.unwrap_or_else(|| "Excluded by context policy".into()),
            )
        };

        ContextPolicyDecisionRecord {
            provider_id: candidate.provider_id.to_string(),
            sensitivity: candidate.sensitivity,
            relevance: candidate.relevance.value(),
            estimate_characters: candidate.estimate.units.characters,
            decision,
            applied_policies: applied,
        }
    }

    /// Evaluate all policies for one content candidate (Sprint B2.7).
    ///
    /// Merge: any deny wins; scores take the minimum combined among allows
    /// (more restrictive ranking).
    pub fn evaluate_candidate_item(
        &self,
        candidate: &ContextCandidate,
        provider_relevance: u8,
        now_unix: i64,
        inputs: &ContextPolicyInputs<'_>,
    ) -> CandidateItemDecision {
        if self.policies.is_empty() {
            let scores = score_candidate(
                candidate,
                provider_relevance,
                now_unix,
                inputs.max_sensitivity,
            );
            if !scores.privacy_ok {
                return CandidateItemDecision::deny("privacy gate", scores);
            }
            return CandidateItemDecision::allow("no policies; retained", scores);
        }

        let mut allowed = true;
        let mut deny_reason: Option<String> = None;
        let mut best_scores: Option<CandidateScores> = None;
        let mut allow_reasons: Vec<String> = Vec::new();

        for policy in &self.policies {
            let decision = policy.evaluate_candidate_item(
                candidate,
                provider_relevance,
                now_unix,
                inputs,
            );
            if !decision.select {
                allowed = false;
                if deny_reason.is_none() {
                    deny_reason = Some(format!("{}: {}", policy.id(), decision.reason));
                }
                continue;
            }
            allow_reasons.push(format!("{}: {}", policy.id(), decision.reason));
            best_scores = Some(match best_scores {
                Some(prev) if prev.combined <= decision.scores.combined => prev,
                _ => decision.scores,
            });
        }

        if allowed {
            CandidateItemDecision::allow(
                allow_reasons.join("; "),
                best_scores.unwrap_or_else(|| {
                    score_candidate(
                        candidate,
                        provider_relevance,
                        now_unix,
                        inputs.max_sensitivity,
                    )
                }),
            )
        } else {
            CandidateItemDecision::deny(
                deny_reason.unwrap_or_else(|| "Excluded by context policy".into()),
                best_scores.unwrap_or_default(),
            )
        }
    }

    /// Evaluate every provider against the active policies.
    pub fn evaluate_providers(
        &self,
        providers: &[Arc<dyn ContextProvider>],
        inputs: &ContextPolicyInputs<'_>,
        relevance_of: &dyn Fn(&Arc<dyn ContextProvider>) -> RelevanceScore,
        estimate_of: &dyn Fn(&Arc<dyn ContextProvider>) -> BudgetEstimate,
    ) -> Vec<ContextPolicyDecisionRecord> {
        providers
            .iter()
            .map(|provider| {
                let candidate = ContextPolicyCandidate {
                    provider_id: provider.id(),
                    provider_priority: provider.priority(),
                    relevance: relevance_of(provider),
                    sensitivity: provider.sensitivity(),
                    estimate: estimate_of(provider),
                    inputs,
                };
                self.evaluate_candidate(&candidate)
            })
            .collect()
    }
}
