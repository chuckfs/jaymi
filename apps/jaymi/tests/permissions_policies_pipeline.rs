//! Integration tests — Action Policy + Permission + Review gating.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_capabilities::{Capability, CapabilityEngine, CapabilityEngineApi};

use jaymi_core::{JaymiResult, Lifecycle, UserRequest};
use jaymi_memory_engine::{InMemoryMemoryStore, MemoryEngine, MemoryEngineApi};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionDecision, PermissionEngine, PermissionRequest,
    PermissionScope,
};
use jaymi_planner::{Planner, PlannerDeps, ReviewIntent, ToolRouteTable};
use jaymi_policies::{
    BuiltinPolicy, ExecutionCandidate, Policy, PolicyDecision, PolicyEngine, PolicyScope,
};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, Tool, ToolInput, ToolMetadata, ToolRisk,
    ToolOrchestrator, ToolOutput, ToolRegistry,
};

#[test]
fn approved_read_requests_include_permission_and_policy() {
    let data_dir = temp_dir("perm-approved-data");
    let work = temp_dir("perm-approved-work");
    fs::write(work.join("a.txt"), "hi").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let response = app.list_directory(&work).expect("list");
    assert!(!response.blocked);
    assert_eq!(
        response.policy_evaluation.as_ref().unwrap().decision,
        PolicyDecision::Allowed
    );
    assert_eq!(
        response.permission_result.as_ref().unwrap().decision,
        PermissionDecision::Allowed
    );

    let snapshot = app
        .diagnostics_from_response(Some(response))
        .expect("diagnostics");
    assert_eq!(snapshot.permission_decision.as_deref(), Some("allowed"));
    assert_eq!(snapshot.policy_allowed, Some(true));
    assert!(!snapshot.request_blocked);
    assert!(snapshot
        .policy_summary
        .as_deref()
        .unwrap_or("")
        .contains("allowed"));
}

#[test]
fn approval_flow_offline_first_pauses_cloud_tool_for_review() {
    let planner = planner_with_only_cloud_search(false);
    let response = planner
        .handle(UserRequest::list_directory(temp_dir("policy-approve-work")))
        .expect("handle");

    assert!(response.blocked);
    assert!(response.awaiting_review);
    assert!(response.entries.is_empty());
    assert_eq!(response.tool_id.as_deref(), Some("cloud_search"));
    assert_eq!(
        response.policy_evaluation.as_ref().unwrap().decision,
        PolicyDecision::RequiresApproval
    );
    assert!(response.permission_result.is_some());
    assert!(response.content.contains("I can do that."));
    assert!(response.content.contains("You can:"));
    assert!(response.content.contains("Offline First"));

    let plan_id = response.execution_plan.expect("plan").id().clone();
    let resumed = planner
        .resolve_review(ReviewIntent::Approve { plan_id })
        .expect("approve through planner");
    assert!(!resumed.awaiting_review);
    assert!(!resumed.blocked);
    assert_eq!(resumed.tool_id.as_deref(), Some("cloud_search"));
}

#[test]
fn denied_flow_privacy_maximum_explains_and_skips_execution() {
    let planner = planner_with_only_cloud_search(true);
    let response = planner
        .handle(UserRequest::list_directory(temp_dir("policy-deny-work")))
        .expect("handle");

    assert!(response.blocked);
    assert!(!response.awaiting_review);
    assert!(response.entries.is_empty());
    assert_eq!(response.tool_id.as_deref(), Some("cloud_search"));
    assert_eq!(
        response.policy_evaluation.as_ref().unwrap().decision,
        PolicyDecision::Denied
    );
    assert!(response.permission_result.is_none());
    assert!(response.content.contains("Denied"));
    assert!(response.content.contains("Privacy Maximum"));
}

#[test]
fn policy_override_privacy_maximum_beats_offline_first_approval() {
    let planner = planner_with_only_cloud_search(true);
    let evaluation = planner
        .handle(UserRequest::list_directory(temp_dir("override-work")))
        .expect("handle")
        .policy_evaluation
        .expect("policy");
    assert_eq!(evaluation.decision, PolicyDecision::Denied);
    assert!(evaluation.explanation().contains("Privacy Maximum"));
    assert!(evaluation
        .policies_applied
        .iter()
        .any(|name| name == "Offline First" || name == "Privacy Maximum"));
}

#[test]
fn policy_explanation_is_user_visible_on_deny_and_approval() {
    let approve = planner_with_only_cloud_search(false)
        .handle(UserRequest::list_directory(temp_dir("explain-approve")))
        .expect("handle");
    assert!(approve.content.contains("Offline First"));
    assert!(approve
        .execution_summary
        .as_ref()
        .and_then(|summary| summary.error.as_ref())
        .is_some_and(|error| error.contains("Offline First")));

    let deny = planner_with_only_cloud_search(true)
        .handle(UserRequest::list_directory(temp_dir("explain-deny")))
        .expect("handle");
    assert!(deny.content.contains("Privacy Maximum"));
    assert!(deny
        .execution_summary
        .as_ref()
        .and_then(|summary| summary.error.as_ref())
        .is_some_and(|error| error.contains("Privacy Maximum")));
}

#[test]
fn denied_permission_prevents_execution() {
    let mut permissions = PermissionEngine::new();
    permissions.initialize().unwrap();
    let result = permissions
        .check(&PermissionRequest {
            category: PermissionCategory::Internet,
            action: PermissionAction::Network,
            scope: PermissionScope::Once,
            explanation: "Call a remote API".into(),
            resource: Some("https://example.com".into()),
        })
        .unwrap();
    assert_eq!(result.decision, PermissionDecision::Denied);
    assert!(!result.allows_execution());
    assert!(result.explanation.contains("not granted"));
}

#[test]
fn offline_first_policy_participates_in_evaluation() {
    let mut policies = PolicyEngine::new();
    policies.initialize().unwrap();

    let local = policies
        .evaluate(&ExecutionCandidate {
            tool_id: "search_files".into(),
            provider_id: "filesystem".into(),
            requires_internet: false,
            local_only: true,
            cloud_only: false,
        })
        .unwrap();
    assert_eq!(local.decision, PolicyDecision::Allowed);
    assert!(local.prefer_local);

    let remote = policies
        .evaluate(&ExecutionCandidate {
            tool_id: "cloud_search".into(),
            provider_id: "cloud".into(),
            requires_internet: true,
            local_only: false,
            cloud_only: true,
        })
        .unwrap();
    assert_eq!(remote.decision, PolicyDecision::RequiresApproval);
    assert!(remote
        .reasons
        .iter()
        .any(|reason| reason.contains("Offline First")));
}

fn planner_with_only_cloud_search(privacy_maximum: bool) -> Planner {
    let mut capabilities = CapabilityEngine::new();
    capabilities.initialize().unwrap();
    capabilities.register(Capability::Search).unwrap();

    let mut providers = ProviderRegistry::new();
    providers.initialize().unwrap();

    let mut tools = ToolRegistry::new();
    tools.initialize().unwrap();
    tools
        .register_tool(Arc::new(CloudSearchTool::new()))
        .unwrap();
    let tools = Arc::new(tools);

    let mut policies = PolicyEngine::new();
    policies.initialize().unwrap();
    if privacy_maximum {
        policies.active.push(Policy {
            name: "Privacy Maximum".into(),
            scope: PolicyScope::Global,
            builtin: Some(BuiltinPolicy::PrivacyMaximum),
        });
    }
    let mut permissions = PermissionEngine::new();
    permissions.initialize().unwrap();

    let memory = {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();
        Arc::new(engine) as Arc<dyn MemoryEngineApi>
    };
    let projects = {
        let mut engine = jaymi_project_engine::ProjectEngine::with_store(Arc::new(
            jaymi_project_engine::InMemoryProjectStore::new(),
        ));
        engine.initialize().unwrap();
        Arc::new(engine) as Arc<dyn jaymi_project_engine::ProjectEngineApi>
    };
    let mut context = jaymi_context::ContextEngine::new();
    context.initialize().unwrap();
    let data = std::env::temp_dir().join(format!(
        "jaymi-policy-context-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&data).unwrap();
    let mut db = jaymi_database::Database::with_data_dir(&data);
    db.initialize().unwrap();
    let db = Arc::new(db);
    let mut knowledge = jaymi_knowledge::SqliteKnowledgeStore::new(Arc::clone(&db));
    knowledge.initialize().unwrap();
    let mut search = jaymi_search::SearchEngine::new(Arc::new(knowledge), None);
    search.initialize().unwrap();
    context
        .bind_sources(jaymi_context::ContextSources {
            memory: Arc::clone(&memory),
            projects: Arc::clone(&projects),
            search: Arc::new(search),
        })
        .unwrap();

    let mut planner = Planner::new(PlannerDeps {
        capabilities: Arc::new(capabilities) as Arc<dyn CapabilityEngineApi>,
        providers: Arc::new(providers),
        tools: Arc::clone(&tools),
        orchestrator: ToolOrchestrator::new(tools),
        policies: Arc::new(policies),
        permissions: Arc::new(permissions),
        memory,
        projects,
        context: Arc::new(context),
        routes: ToolRouteTable::builtin(),
    });
    planner.initialize().unwrap();
    planner
}

struct CloudSearchTool {
    metadata: ToolMetadata,
}

impl CloudSearchTool {
    fn new() -> Self {
        Self {
            metadata: ToolMetadata {
                id: "cloud_search".into(),
                name: "Cloud Search".into(),
                version: "0.1.0".into(),
                description: "Requires internet".into(),
                provider: "cloud".into(),
                capabilities: vec![Capability::Search],
                risk: ToolRisk::External,
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Fast,
                resource_cost: ResourceCost::Low,
                memory_usage: MemoryUsage::Small,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::CloudOnly,
                internet: InternetRequirement::Required,
                reliability: Reliability::Experimental,
                result_type: ResultType::SearchResults,
            },
        }
    }
}

impl Tool for CloudSearchTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
        Ok(())
    }

    fn execute(&self, _input: &ToolInput) -> JaymiResult<ToolOutput> {
        Ok(ToolOutput::directory_listing(Vec::new()))
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-perm-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
