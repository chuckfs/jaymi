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

/// Structured result of policy evaluation for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEvaluation {
    /// Whether policies allow the candidate to proceed to permission checks.
    pub allowed: bool,
    /// Names of policies that influenced the decision.
    pub policies_applied: Vec<String>,
    /// Human-readable reasons for allow/deny.
    pub reasons: Vec<String>,
    /// Whether active policies prefer local execution.
    pub prefer_local: bool,
}

impl PolicyEvaluation {
    /// Short summary suitable for diagnostics.
    pub fn summary(&self) -> String {
        if self.allowed {
            format!("allowed ({})", self.policies_applied.join(", "))
        } else {
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

impl Default for PolicyEvaluation {
    fn default() -> Self {
        Self {
            allowed: true,
            policies_applied: Vec::new(),
            reasons: Vec::new(),
            prefer_local: false,
        }
    }
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
                    evaluation.allowed = false;
                    evaluation.reasons.push(format!(
                        "Offline First rejects tool '{}' (internet/cloud execution)",
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
                    evaluation.allowed = false;
                    evaluation.reasons.push(format!(
                        "Privacy Maximum rejects tool '{}'",
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
