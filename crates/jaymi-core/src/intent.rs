//! Canonical Intent identity for Jaymi.
//!
//! [`IntentId`] is the single shared abstraction for routing and relevance.
//! The Planner resolves a payload-bearing Intent and maps it to [`IntentId`].
//! Context, Capabilities, Behaviors, and Policies must reference this id —
//! they must not invent parallel intent taxonomies or free-text classifiers.
//!
//! Structured requests can be classified here via [`IntentId::from_structured_request`]
//! using the **same priority order as the Planner**. Free-text intent parsing
//! lives only in the Planner; without Planner hints, free text is [`IntentId::Unknown`].

use crate::UserRequest;

/// Stable intent identity shared across Planner, Context, Capabilities, and Policies.
///
/// Unit variants only — request payloads stay on the Planner's Intent enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IntentId {
    /// List the immediate contents of one directory.
    ListDirectory,
    /// Recursively list a project directory tree.
    ListProjectTree,
    /// Read one supported file.
    ReadFile,
    /// Write text content to one file.
    WriteFile,
    /// Create, rename, or delete a filesystem path.
    ManagePath,
    /// Terminal session operation.
    RunTerminal,
    /// Git repository operation.
    Git,
    /// Language Server Protocol operation.
    Lsp,
    /// Query the discovery inventory.
    DiscoverInventory,
    /// Search the knowledge inventory.
    SearchKnowledge,
    /// Index / scan roots into discovery.
    IndexRoots,
    /// Open or switch to a named project.
    ContinueProject,
    /// Open a project by id.
    OpenProject,
    /// Close the active project.
    CloseProject,
    /// Search knowledge belonging to one project.
    SearchProjectKnowledge,
    /// Produce a capability execution plan without executing tools.
    PlanWork,
    /// Could not be mapped to a supported intent.
    #[default]
    Unknown,
}

impl IntentId {
    /// Stable snake_case label for diagnostics, cache keys, and assemble notes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ListDirectory => "list_directory",
            Self::ListProjectTree => "list_project_tree",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::ManagePath => "manage_path",
            Self::RunTerminal => "run_terminal",
            Self::Git => "git",
            Self::Lsp => "lsp",
            Self::DiscoverInventory => "discover_inventory",
            Self::SearchKnowledge => "search_knowledge",
            Self::IndexRoots => "index_roots",
            Self::ContinueProject => "continue_project",
            Self::OpenProject => "open_project",
            Self::CloseProject => "close_project",
            Self::SearchProjectKnowledge => "search_project_knowledge",
            Self::PlanWork => "plan_work",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a stable label produced by [`Self::as_str`].
    pub fn from_str_label(label: &str) -> Option<Self> {
        match label {
            "list_directory" => Some(Self::ListDirectory),
            "list_project_tree" => Some(Self::ListProjectTree),
            "read_file" => Some(Self::ReadFile),
            "write_file" => Some(Self::WriteFile),
            "manage_path" => Some(Self::ManagePath),
            "run_terminal" => Some(Self::RunTerminal),
            "git" => Some(Self::Git),
            "lsp" => Some(Self::Lsp),
            "discover_inventory" => Some(Self::DiscoverInventory),
            "search_knowledge" => Some(Self::SearchKnowledge),
            "index_roots" => Some(Self::IndexRoots),
            "continue_project" => Some(Self::ContinueProject),
            "open_project" => Some(Self::OpenProject),
            "close_project" => Some(Self::CloseProject),
            "search_project_knowledge" => Some(Self::SearchProjectKnowledge),
            "plan_work" => Some(Self::PlanWork),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Classify structured request fields only — **same priority as the Planner**.
    ///
    /// Free-text content is ignored. Use Planner resolution (then [`IntentId`] via
    /// the Planner Intent) when free-text may carry intent.
    pub fn from_structured_request(request: &UserRequest) -> Self {
        if let Some(query) = &request.project_knowledge {
            if !query.project_id.trim().is_empty() {
                return Self::SearchProjectKnowledge;
            }
        }

        if let Some(project_id) = &request.open_project_id {
            if !project_id.trim().is_empty() {
                return Self::OpenProject;
            }
        }

        if request.close_project {
            return Self::CloseProject;
        }

        if request.search.is_some() {
            return Self::SearchKnowledge;
        }

        if request.discovery_kind.is_some() || request.discover {
            return Self::DiscoverInventory;
        }

        if let Some(path) = &request.index_root {
            if !path.as_os_str().is_empty() {
                return Self::IndexRoots;
            }
        }

        if let Some(write) = &request.write_file {
            if !write.path.as_os_str().is_empty() {
                return Self::WriteFile;
            }
        }

        if let Some(manage) = &request.manage_path {
            if !manage.path.as_os_str().is_empty() && !manage.command.trim().is_empty() {
                return Self::ManagePath;
            }
        }

        if let Some(terminal) = &request.terminal {
            let session_id_ok = matches!(
                terminal.operation,
                crate::TerminalOperation::Create
            ) || !terminal.session_id.trim().is_empty();
            if session_id_ok && !terminal.cwd.as_os_str().is_empty() {
                return Self::RunTerminal;
            }
        }

        if let Some(git) = &request.git {
            if !git.repo_root.as_os_str().is_empty() {
                return Self::Git;
            }
        }

        if let Some(lsp) = &request.lsp {
            if !lsp.workspace_root.as_os_str().is_empty() {
                return Self::Lsp;
            }
        }

        if let Some(path) = &request.file {
            if !path.as_os_str().is_empty() {
                return Self::ReadFile;
            }
        }

        if let Some(path) = &request.project_tree {
            if !path.as_os_str().is_empty() {
                return Self::ListProjectTree;
            }
        }

        if let Some(path) = &request.directory {
            if !path.as_os_str().is_empty() {
                return Self::ListDirectory;
            }
        }

        Self::Unknown
    }

    /// Coarse request-kind label for Context relevance / cache (derived from Intent).
    pub fn request_kind(self) -> &'static str {
        match self {
            Self::Unknown => "chat",
            Self::ReadFile => "file_read",
            Self::WriteFile | Self::ManagePath => "file_write",
            Self::SearchKnowledge
            | Self::SearchProjectKnowledge
            | Self::ListDirectory
            | Self::ListProjectTree => "search",
            Self::ContinueProject | Self::OpenProject | Self::CloseProject => "project_session",
            Self::RunTerminal => "terminal",
            Self::Git => "git",
            Self::Lsp => "lsp",
            Self::DiscoverInventory => "discover",
            Self::IndexRoots => "index",
            Self::PlanWork => "chat",
        }
    }

    /// Relevance facet tags derived from this Intent (not a second intent system).
    pub fn relevance_tags(self) -> &'static [&'static str] {
        match self {
            Self::Unknown => &["chat"],
            Self::ListDirectory | Self::ListProjectTree => &["search"],
            Self::ReadFile => &["read"],
            Self::WriteFile | Self::ManagePath => &["write", "code"],
            Self::RunTerminal => &["terminal", "code"],
            Self::Git => &["git", "code"],
            Self::Lsp => &["lsp", "code"],
            Self::DiscoverInventory => &["discover"],
            Self::SearchKnowledge | Self::SearchProjectKnowledge => &["search"],
            Self::IndexRoots => &["index"],
            Self::ContinueProject | Self::OpenProject | Self::CloseProject => &["project"],
            Self::PlanWork => &["code"],
        }
    }

    /// Default capability id hints when the Planner did not supply selections.
    ///
    /// Production assemble always passes Planner capability ids; this is a
    /// fallback for direct ContextEngine tests only.
    pub fn default_capability_ids(self) -> &'static [&'static str] {
        match self {
            Self::SearchKnowledge
            | Self::SearchProjectKnowledge
            | Self::DiscoverInventory
            | Self::ListDirectory
            | Self::ListProjectTree => &["search"],
            Self::ReadFile => &["read_documents"],
            Self::WriteFile | Self::ManagePath => &["file_management"],
            Self::RunTerminal => &["execute_terminal_commands"],
            Self::Git | Self::Lsp | Self::ContinueProject | Self::OpenProject | Self::PlanWork => {
                &["code"]
            }
            Self::CloseProject => &[],
            Self::IndexRoots => &["index"],
            Self::Unknown => &["chat"],
        }
    }
}

impl std::fmt::Display for IntentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for IntentId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_label(s).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchRequest;

    #[test]
    fn structured_search_matches_planner_priority() {
        let request = UserRequest::search(SearchRequest::free_text("fungi"));
        assert_eq!(
            IntentId::from_structured_request(&request),
            IntentId::SearchKnowledge
        );
    }

    #[test]
    fn free_text_alone_is_unknown_without_planner() {
        let request = UserRequest::new("search for fungi please");
        assert_eq!(
            IntentId::from_structured_request(&request),
            IntentId::Unknown
        );
    }

    #[test]
    fn labels_round_trip() {
        for id in [
            IntentId::ReadFile,
            IntentId::SearchKnowledge,
            IntentId::CloseProject,
            IntentId::Unknown,
        ] {
            assert_eq!(IntentId::from_str_label(id.as_str()), Some(id));
        }
    }
}
