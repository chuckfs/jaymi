//! User-facing request types that enter the Planner.

use std::path::PathBuf;

use crate::search::SearchRequest;

/// Structured discovery query kinds answered from the knowledge database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiscoveryQueryKind {
    /// All inventoried entries.
    #[default]
    All,
    /// Files with a specific extension (no leading dot).
    ByExtension {
        /// Lowercased extension without a leading dot.
        extension: String,
    },
    /// Entries under or in a folder.
    ByFolder {
        /// Folder path.
        path: PathBuf,
        /// When true, only immediate children (`parent = path`).
        immediate: bool,
    },
    /// List active logical collections.
    Collections,
    /// Entries in a named logical collection.
    ByCollection {
        /// Collection name or slug (for example `downloads`).
        name: String,
        /// When true, only immediate children of the collection root.
        immediate: bool,
    },
    /// Files ordered by newest modification time.
    RecentlyModified,
    /// Files ordered by newest creation time.
    RecentlyCreated,
    /// Files ordered by largest size.
    Largest,
    /// Hidden files and folders.
    Hidden,
    /// Folders with no inventoried children.
    EmptyFolders,
}

impl DiscoveryQueryKind {
    /// Stable label for diagnostics and tool messages.
    pub fn label(&self) -> String {
        match self {
            Self::All => "all".to_string(),
            Self::ByExtension { extension } => format!("extension:{extension}"),
            Self::ByFolder { immediate: true, .. } => "by_folder".to_string(),
            Self::ByFolder {
                immediate: false, ..
            } => "under_folder".to_string(),
            Self::Collections => "collections".to_string(),
            Self::ByCollection {
                name,
                immediate: true,
            } => format!("collection:{name}"),
            Self::ByCollection {
                name,
                immediate: false,
            } => format!("collection_under:{name}"),
            Self::RecentlyModified => "recently_modified".to_string(),
            Self::RecentlyCreated => "recently_created".to_string(),
            Self::Largest => "largest".to_string(),
            Self::Hidden => "hidden".to_string(),
            Self::EmptyFolders => "empty_folders".to_string(),
        }
    }
}

/// Structured request to search knowledge belonging to one project.
///
/// Distinct from inventory [`SearchRequest`]: this retrieves project-scoped
/// files, memories, tasks, decisions, and conversations through the Project
/// Engine (mediated by the Planner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKnowledgeRequest {
    /// Project that owns the knowledge boundary.
    pub project_id: String,
    /// Free-text query.
    pub text: String,
    /// Optional result limit.
    pub limit: Option<usize>,
}

/// Structured request to write text content to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileRequest {
    /// Destination file path.
    pub path: PathBuf,
    /// Full file contents to write.
    pub content: String,
}

/// Structured request to create, rename, or delete a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagePathRequest {
    /// Operation: `mkdir`, `rename`, or `delete`.
    pub command: String,
    /// Source path (directory to create, path to rename, or path to delete).
    pub path: PathBuf,
    /// Destination path when renaming (also accepted via [`UserRequest`] content helpers).
    pub destination: Option<PathBuf>,
}

/// Terminal operations exposed through the Terminal tool / provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalOperation {
    /// Ensure/spawn a session with a known id.
    Ensure,
    /// Run a shell command in a session.
    Run,
    /// Create a new session (id may be assigned by the provider).
    Create,
    /// Rename a session's display title.
    Rename,
    /// Kill / close a session.
    Kill,
}

impl TerminalOperation {
    /// Stable label for diagnostics and logging.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ensure => "ensure",
            Self::Run => "run",
            Self::Create => "create",
            Self::Rename => "rename",
            Self::Kill => "kill",
        }
    }
}

/// Structured request to manage or run a command in a terminal session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRequest {
    /// Operation to perform.
    pub operation: TerminalOperation,
    /// Stable session id (ignored/overwritten on Create when empty).
    pub session_id: String,
    /// Working directory for the session (usually the project root).
    pub cwd: PathBuf,
    /// Command to run when [`TerminalOperation::Run`].
    pub command: Option<String>,
    /// Display title for Create / Rename.
    pub title: Option<String>,
}

/// Git operations exposed through the Git tool / provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOperation {
    /// Read repository status.
    Status,
    /// Stage paths into the index.
    Stage,
    /// Remove paths from the index (keep worktree).
    Unstage,
    /// Discard worktree / untracked changes for paths.
    Discard,
    /// Create a commit from the staged index.
    Commit,
}

impl GitOperation {
    /// Stable label for diagnostics and logging.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Stage => "stage",
            Self::Unstage => "unstage",
            Self::Discard => "discard",
            Self::Commit => "commit",
        }
    }

    /// Whether this operation mutates the repository.
    pub fn is_mutating(self) -> bool {
        !matches!(self, Self::Status)
    }
}

/// One path with a short Git status code (`M`, `A`, `??`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPathStatus {
    /// Repository-relative path.
    pub path: String,
    /// Short status label.
    pub status: String,
}

/// Structured Git request mediated by the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRequest {
    /// Repository root (usually the project root).
    pub repo_root: PathBuf,
    /// Operation to perform.
    pub operation: GitOperation,
    /// Paths for stage / unstage / discard.
    pub paths: Vec<PathBuf>,
    /// Commit message when [`GitOperation::Commit`].
    pub message: Option<String>,
}

/// A request originating from the conversation interface.
///
/// Every interaction begins with understanding intent. The Planner receives
/// this request and coordinates the rest of the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRequest {
    /// Natural-language content provided by the user.
    pub content: String,
    /// Optional structured directory path for list-directory intents.
    ///
    /// When set, the Decision Engine treats this as an explicit list-directory
    /// request without requiring natural-language parsing.
    pub directory: Option<PathBuf>,
    /// Optional structured root path for a recursive project-tree listing.
    pub project_tree: Option<PathBuf>,
    /// Optional structured file path for read-file intents.
    pub file: Option<PathBuf>,
    /// Optional structured write-file request.
    pub write_file: Option<WriteFileRequest>,
    /// Optional structured path management request (mkdir / rename / delete).
    pub manage_path: Option<ManagePathRequest>,
    /// Optional structured terminal ensure/run request.
    pub terminal: Option<TerminalRequest>,
    /// Optional structured Git request.
    pub git: Option<GitRequest>,
    /// Optional structured Language Server request.
    pub lsp: Option<crate::lsp::LspRequest>,
    /// When true, query the persistent discovery inventory.
    pub discover: bool,
    /// Optional structured discovery query kind.
    pub discovery_kind: Option<DiscoveryQueryKind>,
    /// Optional structured root path for an index/discovery scan.
    pub index_root: Option<PathBuf>,
    /// Optional structured Search Engine request.
    pub search: Option<SearchRequest>,
    /// Optional structured project-knowledge search (Planner-mediated).
    pub project_knowledge: Option<ProjectKnowledgeRequest>,
    /// Optional structured open-project-by-id request (Planner-mediated).
    pub open_project_id: Option<String>,
    /// When true, close the active project workspace through the Planner.
    pub close_project: bool,
}

impl UserRequest {
    fn bare(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            directory: None,
            project_tree: None,
            file: None,
            write_file: None,
            manage_path: None,
            terminal: None,
            git: None,
            lsp: None,
            discover: false,
            discovery_kind: None,
            index_root: None,
            search: None,
            project_knowledge: None,
            open_project_id: None,
            close_project: false,
        }
    }

    /// Create a new user request from free-form content.
    pub fn new(content: impl Into<String>) -> Self {
        Self::bare(content)
    }

    /// Create a structured request to list a single directory.
    pub fn list_directory(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("list {}", path.display()),
            directory: Some(path),
            ..Self::bare("")
        }
    }

    /// Create a structured request to recursively list a project tree.
    pub fn list_project_tree(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("list project tree {}", path.display()),
            project_tree: Some(path),
            ..Self::bare("")
        }
    }

    /// Create a structured request to read a single file.
    pub fn read_file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("read {}", path.display()),
            file: Some(path),
            ..Self::bare("")
        }
    }

    /// Create a structured request to write a single file.
    pub fn write_file(path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        let path = path.into();
        let content = content.into();
        Self {
            content: format!("write {}", path.display()),
            write_file: Some(WriteFileRequest {
                path,
                content,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to create a directory.
    pub fn manage_mkdir(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("mkdir {}", path.display()),
            manage_path: Some(ManagePathRequest {
                command: "mkdir".into(),
                path,
                destination: None,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to rename/move a path.
    pub fn manage_rename(from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> Self {
        let path = from.into();
        let destination = to.into();
        Self {
            content: format!("rename {} → {}", path.display(), destination.display()),
            manage_path: Some(ManagePathRequest {
                command: "rename".into(),
                path,
                destination: Some(destination),
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to delete a file or directory.
    pub fn manage_delete(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("delete {}", path.display()),
            manage_path: Some(ManagePathRequest {
                command: "delete".into(),
                path,
                destination: None,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to ensure a terminal session exists.
    pub fn ensure_terminal(session_id: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        let session_id = session_id.into();
        let cwd = cwd.into();
        Self {
            content: format!("ensure terminal {session_id}"),
            terminal: Some(TerminalRequest {
                operation: TerminalOperation::Ensure,
                session_id,
                cwd,
                command: None,
                title: None,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to run a command in a terminal session.
    pub fn run_terminal(
        session_id: impl Into<String>,
        cwd: impl Into<PathBuf>,
        command: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let cwd = cwd.into();
        let command = command.into();
        Self {
            content: format!("run terminal: {command}"),
            terminal: Some(TerminalRequest {
                operation: TerminalOperation::Run,
                session_id,
                cwd,
                command: Some(command),
                title: None,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to spawn a new terminal session.
    pub fn create_terminal(cwd: impl Into<PathBuf>, title: Option<String>) -> Self {
        let cwd = cwd.into();
        Self {
            content: format!("create terminal at {}", cwd.display()),
            terminal: Some(TerminalRequest {
                operation: TerminalOperation::Create,
                session_id: String::new(),
                cwd,
                command: None,
                title,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to rename a terminal session.
    pub fn rename_terminal(
        session_id: impl Into<String>,
        cwd: impl Into<PathBuf>,
        title: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let title = title.into();
        Self {
            content: format!("rename terminal {session_id} → {title}"),
            terminal: Some(TerminalRequest {
                operation: TerminalOperation::Rename,
                session_id,
                cwd: cwd.into(),
                command: None,
                title: Some(title),
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to kill a terminal session.
    pub fn kill_terminal(session_id: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        let session_id = session_id.into();
        Self {
            content: format!("kill terminal {session_id}"),
            terminal: Some(TerminalRequest {
                operation: TerminalOperation::Kill,
                session_id,
                cwd: cwd.into(),
                command: None,
                title: None,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured Language Server request.
    pub fn lsp(request: crate::lsp::LspRequest) -> Self {
        Self {
            content: format!(
                "lsp {} {}",
                request.operation.as_str(),
                request.workspace_root.display()
            ),
            lsp: Some(request),
            ..Self::bare("")
        }
    }

    /// Create a structured Git status request.
    pub fn git_status(repo_root: impl Into<PathBuf>) -> Self {
        Self::git(repo_root, GitOperation::Status, Vec::new(), None)
    }

    /// Create a structured Git stage request.
    pub fn git_stage(repo_root: impl Into<PathBuf>, paths: Vec<PathBuf>) -> Self {
        Self::git(repo_root, GitOperation::Stage, paths, None)
    }

    /// Create a structured Git unstage request.
    pub fn git_unstage(repo_root: impl Into<PathBuf>, paths: Vec<PathBuf>) -> Self {
        Self::git(repo_root, GitOperation::Unstage, paths, None)
    }

    /// Create a structured Git discard request.
    pub fn git_discard(repo_root: impl Into<PathBuf>, paths: Vec<PathBuf>) -> Self {
        Self::git(repo_root, GitOperation::Discard, paths, None)
    }

    /// Create a structured Git commit request.
    pub fn git_commit(repo_root: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::git(repo_root, GitOperation::Commit, Vec::new(), Some(message.into()))
    }

    fn git(
        repo_root: impl Into<PathBuf>,
        operation: GitOperation,
        paths: Vec<PathBuf>,
        message: Option<String>,
    ) -> Self {
        let repo_root = repo_root.into();
        Self {
            content: format!("git {} {}", operation.as_str(), repo_root.display()),
            git: Some(GitRequest {
                repo_root,
                operation,
                paths,
                message,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured Search Engine request.
    pub fn search(request: SearchRequest) -> Self {
        let content = request
            .free_text
            .as_ref()
            .map(|text| format!("search {text}"))
            .or_else(|| {
                request
                    .filename
                    .as_ref()
                    .map(|name| format!("find file {name}"))
            })
            .unwrap_or_else(|| "search".to_string());
        Self {
            content,
            search: Some(request),
            ..Self::bare("")
        }
    }

    /// Create a structured project-knowledge search request.
    ///
    /// This always enters the Planner (`handle`); Application must not call
    /// the Project Engine for this retrieval directly.
    pub fn search_project_knowledge(
        project_id: impl Into<String>,
        text: impl Into<String>,
        limit: Option<usize>,
    ) -> Self {
        let project_id = project_id.into();
        let text = text.into();
        Self {
            content: format!("search project knowledge in {project_id}: {text}"),
            project_knowledge: Some(ProjectKnowledgeRequest {
                project_id,
                text,
                limit,
            }),
            ..Self::bare("")
        }
    }

    /// Create a structured request to open a project by id through the Planner.
    pub fn open_project(project_id: impl Into<String>) -> Self {
        let project_id = project_id.into();
        Self {
            content: format!("open project {project_id}"),
            open_project_id: Some(project_id),
            ..Self::bare("")
        }
    }

    /// Create a structured request to close the active project through the Planner.
    pub fn close_project() -> Self {
        Self {
            content: "close project".to_string(),
            close_project: true,
            ..Self::bare("")
        }
    }

    /// Create a structured request to query the discovery inventory.
    pub fn discover_inventory() -> Self {
        Self {
            content: "what files exist?".to_string(),
            discover: true,
            discovery_kind: Some(DiscoveryQueryKind::All),
            ..Self::bare("")
        }
    }

    /// Create a structured discovery query request.
    pub fn discover_query(kind: DiscoveryQueryKind) -> Self {
        let content = match &kind {
            DiscoveryQueryKind::All => "what files exist?".to_string(),
            DiscoveryQueryKind::ByExtension { extension } => {
                format!("{extension} files")
            }
            DiscoveryQueryKind::ByFolder { path, immediate } => {
                if *immediate {
                    format!("files in {}", path.display())
                } else {
                    format!("files under {}", path.display())
                }
            }
            DiscoveryQueryKind::Collections => "show collections".to_string(),
            DiscoveryQueryKind::ByCollection { name, immediate } => {
                if *immediate {
                    format!("what's in {name}?")
                } else {
                    format!("files under {name}")
                }
            }
            DiscoveryQueryKind::RecentlyModified => "recently modified files".to_string(),
            DiscoveryQueryKind::RecentlyCreated => "recently created files".to_string(),
            DiscoveryQueryKind::Largest => "largest files".to_string(),
            DiscoveryQueryKind::Hidden => "hidden files".to_string(),
            DiscoveryQueryKind::EmptyFolders => "empty folders".to_string(),
        };
        Self {
            content,
            discover: true,
            discovery_kind: Some(kind),
            ..Self::bare("")
        }
    }

    /// Create a structured request to scan a root into the inventory.
    pub fn index_root(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("index {}", path.display()),
            index_root: Some(path),
            ..Self::bare("")
        }
    }
}
