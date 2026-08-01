//! Request lifecycle stages that every interaction follows.
//!
//! No request bypasses this process.

/// Ordered stages of Planner request handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStage {
    ReceiveRequest,
    DetermineIntent,
    DetermineContextRequirements,
    RetrieveMemory,
    RetrieveKnowledge,
    ReasonIfNecessary,
    BuildExecutionPlan,
    SelectCapabilities,
    SelectTools,
    Execute,
    RequestApprovalIfRequired,
    Respond,
    UpdateMemoryOptional,
}
