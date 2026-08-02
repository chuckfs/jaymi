//! Markdown parser (`.md`, `.markdown`).

use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiResult};

use crate::parser::FileParser;
use crate::util::{build_document, decode_utf8, title_from_path};

/// Parser for Markdown documents.
///
/// Extracts an optional title from the first ATX heading (`# Title`).
/// Performs no semantic analysis beyond that lightweight extraction.
#[derive(Debug, Default)]
pub struct MarkdownParser;

impl FileParser for MarkdownParser {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn name(&self) -> &'static str {
        "Markdown"
    }

    fn supported_types(&self) -> &[FileType] {
        &[FileType::Markdown]
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document> {
        let text = decode_utf8(bytes)?;
        let heading = first_atx_heading(&text);
        let mut metadata = DocumentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("line_count", text.lines().count().to_string());
        if heading.is_some() {
            metadata.insert("title_source", "atx_heading");
        } else {
            metadata.insert("title_source", "filename");
        }

        Ok(build_document(
            path,
            FileType::Markdown,
            heading.or_else(|| title_from_path(path)),
            text,
            metadata,
            self.id(),
        ))
    }
}

fn first_atx_heading(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let title = rest.trim_start_matches('#').trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn extracts_heading_title() {
        let parser = MarkdownParser;
        let document = parser
            .parse(Path::new("ARCHITECTURE.md"), fixtures::markdown())
            .unwrap();
        assert_eq!(document.file_type, FileType::Markdown);
        assert_eq!(document.title.as_deref(), Some("Fixture Title"));
        assert!(document.text.contains("Body paragraph."));
        assert_eq!(document.metadata.get("title_source"), Some("atx_heading"));
    }

    #[test]
    fn falls_back_to_filename_title() {
        let parser = MarkdownParser;
        let document = parser
            .parse(Path::new("README.md"), b"No heading here.\n")
            .unwrap();
        assert_eq!(document.title.as_deref(), Some("README"));
        assert_eq!(document.metadata.get("title_source"), Some("filename"));
    }
}
