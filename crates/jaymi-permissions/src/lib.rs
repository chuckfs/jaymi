//! Permission Engine for Jaymi.
//!
//! Permissions determine whether Jaymi may perform an action.
//! Policies influence how Jaymi behaves. These concerns stay separate.

#![forbid(unsafe_code)]

pub mod categories;
pub mod scope;

use categories::PermissionCategory;
use jaymi_core::JaymiResult;
use scope::PermissionScope;

/// A request for user approval before a protected action.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub category: PermissionCategory,
    pub scope: PermissionScope,
    pub explanation: String,
}

/// Possible outcomes of a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allowed,
    Denied,
    RequiresApproval,
}

/// Permission Engine skeleton.
#[derive(Debug, Default)]
pub struct PermissionEngine;

impl PermissionEngine {
    /// Evaluate whether an action is authorized.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn check(&self, _request: &PermissionRequest) -> JaymiResult<PermissionDecision> {
        Ok(PermissionDecision::RequiresApproval)
    }
}
