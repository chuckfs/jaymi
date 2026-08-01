//! Content parser trait implemented by every format-specific parser.

use jaymi_core::{Content, ContentType, JaymiResult};

use crate::ParseRequest;

/// Converts a raw resource into unified [`Content`].
///
/// Parsers must remain focused: extract text and lightweight metadata only.
/// No semantic analysis, indexing, embeddings, or OCR.
///
/// New formats are added by implementing this trait and registering with the
/// [`crate::ContentRegistry`] — no Planner changes required.
pub trait ContentParser: Send + Sync {
    /// Stable parser identifier (for example `plain_text`).
    fn id(&self) -> &'static str;

    /// Human-readable parser name.
    fn name(&self) -> &'static str;

    /// Content types this parser can handle.
    fn supported_types(&self) -> &[ContentType];

    /// Parse a resource described by [`ParseRequest`] into unified Content.
    fn parse(&self, request: &ParseRequest<'_>) -> JaymiResult<Content>;
}
