//! Outcomes produced when the Policy Engine evaluates an execution candidate.

use crate::Policy;

/// Tool/provider attributes the Policy Engine may consider.
///
/// Kept free of tool-crate types so policies remain independently testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionCandidate {
    /// Tool that would run.
    pub tool_id: String,
    /// Provider backing the tool.
    pub provider_id: String,
    /// Whether the tool requires network access.
    pub requires_internet: bool,
    /// Whether execution is exclusively local.
    pub local_only: bool,
    /// Whether execution is exclusively cloud-hosted.
    pub cloud_only: bool,
}

/// Policy outcome aligned with Permission decisions and Planner gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Candidate may proceed without conversational review (subject to permissions).
    Allowed,
    /// Candidate needs an in-conversation Review Card before execution.
    RequiresApproval,
    /// Candidate must not execute; Planner explains why.
    Denied,
}

impl PolicyDecision {
    /// Stable label for diagnostics and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::RequiresApproval => "requires_approval",
            Self::Denied => "denied",
        }
    }

    /// True when the candidate is not hard-denied.
    pub fn may_proceed(self) -> bool {
        !matches!(self, Self::Denied)
    }

    /// Combine two decisions with Denied > RequiresApproval > Allowed.
    pub fn escalate(self, other: Self) -> Self {
        use PolicyDecision::*;
        match (self, other) {
            (Denied, _) | (_, Denied) => Denied,
            (RequiresApproval, _) | (_, RequiresApproval) => RequiresApproval,
            (Allowed, Allowed) => Allowed,
        }
    }
}

/// Structured result of policy evaluation for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEvaluation {
    /// Final policy decision for this candidate.
    pub decision: PolicyDecision,
    /// Whether policies allow the candidate past a hard deny
    /// (`decision != Denied`). Prefer [`Self::decision`] for gating.
    pub allowed: bool,
    /// Names of policies that influenced the decision.
    pub policies_applied: Vec<String>,
    /// Human-readable reasons for allow / approval / deny.
    pub reasons: Vec<String>,
    /// Whether active policies prefer local execution.
    pub prefer_local: bool,
}

impl PolicyEvaluation {
    /// Short summary suitable for diagnostics and user-facing explanations.
    pub fn summary(&self) -> String {
        match self.decision {
            PolicyDecision::Allowed => {
                format!("allowed ({})", self.policies_applied.join(", "))
            }
            PolicyDecision::RequiresApproval => {
                let reason = self
                    .reasons
                    .iter()
                    .find(|reason| reason.contains("approval") || reason.contains("requires"))
                    .cloned()
                    .or_else(|| self.reasons.first().cloned())
                    .unwrap_or_else(|| "policy requires approval".into());
                format!("requires_approval ({reason})")
            }
            PolicyDecision::Denied => {
                format!(
                    "denied ({})",
                    self.reasons
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "policy".into())
                )
            }
        }
    }

    /// Plain-language explanation for Denied / RequiresApproval responses.
    pub fn explanation(&self) -> String {
        match self.decision {
            PolicyDecision::Denied => self
                .reasons
                .iter()
                .rev()
                .find(|reason| {
                    reason.contains("rejects")
                        || reason.contains("denied")
                        || reason.contains("not permitted")
                })
                .cloned()
                .or_else(|| self.reasons.last().cloned())
                .unwrap_or_else(|| self.summary()),
            PolicyDecision::RequiresApproval => self
                .reasons
                .iter()
                .find(|reason| reason.contains("approval") || reason.contains("requires"))
                .cloned()
                .or_else(|| self.reasons.first().cloned())
                .unwrap_or_else(|| self.summary()),
            PolicyDecision::Allowed => self
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| self.summary()),
        }
    }
}

impl Default for PolicyEvaluation {
    fn default() -> Self {
        Self {
            decision: PolicyDecision::Allowed,
            allowed: true,
            policies_applied: Vec::new(),
            reasons: Vec::new(),
            prefer_local: false,
        }
    }
}

fn set_decision(evaluation: &mut PolicyEvaluation, decision: PolicyDecision) {
    evaluation.decision = evaluation.decision.escalate(decision);
    evaluation.allowed = evaluation.decision.may_proceed();
}

/// Evaluate `candidate` against the provided policy set.
pub fn evaluate_policies(policies: &[Policy], candidate: &ExecutionCandidate) -> PolicyEvaluation {
    use crate::builtin::BuiltinPolicy;

    let mut evaluation = PolicyEvaluation::default();

    if policies.is_empty() {
        evaluation.reasons.push("no active policies".to_string());
        return evaluation;
    }

    for policy in policies {
        evaluation.policies_applied.push(policy.name.clone());

        match policy.builtin {
            Some(BuiltinPolicy::OfflineFirst) => {
                evaluation.prefer_local = true;
                if candidate.requires_internet || candidate.cloud_only {
                    // Offline First does not hard-deny cloud — it requires
                    // explicit conversational approval (Review Card).
                    set_decision(&mut evaluation, PolicyDecision::RequiresApproval);
                    evaluation.reasons.push(format!(
                        "Offline First requires approval before using tool '{}' (internet/cloud execution)",
                        candidate.tool_id
                    ));
                } else if candidate.local_only {
                    evaluation.reasons.push(format!(
                        "Offline First accepts local tool '{}'",
                        candidate.tool_id
                    ));
                } else {
                    evaluation.reasons.push(format!(
                        "Offline First accepts tool '{}' with local-capable execution",
                        candidate.tool_id
                    ));
                }
            }
            Some(BuiltinPolicy::PrivacyMaximum) => {
                evaluation.prefer_local = true;
                if candidate.requires_internet || candidate.cloud_only || !candidate.local_only {
                    // Privacy Maximum hard-denies — no approval path.
                    set_decision(&mut evaluation, PolicyDecision::Denied);
                    evaluation.reasons.push(format!(
                        "Privacy Maximum rejects tool '{}' (non-local execution is not permitted)",
                        candidate.tool_id
                    ));
                } else {
                    evaluation.reasons.push(format!(
                        "Privacy Maximum accepts local-only tool '{}'",
                        candidate.tool_id
                    ));
                }
            }
            Some(_) | None => {
                evaluation.reasons.push(format!(
                    "policy '{}' recorded with no additional constraints for this candidate",
                    policy.name
                ));
            }
        }
    }

    evaluation
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::BuiltinPolicy;
    use crate::scope::PolicyScope;
    use crate::Policy;

    fn cloud_candidate() -> ExecutionCandidate {
        ExecutionCandidate {
            tool_id: "cloud_search".into(),
            provider_id: "cloud".into(),
            requires_internet: true,
            local_only: false,
            cloud_only: true,
        }
    }

    #[test]
    fn offline_first_requires_approval_for_cloud() {
        let policies = vec![Policy {
            name: "Offline First".into(),
            scope: PolicyScope::Global,
            builtin: Some(BuiltinPolicy::OfflineFirst),
        }];
        let evaluation = evaluate_policies(&policies, &cloud_candidate());
        assert_eq!(evaluation.decision, PolicyDecision::RequiresApproval);
        assert!(evaluation.allowed);
        assert!(evaluation.explanation().contains("Offline First"));
    }

    #[test]
    fn privacy_maximum_overrides_offline_first_with_deny() {
        let policies = vec![
            Policy {
                name: "Offline First".into(),
                scope: PolicyScope::Global,
                builtin: Some(BuiltinPolicy::OfflineFirst),
            },
            Policy {
                name: "Privacy Maximum".into(),
                scope: PolicyScope::Global,
                builtin: Some(BuiltinPolicy::PrivacyMaximum),
            },
        ];
        let evaluation = evaluate_policies(&policies, &cloud_candidate());
        assert_eq!(evaluation.decision, PolicyDecision::Denied);
        assert!(!evaluation.allowed);
        assert!(evaluation.explanation().contains("Privacy Maximum"));
    }
}
