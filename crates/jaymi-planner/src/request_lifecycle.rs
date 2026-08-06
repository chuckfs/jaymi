//! Request lifecycle stages that every interaction follows.
//!
//! Canonical vocabulary matches `Planner::handle` and architecture docs.
//! Every user-facing request enters the Planner; stages after Context
//! assemble branch by Intent class (tool-backed / session / plan /
//! conversational).

/// Ordered stages of Planner request handling.
///
/// Stages marked **Planned** are not implemented yet; they remain in the
/// enum so diagnostics and docs share one vocabulary without presenting
/// aspirational behavior as Current.
///
/// **Tool-backed** intents create an Execution Plan, optionally wait for
/// review, then run Action Policy → Permission (during plan gating) → Tool,
/// and finally produce an Execution Summary.
/// Session / PlanWork assemble a ContextBundle and return without
/// action-plan execution. Conversational / unknown intents assemble a
/// ContextBundle, then invoke the Reasoning Engine (never tools, never
/// providers directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStage {
    /// Inbound user request enters the Planner.
    ReceiveRequest,
    /// Decision Engine resolves deterministic Intent.
    DetermineIntent,
    /// Decision Engine selects required Capability id(s).
    ResolveCapability,
    /// Context Policy Engine decides which providers may participate.
    EvaluateContextPolicy,
    /// Selected Context Providers contribute sections.
    CollectFromProviders,
    /// Context Engine assembles an immutable ContextBundle.
    AssembleContextBundle,
    /// Behavior stage — **Planned** (not implemented).
    RunBehavior,
    /// Planner creates an immutable action ExecutionPlan (tool-backed only).
    CreateExecutionPlan,
    /// Review gate when the plan requires approval (tool-backed only).
    ///
    /// The Planner pauses here: plan + tool input are retained so Approve can
    /// resume without replanning. Conversation stays active across the pause.
    ReviewExecutionPlan,
    /// Action Policy Engine evaluates the tool/provider candidate (during plan gating).
    EvaluateActionPolicy,
    /// Permission Engine checks authorization (during plan gating).
    CheckPermissions,
    /// Tool Orchestrator runs the tool for an Approved plan (tool-backed only).
    ExecuteTool,
    /// Bound providers perform work **inside** tool execution (not a second Planner hop).
    InvokeProviders,
    /// Planner records an ExecutionSummary for the plan outcome.
    SummarizeExecution,
    /// Planner returns a response (with ContextBundle attached).
    Respond,
}
