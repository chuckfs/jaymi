//! Planner — the orchestration kernel of Jaymi.
//!
//! Every request passes through the Planner. It understands goals, gathers
//! context, delegates work, enforces permissions, and manages execution.
//! The Planner does not perform the work itself.

#![forbid(unsafe_code)]

pub mod decision;
pub mod request_lifecycle;
pub mod reasoning;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use decision::{DecisionEngine, Intent};
use jaymi_capabilities::{
    compose_capabilities, is_multi_capability, workspace_expansion_for, Capability,
    CapabilityComposition, CapabilityDescriptor, CapabilityDiscoveryReport, CapabilityEngineApi,
    CapabilityInspectorReport, CapabilityInventory, DiscoveredProvider, DiscoveredTool,
    ExecutionPlan, WorkspaceExpansion,
};
use jaymi_core::{
    Citation, Document, FileEntry, HealthReport, JaymiError, JaymiResult, Lifecycle, UserRequest,
};
use jaymi_memory_engine::{
    AppendMessageRequest, ArchiveConversationRequest, AssembleContextRequest, AssembledMemoryContext,
    Conversation, ConversationArchive, ConversationMessage, ConversationMeta,
    CreateConversationRequest, CreatePersonalMemoryRequest, ListProjectDecisionsQuery,
    MemoryEngineApi, MemoryQuery, MemoryRecord, PersonalContext, ProjectDecision, ProjectMeta,
    PromoteMemoryRequest, PromotionAskDecision, PromotionSuggestQuery, PromotionSuggestion,
    RegisterProjectRequest, StoreMemoryRequest, StoreProjectDecisionRequest,
    StoreProjectMemoryRequest, UpdatePersonalMemoryRequest,
};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionCheckResult, PermissionEngine,
    PermissionRequest, PermissionScope,
};
use jaymi_policies::{ExecutionCandidate, PolicyEngine, PolicyEvaluation};
use jaymi_project_engine::{CreateProjectRequest, Project, ProjectContext, ProjectEngineApi};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::{
    InternetRequirement, PrivacyMode, ToolInput, ToolOrchestrator, ToolRegistry,
    QUERY_INVENTORY_TOOL_ID, READ_FILE_TOOL_ID, SCAN_FILESYSTEM_TOOL_ID, SEARCH_FILES_TOOL_ID,
    SEARCH_KNOWLEDGE_TOOL_ID,
};
use reasoning::ReasoningEngine;

const NAME: &str = "planner";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
    "context_engine",
    "capability_engine",
    "provider_registry",
    "tool_registry",
    "project_engine",
];

/// Final response produced after the request lifecycle completes.
#[derive(Debug, Default, Clone)]
pub struct PlannerResponse {
    /// Human-readable summary of the result.
    pub content: String,
    /// Capability selected for the request, when any.
    pub capability: Option<Capability>,
    /// Tool selected for execution, when any.
    pub tool_id: Option<String>,
    /// Provider that fulfilled the tool, when known.
    pub provider_id: Option<String>,
    /// Directory that was listed, when applicable.
    pub listed_path: Option<std::path::PathBuf>,
    /// Structured directory listing entries.
    pub entries: Vec<FileEntry>,
    /// Explainable citations for retrieved search / inventory hits.
    pub citations: Vec<Citation>,
    /// Unified document produced by the Read pipeline.
    pub document: Option<Document>,
    /// Policy evaluation for the selected tool, when evaluated.
    pub policy_evaluation: Option<PolicyEvaluation>,
    /// Permission check result for the selected tool, when evaluated.
    pub permission_result: Option<PermissionCheckResult>,
    /// True when policy or permission blocked tool execution.
    pub blocked: bool,
    /// Assembled project context, when a project is the active workspace.
    pub project_context: Option<ProjectContext>,
    /// Capability execution plan selected for the request (never executed here).
    pub execution_plan: Option<ExecutionPlan>,
    /// Workspace expansion requested by the selected capability (conversation stays).
    pub workspace: Option<WorkspaceExpansion>,
    /// Promotion suggestions from the Memory Engine (never auto-applied).
    pub promotion_suggestions: Vec<PromotionSuggestion>,
    /// Whether the Planner should ask the user about promotions.
    pub promotion_ask: PromotionAskDecision,
    /// Relevant memories assembled for this request (never a full dump).
    pub memory_context: Option<AssembledMemoryContext>,
}

/// Dependencies required to construct the Planner from registries.
#[derive(Clone)]
pub struct PlannerDeps {
    /// Capability Engine used for discovery, validation, and planning.
    pub capabilities: Arc<dyn CapabilityEngineApi>,
    /// Provider registry used for discovery.
    pub providers: Arc<ProviderRegistry>,
    /// Tool registry used for discovery.
    pub tools: Arc<ToolRegistry>,
    /// Orchestrator used to select and execute tools.
    pub orchestrator: ToolOrchestrator,
    /// Policy engine consulted before permissions.
    pub policies: Arc<PolicyEngine>,
    /// Permission engine consulted before tool execution.
    pub permissions: Arc<PermissionEngine>,
    /// Memory Engine — Planner never accesses memory storage directly.
    pub memory: Arc<dyn MemoryEngineApi>,
    /// Project Engine — Planner requests one assembled project context.
    pub projects: Arc<dyn ProjectEngineApi>,
}

/// Planner kernel.
///
/// The Planner remains deterministic. Reasoning is delegated. Execution is
/// delegated. Nothing bypasses this component.
pub struct Planner {
    initialized: bool,
    decision: DecisionEngine,
    reasoning: ReasoningEngine,
    capabilities: Arc<dyn CapabilityEngineApi>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    orchestrator: ToolOrchestrator,
    policies: Arc<PolicyEngine>,
    permissions: Arc<PermissionEngine>,
    memory: Arc<dyn MemoryEngineApi>,
    projects: Arc<dyn ProjectEngineApi>,
}

impl Planner {
    /// Construct a Planner that discovers capabilities through registries.
    pub fn new(deps: PlannerDeps) -> Self {
        Self {
            initialized: false,
            decision: DecisionEngine,
            reasoning: ReasoningEngine,
            capabilities: deps.capabilities,
            providers: deps.providers,
            tools: deps.tools,
            orchestrator: deps.orchestrator,
            policies: deps.policies,
            permissions: deps.permissions,
            memory: deps.memory,
            projects: deps.projects,
        }
    }

    /// Discover registered capabilities through the Capability Engine.
    pub fn discover_capabilities(&self) -> Vec<Capability> {
        self.capabilities.list()
    }

    /// Describe a capability's catalog metadata (registration optional).
    pub fn describe_capability(&self, capability: Capability) -> CapabilityDescriptor {
        self.capabilities.describe(capability)
    }

    /// Resolve a registered capability by stable id.
    pub fn resolve_capability(&self, id: &str) -> JaymiResult<Option<CapabilityDescriptor>> {
        self.capabilities.resolve(id)
    }

    /// Discover what Jaymi can currently do given live tools and providers.
    pub fn discover_capability_status(&self) -> JaymiResult<CapabilityDiscoveryReport> {
        let inventory = self.capability_inventory()?;
        self.capabilities.discover(&inventory)
    }

    /// Inspect the capability system for developers (registered, active, requirements).
    pub fn inspect_capabilities(&self) -> JaymiResult<CapabilityInspectorReport> {
        let inventory = self.capability_inventory()?;
        self.capabilities.inspect(&inventory)
    }

    /// Build a capability execution plan from declared requirements.
    ///
    /// Describes what each capability needs; nothing is executed.
    pub fn build_capability_plan(
        &self,
        capabilities: &[Capability],
    ) -> JaymiResult<ExecutionPlan> {
        self.capabilities.build_execution_plan(capabilities)
    }

    /// Plan work for one capability and optional goal.
    ///
    /// Resolves required tools, providers, and permissions against the live
    /// inventory. Tools are never executed by planning.
    pub fn plan_capability(
        &self,
        capability: Capability,
        goal: Option<&str>,
    ) -> JaymiResult<ExecutionPlan> {
        self.plan_capabilities(&[capability], goal)
    }

    /// Compose independent capabilities into one execution plan.
    ///
    /// Capabilities remain separate plan steps — they are never merged.
    /// Tools are never executed by planning.
    pub fn plan_capabilities(
        &self,
        capabilities: &[Capability],
        goal: Option<&str>,
    ) -> JaymiResult<ExecutionPlan> {
        let inventory = self.capability_inventory()?;
        self.capabilities.compose(capabilities, &inventory, goal)
    }

    /// Compose from a [`CapabilityComposition`] value.
    pub fn compose_capability_plan(
        &self,
        composition: &CapabilityComposition,
    ) -> JaymiResult<ExecutionPlan> {
        let inventory = self.capability_inventory()?;
        self.capabilities.compose_plan(composition, &inventory)
    }

    /// Number of registered providers visible to the Planner.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Number of registered tools visible to the Planner.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Returns true when the Planner completed initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Honest reasoning-backend status for diagnostics.
    pub fn reasoning_status(&self) -> &'static str {
        self.reasoning.status_label()
    }

    /// Whether a reasoning backend is wired.
    pub fn reasoning_implemented(&self) -> bool {
        self.reasoning.is_implemented()
    }

    /// Ask the Memory Engine for relevant memories.
    ///
    /// The Planner requests memory; it never reads the Knowledge Store directly.
    pub fn retrieve_memory(&self, query: &MemoryQuery) -> JaymiResult<Vec<MemoryRecord>> {
        self.ensure_ready()?;
        self.memory.retrieve(query)
    }

    /// Ask the Memory Engine to assemble only relevant memories for a request.
    pub fn assemble_memory_context(
        &self,
        request: &AssembleContextRequest,
    ) -> JaymiResult<AssembledMemoryContext> {
        self.ensure_ready()?;
        self.memory.assemble_context(request)
    }

    /// Store an intentional memory through the Memory Engine.
    pub fn store_memory(&self, request: &StoreMemoryRequest) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        self.memory.store(request)
    }

    /// Forget a memory through the Memory Engine.
    pub fn forget_memory(&self, memory_id: &str) -> JaymiResult<()> {
        self.ensure_ready()?;
        self.memory.forget(memory_id)
    }

    /// Promote a memory up the durability ladder (intentional only).
    pub fn promote_memory(
        &self,
        request: &PromoteMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        self.memory.promote(request)
    }

    /// Ask the Memory Engine for promotion suggestions (never applies them).
    pub fn suggest_memory_promotions(
        &self,
        query: &PromotionSuggestQuery,
    ) -> JaymiResult<Vec<PromotionSuggestion>> {
        self.ensure_ready()?;
        self.memory.suggest_promotions(query)
    }

    /// Decide whether to ask the user about promotion suggestions.
    pub fn decide_promotion_ask(
        &self,
        suggestions: &[PromotionSuggestion],
    ) -> PromotionAskDecision {
        PromotionAskDecision::from_suggestions(suggestions)
    }

    /// Archive a conversation through the Memory Engine.
    pub fn archive_conversation(
        &self,
        request: &ArchiveConversationRequest,
    ) -> JaymiResult<ConversationArchive> {
        self.ensure_ready()?;
        self.memory.archive_conversation(request)
    }

    /// Create a persisted conversation through the Memory Engine.
    pub fn create_conversation(
        &self,
        request: &CreateConversationRequest,
    ) -> JaymiResult<ConversationMeta> {
        self.ensure_ready()?;
        self.memory.create_conversation(request)
    }

    /// Append a message to a conversation through the Memory Engine.
    pub fn append_message(
        &self,
        request: &AppendMessageRequest,
    ) -> JaymiResult<ConversationMessage> {
        self.ensure_ready()?;
        self.memory.append_message(request)
    }

    /// Load an entire conversation through the Memory Engine.
    pub fn load_conversation(
        &self,
        conversation_id: &str,
    ) -> JaymiResult<Option<Conversation>> {
        self.ensure_ready()?;
        self.memory.load_conversation(conversation_id)
    }

    /// List conversations attached to a project.
    pub fn list_project_conversations(
        &self,
        project_id: &str,
    ) -> JaymiResult<Vec<ConversationMeta>> {
        self.ensure_ready()?;
        self.memory.list_conversations_for_project(project_id)
    }

    /// Attach a conversation to exactly one project, or detach it (`None`).
    pub fn attach_conversation_to_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
    ) -> JaymiResult<ConversationMeta> {
        self.ensure_ready()?;
        self.memory
            .attach_conversation_to_project(conversation_id, project_id)
    }

    /// Register a project for memory attachment.
    pub fn register_project(
        &self,
        request: &RegisterProjectRequest,
    ) -> JaymiResult<ProjectMeta> {
        self.ensure_ready()?;
        self.memory.register_project(request)
    }

    /// Activate a project for automatic memory retrieval.
    pub fn set_active_project(
        &self,
        project_id: Option<&str>,
    ) -> JaymiResult<Option<ProjectMeta>> {
        self.ensure_ready()?;
        self.memory.set_active_project(project_id)
    }

    /// Activate a conversation for memory context assembly.
    pub fn set_active_conversation(&self, conversation_id: Option<&str>) -> JaymiResult<()> {
        self.ensure_ready()?;
        self.memory.set_active_conversation(conversation_id)
    }

    /// Current active conversation id (persists across project switches).
    pub fn active_conversation_id(&self) -> Option<String> {
        self.memory.active_conversation_id()
    }

    /// Current active workspace project id, when any.
    pub fn active_project_id(&self) -> Option<String> {
        self.projects.active_project_id()
    }

    /// Store categorized project memory.
    pub fn store_project_memory(
        &self,
        request: &StoreProjectMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        self.memory.store_project_memory(request)
    }

    /// Persist an architectural decision in the project decision log.
    pub fn store_project_decision(
        &self,
        request: &StoreProjectDecisionRequest,
    ) -> JaymiResult<ProjectDecision> {
        self.ensure_ready()?;
        self.memory.store_project_decision(request)
    }

    /// List a project's decision log.
    pub fn list_project_decisions(
        &self,
        query: &ListProjectDecisionsQuery,
    ) -> JaymiResult<Vec<ProjectDecision>> {
        self.ensure_ready()?;
        self.memory.list_project_decisions(query)
    }

    /// Fetch one project decision by memory id.
    pub fn get_project_decision(&self, memory_id: &str) -> JaymiResult<Option<ProjectDecision>> {
        self.ensure_ready()?;
        self.memory.get_project_decision(memory_id)
    }

    /// Create a first-class project through the Project Engine.
    pub fn create_project(&self, request: &CreateProjectRequest) -> JaymiResult<Project> {
        self.ensure_ready()?;
        self.projects.create(request)
    }

    /// Open a project as the active workspace and assemble its context.
    ///
    /// The active conversation is preserved; when none is active the project's
    /// most recent conversation is resumed.
    pub fn open_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.ensure_ready()?;
        self.bind_memory_project(project_id)?;
        let context = self.projects.open(project_id)?;
        self.resume_project_conversation(project_id)?;
        jaymi_logging::info(
            "planner",
            format!(
                "opened project id={} name={} entries={}",
                context.project.id.as_str(),
                context.project.name,
                context.entry_count()
            ),
        );
        Ok(context)
    }

    /// Switch the active workspace to another project.
    pub fn switch_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.open_project(project_id)
    }

    /// Close the session-open project. The active conversation is untouched.
    pub fn close_project(&self) -> JaymiResult<Option<Project>> {
        self.ensure_ready()?;
        let closed = self.projects.close()?;
        let _ = self.memory.set_active_project(None);
        Ok(closed)
    }

    /// Soft-delete a project through the Project Engine.
    pub fn delete_project(&self, project_id: &str) -> JaymiResult<()> {
        self.ensure_ready()?;
        self.projects.delete(project_id)
    }

    /// List active projects through the Project Engine.
    pub fn list_projects(&self) -> JaymiResult<Vec<Project>> {
        self.ensure_ready()?;
        self.projects.list()
    }

    /// Request one assembled project context (defaults to the open project).
    pub fn project_context(
        &self,
        project_id: Option<&str>,
    ) -> JaymiResult<Option<ProjectContext>> {
        self.ensure_ready()?;
        self.projects.project_context(project_id)
    }

    /// Assemble project context for a known project id.
    pub fn assemble_project_context(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.ensure_ready()?;
        self.projects.assemble_context(project_id)
    }

    /// Restore project context through the Project Engine.
    pub fn restore_project_context(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.assemble_project_context(project_id)
    }

    /// Create an intentional personal preference through the Memory Engine.
    pub fn create_personal_memory(
        &self,
        request: &CreatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        self.memory.create_personal_memory(request)
    }

    /// Update a personal preference through the Memory Engine.
    pub fn update_personal_memory(
        &self,
        request: &UpdatePersonalMemoryRequest,
    ) -> JaymiResult<MemoryRecord> {
        self.ensure_ready()?;
        self.memory.update_personal_memory(request)
    }

    /// Delete a personal preference through the Memory Engine.
    pub fn delete_personal_memory(&self, memory_id: &str) -> JaymiResult<()> {
        self.ensure_ready()?;
        self.memory.delete_personal_memory(memory_id)
    }

    /// Load active personal preferences through the Memory Engine.
    pub fn personal_context(&self) -> JaymiResult<PersonalContext> {
        self.ensure_ready()?;
        self.memory.personal_context()
    }

    fn ensure_ready(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("planner is not initialized"))
        }
    }

    /// Snapshot live tools and providers for capability discovery and planning.
    ///
    /// Sorted by id so plans and discovery reports stay deterministic.
    fn capability_inventory(&self) -> JaymiResult<CapabilityInventory> {
        let mut tools: Vec<DiscoveredTool> = self
            .tools
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|metadata| DiscoveredTool {
                id: metadata.id,
                capabilities: metadata.capabilities,
            })
            .collect();
        tools.sort_by(|left, right| left.id.cmp(&right.id));

        let mut providers: Vec<DiscoveredProvider> = self
            .providers
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|identity| DiscoveredProvider {
                id: identity.id,
                capabilities: identity.capabilities,
            })
            .collect();
        providers.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(CapabilityInventory { tools, providers })
    }

    /// Keep the Memory Engine project registry aligned with the workspace.
    fn bind_memory_project(&self, project_id: &str) -> JaymiResult<()> {
        if self.memory.set_active_project(Some(project_id)).is_ok() {
            return Ok(());
        }
        let Some(project) = self.projects.get(project_id)? else {
            return Err(JaymiError::new(format!("project not found: {project_id}")));
        };
        let _ = self.memory.register_project(&RegisterProjectRequest {
            project_id: Some(project.id.as_str().to_string()),
            name: project.name.clone(),
            root_path: project
                .root_directory
                .as_ref()
                .map(|path| path.display().to_string()),
        })?;
        if self.memory.set_active_project(Some(project_id)).is_err() {
            jaymi_logging::warn(
                "planner",
                format!("memory engine has no project registered for {project_id}"),
            );
        }
        Ok(())
    }

    /// Resume the project's latest conversation when none is active.
    fn resume_project_conversation(&self, project_id: &str) -> JaymiResult<()> {
        if self.memory.active_conversation_id().is_some() {
            return Ok(());
        }
        let conversations = self.memory.list_conversations_for_project(project_id)?;
        let Some(latest) = conversations.first() else {
            return Ok(());
        };
        self.memory
            .set_active_conversation(Some(latest.id.as_str()))?;
        jaymi_logging::info(
            "planner",
            format!(
                "resumed conversation id={} for project={}",
                latest.id.as_str(),
                project_id
            ),
        );
        Ok(())
    }

    /// Root directory of the active workspace project, when any.
    fn active_project_root(&self) -> Option<PathBuf> {
        let project_id = self.projects.active_project_id()?;
        self.projects
            .get(&project_id)
            .ok()
            .flatten()
            .and_then(|project| project.root_directory)
    }

    /// Resolve relative request paths against the active project root.
    fn resolve_workspace_path(&self, path: PathBuf) -> PathBuf {
        if path.is_absolute() {
            return path;
        }
        let Some(root) = self.active_project_root() else {
            return path;
        };
        let raw = path.to_string_lossy();
        if raw.is_empty() || raw == "." || raw == "./" {
            root
        } else {
            root.join(path)
        }
    }

    fn handle_continue_project(&self, name: &str) -> JaymiResult<PlannerResponse> {
        let Some(project) = self.projects.find_by_name(name)? else {
            return Ok(PlannerResponse {
                content: format!(
                    "No project named \"{name}\" is registered. Create the project before continuing."
                ),
                ..PlannerResponse::default()
            });
        };
        let context = self.open_project(project.id.as_str())?;
        let content = format_project_context_summary(&context);
        Ok(PlannerResponse {
            content,
            project_context: Some(context),
            ..PlannerResponse::default()
        })
    }

    fn handle_close_project(&self) -> JaymiResult<PlannerResponse> {
        let closed = self.close_project()?;
        let content = match &closed {
            Some(project) => format!(
                "Closed project \"{}\". The active conversation stays open.",
                project.name
            ),
            None => "No project is currently open.".to_string(),
        };
        jaymi_logging::info("planner", &content);
        Ok(PlannerResponse {
            content,
            ..PlannerResponse::default()
        })
    }

    /// Produce an execution plan for a goal without executing any tool.
    ///
    /// One or more independent capabilities may be composed into a single
    /// plan. Planning never requires every capability to be currently
    /// fulfillable — an incomplete plan honestly reports what is still missing.
    fn handle_plan_work(
        &self,
        capabilities: &[Capability],
        goal: &str,
    ) -> JaymiResult<PlannerResponse> {
        let ordered = compose_capabilities(capabilities)?;
        let plan = self.plan_capabilities(&ordered, Some(goal))?;
        let primary = ordered[0];
        let composition_note = if is_multi_capability(&ordered) {
            format!(
                "Composed {} independent capabilities ({}). ",
                ordered.len(),
                ordered
                    .iter()
                    .map(Capability::id)
                    .collect::<Vec<_>>()
                    .join(" → ")
            )
        } else {
            String::new()
        };
        jaymi_logging::info(
            "planner",
            format!(
                "planned work capabilities=[{}] {}",
                ordered
                    .iter()
                    .map(Capability::id)
                    .collect::<Vec<_>>()
                    .join(","),
                plan.summary()
            ),
        );
        let content = format!(
            "Execution plan (planning only — no tools were executed):\n{composition_note}{}",
            plan.render()
        );
        Ok(PlannerResponse {
            content,
            capability: Some(primary),
            execution_plan: Some(plan),
            workspace: workspace_expansion_for(
                primary,
                format!("capability {} requested workspace expansion", primary.id()),
            ),
            ..PlannerResponse::default()
        })
    }

    /// Process a user request through the architectural pipeline.
    ///
    /// Flow for tool-backed intents:
    /// Planner → Capability Engine → Policy Engine → Permission Engine → Tool
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            jaymi_logging::error("planner", "request rejected: planner is not initialized");
            return Err(JaymiError::new("planner is not initialized"));
        }

        jaymi_logging::info(
            "planner",
            format!(
                "request received content={:?} directory={:?} file={:?}",
                truncate_for_log(&request.content),
                request
                    .directory
                    .as_ref()
                    .map(|path| path.display().to_string()),
                request
                    .file
                    .as_ref()
                    .map(|path| path.display().to_string())
            ),
        );

        // RetrieveMemory stage — Memory Engine assembles only relevant memories.
        let memory_context = self.assemble_memory_context(&AssembleContextRequest {
            text: request.content.clone(),
            conversation_id: self.memory.active_conversation_id(),
            project_id: None,
            limit: Some(12),
            ..AssembleContextRequest::default()
        })?;
        let memories = memory_context.records();
        jaymi_logging::info(
            "planner",
            format!(
                "assembled {} relevant memories (candidates={} truncated={})",
                memories.len(),
                memory_context.candidate_count,
                memory_context.truncated
            ),
        );

        // Memory Engine suggests promotions; Planner never auto-applies them.
        let promotion_suggestions = self.suggest_memory_promotions(&PromotionSuggestQuery {
            conversation_id: self.memory.active_conversation_id(),
            project_id: self.memory.active_project_id(),
            min_importance: None,
            limit: Some(5),
        })?;
        let promotion_ask = self.decide_promotion_ask(&promotion_suggestions);
        if !promotion_suggestions.is_empty() {
            jaymi_logging::info(
                "planner",
                format!(
                    "promotion suggestions={} ask={:?}",
                    promotion_suggestions.len(),
                    promotion_ask
                ),
            );
        }

        let intent = self.decision.determine_intent(&request);

        // Workspace intents are answered by the Project Engine, not by tools.
        match &intent {
            Intent::ContinueProject { name } => {
                let response = self.handle_continue_project(name)?;
                return Ok(finalize(
                    response,
                    memory_context,
                    promotion_suggestions,
                    promotion_ask,
                ));
            }
            Intent::CloseProject => {
                let response = self.handle_close_project()?;
                return Ok(finalize(
                    response,
                    memory_context,
                    promotion_suggestions,
                    promotion_ask,
                ));
            }
            _ => {}
        }

        let Some(capability) = self.decision.required_capability(&intent) else {
            jaymi_logging::warn(
                "planner",
                "unsupported request; no capability mapped for intent",
            );
            return Ok(PlannerResponse {
                content: "Unsupported request. Try: list <directory>, read <file>, search <query>, index <path>, or ask what files exist".to_string(),
                promotion_suggestions,
                promotion_ask,
                memory_context: Some(memory_context),
                ..PlannerResponse::default()
            });
        };

        jaymi_logging::info(
            "planner",
            format!("intent resolved capability={}", capability.id()),
        );

        // Planning answers "what would this take" without needing the
        // capability to be fulfillable today.
        if let Intent::PlanWork {
            capabilities,
            goal,
        } = &intent
        {
            let mut response = self.handle_plan_work(capabilities, goal)?;
            response.project_context = self.open_project_context();
            return Ok(finalize(
                response,
                memory_context,
                promotion_suggestions,
                promotion_ask,
            ));
        }

        if !self.capabilities.contains(capability) {
            let message = format!("capability {} is not registered", capability.id());
            jaymi_logging::error("planner", &message);
            return Err(JaymiError::new(message));
        }

        // Reasoning Engine is intentionally unused for these deterministic paths.
        let _ = &self.reasoning;

        let result = match intent {
            Intent::ListDirectory { path } => {
                self.handle_list_directory(capability, self.resolve_workspace_path(path))
            }
            Intent::ReadFile { path } => {
                self.handle_read_file(capability, self.resolve_workspace_path(path))
            }
            Intent::DiscoverInventory { kind } => {
                self.handle_discover_inventory(capability, kind)
            }
            Intent::SearchKnowledge { request } => {
                self.handle_search_knowledge(capability, self.scope_search_request(request))
            }
            Intent::IndexRoots { path } => self.handle_index_roots(capability, path),
            Intent::ContinueProject { .. }
            | Intent::CloseProject
            | Intent::PlanWork { .. } => {
                unreachable!("workspace and planning intents handled earlier")
            }
            Intent::Unknown => {
                jaymi_logging::warn("planner", "unknown intent after capability mapping");
                Ok(PlannerResponse {
                    content: "Unsupported request.".to_string(),
                    ..PlannerResponse::default()
                })
            }
        };

        let result = match result {
            Ok(mut response) => {
                if response.execution_plan.is_none() {
                    response.execution_plan = self.plan_capability(capability, None).ok();
                }
                if response.workspace.is_none() {
                    response.workspace = workspace_expansion_for(
                        capability,
                        format!("capability {} selected for request", capability.id()),
                    );
                }
                if response.project_context.is_none() {
                    response.project_context = self.open_project_context();
                }
                Ok(finalize(
                    response,
                    memory_context,
                    promotion_suggestions,
                    promotion_ask,
                ))
            }
            Err(error) => Err(error),
        };

        match &result {
            Ok(response) => jaymi_logging::info(
                "planner",
                format!(
                    "request completed tool={:?} provider={:?} blocked={} permission={:?}",
                    response.tool_id,
                    response.provider_id,
                    response.blocked,
                    response
                        .permission_result
                        .as_ref()
                        .map(|result| result.decision.as_str())
                ),
            ),
            Err(error) => jaymi_logging::error(
                "planner",
                format!("request failed: {}", error.message()),
            ),
        }

        result
    }

    /// Assembled context for the open project, when one is active.
    fn open_project_context(&self) -> Option<ProjectContext> {
        match self.projects.project_context(None) {
            Ok(context) => context,
            Err(error) => {
                jaymi_logging::warn(
                    "planner",
                    format!("project context unavailable: {}", error.message()),
                );
                None
            }
        }
    }

    /// Constrain search to the active workspace so projects stay isolated.
    fn scope_search_request(
        &self,
        mut request: jaymi_core::SearchRequest,
    ) -> jaymi_core::SearchRequest {
        if request.folder.is_none() {
            if let Some(root) = self.active_project_root() {
                request.folder = Some(root);
                request.folder_immediate = false;
            }
        }
        request
    }

    fn handle_list_directory(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::list_directory(path.clone());
        let prepared = self.prepare_execution(
            capability,
            Some(SEARCH_FILES_TOOL_ID),
            &input,
            &path,
            "List directory",
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let content = format!(
            "Listed {} entries in {} via {} → {} → {}",
            output.entries.len(),
            path.display(),
            capability.id(),
            tool_id,
            provider_id.as_deref().unwrap_or("unknown")
        );

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(path),
            entries: output.entries,
            citations: output.citations,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_read_file(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::read_file(path.clone());
        let prepared = self.prepare_execution(
            capability,
            Some(READ_FILE_TOOL_ID),
            &input,
            &path,
            "Read file",
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let document = output.document.ok_or_else(|| {
            JaymiError::new("read tool succeeded without returning a document")
        })?;
        let provider_id = prepared.provider_id.clone();
        let content = format!(
            "Read {} ({}) via {} → {} → {} → {}",
            path.display(),
            document.file_type,
            capability.id(),
            tool_id,
            provider_id.as_deref().unwrap_or("unknown"),
            document.parser_id
        );

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            document: Some(document),
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_discover_inventory(
        &self,
        capability: Capability,
        kind: jaymi_core::DiscoveryQueryKind,
    ) -> JaymiResult<PlannerResponse> {
        let listed_path = match &kind {
            jaymi_core::DiscoveryQueryKind::ByFolder { path, .. } => Some(path.clone()),
            _ => None,
        };
        let resource_path = listed_path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("inventory"));
        let input = ToolInput::discover(kind);
        let prepared = self.prepare_execution(
            capability,
            Some(QUERY_INVENTORY_TOOL_ID),
            &input,
            &resource_path,
            "Query inventory",
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let content = output.message.unwrap_or_else(|| {
            format!(
                "Found {} inventoried entries via {} → {} (search engine)",
                output.entries.len(),
                capability.id(),
                tool_id
            )
        });

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path,
            entries: output.entries,
            citations: output.citations,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_search_knowledge(
        &self,
        capability: Capability,
        request: jaymi_core::SearchRequest,
    ) -> JaymiResult<PlannerResponse> {
        let resource_path = request
            .folder
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("search"));
        let input = ToolInput::search(request);
        let prepared = self.prepare_execution(
            capability,
            Some(SEARCH_KNOWLEDGE_TOOL_ID),
            &input,
            &resource_path,
            "Search knowledge",
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let content = output.message.unwrap_or_else(|| {
            format!(
                "Found {} search hits via {} → {}",
                output.entries.len(),
                capability.id(),
                tool_id
            )
        });

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(resource_path),
            entries: output.entries,
            citations: output.citations,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_index_roots(
        &self,
        capability: Capability,
        path: Option<std::path::PathBuf>,
    ) -> JaymiResult<PlannerResponse> {
        let resource_path = path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("configured-roots"));
        let input = ToolInput {
            path: path.clone(),
            discovery: None,
            search: None,
        };
        let prepared = self.prepare_execution(
            capability,
            Some(SCAN_FILESYSTEM_TOOL_ID),
            &input,
            &resource_path,
            "Index filesystem",
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let content = output
            .message
            .unwrap_or_else(|| format!("Indexed filesystem via {tool_id}"));

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: path,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    /// Planner → Policy Engine → Permission Engine (tool selected, not yet run).
    fn prepare_execution(
        &self,
        capability: Capability,
        preferred_tool_id: Option<&str>,
        _input: &ToolInput,
        path: &Path,
        action_label: &str,
    ) -> JaymiResult<PreparedExecution> {
        let tool_id = if let Some(preferred) = preferred_tool_id {
            match self.tools.get(preferred) {
                Ok(tool) if tool.metadata().capabilities.contains(&capability) => {
                    preferred.to_string()
                }
                _ => self.orchestrator.select(capability)?.ok_or_else(|| {
                    JaymiError::new(format!(
                        "no tool registered for capability {}",
                        capability.id()
                    ))
                })?,
            }
        } else {
            self.orchestrator.select(capability)?.ok_or_else(|| {
                JaymiError::new(format!(
                    "no tool registered for capability {}",
                    capability.id()
                ))
            })?
        };

        let tool = self.tools.get(&tool_id)?;
        let metadata = tool.metadata();
        let provider_id = metadata.provider.clone();
        let candidate = ExecutionCandidate {
            tool_id: tool_id.clone(),
            provider_id: provider_id.clone(),
            requires_internet: matches!(metadata.internet, InternetRequirement::Required),
            local_only: matches!(metadata.privacy, PrivacyMode::LocalOnly),
            cloud_only: matches!(metadata.privacy, PrivacyMode::CloudOnly),
        };

        jaymi_logging::info(
            "planner",
            format!(
                "policy evaluation tool={} provider={} internet={} local_only={}",
                candidate.tool_id,
                candidate.provider_id,
                candidate.requires_internet,
                candidate.local_only
            ),
        );
        let policy_evaluation = self.policies.evaluate(&candidate)?;
        if !policy_evaluation.allowed {
            jaymi_logging::warn(
                "planner",
                format!(
                    "policy blocked tool={}: {}",
                    tool_id,
                    policy_evaluation.summary()
                ),
            );
            return Ok(PreparedExecution {
                tool_id: tool_id.clone(),
                provider_id: Some(provider_id),
                policy_evaluation: Some(policy_evaluation.clone()),
                permission_result: None,
                blocked_response: Some(PlannerResponse {
                    content: format!(
                        "Blocked by policy before executing '{}': {}",
                        tool_id,
                        policy_evaluation.summary()
                    ),
                    capability: Some(capability),
                    tool_id: Some(tool_id),
                    provider_id: Some(candidate.provider_id),
                    policy_evaluation: Some(policy_evaluation),
                    permission_result: None,
                    blocked: true,
                    ..PlannerResponse::default()
                }),
            });
        }

        let permission_request = PermissionRequest {
            category: PermissionCategory::Filesystem,
            action: PermissionAction::Read,
            scope: PermissionScope::Once,
            explanation: format!("{action_label} at {}", path.display()),
            resource: Some(path.display().to_string()),
        };
        jaymi_logging::info(
            "planner",
            format!(
                "permission check category=filesystem action=read resource={}",
                path.display()
            ),
        );
        let permission_result = self.permissions.check(&permission_request)?;
        if !permission_result.allows_execution() {
            jaymi_logging::warn(
                "planner",
                format!(
                    "permission {} for tool={} resource={}",
                    permission_result.decision.as_str(),
                    tool_id,
                    path.display()
                ),
            );
            return Ok(PreparedExecution {
                tool_id: tool_id.clone(),
                provider_id: Some(provider_id),
                policy_evaluation: Some(policy_evaluation.clone()),
                permission_result: Some(permission_result.clone()),
                blocked_response: Some(PlannerResponse {
                    content: format!(
                        "Blocked by permission ({}) before executing '{}': {}",
                        permission_result.decision.as_str(),
                        tool_id,
                        permission_result.explanation
                    ),
                    capability: Some(capability),
                    tool_id: Some(tool_id),
                    provider_id: Some(candidate.provider_id),
                    policy_evaluation: Some(policy_evaluation),
                    permission_result: Some(permission_result),
                    blocked: true,
                    ..PlannerResponse::default()
                }),
            });
        }

        Ok(PreparedExecution {
            tool_id,
            provider_id: Some(provider_id),
            policy_evaluation: Some(policy_evaluation),
            permission_result: Some(permission_result),
            blocked_response: None,
        })
    }

    fn ensure_success(&self, output: &jaymi_tools::ToolOutput) -> JaymiResult<()> {
        if output.success {
            Ok(())
        } else {
            Err(JaymiError::new(
                output
                    .message
                    .clone()
                    .unwrap_or_else(|| "tool execution failed".to_string()),
            ))
        }
    }
}

struct PreparedExecution {
    tool_id: String,
    provider_id: Option<String>,
    policy_evaluation: Option<PolicyEvaluation>,
    permission_result: Option<PermissionCheckResult>,
    blocked_response: Option<PlannerResponse>,
}

fn finalize(
    mut response: PlannerResponse,
    memory_context: AssembledMemoryContext,
    promotion_suggestions: Vec<PromotionSuggestion>,
    promotion_ask: PromotionAskDecision,
) -> PlannerResponse {
    response.promotion_suggestions = promotion_suggestions;
    response.promotion_ask = promotion_ask;
    response.memory_context = Some(memory_context);
    response
}

fn format_project_context_summary(context: &ProjectContext) -> String {
    let conversation_messages: usize = context
        .conversations
        .iter()
        .map(|conversation| conversation.message_count)
        .sum();
    format!(
        "Restored project \"{}\". indexed_files={} conversations={} conversation_messages={} memories={} tasks={} decisions={} architecture={} documents={} parsed_content={} recent_work={}",
        context.project.name,
        context.indexed_files.len(),
        context.conversations.len(),
        conversation_messages,
        context.memories.entry_count(),
        context.tasks.len(),
        context.decisions.len(),
        context.architecture_documents.len(),
        context.important_documents.len(),
        context.parsed_content.len(),
        context.recent_work.len()
    )
}

fn truncate_for_log(value: &str) -> String {
    const MAX: usize = 120;
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let shortened: String = trimmed.chars().take(MAX).collect();
        format!("{shortened}…")
    }
}

impl Lifecycle for Planner {
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
        if !self.capabilities.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: capability engine is not ready",
            ));
        }
        if !self.providers.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: provider registry is not ready",
            ));
        }
        if !self.tools.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: tool registry is not ready",
            ));
        }
        if !self.policies.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: policy engine is not ready",
            ));
        }
        if !self.permissions.is_initialized() {
            return Err(JaymiError::new(
                "planner cannot initialize: permission engine is not ready",
            ));
        }

        let _ = &self.decision;
        let _ = &self.reasoning;
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let registries_ready = self.capabilities.is_initialized()
            && self.providers.is_initialized()
            && self.tools.is_initialized()
            && self.policies.is_initialized()
            && self.permissions.is_initialized();
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized && registries_ready,
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
    use jaymi_capabilities::CapabilityEngine;
    use jaymi_core::{EntryType, FileType, Lifecycle};
    use jaymi_database::Database;
    use jaymi_knowledge::SqliteKnowledgeStore;
    use jaymi_memory_engine::{InMemoryMemoryStore, MemoryEngine};
    use jaymi_parsers::default_registry;
    use jaymi_permissions::PermissionDecision;
    use jaymi_project_engine::{InMemoryProjectStore, ProjectEngine};
    use jaymi_providers::{FilesystemProvider, Provider, FILESYSTEM_PROVIDER_ID};
    use jaymi_tools::{
        EstimatedRuntime, ExecutionMode, GpuRequirements, MemoryUsage, Reliability, ResourceCost,
        ResultType, Tool, ToolMetadata, ToolOutput, ReadFileTool, SearchFilesTool,
    };
    use jaymi_understanding::{ContentIntelligenceApi, SqliteContentStore, UnderstandingEngine};
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_memory_engine() -> Arc<dyn MemoryEngineApi> {
        let mut engine = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
        engine.initialize().unwrap();
        Arc::new(engine)
    }

    fn test_project_engine() -> Arc<dyn ProjectEngineApi> {
        let mut engine = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
        engine.initialize().unwrap();
        Arc::new(engine)
    }

    fn planner_with_search_and_read() -> Planner {
        planner_with_tools(|tools, filesystem, content_api| {
            tools
                .register_tool(Arc::new(SearchFilesTool::new(Arc::clone(&filesystem))))
                .unwrap();
            tools
                .register_tool(Arc::new(ReadFileTool::new(content_api)))
                .unwrap();
        })
    }

    fn planner_with_tools<F>(register: F) -> Planner
    where
        F: FnOnce(
            &mut ToolRegistry,
            Arc<FilesystemProvider>,
            Arc<ContentIntelligenceApi>,
        ),
    {
        let mut capabilities = CapabilityEngine::new();
        capabilities.initialize().unwrap();
        capabilities.register(Capability::Search).unwrap();
        capabilities.register(Capability::ReadDocuments).unwrap();
        capabilities.register(Capability::Code).unwrap();

        let mut providers = ProviderRegistry::new();
        providers.initialize().unwrap();
        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        providers.register(&filesystem).unwrap();
        let filesystem = Arc::new(filesystem);

        let data = temp_dir().join("planner-data");
        fs::create_dir_all(&data).unwrap();
        let mut db = Database::with_data_dir(&data);
        db.initialize().unwrap();
        let db = Arc::new(db);
        let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
        knowledge.initialize().unwrap();
        let knowledge = Arc::new(knowledge);
        let content = Arc::new(SqliteContentStore::new(Arc::clone(&db)));
        let parsers = Arc::new(default_registry().unwrap());
        let mut understanding = UnderstandingEngine::new(
            knowledge,
            content,
            Arc::clone(&filesystem),
            parsers,
        );
        understanding.initialize().unwrap();
        let understanding = Arc::new(understanding);
        let content_api = Arc::new(ContentIntelligenceApi::new(Arc::clone(&understanding)));

        let mut tools = ToolRegistry::new();
        tools.initialize().unwrap();
        register(
            &mut tools,
            Arc::clone(&filesystem),
            Arc::clone(&content_api),
        );
        let tools = Arc::new(tools);
        let orchestrator = ToolOrchestrator::new(Arc::clone(&tools));

        let mut policies = PolicyEngine::new();
        policies.initialize().unwrap();
        let mut permissions = PermissionEngine::new();
        permissions.initialize().unwrap();

        let mut planner = Planner::new(PlannerDeps {
            capabilities: Arc::new(capabilities) as Arc<dyn CapabilityEngineApi>,
            providers: Arc::new(providers),
            tools,
            orchestrator,
            policies: Arc::new(policies),
            permissions: Arc::new(permissions),
            memory: test_memory_engine(),
            projects: test_project_engine(),
        });
        planner.initialize().unwrap();
        planner
    }

    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jaymi-planner-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
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
            Ok(ToolOutput::directory_listing(Vec::new()))
        }
    }

    #[test]
    fn planner_initializes_from_registries() {
        let planner = planner_with_search_and_read();
        assert!(planner.health_check().healthy);
        assert!(planner.discover_capabilities().contains(&Capability::Search));
        assert!(planner
            .discover_capabilities()
            .contains(&Capability::ReadDocuments));
        assert_eq!(planner.provider_count(), 1);
        assert_eq!(planner.tool_count(), 2);
    }

    #[test]
    fn list_directory_flows_through_architecture() {
        let dir = temp_dir();
        let mut file = File::create(dir.join("readme.md")).unwrap();
        write!(file, "jaymi").unwrap();
        fs::create_dir(dir.join("src")).unwrap();

        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::list_directory(&dir))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert_eq!(response.tool_id.as_deref(), Some("search_files"));
        assert_eq!(response.provider_id.as_deref(), Some(FILESYSTEM_PROVIDER_ID));
        assert!(!response.blocked);
        assert!(response.policy_evaluation.as_ref().unwrap().allowed);
        assert_eq!(
            response.permission_result.as_ref().unwrap().decision,
            PermissionDecision::Allowed
        );
        assert_eq!(response.entries.len(), 2);
        assert!(response
            .entries
            .iter()
            .any(|entry| entry.name == "readme.md" && entry.entry_type == EntryType::File));
        let plan = response.execution_plan.expect("execution plan");
        assert_eq!(plan.capabilities(), vec![Capability::Search]);
    }

    #[test]
    fn read_file_returns_unified_document() {
        let dir = temp_dir();
        let path = dir.join("spec.md");
        let mut file = File::create(&path).unwrap();
        write!(file, "# Spec\n\nDetails.").unwrap();

        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::read_file(&path)).unwrap();

        assert_eq!(response.capability, Some(Capability::ReadDocuments));
        assert_eq!(response.tool_id.as_deref(), Some("read_file"));
        assert!(!response.blocked);
        let document = response.document.expect("document");
        assert_eq!(document.file_type, FileType::Markdown);
        assert_eq!(document.title.as_deref(), Some("Spec"));
        assert_eq!(document.parser_id, "markdown");
        assert!(document.text.contains("Details."));
        assert!(response.content.contains("markdown"));
    }

    #[test]
    fn planner_does_not_call_filesystem_for_unknown_intent() {
        let planner = planner_with_search_and_read();
        let response = planner.handle(UserRequest::new("sing a song")).unwrap();
        assert!(response.entries.is_empty());
        assert!(response.document.is_none());
        assert!(response.capability.is_none());
        assert!(!response.blocked);
    }

    #[test]
    fn offline_first_blocks_cloud_only_tool() {
        let planner = planner_with_tools(|tools, _, _| {
            tools
                .register_tool(Arc::new(CloudSearchTool::new()))
                .unwrap();
        });
        let response = planner
            .handle(UserRequest::list_directory(temp_dir()))
            .unwrap();
        assert!(response.blocked);
        assert!(response.entries.is_empty());
        assert_eq!(response.tool_id.as_deref(), Some("cloud_search"));
        assert!(!response.policy_evaluation.as_ref().unwrap().allowed);
        assert!(response.permission_result.is_none());
    }

    #[test]
    fn coding_goal_produces_a_plan_without_executing_tools() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("Help me build an app."))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Code));
        assert!(response.tool_id.is_none());
        assert!(!response.blocked);
        assert!(response.content.contains("Execution plan"));

        let plan = response.execution_plan.expect("execution plan");
        assert_eq!(plan.goal.as_deref(), Some("Help me build an app."));
        assert_eq!(plan.steps.len(), 1);
        assert!(!plan.is_executable());
        assert!(plan
            .required_permissions()
            .iter()
            .any(|permission| permission.label() == "terminal:execute"));
    }

    #[test]
    fn composed_goal_plans_multiple_independent_capabilities() {
        let planner = planner_with_search_and_read();
        let response = planner
            .handle(UserRequest::new("research then code then create"))
            .unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert!(response.tool_id.is_none());
        let plan = response.execution_plan.expect("composed plan");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(
            plan.capabilities(),
            vec![
                Capability::Search,
                Capability::Code,
                Capability::GenerateImages
            ]
        );
        assert!(response
            .content
            .contains("Composed 3 independent capabilities"));
    }

    #[test]
    fn planning_does_not_require_a_fulfillable_capability() {
        let planner = planner_with_search_and_read();
        let plan = planner
            .plan_capability(Capability::Code, Some("ship a feature"))
            .unwrap();
        assert_eq!(plan.steps[0].capability, Capability::Code);
        assert!(!plan.steps[0].tools_resolved);
        assert_eq!(
            plan.steps[0].required_tools,
            vec![
                "editor".to_string(),
                "language_server".to_string(),
                "terminal".to_string(),
                "git".to_string()
            ]
        );
    }
}
