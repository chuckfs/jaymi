//! Unified content model returned by content parsers.
//!
//! Every parser — regardless of source format — produces this structure so the
//! Planner can reason over Content without knowing parser or provider details.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::id::EntityId;

/// Where a piece of Content originated.
///
/// Only [`ContentSource::File`] is produced by the current pipeline. The other
/// variants are stable placeholders for future providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentSource {
    /// Local or remote filesystem resource.
    File,
    /// Chat / messaging content.
    Message,
    /// Email message body or attachment text.
    Email,
    /// Web page or remote HTML/resource.
    Web,
    /// System clipboard.
    Clipboard,
    /// Text produced by OCR.
    Ocr,
    /// Image-derived content (future vision pipeline).
    Image,
    /// Synthetically generated content.
    Generated,
}

impl ContentSource {
    /// Stable lowercase identity.
    pub fn id(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Message => "message",
            Self::Email => "email",
            Self::Web => "web",
            Self::Clipboard => "clipboard",
            Self::Ocr => "ocr",
            Self::Image => "image",
            Self::Generated => "generated",
        }
    }

    /// Human-readable label for diagnostics and CLI output.
    pub fn label(&self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Message => "Message",
            Self::Email => "Email",
            Self::Web => "Web",
            Self::Clipboard => "Clipboard",
            Self::Ocr => "OCR",
            Self::Image => "Image",
            Self::Generated => "Generated",
        }
    }
}

impl std::fmt::Display for ContentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Logical content type recognized by the reading pipeline.
///
/// Unknown variants allow future parsers to extend detection without changing
/// the Planner. Concrete parsers register against these identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContentType {
    /// Plain text (`.txt`).
    PlainText,
    /// Markdown (`.md`, `.markdown`).
    Markdown,
    /// JSON (`.json`).
    Json,
    /// Extensible catch-all for future formats (PDF, DOCX, source code, etc.).
    Other(String),
}

impl ContentType {
    /// Stable lowercase identity used by the content registry.
    pub fn id(&self) -> &str {
        match self {
            Self::PlainText => "plain_text",
            Self::Markdown => "markdown",
            Self::Json => "json",
            Self::Other(value) => value.as_str(),
        }
    }

    /// Human-readable label for diagnostics and CLI output.
    pub fn label(&self) -> String {
        match self {
            Self::PlainText => "Plain Text".to_string(),
            Self::Markdown => "Markdown".to_string(),
            Self::Json => "JSON".to_string(),
            Self::Other(value) => value.clone(),
        }
    }

    /// Default MIME type associated with this content type.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::PlainText => "text/plain",
            Self::Markdown => "text/markdown",
            Self::Json => "application/json",
            Self::Other(_) => "application/octet-stream",
        }
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Extensible key/value metadata attached to parsed content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentMetadata {
    /// Ordered metadata entries for stable display.
    entries: BTreeMap<String, String>,
}

impl ContentMetadata {
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

/// Unified content representation produced by every content parser.
///
/// The Planner reasons over [`Content`] regardless of whether it originated
/// from a file, message, email, web page, or a future provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    /// Unique content identity.
    pub id: EntityId,
    /// Origin of this content.
    pub source: ContentSource,
    /// Original path when the source exposes one (for example a file path).
    pub path: Option<PathBuf>,
    /// MIME type describing the resource.
    pub mime_type: String,
    /// Logical content type used for parser selection.
    pub content_type: ContentType,
    /// Optional title when the format or source exposes one.
    pub title: Option<String>,
    /// Extracted raw text.
    pub text: String,
    /// Source-specific and common metadata.
    pub metadata: ContentMetadata,
    /// Unix epoch seconds when the underlying resource was created, if known.
    pub created: Option<u64>,
    /// Unix epoch seconds when the underlying resource was last modified, if known.
    pub modified: Option<u64>,
    /// Unix epoch seconds when parsing completed.
    pub parsed_at: u64,
    /// Content parser that produced this object (diagnostics / provenance).
    pub parser_id: String,
}

impl Content {
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
