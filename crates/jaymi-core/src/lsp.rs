//! Language Server Protocol result types shared across Planner / tools / UI.

/// Zero-based text position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LspPosition {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based UTF-16 character offset (LSP convention).
    pub character: u32,
}

/// Inclusive start / exclusive end range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LspRange {
    /// Range start.
    pub start: LspPosition,
    /// Range end.
    pub end: LspPosition,
}

/// A file location with a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspLocation {
    /// Absolute file path.
    pub path: String,
    /// Range within the file.
    pub range: LspRange,
}

/// Hover contents for a cursor position.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LspHover {
    /// Markdown / plaintext hover body.
    pub contents: String,
    /// Optional range the hover applies to.
    pub range: Option<LspRange>,
}

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspCompletionItem {
    /// Primary label shown in the list.
    pub label: String,
    /// Optional kind label (`function`, `variable`, …).
    pub kind: Option<String>,
    /// Optional detail string.
    pub detail: Option<String>,
    /// Text inserted on accept (defaults to label).
    pub insert_text: Option<String>,
}

/// One diagnostic published by a language server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDiagnostic {
    /// Absolute file path.
    pub path: String,
    /// Human-readable message.
    pub message: String,
    /// Severity label (`error`, `warning`, `info`, `hint`).
    pub severity: String,
    /// Diagnostic range.
    pub range: LspRange,
    /// Optional diagnostic source (e.g. `rust-analyzer`).
    pub source: Option<String>,
}

/// One text edit (used by rename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspTextEdit {
    /// Absolute file path.
    pub path: String,
    /// Range to replace.
    pub range: LspRange,
    /// Replacement text.
    pub new_text: String,
}

/// Language server operations exposed through the LSP tool / provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspOperation {
    /// Ensure a language server session for the workspace root.
    Ensure,
    /// Notify the server a document was opened.
    DidOpen,
    /// Notify the server a document changed.
    DidChange,
    /// Notify the server a document was closed.
    DidClose,
    /// Request hover information.
    Hover,
    /// Request completions.
    Completion,
    /// Read cached diagnostics for a path (or whole workspace when path is unset).
    Diagnostics,
    /// Go to definition.
    Definition,
    /// Rename the symbol under the cursor.
    Rename,
    /// Find all references.
    References,
}

impl LspOperation {
    /// Stable label for diagnostics and logging.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ensure => "ensure",
            Self::DidOpen => "did_open",
            Self::DidChange => "did_change",
            Self::DidClose => "did_close",
            Self::Hover => "hover",
            Self::Completion => "completion",
            Self::Diagnostics => "diagnostics",
            Self::Definition => "definition",
            Self::Rename => "rename",
            Self::References => "references",
        }
    }

    /// Whether this operation mutates editor buffers (rename workspace edits).
    pub fn is_mutating(self) -> bool {
        matches!(self, Self::Rename)
    }
}

/// Structured LSP request mediated by the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRequest {
    /// Workspace / project root (rust-analyzer project root).
    pub workspace_root: std::path::PathBuf,
    /// Operation to perform.
    pub operation: LspOperation,
    /// Document path for document-scoped operations.
    pub path: Option<std::path::PathBuf>,
    /// Full document text for didOpen / didChange.
    pub content: Option<String>,
    /// Language id (e.g. `rust`).
    pub language: Option<String>,
    /// Document version for didOpen / didChange.
    pub version: Option<i32>,
    /// Zero-based line for position-based requests.
    pub line: Option<u32>,
    /// Zero-based character for position-based requests.
    pub character: Option<u32>,
    /// New symbol name for rename.
    pub new_name: Option<String>,
}
