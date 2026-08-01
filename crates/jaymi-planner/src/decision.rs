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
    /// Request could not be mapped to a supported intent.
    Unknown,
}

/// Deterministic decision-making component of the Planner.
#[derive(Debug, Default)]
pub struct DecisionEngine;

impl DecisionEngine {
    /// Determine user intent without language-model reasoning.
    ///
    /// Supported list-directory forms:
    /// - structured [`UserRequest::directory`]
    /// - content beginning with `list ` followed by a path
    pub fn determine_intent(&self, request: &UserRequest) -> Intent {
        if let Some(path) = &request.directory {
            if !path.as_os_str().is_empty() {
                return Intent::ListDirectory { path: path.clone() };
            }
        }

        let content = request.content.trim();
        if let Some(rest) = content.strip_prefix("list ") {
            let path = rest.trim().trim_matches('"').trim_matches('\'');
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
            Intent::Unknown => None,
        }
    }
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
    fn parses_list_prefix() {
        let engine = DecisionEngine;
        let request = UserRequest::new("list \"./docs\"");
        assert_eq!(
            engine.determine_intent(&request),
            Intent::ListDirectory {
                path: PathBuf::from("./docs")
            }
        );
    }

    #[test]
    fn unknown_without_list_intent() {
        let engine = DecisionEngine;
        let request = UserRequest::new("hello");
        assert_eq!(engine.determine_intent(&request), Intent::Unknown);
    }
}
