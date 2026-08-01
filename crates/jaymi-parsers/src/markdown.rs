//! Markdown content parser (`.md`, `.markdown`).

use jaymi_core::{Content, ContentMetadata, ContentType, JaymiResult};

use crate::parser::ContentParser;
use crate::util::{build_content, decode_utf8, title_from_path};
use crate::ParseRequest;

/// Parser for Markdown content.
///
/// Extracts an optional title from the first ATX heading (`# Title`).
/// Performs no semantic analysis beyond that lightweight extraction.
#[derive(Debug, Default)]
pub struct MarkdownParser;

impl ContentParser for MarkdownParser {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn name(&self) -> &'static str {
        "Markdown"
    }

    fn supported_types(&self) -> &[ContentType] {
        &[ContentType::Markdown]
    }

    fn parse(&self, request: &ParseRequest<'_>) -> JaymiResult<Content> {
        let text = decode_utf8(request.bytes)?;
        let heading = first_atx_heading(&text);
        let mut metadata = ContentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("line_count", text.lines().count().to_string());
        if heading.is_some() {
            metadata.insert("title_source", "atx_heading");
        } else {
            metadata.insert("title_source", "filename");
        }

        Ok(build_content(
            request,
            ContentType::Markdown,
            heading.or_else(|| title_from_path(request.path)),
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
    use jaymi_core::ContentSource;
    use std::path::Path;

    #[test]
    fn extracts_heading_title() {
        let parser = MarkdownParser;
        let source = b"# Architecture\n\nBody text.\n";
        let content = parser
            .parse(&ParseRequest::file(Path::new("ARCHITECTURE.md"), source))
            .unwrap();
        assert_eq!(content.source, ContentSource::File);
        assert_eq!(content.content_type, ContentType::Markdown);
        assert_eq!(content.mime_type, "text/markdown");
        assert_eq!(content.title.as_deref(), Some("Architecture"));
        assert!(content.text.contains("Body text."));
        assert_eq!(content.metadata.get("title_source"), Some("atx_heading"));
    }

    #[test]
    fn falls_back_to_filename_title() {
        let parser = MarkdownParser;
        let content = parser
            .parse(&ParseRequest::file(
                Path::new("README.md"),
                b"No heading here.\n",
            ))
            .unwrap();
        assert_eq!(content.title.as_deref(), Some("README"));
        assert_eq!(content.metadata.get("title_source"), Some("filename"));
    }
}
