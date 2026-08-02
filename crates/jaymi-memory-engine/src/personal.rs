//! Personal memory — durable, intentionally curated user preferences.
//!
//! Personal memory must never grow from accidental capture. Preferences are
//! created, updated, and deleted through explicit Memory Engine operations.

use crate::types::MemoryRecord;

/// Durable personal preference categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonalMemoryKind {
    /// Preferred name / how the user likes to be addressed.
    PreferredName,
    /// Preferred writing style.
    WritingStyle,
    /// Preferred code style / conventions.
    CodeStyle,
    /// Favorite editor.
    FavoriteEditor,
    /// Preferred UI / visual themes.
    PreferredTheme,
}

impl PersonalMemoryKind {
    /// Stable string identity persisted in `memories.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreferredName => "preferred_name",
            Self::WritingStyle => "writing_style",
            Self::CodeStyle => "code_style",
            Self::FavoriteEditor => "favorite_editor",
            Self::PreferredTheme => "preferred_theme",
        }
    }

    /// Parse a persisted kind label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "preferred_name" | "name" => Some(Self::PreferredName),
            "writing_style" | "writing" => Some(Self::WritingStyle),
            "code_style" | "coding_style" | "code" => Some(Self::CodeStyle),
            "favorite_editor" | "editor" => Some(Self::FavoriteEditor),
            "preferred_theme" | "theme" | "themes" => Some(Self::PreferredTheme),
            _ => None,
        }
    }

    /// All durable preference kinds.
    pub fn all() -> &'static [Self] {
        &[
            Self::PreferredName,
            Self::WritingStyle,
            Self::CodeStyle,
            Self::FavoriteEditor,
            Self::PreferredTheme,
        ]
    }
}

impl std::fmt::Display for PersonalMemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Intentional create of a personal preference memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePersonalMemoryRequest {
    /// Preference category (unique among active personal memories).
    pub kind: PersonalMemoryKind,
    /// Short summary of the preference.
    pub summary: String,
    /// Detailed preference content.
    pub content: String,
    /// Importance `0..=100`.
    pub importance: Option<u32>,
    /// Confidence `0..=100`.
    pub confidence: Option<u32>,
    /// Extra tags.
    pub tags: Vec<String>,
    /// Provenance (must be intentional — never auto-captured chatter).
    pub source: Option<String>,
}

/// Intentional update of an existing personal memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePersonalMemoryRequest {
    /// Memory identity to update.
    pub memory_id: String,
    /// Replacement summary, when set.
    pub summary: Option<String>,
    /// Replacement content, when set.
    pub content: Option<String>,
    /// Replacement importance, when set.
    pub importance: Option<u32>,
    /// Replacement confidence, when set.
    pub confidence: Option<u32>,
    /// Replacement tags, when set.
    pub tags: Option<Vec<String>>,
}

/// Snapshot of active personal preferences.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersonalContext {
    /// Preferred name entries.
    pub preferred_name: Vec<MemoryRecord>,
    /// Writing style entries.
    pub writing_style: Vec<MemoryRecord>,
    /// Code style entries.
    pub code_style: Vec<MemoryRecord>,
    /// Favorite editor entries.
    pub favorite_editor: Vec<MemoryRecord>,
    /// Preferred theme entries.
    pub preferred_theme: Vec<MemoryRecord>,
}

impl PersonalContext {
    /// Total active personal preference entries.
    pub fn entry_count(&self) -> usize {
        self.preferred_name.len()
            + self.writing_style.len()
            + self.code_style.len()
            + self.favorite_editor.len()
            + self.preferred_theme.len()
    }
}
