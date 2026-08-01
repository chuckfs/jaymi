//! Policy Engine for Jaymi.
//!
//! Policies express preferences. They answer "how should this action happen?"
//! Permissions answer "can this action happen?"

#![forbid(unsafe_code)]

pub mod builtin;
pub mod scope;

use builtin::BuiltinPolicy;
use jaymi_core::JaymiResult;
use scope::PolicyScope;

/// An active policy influencing Planner decisions.
#[derive(Debug, Clone)]
pub struct Policy {
    pub name: String,
    pub scope: PolicyScope,
    pub builtin: Option<BuiltinPolicy>,
}

/// Policy Engine skeleton.
#[derive(Debug, Default)]
pub struct PolicyEngine {
    pub active: Vec<Policy>,
}

impl PolicyEngine {
    /// Resolve the effective policy set for the current request.
    ///
    /// More specific scopes override broader ones:
    /// Task → Project → Conversation → Global
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn resolve(&self) -> JaymiResult<Vec<Policy>> {
        Ok(self.active.clone())
    }
}
