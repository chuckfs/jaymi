//! Planner — the orchestration kernel of Jaymi.
//!
//! Every request passes through the Planner. It understands goals, gathers
//! context via the Context Engine, selects capabilities, enforces policy and
//! permissions, builds an execution plan, and delegates to tools.
//!
//! The Planner does not own long-lived Memory or Project CRUD APIs. Those
//! belong to the Memory Engine and Project Engine. Application (or tools)
//! call those engines directly for administrative operations.

#![forbid(unsafe_code)]

pub mod decision;
pub mod reasoning;
pub mod request_lifecycle;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use decision::{DecisionEngine, Intent};
use jaymi_capabilities::{
    compose_capabilities, is_multi_capability, workspace_expansion_for, Capability,
    CapabilityComposition, CapabilityDescriptor, CapabilityDiscoveryReport, CapabilityEngineApi,
    CapabilityInspectorReport, CapabilityInventory, DiscoveredProvider, DiscoveredTool,
    ExecutionPlan, WorkspaceExpansion,
};
use jaymi_context::{ContextBundle, ContextEngine};
use jaymi_core::{
    Citation, Document, FileEntry, GitOperation, GitPathStatus, HealthReport, JaymiError,
    JaymiResult, Lifecycle, LspCompletionItem, LspDiagnostic, LspHover, LspLocation, LspRequest,
    LspTextEdit, ProjectKnowledgeRequest, TerminalOperation, UserRequest,
};
use jaymi_memory_engine::{
    AssembledMemoryContext, MemoryEngineApi, PromotionAskDecision, PromotionSuggestion,
};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionCheckResult, PermissionEngine,
    PermissionRequest, PermissionScope,
};
use jaymi_policies::{ExecutionCandidate, PolicyEngine, PolicyEvaluation};
use jaymi_project_engine::{Project, ProjectContext, ProjectEngineApi, ProjectKnowledgeHit};
use jaymi_providers::ProviderRegistry;
use jaymi_tools::{
    InternetRequirement, PrivacyMode, ToolInput, ToolOrchestrator, ToolRegistry, GIT_TOOL_ID,
    LANGUAGE_SERVER_TOOL_ID, LIST_PROJECT_TREE_TOOL_ID, MANAGE_PATH_TOOL_ID,
    QUERY_INVENTORY_TOOL_ID, READ_FILE_TOOL_ID, SCAN_FILESYSTEM_TOOL_ID, SEARCH_FILES_TOOL_ID,
    SEARCH_KNOWLEDGE_TOOL_ID, SEARCH_PROJECT_KNOWLEDGE_TOOL_ID, TERMINAL_TOOL_ID,
    WRITE_FILE_TOOL_ID,
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
    /// Project closed by a Close intent, when any.
    pub closed_project: Option<Project>,
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
    /// Immutable Context Engine snapshot for this request (Planner / Behaviors / LLM).
    pub context_bundle: Option<ContextBundle>,
    /// Project-scoped knowledge hits (files, memories, tasks, decisions, …).
    pub project_knowledge: Vec<ProjectKnowledgeHit>,
    /// Terminal session id when a terminal tool ran.
    pub terminal_session_id: Option<String>,
    /// Output produced by the latest terminal command.
    pub terminal_output: Option<String>,
    /// Full terminal scrollback for the session.
    pub terminal_scrollback: Option<String>,
    /// Terminal command history (oldest first).
    pub terminal_history: Vec<String>,
    /// Display title for the terminal session, when a terminal tool ran.
    pub terminal_title: Option<String>,
    /// Whether the terminal session is still alive after the operation.
    pub terminal_alive: Option<bool>,
    /// Current Git branch when a Git tool ran.
    pub git_branch: Option<String>,
    /// Short Git status summary.
    pub git_summary: Option<String>,
    /// Whether the probed path is inside a Git work tree.
    pub git_is_repository: Option<bool>,
    /// Unstaged modified files.
    pub git_modified: Vec<GitPathStatus>,
    /// Newly staged (added) files.
    pub git_added: Vec<GitPathStatus>,
    /// Deleted files (worktree and/or index).
    pub git_deleted: Vec<GitPathStatus>,
    /// Staged files.
    pub git_staged: Vec<GitPathStatus>,
    /// Untracked files.
    pub git_untracked: Vec<GitPathStatus>,
    /// Hover result from the language server.
    pub lsp_hover: Option<LspHover>,
    /// Completion candidates from the language server.
    pub lsp_completions: Vec<LspCompletionItem>,
    /// Diagnostics from the language server.
    pub lsp_diagnostics: Vec<LspDiagnostic>,
    /// Go-to-definition locations.
    pub lsp_definitions: Vec<LspLocation>,
    /// Find-references locations.
    pub lsp_references: Vec<LspLocation>,
    /// Rename / workspace text edits.
    pub lsp_edits: Vec<LspTextEdit>,
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
    /// Context Engine — sole assembler of request context for `handle`.
    pub context: Arc<ContextEngine>,
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
    context: Arc<ContextEngine>,
    /// How many times [`Self::handle`] has been entered (integrity tests).
    handle_count: AtomicU64,
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
            context: deps.context,
            handle_count: AtomicU64::new(0),
        }
    }

    /// Number of times [`Self::handle`] has been entered.
    pub fn handle_count(&self) -> u64 {
        self.handle_count.load(Ordering::Relaxed)
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
    /// Uses the live tool/provider inventory so availability reflects what is
    /// currently executable. Nothing is executed.
    pub fn build_capability_plan(&self, capabilities: &[Capability]) -> JaymiResult<ExecutionPlan> {
        let inventory = self.capability_inventory()?;
        self.capabilities.plan(capabilities, &inventory, None)
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

    /// Sync Memory's session hint to the Project Engine open project.
    ///
    /// Memory does not own session open state — it only mirrors the id for
    /// context assembly. Called only from [`Self::open_project`] /
    /// [`Self::close_project`].
    fn bind_memory_project(&self, project_id: Option<&str>) -> JaymiResult<()> {
        self.memory.set_active_project(project_id)?;
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

    /// Sole project session open lifecycle.
    ///
    /// Continue / Open-by-id intents and Application open helpers all enter
    /// [`Self::handle`], which calls this once. Order:
    /// 1. Project Engine owns open state (`open`)
    /// 2. Memory mirrors the id for context assembly
    /// 3. Resume the project's latest conversation when none is active
    ///
    /// Do not call `projects.open` or `memory.set_active_project` for session
    /// activation anywhere else.
    fn open_project(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.ensure_ready()?;
        let context = self.projects.open(project_id)?;
        self.bind_memory_project(Some(project_id))?;
        self.resume_project_conversation(project_id)?;
        self.context.invalidate_cache("project_changed");
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

    /// Sole project session close lifecycle.
    ///
    /// Project Engine clears open state first; Memory's session hint is cleared
    /// to match. The active conversation is untouched.
    fn close_project(&self) -> JaymiResult<Option<Project>> {
        self.ensure_ready()?;
        let closed = self.projects.close()?;
        let _ = self.bind_memory_project(None);
        self.context.invalidate_cache("project_changed");
        Ok(closed)
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
        self.handle_open_project_id(project.id.as_str())
    }

    fn handle_open_project_id(&self, project_id: &str) -> JaymiResult<PlannerResponse> {
        let context = self.open_project(project_id)?;
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
            closed_project: closed,
            ..PlannerResponse::default()
        })
    }

    /// Search project-scoped knowledge through Cap → Policy → Permission → Tool.
    fn handle_search_project_knowledge(
        &self,
        capability: Capability,
        project_id: &str,
        text: &str,
        limit: Option<usize>,
    ) -> JaymiResult<PlannerResponse> {
        let request = ProjectKnowledgeRequest {
            project_id: project_id.to_string(),
            text: text.to_string(),
            limit,
        };
        let resource = std::path::PathBuf::from(format!("project:{project_id}"));
        let input = ToolInput::project_knowledge(request);
        let prepared = self.prepare_execution(
            capability,
            Some(SEARCH_PROJECT_KNOWLEDGE_TOOL_ID),
            &input,
            &resource,
            "Search project knowledge",
            PermissionCategory::Filesystem,
            PermissionAction::Read,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let content = output.message.clone().unwrap_or_else(|| {
            format!(
                "Project knowledge search completed via {} → {} → {}",
                capability.id(),
                tool_id,
                provider_id.as_deref().unwrap_or("unknown")
            )
        });

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            project_knowledge: output.project_knowledge,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
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
    /// Request → Context → Capability → Policy → Permission →
    /// Execution Plan → Tool Engine → Response
    pub fn handle(&self, request: UserRequest) -> JaymiResult<PlannerResponse> {
        if !self.initialized {
            jaymi_logging::error("planner", "request rejected: planner is not initialized");
            return Err(JaymiError::new("planner is not initialized"));
        }

        self.handle_count.fetch_add(1, Ordering::Relaxed);

        jaymi_logging::info(
            "planner",
            format!(
                "request received content={:?} directory={:?} file={:?}",
                truncate_for_log(&request.content),
                request
                    .directory
                    .as_ref()
                    .map(|path| path.display().to_string()),
                request.file.as_ref().map(|path| path.display().to_string())
            ),
        );

        // Context Engine is the sole assembler of request context.
        let context = self.context.assemble(&request)?;
        if !context.promotion_suggestions().is_empty() {
            jaymi_logging::info(
                "planner",
                format!(
                    "promotion suggestions={} ask={:?}",
                    context.promotion_suggestions().len(),
                    context.promotion_ask()
                ),
            );
        }

        let intent = self.decision.determine_intent(&request);

        // Workspace session intents answer after Context assemble (no tool).
        match &intent {
            Intent::ContinueProject { name } => {
                let response = self.handle_continue_project(name)?;
                return Ok(finalize(response, context.clone()));
            }
            Intent::OpenProject { project_id } => {
                let response = self.handle_open_project_id(project_id)?;
                return Ok(finalize(response, context.clone()));
            }
            Intent::CloseProject => {
                let response = self.handle_close_project()?;
                return Ok(finalize(response, context.clone()));
            }
            _ => {}
        }

        let Some(capability) = self.decision.required_capability(&intent) else {
            jaymi_logging::warn(
                "planner",
                "unsupported request; no capability mapped for intent",
            );
            return Ok(finalize(
                PlannerResponse {
                    content: "Unsupported request. Try: list <directory>, read <file>, search <query>, index <path>, or ask what files exist".to_string(),
                    ..PlannerResponse::default()
                },
                context,
            ));
        };

        jaymi_logging::info(
            "planner",
            format!("intent resolved capability={}", capability.id()),
        );

        // Planning answers "what would this take" without needing the
        // capability to be fulfillable today.
        if let Intent::PlanWork { capabilities, goal } = &intent {
            let mut response = self.handle_plan_work(capabilities, goal)?;
            if response.project_context.is_none() {
                response.project_context = context.project().cloned();
            }
            return Ok(finalize(response, context.clone()));
        }

        let availability = self.capabilities.validate(capability);
        if !availability.is_executable_tier() {
            let message = format!(
                "capability {} is not executable (availability={})",
                capability.id(),
                availability.as_str()
            );
            jaymi_logging::error("planner", &message);
            return Err(JaymiError::new(message));
        }

        // Reasoning Engine is intentionally unused for these deterministic paths.
        let _ = &self.reasoning;

        let result = match intent {
            Intent::ListDirectory { path } => {
                self.handle_list_directory(capability, self.resolve_workspace_path(path))
            }
            Intent::ListProjectTree { path } => {
                self.handle_list_project_tree(capability, self.resolve_workspace_path(path))
            }
            Intent::ReadFile { path } => {
                self.handle_read_file(capability, self.resolve_workspace_path(path))
            }
            Intent::WriteFile { path, content } => {
                self.handle_write_file(capability, self.resolve_workspace_path(path), content)
            }
            Intent::ManagePath {
                command,
                path,
                destination,
            } => self.handle_manage_path(
                capability,
                command,
                self.resolve_workspace_path(path),
                destination.map(|path| self.resolve_workspace_path(path)),
            ),
            Intent::RunTerminal {
                operation,
                session_id,
                cwd,
                command,
                title,
            } => self.handle_run_terminal(
                capability,
                operation,
                session_id,
                self.resolve_workspace_path(cwd),
                command,
                title,
            ),
            Intent::Git {
                repo_root,
                operation,
                paths,
                message,
            } => self.handle_git(
                capability,
                self.resolve_workspace_path(repo_root),
                operation,
                paths,
                message,
            ),
            Intent::Lsp { request } => {
                let mut request = request;
                request.workspace_root = self.resolve_workspace_path(request.workspace_root);
                if let Some(path) = request.path {
                    request.path = Some(self.resolve_workspace_path(path));
                }
                self.handle_lsp(capability, request)
            }
            Intent::DiscoverInventory { kind } => self.handle_discover_inventory(capability, kind),
            Intent::SearchKnowledge { request } => {
                self.handle_search_knowledge(capability, self.scope_search_request(request))
            }
            Intent::SearchProjectKnowledge {
                project_id,
                text,
                limit,
            } => self.handle_search_project_knowledge(capability, &project_id, &text, limit),
            Intent::IndexRoots { path } => self.handle_index_roots(capability, path),
            Intent::ContinueProject { .. }
            | Intent::OpenProject { .. }
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
                    response.project_context = context.project().cloned();
                }
                Ok(finalize(response, context.clone()))
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
            Err(error) => {
                jaymi_logging::error("planner", format!("request failed: {}", error.message()))
            }
        }

        result
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
            PermissionCategory::Filesystem,
            PermissionAction::Read,
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

    fn handle_list_project_tree(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::list_directory(path.clone());
        let prepared = self.prepare_execution(
            capability,
            Some(LIST_PROJECT_TREE_TOOL_ID),
            &input,
            &path,
            "List project tree",
            PermissionCategory::Filesystem,
            PermissionAction::Read,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let listed_path = output.listed_path.clone().unwrap_or(path);
        let content = format!(
            "Listed project tree with {} entries under {} via {} → {} → {}",
            output.entries.len(),
            listed_path.display(),
            capability.id(),
            tool_id,
            provider_id.as_deref().unwrap_or("unknown")
        );

        Ok(PlannerResponse {
            content,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(listed_path),
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
            PermissionCategory::Filesystem,
            PermissionAction::Read,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let document = output
            .document
            .ok_or_else(|| JaymiError::new("read tool succeeded without returning a document"))?;
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

    fn handle_write_file(
        &self,
        capability: Capability,
        path: std::path::PathBuf,
        content: String,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::write_file(path.clone(), content);
        let prepared = self.prepare_execution(
            capability,
            Some(WRITE_FILE_TOOL_ID),
            &input,
            &path,
            "Write file",
            PermissionCategory::Filesystem,
            PermissionAction::Write,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;
        self.context.invalidate_cache("files_changed");

        let provider_id = prepared.provider_id.clone();
        let summary = output.message.unwrap_or_else(|| {
            format!(
                "Wrote {} via {} → {} → {}",
                path.display(),
                capability.id(),
                tool_id,
                provider_id.as_deref().unwrap_or("unknown")
            )
        });

        Ok(PlannerResponse {
            content: summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(path),
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_manage_path(
        &self,
        capability: Capability,
        command: String,
        path: std::path::PathBuf,
        destination: Option<std::path::PathBuf>,
    ) -> JaymiResult<PlannerResponse> {
        let content = destination
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let input = ToolInput::manage_path(command.clone(), path.clone(), content);
        let action = if command == "delete" {
            PermissionAction::Delete
        } else {
            PermissionAction::Write
        };
        let prepared = self.prepare_execution(
            capability,
            Some(MANAGE_PATH_TOOL_ID),
            &input,
            &path,
            "Manage path",
            PermissionCategory::Filesystem,
            action,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;
        self.context.invalidate_cache("files_changed");

        let provider_id = prepared.provider_id.clone();
        let listed = output
            .listed_path
            .clone()
            .or(destination)
            .or(Some(path.clone()));
        let summary = output.message.unwrap_or_else(|| {
            format!(
                "Managed path {} ({}) via {} → {} → {}",
                path.display(),
                command,
                capability.id(),
                tool_id,
                provider_id.as_deref().unwrap_or("unknown")
            )
        });

        Ok(PlannerResponse {
            content: summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: listed,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_run_terminal(
        &self,
        capability: Capability,
        operation: TerminalOperation,
        session_id: String,
        cwd: std::path::PathBuf,
        command: Option<String>,
        title: Option<String>,
    ) -> JaymiResult<PlannerResponse> {
        let input = match operation {
            TerminalOperation::Ensure => {
                ToolInput::ensure_terminal(session_id.clone(), cwd.clone())
            }
            TerminalOperation::Run => {
                let command = command
                    .clone()
                    .ok_or_else(|| JaymiError::new("terminal run requires a command"))?;
                ToolInput::run_terminal(session_id.clone(), cwd.clone(), command)
            }
            TerminalOperation::Create => ToolInput::create_terminal(cwd.clone(), title.clone()),
            TerminalOperation::Rename => {
                let title = title
                    .clone()
                    .ok_or_else(|| JaymiError::new("terminal rename requires a title"))?;
                ToolInput::rename_terminal(session_id.clone(), cwd.clone(), title)
            }
            TerminalOperation::Kill => ToolInput::kill_terminal(session_id.clone(), cwd.clone()),
        };
        let prepared = self.prepare_execution(
            capability,
            Some(TERMINAL_TOOL_ID),
            &input,
            &cwd,
            "Execute terminal command",
            PermissionCategory::Terminal,
            PermissionAction::Execute,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let summary = output.message.clone().unwrap_or_else(|| {
            format!(
                "Terminal session {} via {} → {} → {}",
                session_id,
                capability.id(),
                tool_id,
                provider_id.as_deref().unwrap_or("unknown")
            )
        });

        Ok(PlannerResponse {
            content: summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(cwd),
            terminal_session_id: output.session_id.or(Some(session_id)),
            terminal_output: output.terminal_output,
            terminal_scrollback: output.terminal_scrollback,
            terminal_history: output.terminal_history,
            terminal_title: output.terminal_title,
            terminal_alive: output.terminal_alive,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_git(
        &self,
        capability: Capability,
        repo_root: std::path::PathBuf,
        operation: GitOperation,
        paths: Vec<std::path::PathBuf>,
        message: Option<String>,
    ) -> JaymiResult<PlannerResponse> {
        let input = ToolInput::git(repo_root.clone(), operation, paths, message);
        let permission_action = if operation.is_mutating() {
            PermissionAction::Write
        } else {
            PermissionAction::Read
        };
        let prepared = self.prepare_execution(
            capability,
            Some(GIT_TOOL_ID),
            &input,
            &repo_root,
            &format!("Git {}", operation.as_str()),
            PermissionCategory::Filesystem,
            permission_action,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let summary = output.message.clone().unwrap_or_else(|| {
            format!(
                "Git {} via {} → {} → {}",
                operation.as_str(),
                capability.id(),
                tool_id,
                provider_id.as_deref().unwrap_or("unknown")
            )
        });

        Ok(PlannerResponse {
            content: summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(repo_root),
            git_branch: output.git_branch,
            git_summary: output.git_summary,
            git_is_repository: output.git_is_repository,
            git_modified: output.git_modified,
            git_added: output.git_added,
            git_deleted: output.git_deleted,
            git_staged: output.git_staged,
            git_untracked: output.git_untracked,
            policy_evaluation: prepared.policy_evaluation,
            permission_result: prepared.permission_result,
            ..PlannerResponse::default()
        })
    }

    fn handle_lsp(
        &self,
        capability: Capability,
        request: LspRequest,
    ) -> JaymiResult<PlannerResponse> {
        let workspace_root = request.workspace_root.clone();
        let operation = request.operation;
        let input = ToolInput::lsp(request);
        let permission_action = if operation.is_mutating() {
            PermissionAction::Write
        } else {
            PermissionAction::Read
        };
        let prepared = self.prepare_execution(
            capability,
            Some(LANGUAGE_SERVER_TOOL_ID),
            &input,
            &workspace_root,
            &format!("LSP {}", operation.as_str()),
            PermissionCategory::Filesystem,
            permission_action,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;

        let provider_id = prepared.provider_id.clone();
        let summary = output.message.clone().unwrap_or_else(|| {
            format!(
                "LSP {} via {} → {} → {}",
                operation.as_str(),
                capability.id(),
                tool_id,
                provider_id.as_deref().unwrap_or("unknown")
            )
        });

        Ok(PlannerResponse {
            content: summary,
            capability: Some(capability),
            tool_id: Some(tool_id),
            provider_id,
            listed_path: Some(workspace_root),
            lsp_hover: output.lsp_hover,
            lsp_completions: output.lsp_completions,
            lsp_diagnostics: output.lsp_diagnostics,
            lsp_definitions: output.lsp_definitions,
            lsp_references: output.lsp_references,
            lsp_edits: output.lsp_edits,
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
            PermissionCategory::Filesystem,
            PermissionAction::Read,
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
            PermissionCategory::Filesystem,
            PermissionAction::Read,
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
            ..ToolInput::default()
        };
        let prepared = self.prepare_execution(
            capability,
            Some(SCAN_FILESYSTEM_TOOL_ID),
            &input,
            &resource_path,
            "Index filesystem",
            PermissionCategory::Filesystem,
            PermissionAction::Read,
        )?;
        if let Some(blocked) = prepared.blocked_response {
            return Ok(blocked);
        }

        let tool_id = prepared.tool_id.clone();
        let output = self.orchestrator.execute(&tool_id, input)?;
        self.ensure_success(&output)?;
        self.context.invalidate_cache("search_index_updated");

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
    #[allow(clippy::too_many_arguments)]
    fn prepare_execution(
        &self,
        capability: Capability,
        preferred_tool_id: Option<&str>,
        _input: &ToolInput,
        path: &Path,
        action_label: &str,
        permission_category: PermissionCategory,
        permission_action: PermissionAction,
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
            category: permission_category,
            action: permission_action,
            scope: PermissionScope::Once,
            explanation: format!("{action_label} at {}", path.display()),
            resource: Some(path.display().to_string()),
        };
        jaymi_logging::info(
            "planner",
            format!(
                "permission check category={} action={} resource={}",
                permission_category_label(permission_category),
                permission_action_label(permission_action),
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

fn finalize(mut response: PlannerResponse, bundle: ContextBundle) -> PlannerResponse {
    response.promotion_suggestions = bundle.promotion_suggestions().to_vec();
    response.promotion_ask = bundle.promotion_ask();
    response.memory_context = Some(bundle.memory().clone());
    if response.project_context.is_none() {
        response.project_context = bundle.project().cloned();
    }
    response.context_bundle = Some(bundle);
    response
}

fn permission_category_label(category: PermissionCategory) -> &'static str {
    match category {
        PermissionCategory::Filesystem => "filesystem",
        PermissionCategory::Internet => "internet",
        PermissionCategory::Terminal => "terminal",
        PermissionCategory::Communication => "communication",
        PermissionCategory::System => "system",
        PermissionCategory::AiProviders => "ai_providers",
    }
}

fn permission_action_label(action: PermissionAction) -> &'static str {
    match action {
        PermissionAction::Read => "read",
        PermissionAction::Write => "write",
        PermissionAction::Execute => "execute",
        PermissionAction::Delete => "delete",
        PermissionAction::Network => "network",
        PermissionAction::Import => "import",
        PermissionAction::Export => "export",
    }
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
        EstimatedRuntime, ExecutionMode, GpuRequirements, MemoryUsage, ReadFileTool, Reliability,
        ResourceCost, ResultType, SearchFilesTool, Tool, ToolMetadata, ToolOutput,
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
        F: FnOnce(&mut ToolRegistry, Arc<FilesystemProvider>, Arc<ContentIntelligenceApi>),
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
            Arc::clone(&knowledge),
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

        let memory = test_memory_engine();
        let projects = test_project_engine();
        let mut search = jaymi_search::SearchEngine::new(Arc::clone(&knowledge), None);
        search.initialize().unwrap();
        let mut context = jaymi_context::ContextEngine::new();
        context.initialize().unwrap();
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
            tools,
            orchestrator,
            policies: Arc::new(policies),
            permissions: Arc::new(permissions),
            memory,
            projects,
            context: Arc::new(context),
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
        assert!(planner
            .discover_capabilities()
            .contains(&Capability::Search));
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
        let response = planner.handle(UserRequest::list_directory(&dir)).unwrap();

        assert_eq!(response.capability, Some(Capability::Search));
        assert_eq!(response.tool_id.as_deref(), Some("search_files"));
        assert_eq!(
            response.provider_id.as_deref(),
            Some(FILESYSTEM_PROVIDER_ID)
        );
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
