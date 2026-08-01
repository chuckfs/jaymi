//! Policy Engine for Jaymi.
//!
//! Fourth subsystem in the deterministic boot sequence.
//! Policies express preferences. Permissions answer authorization.

#![forbid(unsafe_code)]

pub mod builtin;
pub mod scope;

use builtin::BuiltinPolicy;
use jaymi_core::{HealthReport, JaymiResult, Lifecycle};
use scope::PolicyScope;

const NAME: &str = "policy_engine";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database"];

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

    /// Resolve the effective policy set for the current request.
    ///
    /// Intentionally unimplemented beyond returning the active list.
    pub fn resolve(&self) -> JaymiResult<Vec<Policy>> {
        Ok(self.active.clone())
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
        // Default Offline First policy preference placeholder — no behavior yet.
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

    #[test]
    fn initialize_loads_default_policy() {
        let mut engine = PolicyEngine::new();
        engine.initialize().unwrap();
        assert_eq!(engine.active.len(), 1);
        assert!(engine.health_check().healthy);
    }
}
