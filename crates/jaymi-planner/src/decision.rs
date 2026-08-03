//! Decision Engine — deterministic application logic.
//!
//! Answers questions that should never depend on a language model:
//! intent routing, project awareness, permission needs, capability selection.

use std::path::PathBuf;

use jaymi_capabilities::Capability;
use jaymi_core::{DiscoveryQueryKind, GitOperation, SearchRequest, UserRequest};

/// Deterministic intents recognized by the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// List the immediate contents of one directory.
    ListDirectory {
        /// Directory path to list.
        path: PathBuf,
    },
    /// Recursively list a project directory tree (Coding Explorer).
    ListProjectTree {
        /// Project root directory path.
        path: PathBuf,
    },
    /// Read one supported file into a unified document.
    ReadFile {
        /// File path to read.
        path: PathBuf,
    },
    /// Write text content to one file.
    WriteFile {
        /// Destination path.
        path: PathBuf,
        /// Full contents to write.
        content: String,
    },
    /// Ensure or run a command in a persistent terminal session.
    RunTerminal {
        /// Stable session id.
        session_id: String,
        /// Working directory for the session.
        cwd: PathBuf,
        /// Command to run; `None` ensures/spawns only.
        command: Option<String>,
    },
    /// Inspect or mutate a local Git repository.
    Git {
        /// Repository root.
        repo_root: PathBuf,
        /// Operation to perform.
        operation: GitOperation,
        /// Paths for stage / unstage / discard.
        paths: Vec<PathBuf>,
        /// Commit message when committing.
        message: Option<String>,
    },
    /// Language Server Protocol operation (Rust Analyzer).
    Lsp {
        /// Structured LSP request.
        request: jaymi_core::LspRequest,
    },
    /// Query the persistent discovery inventory.
    DiscoverInventory {
        /// Discovery query kind.
        kind: DiscoveryQueryKind,
    },
    /// Search the knowledge inventory through the Search Engine.
    SearchKnowledge {
        /// Structured search request.
        request: SearchRequest,
    },
    /// Recursively scan roots into the discovery inventory.
    IndexRoots {
        /// Optional explicit root; otherwise configured roots are used.
        path: Option<PathBuf>,
    },
    /// Open or switch to a named project and restore its workspace context.
    ContinueProject {
        /// Project display name (e.g. "Jaymi").
        name: String,
    },
    /// Open a project by stable id (structured request; same session wiring as Continue).
    OpenProject {
        /// Project id.
        project_id: String,
    },
    /// Close the currently active project workspace.
    CloseProject,
    /// Search knowledge belonging to one project (Project Engine, via Planner).
    SearchProjectKnowledge {
        /// Owning project id.
        project_id: String,
        /// Free-text query.
        text: String,
        /// Optional result limit.
        limit: Option<usize>,
    },
    /// Produce a capability execution plan without executing tools.
    ///
    /// One or more independent capabilities may be composed into a single
    /// plan. Capabilities are never merged — each becomes its own step.
    PlanWork {
        /// Capabilities to plan, in cooperation order.
        capabilities: Vec<Capability>,
        /// Original user goal text.
        goal: String,
    },
    /// Request could not be mapped to a supported intent.
    Unknown,
}

/// Deterministic decision-making component of the Planner.
#[derive(Debug, Default)]
pub struct DecisionEngine;

impl DecisionEngine {
    /// Determine user intent without language-model reasoning.
    pub fn determine_intent(&self, request: &UserRequest) -> Intent {
        if let Some(query) = &request.project_knowledge {
            return Intent::SearchProjectKnowledge {
                project_id: query.project_id.clone(),
                text: query.text.clone(),
                limit: query.limit,
            };
        }

        if let Some(project_id) = &request.open_project_id {
            if !project_id.trim().is_empty() {
                return Intent::OpenProject {
                    project_id: project_id.clone(),
                };
            }
        }

        if request.close_project {
            return Intent::CloseProject;
        }

        if let Some(search) = &request.search {
            return Intent::SearchKnowledge {
                request: search.clone(),
            };
        }

        if let Some(kind) = &request.discovery_kind {
            return Intent::DiscoverInventory { kind: kind.clone() };
        }

        if request.discover {
            return Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::All,
            };
        }

        if let Some(path) = &request.index_root {
            if !path.as_os_str().is_empty() {
                return Intent::IndexRoots {
                    path: Some(path.clone()),
                };
            }
        }

        if let Some(write) = &request.write_file {
            if !write.path.as_os_str().is_empty() {
                return Intent::WriteFile {
                    path: write.path.clone(),
                    content: write.content.clone(),
                };
            }
        }

        if let Some(terminal) = &request.terminal {
            if !terminal.session_id.trim().is_empty() && !terminal.cwd.as_os_str().is_empty() {
                return Intent::RunTerminal {
                    session_id: terminal.session_id.clone(),
                    cwd: terminal.cwd.clone(),
                    command: terminal.command.clone(),
                };
            }
        }

        if let Some(git) = &request.git {
            if !git.repo_root.as_os_str().is_empty() {
                return Intent::Git {
                    repo_root: git.repo_root.clone(),
                    operation: git.operation,
                    paths: git.paths.clone(),
                    message: git.message.clone(),
                };
            }
        }

        if let Some(lsp) = &request.lsp {
            if !lsp.workspace_root.as_os_str().is_empty() {
                return Intent::Lsp {
                    request: lsp.clone(),
                };
            }
        }

        if let Some(path) = &request.file {
            if !path.as_os_str().is_empty() {
                return Intent::ReadFile { path: path.clone() };
            }
        }

        if let Some(path) = &request.project_tree {
            if !path.as_os_str().is_empty() {
                return Intent::ListProjectTree { path: path.clone() };
            }
        }

        if let Some(path) = &request.directory {
            if !path.as_os_str().is_empty() {
                return Intent::ListDirectory { path: path.clone() };
            }
        }

        let content = request.content.trim();
        let lower = content.to_ascii_lowercase();

        if parse_close_project(&lower) {
            return Intent::CloseProject;
        }

        if let Some(name) = parse_continue_project(&lower, content) {
            return Intent::ContinueProject { name };
        }

        // Multi-capability composition before single-capability search/discovery
        // so "search then code then create" is not stolen as a search query.
        if let Some(capabilities) = parse_composed_capabilities(&lower) {
            return Intent::PlanWork {
                capabilities,
                goal: content.to_string(),
            };
        }

        if let Some(kind) = parse_discovery_kind(&lower, content) {
            return Intent::DiscoverInventory { kind };
        }

        if let Some(request) = parse_search_request(&lower, content) {
            return Intent::SearchKnowledge { request };
        }

        if let Some(rest) = content.strip_prefix("index ") {
            let path = strip_quotes(rest);
            if path.is_empty() {
                return Intent::IndexRoots { path: None };
            }
            return Intent::IndexRoots {
                path: Some(PathBuf::from(path)),
            };
        }
        if lower == "index" {
            return Intent::IndexRoots { path: None };
        }

        if let Some(rest) = content.strip_prefix("read ") {
            let path = strip_quotes(rest);
            if !path.is_empty() {
                return Intent::ReadFile {
                    path: PathBuf::from(path),
                };
            }
        }

        if let Some(rest) = content.strip_prefix("list ") {
            let path = strip_quotes(rest);
            if !path.is_empty() {
                return Intent::ListDirectory {
                    path: PathBuf::from(path),
                };
            }
        }

        if let Some(capabilities) = parse_single_plan_work_capabilities(&lower) {
            return Intent::PlanWork {
                capabilities,
                goal: content.to_string(),
            };
        }

        Intent::Unknown
    }

    /// Map an intent to the primary capability required to fulfill it.
    pub fn required_capability(&self, intent: &Intent) -> Option<Capability> {
        match intent {
            Intent::ListDirectory { .. } => Some(Capability::Search),
            Intent::ListProjectTree { .. } => Some(Capability::Search),
            Intent::SearchKnowledge { .. } => Some(Capability::Search),
            Intent::SearchProjectKnowledge { .. } => Some(Capability::Search),
            Intent::ReadFile { .. } => Some(Capability::ReadDocuments),
            Intent::WriteFile { .. } => Some(Capability::FileManagement),
            Intent::RunTerminal { .. } => Some(Capability::ExecuteTerminalCommands),
            Intent::Git { .. } => Some(Capability::Code),
            Intent::Lsp { .. } => Some(Capability::Code),
            Intent::DiscoverInventory { .. } => Some(Capability::Discover),
            Intent::IndexRoots { .. } => Some(Capability::Index),
            Intent::PlanWork { capabilities, .. } => capabilities.first().copied(),
            Intent::ContinueProject { .. } => None,
            Intent::OpenProject { .. } => None,
            Intent::CloseProject => None,
            Intent::Unknown => None,
        }
    }

    /// Map an intent to all capabilities that should cooperate for the request.
    ///
    /// Single-capability intents return a one-element list. Composed PlanWork
    /// intents return every independent capability in order.
    pub fn required_capabilities(&self, intent: &Intent) -> Vec<Capability> {
        match intent {
            Intent::PlanWork { capabilities, .. } => capabilities.clone(),
            other => self
                .required_capability(other)
                .into_iter()
                .collect(),
        }
    }
}

fn parse_close_project(lower: &str) -> bool {
    matches!(
        lower.trim_end_matches('.'),
        "close project"
            | "close the project"
            | "close active project"
            | "leave project"
            | "leave the project"
    )
}

fn parse_continue_project(lower: &str, original: &str) -> Option<String> {
    let prefixes = [
        "continue working on ",
        "continue on ",
        "resume working on ",
        "resume ",
        "open project ",
        "switch to project ",
        "switch project ",
        "work on ",
    ];
    for prefix in prefixes {
        if let Some(_rest) = lower.strip_prefix(prefix) {
            if original.len() < prefix.len() {
                continue;
            }
            let name = strip_quotes(&original[prefix.len()..])
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn parse_search_request(lower: &str, original: &str) -> Option<SearchRequest> {
    if let Some(_rest) = lower
        .strip_prefix("search ")
        .or_else(|| lower.strip_prefix("find "))
        .or_else(|| lower.strip_prefix("find file "))
    {
        let query = if lower.starts_with("search ") {
            strip_quotes(&original["search ".len()..])
        } else if lower.starts_with("find file ") {
            strip_quotes(&original["find file ".len()..])
        } else {
            strip_quotes(&original["find ".len()..])
        };
        if query.is_empty() {
            return None;
        }
        if lower.starts_with("find file ") {
            return Some(SearchRequest::filename(query));
        }
        return Some(SearchRequest::free_text(query));
    }
    None
}

fn parse_discovery_kind(lower: &str, original: &str) -> Option<DiscoveryQueryKind> {
    if lower == "what files exist?"
        || lower == "what files exist"
        || lower == "discover"
        || lower == "show all files"
    {
        return Some(DiscoveryQueryKind::All);
    }
    if lower == "show collections"
        || lower == "list collections"
        || lower == "collections"
        || lower == "what collections do i have"
        || lower == "what collections do i have?"
        || lower == "what collections exist"
        || lower == "what collections exist?"
    {
        return Some(DiscoveryQueryKind::Collections);
    }
    if lower == "what projects do i have"
        || lower == "what projects do i have?"
        || lower == "my projects"
        || lower == "show projects"
    {
        return Some(DiscoveryQueryKind::ByCollection {
            name: "projects".to_string(),
            immediate: true,
        });
    }
    if lower == "recently modified files"
        || lower == "recently modified"
        || lower == "newest modified files"
    {
        return Some(DiscoveryQueryKind::RecentlyModified);
    }
    if lower == "recently created files"
        || lower == "recently created"
        || lower == "newest files"
    {
        return Some(DiscoveryQueryKind::RecentlyCreated);
    }
    if lower == "largest files" || lower == "biggest files" {
        return Some(DiscoveryQueryKind::Largest);
    }
    if lower == "hidden files" || lower == "show hidden files" {
        return Some(DiscoveryQueryKind::Hidden);
    }
    if lower == "empty folders" || lower == "empty directories" {
        return Some(DiscoveryQueryKind::EmptyFolders);
    }

    if let Some(name) = parse_whats_in_collection(lower) {
        return Some(DiscoveryQueryKind::ByCollection {
            name: name.to_string(),
            immediate: true,
        });
    }

    if let Some(rest) = lower.strip_prefix("files with extension ") {
        let extension = rest.trim().trim_start_matches('.').to_string();
        if !extension.is_empty() {
            return Some(DiscoveryQueryKind::ByExtension { extension });
        }
    }
    if let Some(rest) = lower.strip_prefix("*.") {
        let extension = rest.trim().to_string();
        if !extension.is_empty() && !extension.contains(' ') {
            return Some(DiscoveryQueryKind::ByExtension { extension });
        }
    }
    if lower.ends_with(" files") {
        let stem = lower.trim_end_matches(" files").trim();
        if let Some(slug) = jaymi_core::parse_collection_slug(stem) {
            return Some(DiscoveryQueryKind::ByCollection {
                name: slug.to_string(),
                immediate: true,
            });
        }
        if !stem.is_empty()
            && !stem.contains(' ')
            && stem != "hidden"
            && stem != "largest"
            && stem != "biggest"
        {
            return Some(DiscoveryQueryKind::ByExtension {
                extension: stem.trim_start_matches('.').to_string(),
            });
        }
    }

    if let Some(rest) = lower
        .strip_prefix("files in ")
        .or_else(|| lower.strip_prefix("files under "))
    {
        let immediate = lower.starts_with("files in ");
        let path = strip_quotes(rest);
        if !path.is_empty() {
            if let Some(slug) = jaymi_core::parse_collection_slug(path) {
                return Some(DiscoveryQueryKind::ByCollection {
                    name: slug.to_string(),
                    immediate,
                });
            }
            let original_path = original
                .get(original.len().saturating_sub(rest.len())..)
                .map(strip_quotes)
                .filter(|value| !value.is_empty())
                .unwrap_or(path);
            return Some(DiscoveryQueryKind::ByFolder {
                path: PathBuf::from(original_path),
                immediate,
            });
        }
    }

    if let Some(slug) = jaymi_core::parse_collection_slug(lower) {
        return Some(DiscoveryQueryKind::ByCollection {
            name: slug.to_string(),
            immediate: true,
        });
    }

    if lower.starts_with("discover ") {
        let rest = strip_quotes(&original["discover ".len()..]);
        if rest.is_empty() {
            return Some(DiscoveryQueryKind::All);
        }
        if let Some(slug) = jaymi_core::parse_collection_slug(rest) {
            return Some(DiscoveryQueryKind::ByCollection {
                name: slug.to_string(),
                immediate: false,
            });
        }
        return Some(DiscoveryQueryKind::ByFolder {
            path: PathBuf::from(rest),
            immediate: false,
        });
    }

    None
}

fn parse_whats_in_collection(lower: &str) -> Option<&'static str> {
    let rest = lower
        .strip_prefix("what's in ")
        .or_else(|| lower.strip_prefix("whats in "))
        .or_else(|| lower.strip_prefix("what is in "))
        .or_else(|| lower.strip_prefix("what is inside "))
        .or_else(|| lower.strip_prefix("show "))?;
    let name = rest.trim().trim_end_matches('?').trim();
    jaymi_core::parse_collection_slug(name)
}

fn parse_single_plan_work_capabilities(lower: &str) -> Option<Vec<Capability>> {
    let normalized = lower.trim().trim_end_matches('.').trim();
    let coding_phrases = [
        "help me build an app",
        "help me build a app",
        "help me build an application",
        "help me build a application",
        "build an app",
        "build a app",
        "build an application",
        "help me write code",
        "help me code",
        "i want to build an app",
        "i want to code",
        "plan coding",
        "plan code",
    ];
    if coding_phrases
        .iter()
        .any(|phrase| normalized == *phrase || normalized.starts_with(&format!("{phrase} ")))
    {
        return Some(vec![Capability::Code]);
    }
    if normalized.contains("build an app")
        || normalized.contains("build a app")
        || (normalized.contains("help me build") && normalized.contains("app"))
    {
        return Some(vec![Capability::Code]);
    }
    None
}

/// Parse multi-capability cooperation phrases (Research → Coding → Creation).
fn parse_composed_capabilities(lower: &str) -> Option<Vec<Capability>> {
    let normalized = lower.trim().trim_end_matches('.').trim();
    let composition_phrases = [
        "research then code then create",
        "research then coding then creation",
        "research then code then creation",
        "research, code, and create",
        "research, coding, and creation",
        "research → coding → creation",
        "research -> coding -> creation",
        "research → code → create",
        "research -> code -> create",
        "compose research coding creation",
        "compose research code create",
        "plan research then code then create",
        "plan research coding creation",
        "search then code then create",
        "search then code then generate_images",
    ];
    if composition_phrases
        .iter()
        .any(|phrase| normalized == *phrase)
    {
        return Some(jaymi_capabilities::research_coding_creation());
    }

    // Generic "compose a, b, and c" / "a then b then c" with known capability tokens.
    if let Some(rest) = normalized
        .strip_prefix("compose ")
        .or_else(|| normalized.strip_prefix("plan composition "))
        .or_else(|| normalized.strip_prefix("plan composed "))
    {
        if let Some(caps) = parse_capability_token_sequence(rest) {
            if caps.len() > 1 {
                return Some(caps);
            }
        }
    }

    if normalized.contains(" then ")
        || normalized.contains(" → ")
        || normalized.contains(" -> ")
    {
        let unified = normalized.replace(" → ", " then ").replace(" -> ", " then ");
        if let Some(caps) = parse_capability_token_sequence(&unified) {
            if caps.len() > 1 {
                return Some(caps);
            }
        }
    }

    None
}

fn parse_capability_token_sequence(text: &str) -> Option<Vec<Capability>> {
    let cleaned = text
        .replace(',', " ")
        .replace(" and ", " ")
        .replace(" → ", " ")
        .replace(" -> ", " ")
        .replace(" then ", " ");
    let mut capabilities = Vec::new();
    for token in cleaned.split_whitespace() {
        let token = token.trim().trim_matches('.').trim_matches(',');
        if token.is_empty() || token == "&" {
            continue;
        }
        if let Some(capability) = parse_capability_token(token) {
            if !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
    }
    if capabilities.len() < 2 {
        None
    } else {
        Some(capabilities)
    }
}

fn parse_capability_token(token: &str) -> Option<Capability> {
    match token {
        "research" | "search" => Some(Capability::Search),
        "code" | "coding" => Some(Capability::Code),
        "create" | "creation" | "generate" | "images" | "generate_images" => {
            Some(Capability::GenerateImages)
        }
        "read" | "documents" | "read_documents" => Some(Capability::ReadDocuments),
        "discover" | "discovery" => Some(Capability::Discover),
        "index" | "indexing" => Some(Capability::Index),
        _ => Capability::from_id(token),
    }
}

fn strip_quotes(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_directory_request() {
        let engine = DecisionEngine;
        let request = UserRequest::list_directory("/tmp");
        assert_eq!(
            engine.determine_intent(&request),
            Intent::ListDirectory {
                path: PathBuf::from("/tmp")
            }
        );
        assert_eq!(
            engine.required_capability(&engine.determine_intent(&request)),
            Some(Capability::Search)
        );
    }

    #[test]
    fn structured_read_request() {
        let engine = DecisionEngine;
        let request = UserRequest::read_file("README.md");
        assert_eq!(
            engine.determine_intent(&request),
            Intent::ReadFile {
                path: PathBuf::from("README.md")
            }
        );
        assert_eq!(
            engine.required_capability(&engine.determine_intent(&request)),
            Some(Capability::ReadDocuments)
        );
    }

    #[test]
    fn parses_discovery_query_kinds() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("what files exist?")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::All
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("pdf files")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::ByExtension {
                    extension: "pdf".into()
                }
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("recently modified files")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::RecentlyModified
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("empty folders")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::EmptyFolders
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("what's in Downloads?")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::ByCollection {
                    name: "downloads".into(),
                    immediate: true,
                }
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("what projects do I have?")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::ByCollection {
                    name: "projects".into(),
                    immediate: true,
                }
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("show collections")),
            Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::Collections
            }
        );
        assert_eq!(
            engine.required_capability(&Intent::DiscoverInventory {
                kind: DiscoveryQueryKind::All
            }),
            Some(Capability::Discover)
        );
    }

    #[test]
    fn index_roots_intent() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::index_root("/tmp/docs")),
            Intent::IndexRoots {
                path: Some(PathBuf::from("/tmp/docs"))
            }
        );
        assert_eq!(
            engine.required_capability(&Intent::IndexRoots { path: None }),
            Some(Capability::Index)
        );
    }

    #[test]
    fn parses_search_requests() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("search fungi")),
            Intent::SearchKnowledge {
                request: SearchRequest::free_text("fungi"),
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("find file report.pdf")),
            Intent::SearchKnowledge {
                request: SearchRequest::filename("report.pdf"),
            }
        );
        assert_eq!(
            engine.required_capability(&Intent::SearchKnowledge {
                request: SearchRequest::free_text("x"),
            }),
            Some(Capability::Search)
        );
    }

    #[test]
    fn structured_project_knowledge_search_is_planner_intent() {
        let engine = DecisionEngine;
        let intent = engine.determine_intent(&UserRequest::search_project_knowledge(
            "project:jaymi",
            "architecture",
            Some(10),
        ));
        assert_eq!(
            intent,
            Intent::SearchProjectKnowledge {
                project_id: "project:jaymi".into(),
                text: "architecture".into(),
                limit: Some(10),
            }
        );
        assert_eq!(engine.required_capability(&intent), Some(Capability::Search));
    }

    #[test]
    fn parses_continue_working_on_project() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("Continue working on Jaymi.")),
            Intent::ContinueProject {
                name: "Jaymi".into()
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("switch to project OtherApp")),
            Intent::ContinueProject {
                name: "OtherApp".into()
            }
        );
        assert_eq!(
            engine.required_capability(&Intent::ContinueProject {
                name: "Jaymi".into()
            }),
            None
        );
    }

    #[test]
    fn recognizes_close_project() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("close project")),
            Intent::CloseProject
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("Leave the project.")),
            Intent::CloseProject
        );
        assert_eq!(engine.required_capability(&Intent::CloseProject), None);
    }

    #[test]
    fn parses_coding_plan_work_requests() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("Help me build an app.")),
            Intent::PlanWork {
                capabilities: vec![Capability::Code],
                goal: "Help me build an app.".into(),
            }
        );
        assert_eq!(
            engine.required_capability(&engine.determine_intent(&UserRequest::new(
                "Help me build an app."
            ))),
            Some(Capability::Code)
        );
    }

    #[test]
    fn parses_research_coding_creation_composition() {
        let engine = DecisionEngine;
        let intent = engine.determine_intent(&UserRequest::new(
            "research then code then create",
        ));
        assert_eq!(
            intent,
            Intent::PlanWork {
                capabilities: vec![
                    Capability::Search,
                    Capability::Code,
                    Capability::GenerateImages,
                ],
                goal: "research then code then create".into(),
            }
        );
        assert_eq!(
            engine.required_capabilities(&intent),
            vec![
                Capability::Search,
                Capability::Code,
                Capability::GenerateImages,
            ]
        );
        assert_eq!(engine.required_capability(&intent), Some(Capability::Search));
    }

    #[test]
    fn unknown_without_supported_intent() {
        let engine = DecisionEngine;
        let request = UserRequest::new("hello");
        assert_eq!(engine.determine_intent(&request), Intent::Unknown);
    }
}
