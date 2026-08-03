//! Project types for the Project Engine.

use std::path::PathBuf;

use jaymi_core::EntityId;

/// High-level project classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProjectType {
    /// Unspecified / general workspace.
    #[default]
    General,
    /// Source-code focused project.
    Code,
    /// Document / notes focused project.
    Documents,
    /// Mixed code and documents.
    Mixed,
}

impl ProjectType {
    /// Stable persistence label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Code => "code",
            Self::Documents => "documents",
            Self::Mixed => "mixed",
        }
    }

    /// Parse a persisted type label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "general" | "default" => Some(Self::General),
            "code" | "software" | "dev" => Some(Self::Code),
            "documents" | "docs" | "notes" => Some(Self::Documents),
            "mixed" => Some(Self::Mixed),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Project lifecycle status in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProjectStatus {
    /// Available for open / list.
    #[default]
    Active,
    /// Soft-deleted.
    Deleted,
}

impl ProjectStatus {
    /// Stable persistence label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deleted => "deleted",
        }
    }

    /// Parse a persisted status label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

/// First-class Jaymi project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Unique identity.
    pub id: EntityId,
    /// Display name.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// Workspace root directory, when known.
    pub root_directory: Option<PathBuf>,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds last updated.
    pub updated_at: i64,
    /// Unix seconds last opened, when any.
    pub last_opened_at: Option<i64>,
    /// Project classification.
    pub project_type: ProjectType,
    /// Store status.
    pub status: ProjectStatus,
}

/// Request to create a persistent project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectRequest {
    /// Optional explicit identity.
    pub project_id: Option<String>,
    /// Display name (required).
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional workspace root.
    pub root_directory: Option<PathBuf>,
    /// Optional type (default General).
    pub project_type: Option<ProjectType>,
}

/// Aggregate Project Engine statistics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectStats {
    /// Active project count.
    pub active_count: u64,
    /// Soft-deleted count.
    pub deleted_count: u64,
    /// Currently open project id, when any.
    pub open_project_id: Option<String>,
}

/// Health snapshot for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectHealth {
    /// Whether initialization completed.
    pub initialized: bool,
    /// Whether the engine can serve project operations.
    pub healthy: bool,
    /// Version string.
    pub version: String,
    /// Short detail.
    pub detail: String,
    /// Latest stats.
    pub statistics: ProjectStats,
}

/// Build a URL-safe slug from a project name.
pub fn slugify_project_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "project".into()
    } else {
        slug
    }
}
