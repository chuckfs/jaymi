//! File parsers and parser registry for Jaymi.
//!
//! Converts supported files into the unified [`jaymi_core::Document`] model.
//! The Planner never depends on concrete parser implementations.

#![forbid(unsafe_code)]

pub mod json;
pub mod markdown;
pub mod parser;
pub mod plain_text;
pub mod registry;
mod util;

pub use json::JsonParser;
pub use markdown::MarkdownParser;
pub use parser::FileParser;
pub use plain_text::PlainTextParser;
pub use registry::ParserRegistry;

use std::sync::Arc;

use jaymi_core::JaymiResult;

/// Create an initialized registry with the built-in TXT / Markdown / JSON parsers.
pub fn default_registry() -> JaymiResult<ParserRegistry> {
    let mut registry = ParserRegistry::new();
    registry.initialize()?;
    registry.register(Arc::new(PlainTextParser))?;
    registry.register(Arc::new(MarkdownParser))?;
    registry.register(Arc::new(JsonParser))?;
    Ok(registry)
}
