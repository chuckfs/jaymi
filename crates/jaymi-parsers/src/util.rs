//! Shared helpers for content parsers.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{
    Content, ContentMetadata, ContentSource, ContentType, EntityId, JaymiError, JaymiResult,
};

use crate::ParseRequest;

/// Decode resource bytes as UTF-8 text.
pub fn decode_utf8(bytes: &[u8]) -> JaymiResult<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|error| JaymiError::new(format!("content is not valid UTF-8: {error}")))
}

/// Current Unix epoch seconds.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Build a content identity from source, parser, and optional path.
pub fn content_id(source: ContentSource, parser_id: &str, path: Option<&Path>) -> EntityId {
    let location = path
        .map(|value| value.display().to_string())
        .unwrap_or_else(|| "anonymous".to_string());
    EntityId::new(format!("content:{}:{}:{location}", source.id(), parser_id))
}

/// Default title from the file stem when the format has no explicit title.
pub fn title_from_path(path: Option<&Path>) -> Option<String> {
    path.and_then(|value| value.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
}

/// Read created/modified timestamps from the filesystem when available.
pub fn file_timestamps(path: Option<&Path>) -> (Option<u64>, Option<u64>) {
    let Some(path) = path else {
        return (None, None);
    };
    let Ok(metadata) = fs::metadata(path) else {
        return (None, None);
    };
    (
        system_time_secs(metadata.created().ok()),
        system_time_secs(metadata.modified().ok()),
    )
}

fn system_time_secs(value: Option<SystemTime>) -> Option<u64> {
    value.and_then(|time| time.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs()))
}

/// Assemble [`Content`] with common fields filled in.
pub fn build_content(
    request: &ParseRequest<'_>,
    content_type: ContentType,
    title: Option<String>,
    text: String,
    mut metadata: ContentMetadata,
    parser_id: &str,
) -> Content {
    metadata.insert("byte_length", text.len().to_string());
    if let Some(path) = request.path {
        metadata.insert("extension", extension_of(path));
    }

    let (created, modified) = match (request.created, request.modified) {
        (None, None) => file_timestamps(request.path),
        other => other,
    };

    Content {
        id: content_id(request.source, parser_id, request.path),
        source: request.source,
        path: request.path.map(Path::to_path_buf),
        mime_type: content_type.mime_type().to_string(),
        content_type,
        title,
        text,
        metadata,
        created,
        modified,
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
