//! Content parsers and content registry for Jaymi.
//!
//! Converts supported resources into the unified [`jaymi_core::Content`] model.
//! The Planner never depends on concrete parser implementations.
//!
//! Future providers (messages, email, web, vision) can produce Content through
//! the same registry without Planner changes.

#![forbid(unsafe_code)]

pub mod json;
pub mod markdown;
pub mod parser;
pub mod plain_text;
pub mod registry;
mod util;

pub use json::JsonParser;
pub use markdown::MarkdownParser;
pub use parser::ContentParser;
pub use plain_text::PlainTextParser;
pub use registry::ContentRegistry;

use std::path::Path;
use std::sync::Arc;

use jaymi_core::{ContentSource, JaymiResult};

/// Input supplied to a [`ContentParser`].
///
/// File-backed reads populate `path` and timestamps. Future providers may omit
/// the path while still producing Content through the same trait.
#[derive(Debug, Clone, Copy)]
pub struct ParseRequest<'a> {
    /// Origin of the raw resource.
    pub source: ContentSource,
    /// Optional original path (filesystem resources).
    pub path: Option<&'a Path>,
    /// Raw resource bytes.
    pub bytes: &'a [u8],
    /// Optional created timestamp (unix seconds).
    pub created: Option<u64>,
    /// Optional modified timestamp (unix seconds).
    pub modified: Option<u64>,
}

impl<'a> ParseRequest<'a> {
    /// Build a file-sourced parse request.
    pub fn file(path: &'a Path, bytes: &'a [u8]) -> Self {
        Self {
            source: ContentSource::File,
            path: Some(path),
            bytes,
            created: None,
            modified: None,
        }
    }
}

/// Create an initialized registry with the built-in TXT / Markdown / JSON parsers.
pub fn default_registry() -> JaymiResult<ContentRegistry> {
    let mut registry = ContentRegistry::new();
    registry.initialize()?;
    registry.register(Arc::new(PlainTextParser))?;
    registry.register(Arc::new(MarkdownParser))?;
    registry.register(Arc::new(JsonParser))?;
    Ok(registry)
}
