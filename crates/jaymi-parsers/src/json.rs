//! JSON parser (`.json`).

use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiError, JaymiResult};

use crate::parser::FileParser;
use crate::util::{build_document, decode_utf8, title_from_path};

/// Parser for JSON documents.
///
/// Validates JSON with `serde_json` and stores the original text. No schema
/// inference or semantic analysis is performed.
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

        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|error| JaymiError::new(format!("invalid JSON: {error}")))?;

        let kind = json_kind(&value);
        let title = extract_json_title(&value).or_else(|| title_from_path(path));

        let mut metadata = DocumentMetadata::new();
        metadata.insert("encoding", "utf-8");
        metadata.insert("json_kind", kind);
        metadata.insert("valid_json", "true");

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

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Object(_) => "object",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Null => "null",
    }
}

fn extract_json_title(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    for key in ["title", "name"] {
        if let Some(serde_json::Value::String(title)) = object.get(key) {
            let trimmed = title.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
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
    fn parses_json_object_with_title() {
        let parser = JsonParser;
        let document = parser
            .parse(Path::new("config.json"), fixtures::json())
            .unwrap();
        assert_eq!(document.file_type, FileType::Json);
        assert_eq!(document.title.as_deref(), Some("Fixture JSON"));
        assert_eq!(document.metadata.get("json_kind"), Some("object"));
        assert!(document.text.contains("tags"));
        assert!(document.metadata.get("modification_date").is_none()); // path may not exist
    }

    #[test]
    fn parses_json_array() {
        let parser = JsonParser;
        let document = parser.parse(Path::new("items.json"), b"[1, 2, 3]").unwrap();
        assert_eq!(document.metadata.get("json_kind"), Some("array"));
        assert_eq!(document.title.as_deref(), Some("items"));
    }

    #[test]
    fn rejects_invalid_json() {
        let parser = JsonParser;
        let error = parser
            .parse(Path::new("bad.json"), b"{not-json")
            .unwrap_err();
        assert!(error.message().contains("invalid JSON"));
    }

    #[test]
    fn rejects_empty_json() {
        let parser = JsonParser;
        let error = parser.parse(Path::new("empty.json"), b"   ").unwrap_err();
        assert!(error.message().contains("empty"));
    }
}
