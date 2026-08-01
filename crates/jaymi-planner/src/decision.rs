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
    ///
    /// Supported forms:
    /// - structured [`UserRequest::directory`] / [`UserRequest::file`]
    /// - content beginning with `list ` or `read ` followed by a path
    /// - any other non-empty content as conversational chat
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
            Intent::ListDirectory { .. } => Some(Capability::Search),
            Intent::ReadFile { .. } => Some(Capability::ReadContent),
            Intent::Chat { .. } => Some(Capability::Chat),
            Intent::Unknown => None,
        }
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
