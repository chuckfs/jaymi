//! User-facing request types that enter the Planner.

use std::path::PathBuf;

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
    /// Optional structured file path for read-file intents.
    pub file: Option<PathBuf>,
}

impl UserRequest {
    /// Create a new user request from free-form content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            directory: None,
            file: None,
        }
    }

    /// Create a structured request to list a single directory.
    pub fn list_directory(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("list {}", path.display()),
            directory: Some(path),
            file: None,
        }
    }

    /// Create a structured request to read a single file.
    pub fn read_file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("read {}", path.display()),
            directory: None,
            file: Some(path),
        }
    }
}
