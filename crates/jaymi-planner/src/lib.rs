//! Planner — the orchestration kernel of Jaymi.
//!
//! Every request passes through the Planner. It understands goals, gathers
//! context, delegates work, enforces permissions, and manages execution.
//! The Planner does not perform the work itself.

#![forbid(unsafe_code)]

pub mod decision;
pub mod lifecycle;
pub mod reasoning;

use decision::DecisionEngine;
use jaymi_capabilities::CapabilityEngine;
use jaymi_context::ContextEngine;
use jaymi_core::{JaymiResult, UserRequest};
use jaymi_memory::MemoryEngine;
use jaymi_permissions::PermissionEngine;
use jaymi_policies::PolicyEngine;
use jaymi_tools::ToolOrchestrator;
use reasoning::ReasoningEngine;

/// Final response produced after the request lifecycle completes.
#[derive(Debug, Default, Clone)]
pub struct PlannerResponse {
    pub content: String,
}

/// Planner kernel skeleton.
///
/// The Planner remains deterministic. Reasoning is delegated. Execution is
/// delegated. Nothing bypasses this component.
#[derive(Debug, Default)]
pub struct Planner {
    pub decision: DecisionEngine,
    pub reasoning: ReasoningEngine,
    pub context: ContextEngine,
    pub memory: MemoryEngine,
    pub permissions: PermissionEngine,
    pub policies: PolicyEngine,
    pub capabilities: CapabilityEngine,
    pub tools: ToolOrchestrator,
}

impl Planner {
    /// Process a user request through the full Planner lifecycle.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn handle(&self, _request: UserRequest) -> JaymiResult<PlannerResponse> {
        Ok(PlannerResponse::default())
    }
}
