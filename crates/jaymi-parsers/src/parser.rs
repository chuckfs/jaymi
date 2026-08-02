//! File parser trait implemented by every format-specific parser.

use std::path::Path;

use jaymi_core::{Document, FileType, JaymiResult};

/// Converts raw file bytes into a [`Document`].
///
/// Parsers must remain focused: extract text and lightweight metadata only.
/// No semantic analysis, indexing, embeddings, or OCR.
pub trait FileParser: Send + Sync {
    /// Stable parser identifier (for example `plain_text`).
    fn id(&self) -> &'static str;

    /// Human-readable parser name.
    fn name(&self) -> &'static str;

    /// File types this parser can handle.
    fn supported_types(&self) -> &[FileType];

    /// Parse file bytes from `path` into a unified document.
    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document>;

    /// Parser package/version string stored with normalized content.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}
