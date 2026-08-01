//! Permission Engine for Jaymi.
//!
//! Fifth subsystem in the deterministic boot sequence.
//! Permissions determine whether Jaymi may perform an action.

#![forbid(unsafe_code)]

pub mod categories;
pub mod scope;

use categories::PermissionCategory;
use jaymi_core::{HealthReport, JaymiResult, Lifecycle};
use scope::PermissionScope;

const NAME: &str = "permission_engine";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database", "policy_engine"];

/// A request for user approval before a protected action.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// Category of the requested action.
    pub category: PermissionCategory,
    /// Scope requested for the grant.
    pub scope: PermissionScope,
    /// Plain-language explanation shown to the user.
    pub explanation: String,
}

/// Possible outcomes of a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Action may proceed.
    Allowed,
    /// Action must not proceed.
    Denied,
    /// User approval is required before proceeding.
    RequiresApproval,
}

/// Permission Engine lifecycle.
#[derive(Debug, Default)]
pub struct PermissionEngine {
    initialized: bool,
}

impl PermissionEngine {
    /// Create an uninitialized permission engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate whether an action is authorized.
    ///
    /// Lifecycle milestone: defaults to requiring approval. No policy logic yet.
    pub fn check(&self, _request: &PermissionRequest) -> JaymiResult<PermissionDecision> {
        Ok(PermissionDecision::RequiresApproval)
    }
}

impl Lifecycle for PermissionEngine {
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
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_health_requires_initialize() {
        let mut engine = PermissionEngine::new();
        assert!(!engine.health_check().healthy);
        engine.initialize().unwrap();
        assert!(engine.health_check().healthy);
    }
}
