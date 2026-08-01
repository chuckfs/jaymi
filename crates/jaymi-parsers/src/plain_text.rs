//! Plain text content parser (`.txt`).

use jaymi_core::{Content, ContentMetadata, ContentType, JaymiResult};

use crate::parser::ContentParser;
use crate::util::{build_content, decode_utf8, title_from_path};
use crate::ParseRequest;

/// Parser for plain UTF-8 text resources.
#[derive(Debug, Default)]
pub struct PlainTextParser;

impl ContentParser for PlainTextParser {
    fn id(&self) -> &'static str {
        "plain_text"
    }

    fn name(&self) -> &'static str {
        "Plain Text"
    }

    fn supported_types(&self) -> &[ContentType] {
        &[ContentType::PlainText]
    }

    fn parse(&self, request: &ParseRequest<'_>) -> JaymiResult<Content> {
        let text = decode_utf8(request.bytes)?;
        let mut metadata = ContentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("line_count", text.lines().count().to_string());

        Ok(build_content(
            request,
            ContentType::PlainText,
            title_from_path(request.path),
            text,
            metadata,
            self.id(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::ContentSource;
    use std::path::Path;

    #[test]
    fn parses_plain_text() {
        let parser = PlainTextParser;
        let path = Path::new("/tmp/notes.txt");
        let content = parser
            .parse(&ParseRequest::file(path, b"hello\nworld"))
            .unwrap();
        assert_eq!(content.source, ContentSource::File);
        assert_eq!(content.content_type, ContentType::PlainText);
        assert_eq!(content.mime_type, "text/plain");
        assert_eq!(content.title.as_deref(), Some("notes"));
        assert_eq!(content.text, "hello\nworld");
        assert_eq!(content.parser_id, "plain_text");
        assert_eq!(content.metadata.get("line_count"), Some("2"));
        assert_eq!(content.path.as_deref(), Some(path));
    }
}
