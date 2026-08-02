//! Shared helpers for format parsers.

use std::fs;
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

/// Insert filesystem creation/modification timestamps when available.
///
/// Does not overwrite dates already supplied by the format (PDF Info, DOCX
/// core properties, etc.).
pub fn insert_filesystem_dates(metadata: &mut DocumentMetadata, path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        if metadata.get("modification_date").is_none() {
            if let Ok(modified) = meta.modified() {
                if let Some(secs) = system_time_secs(modified) {
                    metadata.insert("modification_date", secs.to_string());
                    metadata.insert("modification_date_source", "filesystem");
                }
            }
        }
        if metadata.get("creation_date").is_none() {
            if let Ok(created) = meta.created() {
                if let Some(secs) = system_time_secs(created) {
                    metadata.insert("creation_date", secs.to_string());
                    metadata.insert("creation_date_source", "filesystem");
                }
            }
        }
    }
}

/// Insert optional author metadata when present.
pub fn insert_author(metadata: &mut DocumentMetadata, author: Option<&str>) {
    if let Some(author) = author.map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert("author", author);
    }
}

/// Insert page count when applicable.
pub fn insert_page_count(metadata: &mut DocumentMetadata, pages: Option<u64>) {
    if let Some(pages) = pages {
        metadata.insert("page_count", pages.to_string());
    }
}

fn system_time_secs(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
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
    insert_filesystem_dates(&mut metadata, path);
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
