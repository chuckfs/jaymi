//! Integration tests for Slice 0.4 — permissions and policies in the Planner.

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
use jaymi_planner::{Planner, PlannerDeps};
use jaymi_policies::{ExecutionCandidate, PolicyEngine};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, Tool, ToolInput, ToolMetadata,
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
    assert!(response.policy_evaluation.as_ref().unwrap().allowed);
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
fn policy_evaluation_denies_cloud_tool_before_permission() {
    let planner = planner_with_only_cloud_search();
    let response = planner
        .handle(UserRequest::list_directory(temp_dir("policy-deny-work")))
        .expect("handle");

    assert!(response.blocked);
    assert!(response.entries.is_empty());
    assert_eq!(response.tool_id.as_deref(), Some("cloud_search"));
    assert!(!response.policy_evaluation.as_ref().unwrap().allowed);
    assert!(response.permission_result.is_none());
    assert!(response.content.contains("Blocked by policy"));
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
    assert!(local.allowed);
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
    assert!(!remote.allowed);
    assert!(remote
        .reasons
        .iter()
        .any(|reason| reason.contains("Offline First")));
}

fn planner_with_only_cloud_search() -> Planner {
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
        panic!("cloud tool must not execute when Offline First is active");
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
