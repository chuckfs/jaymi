//! Shared helpers for format parsers.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{Document, DocumentMetadata, EntityId, FileType, JaymiError, JaymiResult};

/// Decode file bytes as UTF-8 text.
pub fn decode_utf8(bytes: &[u8]) -> JaymiResult<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|error| JaymiError::new(format!("file is not valid UTF-8: {error}")))
}

/// Current Unix epoch seconds.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Build a document identity from path and parser.
pub fn document_id(path: &Path, parser_id: &str) -> EntityId {
    EntityId::new(format!("doc:{}:{}", parser_id, path.display()))
}

/// Default title from the file stem when the format has no explicit title.
pub fn title_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
}

/// Assemble a [`Document`] with common fields filled in.
pub fn build_document(
    path: &Path,
    file_type: FileType,
    title: Option<String>,
    text: String,
    mut metadata: DocumentMetadata,
    parser_id: &str,
) -> Document {
    metadata.insert("byte_length", text.len().to_string());
    metadata.insert("extension", extension_of(path));
    Document {
        id: document_id(path, parser_id),
        path: path.to_path_buf(),
        file_type,
        title,
        text,
        metadata,
        parsed_at: now_unix(),
        parser_id: parser_id.to_string(),
    }
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
