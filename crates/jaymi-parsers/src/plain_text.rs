//! Plain text parser (`.txt`).

use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiResult};

use crate::parser::FileParser;
use crate::util::{build_document, decode_utf8, title_from_path};

/// Parser for plain UTF-8 text files.
#[derive(Debug, Default)]
pub struct PlainTextParser;

impl FileParser for PlainTextParser {
    fn id(&self) -> &'static str {
        "plain_text"
    }

    fn name(&self) -> &'static str {
        "Plain Text"
    }

    fn supported_types(&self) -> &[FileType] {
        &[FileType::PlainText]
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document> {
        let text = decode_utf8(bytes)?;
        let mut metadata = DocumentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("line_count", text.lines().count().to_string());

        Ok(build_document(
            path,
            FileType::PlainText,
            title_from_path(path),
            text,
            metadata,
            self.id(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn parses_plain_text() {
        let parser = PlainTextParser;
        let document = parser
            .parse(Path::new("/tmp/notes.txt"), fixtures::plain_text())
            .unwrap();
        assert_eq!(document.file_type, FileType::PlainText);
        assert_eq!(document.title.as_deref(), Some("notes"));
        assert!(document.text.contains("Hello plain text"));
        assert_eq!(document.parser_id, "plain_text");
        assert_eq!(document.metadata.get("line_count"), Some("2"));
        assert!(document.metadata.get("extension").is_some());
    }
}
