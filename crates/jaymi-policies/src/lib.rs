//! Action Policy Engine for Jaymi.
//!
//! Fourth subsystem in the deterministic boot sequence (lifecycle name
//! `policy_engine`). Action Policies express preferences for tool/provider
//! candidates. They are distinct from Context Policies in `jaymi-context`.
//! Permissions answer authorization.

#![forbid(unsafe_code)]

pub mod builtin;
pub mod evaluation;
pub mod scope;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

pub use builtin::BuiltinPolicy;
pub use evaluation::{evaluate_policies, ExecutionCandidate, PolicyDecision, PolicyEvaluation};
pub use scope::PolicyScope;

const NAME: &str = "policy_engine";
// Preferences only — persistence is Target; no database peer yet.
const DEPENDENCIES: &[&str] = &["configuration", "logging"];

/// An active policy influencing Planner decisions.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Human-readable policy name.
    pub name: String,
    /// Scope at which the policy applies.
    pub scope: PolicyScope,
    /// Optional built-in policy identity.
    pub builtin: Option<BuiltinPolicy>,
}

/// Policy Engine lifecycle and registry of active policies.
#[derive(Debug, Default)]
pub struct PolicyEngine {
    initialized: bool,
    /// Currently active policies.
    pub active: Vec<Policy>,
}

impl PolicyEngine {
    /// Create an uninitialized policy engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true after initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Resolve the effective policy set for the current request.
    pub fn resolve(&self) -> JaymiResult<Vec<Policy>> {
        self.ensure_initialized()?;
        Ok(self.active.clone())
    }

    /// Evaluate whether policies allow, require approval for, or deny a candidate.
    ///
    /// Offline First requires approval for internet/cloud tools. Privacy Maximum
    /// hard-denies non-local candidates (overrides softer policies).
    pub fn evaluate(&self, candidate: &ExecutionCandidate) -> JaymiResult<PolicyEvaluation> {
        let policies = self.resolve()?;
        Ok(evaluate_policies(&policies, candidate))
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("policy engine is not initialized"))
        }
    }
}

impl Lifecycle for PolicyEngine {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        if self.active.is_empty() {
            self.active.push(Policy {
                name: "Offline First".to_string(),
                scope: PolicyScope::Global,
                builtin: Some(BuiltinPolicy::OfflineFirst),
            });
        }
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            ("active_policies".to_string(), self.active.len().to_string()),
            (
                "offline_first".to_string(),
                self.active
                    .iter()
                    .any(|policy| policy.builtin == Some(BuiltinPolicy::OfflineFirst))
                    .to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.active.clear();
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_candidate() -> ExecutionCandidate {
        ExecutionCandidate {
            tool_id: "search_files".into(),
            provider_id: "filesystem".into(),
            requires_internet: false,
            local_only: true,
            cloud_only: false,
        }
    }

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
    fn initialize_loads_default_policy() {
        let mut engine = PolicyEngine::new();
        engine.initialize().unwrap();
        assert_eq!(engine.active.len(), 1);
        assert!(engine.health_check().healthy);
    }

    #[test]
    fn offline_first_allows_local_tools() {
        let mut engine = PolicyEngine::new();
        engine.initialize().unwrap();
        let evaluation = engine.evaluate(&local_candidate()).unwrap();
        assert_eq!(evaluation.decision, PolicyDecision::Allowed);
        assert!(evaluation.allowed);
        assert!(evaluation.prefer_local);
        assert!(evaluation
            .policies_applied
            .iter()
            .any(|name| name == "Offline First"));
    }

    #[test]
    fn offline_first_requires_approval_for_internet_tools() {
        let mut engine = PolicyEngine::new();
        engine.initialize().unwrap();
        let evaluation = engine.evaluate(&cloud_candidate()).unwrap();
        assert_eq!(evaluation.decision, PolicyDecision::RequiresApproval);
        assert!(evaluation.allowed);
        assert!(evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("Offline First")));
    }
}
