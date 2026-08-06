//! Request lifecycle stages that every interaction follows.
//!
//! Canonical vocabulary matches `Planner::handle` and architecture docs.
//! Every user-facing request enters the Planner; stages after Context
//! assemble branch by Intent class (tool-backed / session / plan / unsupported).

/// Ordered stages of Planner request handling.
///
/// Stages marked **Planned** are not implemented yet; they remain in the
/// enum so diagnostics and docs share one vocabulary without presenting
/// aspirational behavior as Current.
///
/// **Tool-backed** intents run through Action Policy → Permission → Tool.
/// Session / PlanWork / unsupported paths assemble a ContextBundle and
/// return without those stages.
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
    /// Action Policy Engine evaluates the tool/provider candidate (tool-backed only).
    EvaluateActionPolicy,
    /// Permission Engine checks authorization (tool-backed only).
    CheckPermissions,
    /// Tool Orchestrator selects and runs the tool (tool-backed only).
    ExecuteTool,
    /// Bound providers perform work **inside** tool execution (not a second Planner hop).
    InvokeProviders,
    /// Planner returns a response (with ContextBundle attached).
    Respond,
}
