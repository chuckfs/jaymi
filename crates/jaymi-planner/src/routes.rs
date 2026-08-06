//! Built-in Intent → Tool route handlers.
//!
//! Adding a shipping tool-backed path: implement [`IntentToolHandler`], add it to
//! [`register_builtin_routes`], register the tool at boot, and map Intent →
//! Capability in the Decision Engine. No new `Planner::handle` match arm.

use std::path::PathBuf;

use jaymi_capabilities::Capability;
use jaymi_core::{
    DeletionMethod, GitOperation, IntentId, JaymiError, JaymiResult, TerminalOperation,
};
use jaymi_permissions::{PermissionAction, PermissionCategory};
use jaymi_tools::{
    ToolInput, GIT_TOOL_ID, LANGUAGE_SERVER_TOOL_ID, LIST_PROJECT_TREE_TOOL_ID, MANAGE_PATH_TOOL_ID,
    QUERY_INVENTORY_TOOL_ID, READ_FILE_TOOL_ID, SCAN_FILESYSTEM_TOOL_ID, SEARCH_FILES_TOOL_ID,
    SEARCH_KNOWLEDGE_TOOL_ID, SEARCH_PROJECT_KNOWLEDGE_TOOL_ID, TERMINAL_TOOL_ID, WRITE_FILE_TOOL_ID,
};

use crate::decision::Intent;
use crate::dispatch::{
    response_from_meta, DispatchSupport, ExecutionMeta, IntentToolHandler, PreparedToolCall,
    ToolRoute, ToolRouteTable,
};

/// Register every shipping tool-backed route (compile-time list).
pub fn register_builtin_routes(table: &mut ToolRouteTable) {
    table.register_handler(ListDirectoryRoute);
    table.register_handler(ListProjectTreeRoute);
    table.register_handler(ReadFileRoute);
    table.register_handler(WriteFileRoute);
    table.register_handler(ManagePathRoute);
    table.register_handler(RunTerminalRoute);
    table.register_handler(GitRoute);
    table.register_handler(LspRoute);
    table.register_handler(DiscoverInventoryRoute);
    table.register_handler(SearchKnowledgeRoute);
    table.register_handler(SearchProjectKnowledgeRoute);
    table.register_handler(IndexRootsRoute);
}

fn expect_intent<'a, F, T>(intent: &'a Intent, map: F, label: &str) -> JaymiResult<T>
where
    F: FnOnce(&'a Intent) -> Option<T>,
{
    map(intent).ok_or_else(|| {
        JaymiError::new(format!(
            "route {label} received mismatched intent {}",
            intent.id().as_str()
        ))
    })
}

struct ListDirectoryRoute;
impl IntentToolHandler for ListDirectoryRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::ListDirectory,
            capability: Capability::Search,
            tool_id: SEARCH_FILES_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let path = expect_intent(
            intent,
            |intent| match intent {
                Intent::ListDirectory { path } => Some(host.resolve_workspace_path(path.clone())),
                _ => None,
            },
            "list_directory",
        )?;
        Ok(PreparedToolCall {
            input: ToolInput::list_directory(path.clone()),
            resource_path: path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "List directory".into(),
            action_label: "List directory".into(),
            expected_outputs: vec!["directory listing".into()],
            invalidate_cache: None,
            soft_failure: true,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let content = format!(
            "Listed {} entries in {} via {} → {} → {}",
            output.entries.len(),
            call.resource_path.display(),
            meta.capability.id(),
            meta.tool_id,
            meta.provider_id.as_deref().unwrap_or("unknown")
        );
        let mut response = response_from_meta(&meta, content);
        response.listed_path = Some(call.resource_path.clone());
        response.entries = output.entries;
        response.citations = output.citations;
        Ok(response)
    }
}

// Alias so respond signature can use PlannerResponse without importing in every
// handler — use crate::PlannerResponse via type alias for readability.
type PlannerResponseParts = crate::PlannerResponse;

struct ListProjectTreeRoute;
impl IntentToolHandler for ListProjectTreeRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::ListProjectTree,
            capability: Capability::Search,
            tool_id: LIST_PROJECT_TREE_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let path = expect_intent(
            intent,
            |intent| match intent {
                Intent::ListProjectTree { path } => Some(host.resolve_workspace_path(path.clone())),
                _ => None,
            },
            "list_project_tree",
        )?;
        Ok(PreparedToolCall {
            input: ToolInput::list_directory(path.clone()),
            resource_path: path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "List project tree".into(),
            action_label: "List project tree".into(),
            expected_outputs: vec!["project tree listing".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let listed_path = output
            .listed_path
            .clone()
            .unwrap_or_else(|| call.resource_path.clone());
        let content = format!(
            "Listed project tree with {} entries under {} via {} → {} → {}",
            output.entries.len(),
            listed_path.display(),
            meta.capability.id(),
            meta.tool_id,
            meta.provider_id.as_deref().unwrap_or("unknown")
        );
        let mut response = response_from_meta(&meta, content);
        response.listed_path = Some(listed_path);
        response.entries = output.entries;
        response.citations = output.citations;
        Ok(response)
    }
}

struct ReadFileRoute;
impl IntentToolHandler for ReadFileRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::ReadFile,
            capability: Capability::ReadDocuments,
            tool_id: READ_FILE_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let path = expect_intent(
            intent,
            |intent| match intent {
                Intent::ReadFile { path } => Some(host.resolve_workspace_path(path.clone())),
                _ => None,
            },
            "read_file",
        )?;
        Ok(PreparedToolCall {
            input: ToolInput::read_file(path.clone()),
            resource_path: path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "Read file".into(),
            action_label: "Read file".into(),
            expected_outputs: vec!["unified document".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let document = output
            .document
            .ok_or_else(|| JaymiError::new("read tool succeeded without returning a document"))?;
        let content = format!(
            "Read {} ({}) via {} → {} → {} → {}",
            call.resource_path.display(),
            document.file_type,
            meta.capability.id(),
            meta.tool_id,
            meta.provider_id.as_deref().unwrap_or("unknown"),
            document.parser_id
        );
        let mut response = response_from_meta(&meta, content);
        response.document = Some(document);
        Ok(response)
    }
}

struct WriteFileRoute;
impl IntentToolHandler for WriteFileRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            tool_id: WRITE_FILE_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let (path, content) = expect_intent(
            intent,
            |intent| match intent {
                Intent::WriteFile { path, content } => {
                    Some((host.resolve_workspace_path(path.clone()), content.clone()))
                }
                _ => None,
            },
            "write_file",
        )?;
        Ok(PreparedToolCall {
            input: ToolInput::write_file(path.clone(), content),
            resource_path: path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Write,
            originating_request: "Write file".into(),
            action_label: "Write file".into(),
            expected_outputs: vec!["written file".into()],
            invalidate_cache: Some("files_changed"),
            soft_failure: true,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let summary = output.message.unwrap_or_else(|| {
            format!(
                "Wrote {} via {} → {} → {}",
                call.resource_path.display(),
                meta.capability.id(),
                meta.tool_id,
                meta.provider_id.as_deref().unwrap_or("unknown")
            )
        });
        let mut response = response_from_meta(&meta, summary);
        response.listed_path = Some(call.resource_path.clone());
        Ok(response)
    }
}

struct ManagePathRoute;
impl IntentToolHandler for ManagePathRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::ManagePath,
            capability: Capability::FileManagement,
            tool_id: MANAGE_PATH_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let (command, path, destination, requested_method) = expect_intent(
            intent,
            |intent| match intent {
                Intent::ManagePath {
                    command,
                    path,
                    destination,
                    deletion_method,
                } => Some((
                    command.clone(),
                    host.resolve_workspace_path(path.clone()),
                    destination
                        .as_ref()
                        .map(|path| host.resolve_workspace_path(path.clone())),
                    *deletion_method,
                )),
                _ => None,
            },
            "manage_path",
        )?;
        let content = destination
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let action = if command == "delete" {
            PermissionAction::Delete
        } else {
            PermissionAction::Write
        };
        let path_label = path.display().to_string();
        let deletion_method = if command == "delete" {
            Some(host.resolve_deletion_method(requested_method, request_text)?)
        } else {
            None
        };
        let mut input = ToolInput::manage_path(command.clone(), path.clone(), content);
        input.deletion_method = deletion_method;

        let (originating_request, action_label, expected_outputs) =
            match (command.as_str(), deletion_method) {
                ("delete", Some(DeletionMethod::Trash)) => (
                    format!("Delete {path_label}"),
                    format!("Move {path_label} to Trash"),
                    vec![
                        format!("Delete {path_label}"),
                        "Deletion Method: Trash".into(),
                        "Move the selected files to the Trash".into(),
                        "Update the project index afterward".into(),
                    ],
                ),
                ("delete", Some(DeletionMethod::Permanent)) => (
                    format!("Permanently delete {path_label}"),
                    format!("Permanently delete {path_label}"),
                    vec![
                        format!("Permanently delete {path_label}"),
                        "Deletion Method: Permanent".into(),
                        "Remove the folder or file with no Trash recovery".into(),
                        "Update the project index afterward".into(),
                    ],
                ),
                ("delete", None) => (
                    format!("Delete {path_label}"),
                    format!("Delete {path_label}"),
                    vec![format!("Delete {path_label}")],
                ),
                ("rename", _) => (
                    format!(
                        "Rename {path_label}{}",
                        destination
                            .as_ref()
                            .map(|to| format!(" → {}", to.display()))
                            .unwrap_or_default()
                    ),
                    format!("Rename {path_label}"),
                    vec![format!("Renamed {path_label}")],
                ),
                ("mkdir", _) => (
                    format!("Create directory {path_label}"),
                    format!("Create directory {path_label}"),
                    vec![format!("Created {path_label}")],
                ),
                _ => (
                    format!("Manage path {path_label}"),
                    format!("Manage path {path_label}"),
                    vec!["managed path".into()],
                ),
            };

        Ok(PreparedToolCall {
            input,
            resource_path: path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: action,
            originating_request,
            action_label,
            expected_outputs,
            invalidate_cache: Some("files_changed"),
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let command = call.input.command.clone().unwrap_or_default();
        let destination = call
            .input
            .content
            .as_ref()
            .map(PathBuf::from);
        let listed = output
            .listed_path
            .clone()
            .or(destination)
            .or_else(|| Some(call.resource_path.clone()));
        let summary = output.message.unwrap_or_else(|| {
            format!(
                "Managed path {} ({}) via {} → {} → {}",
                call.resource_path.display(),
                command,
                meta.capability.id(),
                meta.tool_id,
                meta.provider_id.as_deref().unwrap_or("unknown")
            )
        });
        let mut response = response_from_meta(&meta, summary);
        response.listed_path = listed;
        Ok(response)
    }
}

struct RunTerminalRoute;
impl IntentToolHandler for RunTerminalRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::RunTerminal,
            capability: Capability::ExecuteTerminalCommands,
            tool_id: TERMINAL_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let (operation, session_id, cwd, command, title) = expect_intent(
            intent,
            |intent| match intent {
                Intent::RunTerminal {
                    operation,
                    session_id,
                    cwd,
                    command,
                    title,
                } => Some((
                    *operation,
                    session_id.clone(),
                    host.resolve_workspace_path(cwd.clone()),
                    command.clone(),
                    title.clone(),
                )),
                _ => None,
            },
            "run_terminal",
        )?;
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
        Ok(PreparedToolCall {
            input,
            resource_path: cwd,
            permission_category: PermissionCategory::Terminal,
            permission_action: PermissionAction::Execute,
            originating_request: "Execute terminal command".into(),
            action_label: "Execute terminal command".into(),
            expected_outputs: vec!["terminal output".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let session_id = call
            .input
            .session_id
            .clone()
            .unwrap_or_else(|| output.session_id.clone().unwrap_or_default());
        let summary = output.message.clone().unwrap_or_else(|| {
            format!(
                "Terminal session {} via {} → {} → {}",
                session_id,
                meta.capability.id(),
                meta.tool_id,
                meta.provider_id.as_deref().unwrap_or("unknown")
            )
        });
        let mut response = response_from_meta(&meta, summary);
        response.listed_path = Some(call.resource_path.clone());
        response.terminal_session_id = output.session_id.or(Some(session_id));
        response.terminal_output = output.terminal_output;
        response.terminal_scrollback = output.terminal_scrollback;
        response.terminal_history = output.terminal_history;
        response.terminal_title = output.terminal_title;
        response.terminal_alive = output.terminal_alive;
        Ok(response)
    }
}

struct GitRoute;
impl IntentToolHandler for GitRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::Git,
            capability: Capability::Code,
            tool_id: GIT_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let (repo_root, operation, paths, message) = expect_intent(
            intent,
            |intent| match intent {
                Intent::Git {
                    repo_root,
                    operation,
                    paths,
                    message,
                } => Some((
                    host.resolve_workspace_path(repo_root.clone()),
                    *operation,
                    paths.clone(),
                    message.clone(),
                )),
                _ => None,
            },
            "git",
        )?;
        let permission_action = if operation.is_mutating() {
            PermissionAction::Write
        } else {
            PermissionAction::Read
        };
        let action_label = format!("Git {}", operation.as_str());
        Ok(PreparedToolCall {
            input: ToolInput::git(repo_root.clone(), operation, paths, message),
            resource_path: repo_root,
            permission_category: PermissionCategory::Filesystem,
            permission_action,
            originating_request: action_label.clone(),
            action_label,
            expected_outputs: vec!["git result".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let operation = call
            .input
            .git_operation
            .unwrap_or(GitOperation::Status)
            .as_str();
        let summary = output.message.clone().unwrap_or_else(|| {
            format!(
                "Git {} via {} → {} → {}",
                operation,
                meta.capability.id(),
                meta.tool_id,
                meta.provider_id.as_deref().unwrap_or("unknown")
            )
        });
        let mut response = response_from_meta(&meta, summary);
        response.listed_path = Some(call.resource_path.clone());
        response.git_branch = output.git_branch;
        response.git_summary = output.git_summary;
        response.git_is_repository = output.git_is_repository;
        response.git_modified = output.git_modified;
        response.git_added = output.git_added;
        response.git_deleted = output.git_deleted;
        response.git_staged = output.git_staged;
        response.git_untracked = output.git_untracked;
        Ok(response)
    }
}

struct LspRoute;
impl IntentToolHandler for LspRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::Lsp,
            capability: Capability::Code,
            tool_id: LANGUAGE_SERVER_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let mut request = expect_intent(
            intent,
            |intent| match intent {
                Intent::Lsp { request } => Some(request.clone()),
                _ => None,
            },
            "lsp",
        )?;
        request.workspace_root = host.resolve_workspace_path(request.workspace_root);
        if let Some(path) = request.path.take() {
            request.path = Some(host.resolve_workspace_path(path));
        }
        let workspace_root = request.workspace_root.clone();
        let operation = request.operation;
        let permission_action = if operation.is_mutating() {
            PermissionAction::Write
        } else {
            PermissionAction::Read
        };
        let action_label = format!("LSP {}", operation.as_str());
        Ok(PreparedToolCall {
            input: ToolInput::lsp(request),
            resource_path: workspace_root,
            permission_category: PermissionCategory::Filesystem,
            permission_action,
            originating_request: action_label.clone(),
            action_label,
            expected_outputs: vec!["lsp result".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let operation = call
            .input
            .lsp
            .as_ref()
            .map(|request| request.operation.as_str())
            .unwrap_or("lsp");
        let summary = output.message.clone().unwrap_or_else(|| {
            format!(
                "LSP {} via {} → {} → {}",
                operation,
                meta.capability.id(),
                meta.tool_id,
                meta.provider_id.as_deref().unwrap_or("unknown")
            )
        });
        let mut response = response_from_meta(&meta, summary);
        response.listed_path = Some(call.resource_path.clone());
        response.lsp_hover = output.lsp_hover;
        response.lsp_completions = output.lsp_completions;
        response.lsp_diagnostics = output.lsp_diagnostics;
        response.lsp_definitions = output.lsp_definitions;
        response.lsp_references = output.lsp_references;
        response.lsp_edits = output.lsp_edits;
        Ok(response)
    }
}

struct DiscoverInventoryRoute;
impl IntentToolHandler for DiscoverInventoryRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::DiscoverInventory,
            capability: Capability::Discover,
            tool_id: QUERY_INVENTORY_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        _host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let kind = expect_intent(
            intent,
            |intent| match intent {
                Intent::DiscoverInventory { kind } => Some(kind.clone()),
                _ => None,
            },
            "discover_inventory",
        )?;
        let listed_path = match &kind {
            jaymi_core::DiscoveryQueryKind::ByFolder { path, .. } => Some(path.clone()),
            _ => None,
        };
        let resource_path = listed_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("inventory"));
        Ok(PreparedToolCall {
            input: ToolInput::discover(kind),
            resource_path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "Query inventory".into(),
            action_label: "Query inventory".into(),
            expected_outputs: vec!["inventory entries".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let listed_path = match &call.input.discovery {
            Some(jaymi_core::DiscoveryQueryKind::ByFolder { path, .. }) => Some(path.clone()),
            _ => None,
        };
        let content = output.message.unwrap_or_else(|| {
            format!(
                "Found {} inventoried entries via {} → {} (search engine)",
                output.entries.len(),
                meta.capability.id(),
                meta.tool_id
            )
        });
        let mut response = response_from_meta(&meta, content);
        response.listed_path = listed_path;
        response.entries = output.entries;
        response.citations = output.citations;
        Ok(response)
    }
}

struct SearchKnowledgeRoute;
impl IntentToolHandler for SearchKnowledgeRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::SearchKnowledge,
            capability: Capability::Search,
            tool_id: SEARCH_KNOWLEDGE_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let request = expect_intent(
            intent,
            |intent| match intent {
                Intent::SearchKnowledge { request } => {
                    Some(host.scope_search_request(request.clone()))
                }
                _ => None,
            },
            "search_knowledge",
        )?;
        let resource_path = request
            .folder
            .clone()
            .unwrap_or_else(|| PathBuf::from("search"));
        Ok(PreparedToolCall {
            input: ToolInput::search(request),
            resource_path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "Search knowledge".into(),
            action_label: "Search knowledge".into(),
            expected_outputs: vec!["search hits".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let content = output.message.unwrap_or_else(|| {
            format!(
                "Found {} search hits via {} → {}",
                output.entries.len(),
                meta.capability.id(),
                meta.tool_id
            )
        });
        let mut response = response_from_meta(&meta, content);
        response.listed_path = Some(call.resource_path.clone());
        response.entries = output.entries;
        response.citations = output.citations;
        Ok(response)
    }
}

struct SearchProjectKnowledgeRoute;
impl IntentToolHandler for SearchProjectKnowledgeRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::SearchProjectKnowledge,
            capability: Capability::Search,
            tool_id: SEARCH_PROJECT_KNOWLEDGE_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        _host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let (project_id, text, limit) = expect_intent(
            intent,
            |intent| match intent {
                Intent::SearchProjectKnowledge {
                    project_id,
                    text,
                    limit,
                } => Some((project_id.clone(), text.clone(), *limit)),
                _ => None,
            },
            "search_project_knowledge",
        )?;
        let request = jaymi_core::ProjectKnowledgeRequest {
            project_id: project_id.clone(),
            text,
            limit,
        };
        Ok(PreparedToolCall {
            input: ToolInput::project_knowledge(request),
            resource_path: PathBuf::from(format!("project:{project_id}")),
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "Search project knowledge".into(),
            action_label: "Search project knowledge".into(),
            expected_outputs: vec!["project knowledge hits".into()],
            invalidate_cache: None,
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        _call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let content = output.message.clone().unwrap_or_else(|| {
            format!(
                "Project knowledge search completed via {} → {} → {}",
                meta.capability.id(),
                meta.tool_id,
                meta.provider_id.as_deref().unwrap_or("unknown")
            )
        });
        let mut response = response_from_meta(&meta, content);
        response.project_knowledge = output.project_knowledge;
        Ok(response)
    }
}

struct IndexRootsRoute;
impl IntentToolHandler for IndexRootsRoute {
    fn route(&self) -> ToolRoute {
        ToolRoute {
            intent: IntentId::IndexRoots,
            capability: Capability::Index,
            tool_id: SCAN_FILESYSTEM_TOOL_ID,
        }
    }

    fn prepare(
        &self,
        intent: &Intent,
        _request_text: &str,
        _host: &dyn DispatchSupport,
    ) -> JaymiResult<PreparedToolCall> {
        let path = expect_intent(
            intent,
            |intent| match intent {
                Intent::IndexRoots { path } => Some(path.clone()),
                _ => None,
            },
            "index_roots",
        )?;
        let resource_path = path
            .clone()
            .unwrap_or_else(|| PathBuf::from("configured-roots"));
        Ok(PreparedToolCall {
            input: ToolInput {
                path: path.clone(),
                ..ToolInput::default()
            },
            resource_path,
            permission_category: PermissionCategory::Filesystem,
            permission_action: PermissionAction::Read,
            originating_request: "Index filesystem".into(),
            action_label: "Index filesystem".into(),
            expected_outputs: vec!["index summary".into()],
            invalidate_cache: Some("search_index_updated"),
            soft_failure: false,
        })
    }

    fn respond(
        &self,
        call: &PreparedToolCall,
        output: jaymi_tools::ToolOutput,
        meta: ExecutionMeta,
    ) -> JaymiResult<PlannerResponseParts> {
        let content = output
            .message
            .unwrap_or_else(|| format!("Indexed filesystem via {}", meta.tool_id));
        let mut response = response_from_meta(&meta, content);
        response.listed_path = call.input.path.clone();
        Ok(response)
    }
}
