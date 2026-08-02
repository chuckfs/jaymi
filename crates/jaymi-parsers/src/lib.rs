//! File parsers and parser registry for Jaymi.
//!
//! Converts supported files into the unified [`jaymi_core::Document`] model.
//! The Planner never depends on concrete parser implementations.

#![forbid(unsafe_code)]

pub mod docx;
pub mod fixtures;
pub mod image;
pub mod json;
pub mod markdown;
pub mod parser;
pub mod pdf;
pub mod plain_text;
pub mod registry;
mod util;

pub use docx::DocxParser;
pub use image::ImageParser;
pub use json::JsonParser;
pub use markdown::MarkdownParser;
pub use parser::FileParser;
pub use pdf::PdfParser;
pub use plain_text::PlainTextParser;
pub use registry::ParserRegistry;

use std::sync::Arc;

use jaymi_core::JaymiResult;

/// Create an initialized registry with the built-in document and image parsers.
pub fn default_registry() -> JaymiResult<ParserRegistry> {
    let mut registry = ParserRegistry::new();
    registry.initialize()?;
    registry.register(Arc::new(PlainTextParser))?;
    registry.register(Arc::new(MarkdownParser))?;
    registry.register(Arc::new(JsonParser))?;
    registry.register(Arc::new(PdfParser))?;
    registry.register(Arc::new(DocxParser))?;
    registry.register(Arc::new(ImageParser))?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::FileType;
    use std::path::Path;

    #[test]
    fn default_registry_registers_document_and_image_parsers() {
        let registry = default_registry().unwrap();
        assert_eq!(
            registry.parser_ids(),
            vec![
                "docx",
                "image",
                "json",
                "markdown",
                "pdf",
                "plain_text"
            ]
        );
        for file_type in [
            FileType::PlainText,
            FileType::Markdown,
            FileType::Json,
            FileType::Pdf,
            FileType::Docx,
            FileType::Image,
        ] {
            assert!(registry.resolve(&file_type).is_ok());
        }
        assert_eq!(
            ParserRegistry::detect_type(Path::new("a.png")),
            Some(FileType::Image)
        );
    }
}
