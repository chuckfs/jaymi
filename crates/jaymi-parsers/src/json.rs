//! JSON parser (`.json`).

use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiError, JaymiResult};

use crate::parser::FileParser;
use crate::util::{build_document, decode_utf8, title_from_path};

/// Parser for JSON documents.
///
/// Validates UTF-8 JSON syntax at a shallow level and stores the original text.
/// No schema inference or semantic analysis is performed.
#[derive(Debug, Default)]
pub struct JsonParser;

impl FileParser for JsonParser {
    fn id(&self) -> &'static str {
        "json"
    }

    fn name(&self) -> &'static str {
        "JSON"
    }

    fn supported_types(&self) -> &[FileType] {
        &[FileType::Json]
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document> {
        let text = decode_utf8(bytes)?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(JaymiError::new("JSON file is empty"));
        }

        let kind = classify_json(trimmed)?;
        let title = extract_json_title(trimmed).or_else(|| title_from_path(path));

        let mut metadata = DocumentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("json_kind", kind);
        metadata.insert("valid_json_shape", "true");

        Ok(build_document(
            path,
            FileType::Json,
            title,
            text,
            metadata,
            self.id(),
        ))
    }
}

fn classify_json(text: &str) -> JaymiResult<&'static str> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        return Ok("object");
    }
    if trimmed.starts_with('[') {
        return Ok("array");
    }
    if trimmed.starts_with('"') {
        return Ok("string");
    }
    if trimmed.starts_with("true") || trimmed.starts_with("false") {
        return Ok("boolean");
    }
    if trimmed.starts_with("null") {
        return Ok("null");
    }
    if trimmed
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit() || ch == '-')
    {
        return Ok("number");
    }
    Err(JaymiError::new(format!(
        "file does not look like JSON (starts with '{}')",
        trimmed.chars().next().unwrap_or('?')
    )))
}

fn extract_json_title(text: &str) -> Option<String> {
    // Lightweight extraction: "title": "..." or "name": "..."
    for key in ["\"title\"", "\"name\""] {
        if let Some(index) = text.find(key) {
            let after = &text[index + key.len()..];
            let after = after.trim_start().strip_prefix(':')?.trim_start();
            if let Some(rest) = after.strip_prefix('"') {
                let mut value = String::new();
                let mut chars = rest.chars();
                while let Some(ch) = chars.next() {
                    match ch {
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                value.push(escaped);
                            }
                        }
                        '"' => return Some(value),
                        other => value.push(other),
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_object_with_title() {
        let parser = JsonParser;
        let source = br#"{ "title": "Config", "enabled": true }"#;
        let document = parser.parse(Path::new("config.json"), source).unwrap();
        assert_eq!(document.file_type, FileType::Json);
        assert_eq!(document.title.as_deref(), Some("Config"));
        assert_eq!(document.metadata.get("json_kind"), Some("object"));
        assert!(document.text.contains("enabled"));
    }

    #[test]
    fn parses_json_array() {
        let parser = JsonParser;
        let document = parser
            .parse(Path::new("items.json"), b"[1, 2, 3]")
            .unwrap();
        assert_eq!(document.metadata.get("json_kind"), Some("array"));
        assert_eq!(document.title.as_deref(), Some("items"));
    }

    #[test]
    fn rejects_non_json_prefix() {
        let parser = JsonParser;
        let error = parser
            .parse(Path::new("bad.json"), b"not-json")
            .unwrap_err();
        assert!(error.message().contains("does not look like JSON"));
    }
}
