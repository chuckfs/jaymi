//! Decision Engine — deterministic application logic.
//!
//! Answers questions that should never depend on a language model:
//! intent routing, project awareness, permission needs, capability selection.

use std::path::PathBuf;

use jaymi_capabilities::Capability;
use jaymi_core::{DiscoveryQueryKind, SearchRequest, UserRequest};

/// Deterministic intents recognized by the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// List the immediate contents of one directory.
    ListDirectory {
        /// Directory path to list.
        path: PathBuf,
    },
    /// Read one supported file into a unified document.
    ReadFile {
        /// File path to read.
        path: PathBuf,
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
    /// Resume work on a named project and restore its memory context.
    ContinueProject {
        /// Project display name (e.g. "Jaymi").
        name: String,
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

        if let Some(path) = &request.file {
            if !path.as_os_str().is_empty() {
                return Intent::ReadFile { path: path.clone() };
            }
        }

        if let Some(path) = &request.directory {
            if !path.as_os_str().is_empty() {
                return Intent::ListDirectory { path: path.clone() };
            }
        }

        let content = request.content.trim();
        let lower = content.to_ascii_lowercase();

        if let Some(name) = parse_continue_project(&lower, content) {
            return Intent::ContinueProject { name };
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

        Intent::Unknown
    }

    /// Map an intent to the capability required to fulfill it.
    pub fn required_capability(&self, intent: &Intent) -> Option<Capability> {
        match intent {
            Intent::ListDirectory { .. } => Some(Capability::Search),
            Intent::SearchKnowledge { .. } => Some(Capability::Search),
            Intent::ReadFile { .. } => Some(Capability::ReadDocuments),
            Intent::DiscoverInventory { .. } => Some(Capability::Discover),
            Intent::IndexRoots { .. } => Some(Capability::Index),
            Intent::ContinueProject { .. } => None,
            Intent::Unknown => None,
        }
    }
}

fn parse_continue_project(lower: &str, original: &str) -> Option<String> {
    let prefixes = [
        "continue working on ",
        "continue on ",
        "resume working on ",
        "resume ",
        "open project ",
        "switch to project ",
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
    fn parses_continue_working_on_project() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("Continue working on Jaymi.")),
            Intent::ContinueProject {
                name: "Jaymi".into()
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
    fn unknown_without_supported_intent() {
        let engine = DecisionEngine;
        let request = UserRequest::new("hello");
        assert_eq!(engine.determine_intent(&request), Intent::Unknown);
    }
}
