//! Content store surface — consumers never touch SQLite content tables.

use jaymi_core::JaymiResult;

use crate::content::Content;

/// Persistence API for normalized content.
pub trait ContentStore: Send + Sync {
    /// Load content by source path identity.
    fn get_by_source_id(&self, source_id: &str) -> JaymiResult<Option<Content>>;

    /// Insert or replace content for a source.
    fn upsert(&self, content: &Content) -> JaymiResult<()>;

    /// Remove content for a source path.
    fn remove_by_source_id(&self, source_id: &str) -> JaymiResult<()>;

    /// True when content exists for the source.
    fn exists(&self, source_id: &str) -> JaymiResult<bool>;

    /// Number of stored documents.
    fn document_count(&self) -> JaymiResult<u64>;

    /// Parser usage histogram `(parser_id, count)`.
    fn parser_usage(&self) -> JaymiResult<Vec<(String, u64)>>;
}
