//! Decision Engine — deterministic application logic.
//!
//! Answers questions that should never depend on a language model:
//! intent routing, project awareness, permission needs, capability selection.

use std::path::PathBuf;

use jaymi_capabilities::Capability;
use jaymi_core::UserRequest;

/// Deterministic intents recognized by the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// List the immediate contents of one directory.
    ListDirectory {
        /// Directory path to list.
        path: PathBuf,
    },
    /// Read one supported file into unified Content.
    ReadFile {
        /// File path to read.
        path: PathBuf,
    },
    /// Query the local knowledge index (“what exists?”).
    QueryIndex {
        /// Optional free-text filter.
        query: Option<String>,
        /// Optional indexed root label.
        source_root: Option<String>,
    },
    /// Refresh / build the local knowledge index.
    IndexKnowledge,
    /// Free-form conversational message.
    Chat {
        /// Message text from the user.
        message: String,
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

        if looks_like_index_request(content) {
            return Intent::IndexKnowledge;
        }

        if let Some(intent) = looks_like_existence_query(content) {
            return intent;
        }

        if !content.is_empty() {
            return Intent::Chat {
                message: content.to_string(),
            };
        }

        Intent::Unknown
    }

    /// Map an intent to the capability required to fulfill it.
    pub fn required_capability(&self, intent: &Intent) -> Option<Capability> {
        match intent {
            Intent::ListDirectory { .. }
            | Intent::QueryIndex { .. }
            | Intent::IndexKnowledge => Some(Capability::Search),
            Intent::ReadFile { .. } => Some(Capability::ReadContent),
            Intent::Chat { .. } => Some(Capability::Chat),
            Intent::Unknown => None,
        }
    }
}

fn strip_quotes(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

fn looks_like_index_request(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower == "index"
        || lower == "index files"
        || lower == "index my files"
        || lower == "refresh index"
        || lower == "scan files"
        || lower == "update index"
        || lower.contains("index my files")
        || lower.contains("refresh the index")
        || lower.contains("scan my files")
}

fn looks_like_existence_query(content: &str) -> Option<Intent> {
    let lower = content.to_ascii_lowercase();
    let root = if lower.contains("download") {
        Some("downloads".to_string())
    } else if lower.contains("document") {
        Some("documents".to_string())
    } else if lower.contains("workspace") || lower.contains("this project") {
        Some("workspace".to_string())
    } else {
        None
    };

    let asks_existence = lower.contains("what exists")
        || lower.contains("what's on my computer")
        || lower.contains("whats on my computer")
        || lower.contains("what files")
        || lower.contains("what folders")
        || lower.contains("show files")
        || lower.contains("show me files")
        || lower.contains("list files")
        || lower.contains("what's in")
        || lower.contains("whats in")
        || lower.contains("what is in")
        || lower.starts_with("find ")
        || lower.contains("do i have");

    if !asks_existence && root.is_none() {
        return None;
    }

    // “what's in downloads?” without explicit existence phrasing still counts.
    if root.is_some()
        && (asks_existence
            || lower.contains("what's in")
            || lower.contains("whats in")
            || lower.contains("show")
            || lower.contains("list"))
    {
        return Some(Intent::QueryIndex {
            query: extract_search_term(&lower),
            source_root: root,
        });
    }

    if asks_existence {
        return Some(Intent::QueryIndex {
            query: extract_search_term(&lower),
            source_root: root,
        });
    }

    None
}

fn extract_search_term(lower: &str) -> Option<String> {
    for prefix in ["find ", "search for ", "search "] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let term = rest
                .trim()
                .trim_matches('?')
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if !term.is_empty()
                && !term.contains("download")
                && !term.contains("document")
                && term != "files"
                && term != "folders"
            {
                return Some(term.to_string());
            }
        }
    }
    None
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
            Some(Capability::ReadContent)
        );
    }

    #[test]
    fn parses_list_and_read_prefixes() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("list \"./docs\"")),
            Intent::ListDirectory {
                path: PathBuf::from("./docs")
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("read 'notes.txt'")),
            Intent::ReadFile {
                path: PathBuf::from("notes.txt")
            }
        );
    }

    #[test]
    fn what_exists_queries_index() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("What exists?")),
            Intent::QueryIndex {
                query: None,
                source_root: None
            }
        );
        assert_eq!(
            engine.determine_intent(&UserRequest::new("What's in Downloads?")),
            Intent::QueryIndex {
                query: None,
                source_root: Some("downloads".to_string())
            }
        );
    }

    #[test]
    fn index_request_detected() {
        let engine = DecisionEngine;
        assert_eq!(
            engine.determine_intent(&UserRequest::new("index my files")),
            Intent::IndexKnowledge
        );
    }

    #[test]
    fn free_form_message_is_chat() {
        let engine = DecisionEngine;
        let request = UserRequest::new("hello");
        assert_eq!(
            engine.determine_intent(&request),
            Intent::Chat {
                message: "hello".to_string()
            }
        );
        assert_eq!(
            engine.required_capability(&engine.determine_intent(&request)),
            Some(Capability::Chat)
        );
    }

    #[test]
    fn empty_message_is_unknown() {
        let engine = DecisionEngine;
        let request = UserRequest::new("   ");
        assert_eq!(engine.determine_intent(&request), Intent::Unknown);
    }
}
