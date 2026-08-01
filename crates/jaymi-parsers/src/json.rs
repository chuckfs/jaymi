//! JSON content parser (`.json`).

use jaymi_core::{Content, ContentMetadata, ContentType, JaymiError, JaymiResult};

use crate::parser::ContentParser;
use crate::util::{build_content, decode_utf8, title_from_path};
use crate::ParseRequest;

/// Parser for JSON content.
///
/// Validates UTF-8 JSON syntax at a shallow level and stores the original text.
/// No schema inference or semantic analysis is performed.
#[derive(Debug, Default)]
pub struct JsonParser;

impl ContentParser for JsonParser {
    fn id(&self) -> &'static str {
        "json"
    }

    fn name(&self) -> &'static str {
        "JSON"
    }

    fn supported_types(&self) -> &[ContentType] {
        &[ContentType::Json]
    }

    fn parse(&self, request: &ParseRequest<'_>) -> JaymiResult<Content> {
        let text = decode_utf8(request.bytes)?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(JaymiError::new("JSON content is empty"));
        }

        let kind = classify_json(trimmed)?;
        let title = extract_json_title(trimmed).or_else(|| title_from_path(request.path));

        let mut metadata = ContentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("json_kind", kind);
        metadata.insert("valid_json_shape", "true");

        Ok(build_content(
            request,
            ContentType::Json,
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
        "content does not look like JSON (starts with '{}')",
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
    use jaymi_core::ContentSource;
    use std::path::Path;

    #[test]
    fn parses_json_object_with_title() {
        let parser = JsonParser;
        let source = br#"{ "title": "Config", "enabled": true }"#;
        let content = parser
            .parse(&ParseRequest::file(Path::new("config.json"), source))
            .unwrap();
        assert_eq!(content.source, ContentSource::File);
        assert_eq!(content.content_type, ContentType::Json);
        assert_eq!(content.mime_type, "application/json");
        assert_eq!(content.title.as_deref(), Some("Config"));
        assert_eq!(content.metadata.get("json_kind"), Some("object"));
        assert!(content.text.contains("enabled"));
    }

    #[test]
    fn parses_json_array() {
        let parser = JsonParser;
        let content = parser
            .parse(&ParseRequest::file(Path::new("items.json"), b"[1, 2, 3]"))
            .unwrap();
        assert_eq!(content.metadata.get("json_kind"), Some("array"));
        assert_eq!(content.title.as_deref(), Some("items"));
    }

    #[test]
    fn rejects_non_json_prefix() {
        let parser = JsonParser;
        let error = parser
            .parse(&ParseRequest::file(Path::new("bad.json"), b"not-json"))
            .unwrap_err();
        assert!(error.message().contains("does not look like JSON"));
    }
}
