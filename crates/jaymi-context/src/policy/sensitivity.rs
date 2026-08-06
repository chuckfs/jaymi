//! Sensitivity labels for context provider metadata.

use serde::Serialize;

/// How sensitive a provider's contribution is.
///
/// Ordered from least to most sensitive. Policies prevent higher-sensitivity
/// context from being assembled unless the request requires it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Safe to share broadly (summaries, public capability ids).
    Public = 0,
    /// Tied to the active UX workspace.
    Workspace = 1,
    /// Tied to the open project.
    Project = 2,
    /// User-private (conversation, editor buffers, memories).
    Private = 3,
    /// Highly sensitive (secrets, credentials, private personal data).
    Sensitive = 4,
}

impl Sensitivity {
    /// Stable label for diagnostics and serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Workspace => "workspace",
            Self::Project => "project",
            Self::Private => "private",
            Self::Sensitive => "sensitive",
        }
    }

    /// Default sensitivity for a known provider id.
    pub fn for_provider(provider_id: &str) -> Self {
        match provider_id {
            "permission" => Self::Public,
            "workspace" => Self::Workspace,
            "project" | "diagnostics" | "search" => Self::Project,
            "conversation" | "editor" | "memory" => Self::Private,
            _ => Self::Private,
        }
    }
}

impl std::fmt::Display for Sensitivity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
