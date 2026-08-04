//! Tool interface and structured I/O.

use crate::metadata::ToolMetadata;
use jaymi_core::{
    Citation, DiscoveryQueryKind, Document, FileEntry, GitOperation, GitPathStatus, JaymiResult,
    LspCompletionItem, LspDiagnostic, LspHover, LspLocation, LspRequest, LspTextEdit,
    ProjectKnowledgeRequest, SearchRequest, TerminalOperation,
};
use jaymi_project_engine::ProjectKnowledgeHit;
use jaymi_providers::LspOperationResult;

/// Structured input supplied by the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolInput {
    /// Directory or file path for filesystem tools.
    pub path: Option<std::path::PathBuf>,
    /// Text content for write tools / commit messages.
    pub content: Option<String>,
    /// Structured discovery query for inventory tools.
    pub discovery: Option<DiscoveryQueryKind>,
    /// Structured Search Engine request.
    pub search: Option<SearchRequest>,
    /// Structured project-knowledge search request.
    pub project_knowledge: Option<ProjectKnowledgeRequest>,
    /// Terminal session id for PTY tools.
    pub session_id: Option<String>,
    /// Shell command for terminal tools (`None` = ensure/spawn only).
    pub command: Option<String>,
    /// Terminal operation to perform (`None` defaults to Run when `command`
    /// is set, otherwise Ensure).
    pub terminal_operation: Option<TerminalOperation>,
    /// Display title for terminal Create / Rename operations.
    pub title: Option<String>,
    /// Git operation for the Git tool.
    pub git_operation: Option<GitOperation>,
    /// Paths for Git stage / unstage / discard.
    pub paths: Vec<std::path::PathBuf>,
    /// Structured Language Server request.
    pub lsp: Option<LspRequest>,
}

impl ToolInput {
    /// Create input for a single-directory listing operation.
    pub fn list_directory(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            ..Self::default()
        }
    }

    /// Create input for a single-file read operation.
    pub fn read_file(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            ..Self::default()
        }
    }

    /// Create input for a single-file write operation.
    pub fn write_file(path: impl Into<std::path::PathBuf>, content: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            content: Some(content.into()),
            ..Self::default()
        }
    }

    /// Create input for mkdir / rename / delete path management.
    ///
    /// `command` is `mkdir`, `rename`, or `delete`. For rename, `content` is the
    /// destination path string.
    pub fn manage_path(
        command: impl Into<String>,
        path: impl Into<std::path::PathBuf>,
        content: Option<impl Into<String>>,
    ) -> Self {
        Self {
            path: Some(path.into()),
            command: Some(command.into()),
            content: content.map(Into::into),
            ..Self::default()
        }
    }

    /// Create input for a discovery inventory query.
    pub fn discover(kind: DiscoveryQueryKind) -> Self {
        let path = match &kind {
            DiscoveryQueryKind::ByFolder { path, .. } => Some(path.clone()),
            _ => None,
        };
        Self {
            path,
            discovery: Some(kind),
            ..Self::default()
        }
    }

    /// Create input for a Search Engine request.
    pub fn search(request: SearchRequest) -> Self {
        Self {
            path: request.folder.clone(),
            search: Some(request),
            ..Self::default()
        }
    }

    /// Create input for a project-knowledge search.
    pub fn project_knowledge(request: ProjectKnowledgeRequest) -> Self {
        Self {
            project_knowledge: Some(request),
            ..Self::default()
        }
    }

    /// Create input to ensure a terminal session exists.
    pub fn ensure_terminal(
        session_id: impl Into<String>,
        cwd: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            path: Some(cwd.into()),
            session_id: Some(session_id.into()),
            command: None,
            terminal_operation: Some(TerminalOperation::Ensure),
            ..Self::default()
        }
    }

    /// Create input to run a command in a terminal session.
    pub fn run_terminal(
        session_id: impl Into<String>,
        cwd: impl Into<std::path::PathBuf>,
        command: impl Into<String>,
    ) -> Self {
        Self {
            path: Some(cwd.into()),
            session_id: Some(session_id.into()),
            command: Some(command.into()),
            terminal_operation: Some(TerminalOperation::Run),
            ..Self::default()
        }
    }

    /// Create input to spawn a new terminal session (cwd is the project root).
    pub fn create_terminal(
        cwd: impl Into<std::path::PathBuf>,
        title: Option<String>,
    ) -> Self {
        Self {
            path: Some(cwd.into()),
            session_id: None,
            command: None,
            terminal_operation: Some(TerminalOperation::Create),
            title,
            ..Self::default()
        }
    }

    /// Create input to rename a terminal session's display title.
    pub fn rename_terminal(
        session_id: impl Into<String>,
        cwd: impl Into<std::path::PathBuf>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            path: Some(cwd.into()),
            session_id: Some(session_id.into()),
            command: None,
            terminal_operation: Some(TerminalOperation::Rename),
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// Create input to kill / close a terminal session.
    pub fn kill_terminal(
        session_id: impl Into<String>,
        cwd: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            path: Some(cwd.into()),
            session_id: Some(session_id.into()),
            command: None,
            terminal_operation: Some(TerminalOperation::Kill),
            ..Self::default()
        }
    }

    /// Create input for a Git operation.
    pub fn git(
        repo_root: impl Into<std::path::PathBuf>,
        operation: GitOperation,
        paths: Vec<std::path::PathBuf>,
        message: Option<String>,
    ) -> Self {
        Self {
            path: Some(repo_root.into()),
            content: message,
            git_operation: Some(operation),
            paths,
            ..Self::default()
        }
    }

    /// Create input for a Language Server operation.
    pub fn lsp(request: LspRequest) -> Self {
        Self {
            path: Some(request.workspace_root.clone()),
            lsp: Some(request),
            ..Self::default()
        }
    }
}

/// Structured result returned to the Planner.
#[derive(Debug, Default, Clone)]
pub struct ToolOutput {
    /// Whether the tool completed successfully.
    pub success: bool,
    /// Directory listing entries when applicable.
    pub entries: Vec<FileEntry>,
    /// Explainable citations for search / inventory hits.
    pub citations: Vec<Citation>,
    /// Unified document produced by the Read pipeline.
    pub document: Option<Document>,
    /// Parser selected for a read operation, when any.
    pub parser_id: Option<String>,
    /// Optional human-readable message.
    pub message: Option<String>,
    /// Resolved/canonical path for listing tools, when applicable.
    pub listed_path: Option<std::path::PathBuf>,
    /// Project-scoped knowledge hits when applicable.
    pub project_knowledge: Vec<ProjectKnowledgeHit>,
    /// Terminal session id when applicable.
    pub session_id: Option<String>,
    /// Output produced by the latest terminal command.
    pub terminal_output: Option<String>,
    /// Full terminal scrollback for the session.
    pub terminal_scrollback: Option<String>,
    /// Terminal command history (oldest first).
    pub terminal_history: Vec<String>,
    /// Display title for the terminal session.
    pub terminal_title: Option<String>,
    /// Whether the terminal session is still alive after the operation.
    pub terminal_alive: Option<bool>,
    /// Current Git branch when applicable.
    pub git_branch: Option<String>,
    /// Short Git status summary when applicable.
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

impl ToolOutput {
    /// Successful directory listing.
    pub fn directory_listing(entries: Vec<FileEntry>) -> Self {
        Self {
            success: true,
            entries,
            ..Self::default()
        }
    }

    /// Successful recursive project-tree listing with canonical root.
    pub fn project_tree(root: impl Into<std::path::PathBuf>, entries: Vec<FileEntry>) -> Self {
        Self {
            success: true,
            entries,
            listed_path: Some(root.into()),
            ..Self::default()
        }
    }

    /// Successful document read.
    pub fn document(document: Document) -> Self {
        let parser_id = document.parser_id.clone();
        Self {
            success: true,
            document: Some(document),
            parser_id: Some(parser_id),
            ..Self::default()
        }
    }

    /// Successful terminal operation.
    #[allow(clippy::too_many_arguments)]
    pub fn terminal(
        session_id: impl Into<String>,
        cwd: impl Into<std::path::PathBuf>,
        output: impl Into<String>,
        scrollback: impl Into<String>,
        history: Vec<String>,
        title: impl Into<String>,
        alive: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            listed_path: Some(cwd.into()),
            session_id: Some(session_id.into()),
            terminal_output: Some(output.into()),
            terminal_scrollback: Some(scrollback.into()),
            terminal_history: history,
            terminal_title: Some(title.into()),
            terminal_alive: Some(alive),
            ..Self::default()
        }
    }

    /// Successful Git operation with refreshed status.
    pub fn git_status(
        repo_root: impl Into<std::path::PathBuf>,
        is_repository: bool,
        branch: Option<String>,
        summary: impl Into<String>,
        modified: Vec<GitPathStatus>,
        added: Vec<GitPathStatus>,
        deleted: Vec<GitPathStatus>,
        staged: Vec<GitPathStatus>,
        untracked: Vec<GitPathStatus>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            listed_path: Some(repo_root.into()),
            git_branch: branch,
            git_summary: Some(summary.into()),
            git_is_repository: Some(is_repository),
            git_modified: modified,
            git_added: added,
            git_deleted: deleted,
            git_staged: staged,
            git_untracked: untracked,
            ..Self::default()
        }
    }

    /// Successful Language Server operation.
    pub fn lsp(result: LspOperationResult, request: LspRequest) -> Self {
        Self {
            success: true,
            message: Some(result.message),
            listed_path: Some(request.workspace_root),
            lsp_hover: result.hover,
            lsp_completions: result.completions,
            lsp_diagnostics: result.diagnostics,
            lsp_definitions: result.definitions,
            lsp_references: result.references,
            lsp_edits: result.edits,
            ..Self::default()
        }
    }

    /// Failed tool execution with an explanatory message.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            ..Self::default()
        }
    }
}

/// Tool trait — validate, execute, return structured output.
///
/// A Tool is not responsible for planning, choosing providers, memory,
/// permissions, or user interaction.
pub trait Tool: Send + Sync {
    /// Describe this tool for Planner selection.
    fn metadata(&self) -> &ToolMetadata;

    /// Validate input before execution.
    fn validate(&self, input: &ToolInput) -> JaymiResult<()>;

    /// Execute the operation through the appropriate provider.
    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput>;
}
