//! Registration-based tool execution routing.
//!
//! Intent → Capability → Execution Plan → Review → Tool stays Planner-owned.
//! Tool-backed intents resolve through a compile-time [`ToolRouteTable`] so
//! adding a tool does not require a new `Planner::handle` match arm.
//!
//! No reflection: routes are explicit Rust types registered at construction.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{
    DeletionMethod, IntentId, JaymiError, JaymiResult, SearchRequest,
};
use jaymi_permissions::{PermissionAction, PermissionCategory, PermissionCheckResult};
use jaymi_policies::PolicyEvaluation;
use jaymi_tools::ToolInput;

use crate::decision::Intent;
use crate::execution_plan::{ExecutionPlan, ExecutionSummary};
use crate::PlannerResponse;

/// Host services routes may need while preparing a tool call.
///
/// Implemented by [`crate::Planner`] so routes never bypass Planner policy.
pub trait DispatchSupport {
    /// Resolve a path relative to the active project workspace when needed.
    fn resolve_workspace_path(&self, path: PathBuf) -> PathBuf;
    /// Scope a search request to the open project when appropriate.
    fn scope_search_request(&self, request: SearchRequest) -> SearchRequest;
    /// Planner-owned deletion method selection (Trash by default).
    fn resolve_deletion_method(
        &self,
        requested: Option<DeletionMethod>,
        request_text: &str,
    ) -> JaymiResult<DeletionMethod>;
}

/// Static route metadata linking an intent to a registered tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolRoute {
    /// Intent this route fulfills.
    pub intent: IntentId,
    /// Capability the Decision Engine must select for this intent.
    pub capability: Capability,
    /// Preferred tool id (must be registered and advertise `capability`).
    pub tool_id: &'static str,
}

/// Prepared tool invocation produced by a route handler.
#[derive(Debug, Clone)]
pub struct PreparedToolCall {
    /// Tool input for validate / preview / execute.
    pub input: ToolInput,
    /// Primary resource path for permissions and plan labeling.
    pub resource_path: PathBuf,
    /// Permission category for the gate.
    pub permission_category: PermissionCategory,
    /// Permission action for the gate.
    pub permission_action: PermissionAction,
    /// Originating request line on the Execution Plan.
    pub originating_request: String,
    /// Action label for permissions / steps.
    pub action_label: String,
    /// Expected outputs / conversational plan bullets.
    pub expected_outputs: Vec<String>,
    /// When set, invalidate Context cache with this reason after success.
    pub invalidate_cache: Option<&'static str>,
    /// When true, soft-fail into a blocked response instead of `Err`.
    pub soft_failure: bool,
}

/// Shared execution metadata passed to route responders.
#[derive(Debug, Clone)]
pub struct ExecutionMeta {
    pub capability: Capability,
    pub tool_id: String,
    pub provider_id: Option<String>,
    pub policy_evaluation: Option<PolicyEvaluation>,
    pub permission_result: Option<PermissionCheckResult>,
    pub plan: ExecutionPlan,
    pub execution_summary: ExecutionSummary,
}

/// Compile-time friendly handler for one tool-backed intent.
pub trait IntentToolHandler: Send + Sync {
    /// Route metadata (intent / capability / tool id).
    fn route(&self) -> ToolRoute;

    /// Build the tool call from a resolved intent.
    fn prepare(
        &self,
        intent: &Intent,
        request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall>;

    /// Map a successful tool output into a Planner response.
    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponse>;
}

/// Registry of intent → tool handlers.
#[derive(Clone, Default)]
pub struct ToolRouteTable {
    handlers: HashMap<IntentId, Arc<dyn IntentToolHandler>>,
}

impl ToolRouteTable {
    /// Empty table (tests / custom registration).
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Built-in shipping routes (compile-time list; no reflection).
    pub fn builtin() -> Self {
        let mut table = Self::new();
        crate::routes::register_builtin_routes(&mut table);
        table
    }

    /// Register a handler. Duplicate intents replace the previous handler.
    pub fn register(&mut self, handler: Arc<dyn IntentToolHandler>) {
        let intent = handler.route().intent;
        self.handlers.insert(intent, handler);
    }

    /// Register a concrete handler type.
    pub fn register_handler<H>(&mut self, handler: H)
    where
        H: IntentToolHandler + 'static,
    {
        self.register(Arc::new(handler));
    }

    /// Look up a handler by intent id.
    pub fn get(&self, intent: IntentId) -> Option<Arc<dyn IntentToolHandler>> {
        self.handlers.get(&intent).cloned()
    }

    /// True when an intent has a registered tool route.
    pub fn contains(&self, intent: IntentId) -> bool {
        self.handlers.contains_key(&intent)
    }

    /// Number of registered routes.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// True when no routes are registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Snapshot of registered route metadata (stable intent order not guaranteed).
    pub fn routes(&self) -> Vec<ToolRoute> {
        self.handlers
            .values()
            .map(|handler| handler.route())
            .collect()
    }

    /// Intent ids with registered tool routes.
    pub fn intent_ids(&self) -> Vec<IntentId> {
        self.handlers.keys().copied().collect()
    }
}

/// Build a base PlannerResponse from execution metadata.
pub fn response_from_meta(meta: &ExecutionMeta, content: String) -> PlannerResponse {
    PlannerResponse {
        content,
        capability: Some(meta.capability),
        tool_id: Some(meta.tool_id.clone()),
        provider_id: meta.provider_id.clone(),
        policy_evaluation: meta.policy_evaluation.clone(),
        permission_result: meta.permission_result.clone(),
        execution_plan: Some(meta.plan.clone()),
        execution_summary: Some(meta.execution_summary.clone()),
        ..PlannerResponse::default()
    }
}

/// Error when a route's preferred tool is not in the ToolRegistry.
pub fn unknown_tool_error(tool_id: &str) -> JaymiError {
    JaymiError::new(format!("unknown tool '{tool_id}'"))
}

/// Error when a registered tool does not advertise the required capability.
pub fn missing_capability_error(tool_id: &str, capability: Capability) -> JaymiError {
    JaymiError::new(format!(
        "tool '{tool_id}' does not fulfill capability {}",
        capability.id()
    ))
}

/// Error when no route is registered for a tool-backed intent.
pub fn missing_route_error(intent: IntentId) -> JaymiError {
    JaymiError::new(format!(
        "no tool route registered for intent {}",
        intent.as_str()
    ))
}
