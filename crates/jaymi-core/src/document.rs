//! Unified document model returned by file parsers.
//!
//! Every parser — regardless of source format — produces this structure so the
//! Planner can reason over documents without knowing parser details.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::id::EntityId;

/// Logical file type recognized by the reading pipeline.
///
/// Unknown variants allow future parsers to extend detection without changing
/// the Planner. Concrete parsers register against these identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileType {
    /// Plain text (`.txt`).
    PlainText,
    /// Markdown (`.md`, `.markdown`).
    Markdown,
    /// JSON (`.json`).
    Json,
    /// PDF (`.pdf`).
    Pdf,
    /// Office Open XML Word document (`.docx`).
    Docx,
    /// Raster image (`.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.tif`, `.tiff`).
    Image,
    /// Extensible catch-all for future formats.
    Other(String),
}

impl FileType {
    /// Stable lowercase identity used by the parser registry.
    pub fn id(&self) -> &str {
        match self {
            Self::PlainText => "plain_text",
            Self::Markdown => "markdown",
            Self::Json => "json",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Image => "image",
            Self::Other(value) => value.as_str(),
        }
    }

    /// Human-readable label for diagnostics and CLI output.
    pub fn label(&self) -> String {
        match self {
            Self::PlainText => "Plain Text".to_string(),
            Self::Markdown => "Markdown".to_string(),
            Self::Json => "JSON".to_string(),
            Self::Pdf => "PDF".to_string(),
            Self::Docx => "DOCX".to_string(),
            Self::Image => "Image".to_string(),
            Self::Other(value) => value.clone(),
        }
    }
}

impl std::fmt::Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Extensible key/value metadata attached to a parsed document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    /// Ordered metadata entries for stable display.
    entries: BTreeMap<String, String>,
}

impl DocumentMetadata {
    /// Create empty metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a metadata value.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Borrow a metadata value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Iterate metadata entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.entries.iter()
    }

    /// Number of metadata entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no metadata is present.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Unified document representation produced by every file parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Unique document identity.
    pub id: EntityId,
    /// Original filesystem path.
    pub path: PathBuf,
    /// Detected / declared file type.
    pub file_type: FileType,
    /// Optional document title when the format exposes one.
    pub title: Option<String>,
    /// Extracted raw text content.
    pub text: String,
    /// Format-specific and common metadata.
    pub metadata: DocumentMetadata,
    /// Unix epoch seconds when parsing completed.
    pub parsed_at: u64,
    /// Parser that produced this document.
    pub parser_id: String,
}

impl Document {
    /// Character count of the parsed text.
    pub fn character_count(&self) -> usize {
        self.text.chars().count()
    }

    /// Return a preview of the parsed text limited to `max_chars`.
    pub fn preview(&self, max_chars: usize) -> String {
        let count = self.text.chars().count();
        if count <= max_chars {
            return self.text.clone();
        }
        let preview: String = self.text.chars().take(max_chars).collect();
        format!("{preview}…")
    }
}
