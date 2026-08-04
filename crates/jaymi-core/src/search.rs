//! Structured search request types that enter the Planner and Search Engine.

use std::path::PathBuf;

/// Metadata filters applied during search.
///
/// Inventory browse filters and structured content-field filters both live here.
/// Content-field filters are evaluated with SQL against the content store and
/// never use full-text MATCH. Semantic ranking is handled separately by the
/// Search Engine via the Embedding Provider.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MetadataFilters {
    /// Restrict results to files.
    pub files_only: bool,
    /// Restrict results to directories.
    pub directories_only: bool,
    /// Restrict results to hidden entries.
    pub hidden_only: bool,
    /// Return only empty folders.
    pub empty_folders: bool,
    /// Order by newest modification time.
    pub recently_modified: bool,
    /// Order by newest creation time.
    pub recently_created: bool,
    /// Order by largest size.
    pub largest: bool,
    /// Named logical collection (slug or display name).
    pub collection: Option<String>,
    /// When true with `collection`, only immediate children of the collection root.
    pub collection_immediate: bool,
    /// List active collections instead of file hits.
    pub list_collections: bool,

    /// Exact content type identity (`markdown`, `pdf`, `docx`, …).
    pub content_type: Option<String>,
    /// Exact language tag (`en`, `es`, …).
    pub language: Option<String>,
    /// Author substring (case-insensitive).
    pub author: Option<String>,
    /// Require this tag (case-insensitive exact tag match).
    pub tag: Option<String>,
    /// Heading text substring (case-insensitive).
    pub heading_contains: Option<String>,
    /// Title substring (case-insensitive) — structured metadata, not FTS.
    pub title_contains: Option<String>,

    /// Inclusive lower bound on filesystem modification time (unix seconds).
    pub modified_after: Option<i64>,
    /// Inclusive upper bound on filesystem modification time (unix seconds).
    pub modified_before: Option<i64>,
    /// Inclusive lower bound on filesystem creation time (unix seconds).
    pub created_after: Option<i64>,
    /// Inclusive upper bound on filesystem creation time (unix seconds).
    pub created_before: Option<i64>,
    /// Inclusive lower bound on content extraction time (unix seconds).
    pub extracted_after: Option<i64>,
    /// Inclusive upper bound on content extraction time (unix seconds).
    pub extracted_before: Option<i64>,
}

impl MetadataFilters {
    /// True when any inventory browse filter is set.
    pub fn has_inventory_filters(&self) -> bool {
        self.files_only
            || self.directories_only
            || self.hidden_only
            || self.empty_folders
            || self.recently_modified
            || self.recently_created
            || self.largest
            || self.collection.is_some()
            || self.list_collections
    }

    /// True when any structured content-field filter is set.
    pub fn has_content_filters(&self) -> bool {
        self.content_type
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            || self
                .language
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self
                .author
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self
                .tag
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self
                .heading_contains
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self
                .title_contains
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self.modified_after.is_some()
            || self.modified_before.is_some()
            || self.created_after.is_some()
            || self.created_before.is_some()
            || self.extracted_after.is_some()
            || self.extracted_before.is_some()
    }

    /// True when any metadata filter dimension is set.
    pub fn is_active(&self) -> bool {
        self.has_inventory_filters() || self.has_content_filters()
    }
}

/// Structured search request accepted by the Search Engine.
///
/// Supports free text, filename, extension, folder, and metadata filters.
/// Match options (regex / case / whole-word) refine content locating without a
/// second index — Layer 3 still owns retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchRequest {
    /// Free-text query matched against filenames and available previews / body.
    pub free_text: Option<String>,
    /// Filename substring (case-insensitive unless [`Self::case_sensitive`]).
    pub filename: Option<String>,
    /// Extension without a leading dot (case-insensitive).
    pub extension: Option<String>,
    /// Folder path constraint.
    pub folder: Option<PathBuf>,
    /// When true with `folder`, only immediate children.
    pub folder_immediate: bool,
    /// Additional metadata filters.
    pub metadata: MetadataFilters,
    /// Maximum number of ranked hits to return.
    pub limit: Option<usize>,
    /// Case-sensitive matching for filename / content locate.
    pub case_sensitive: bool,
    /// Match whole words only (content locate).
    pub whole_word: bool,
    /// Treat `free_text` as a regular expression when locating content matches.
    pub use_regex: bool,
    /// When true with `free_text`, skip body matching and only match filenames.
    pub filename_only: bool,
}

impl SearchRequest {
    /// Create an empty search request (browse / metadata-only default).
    pub fn new() -> Self {
        Self::default()
    }

    /// Free-text search (content + filename).
    pub fn free_text(text: impl Into<String>) -> Self {
        Self {
            free_text: Some(text.into()),
            limit: Some(100),
            ..Self::default()
        }
    }

    /// Filename substring search (Quick Open / File Search).
    pub fn filename(name: impl Into<String>) -> Self {
        Self {
            filename: Some(name.into()),
            filename_only: true,
            limit: Some(100),
            ..Self::default()
        }
    }

    /// Extension search (no leading dot).
    pub fn extension(ext: impl Into<String>) -> Self {
        Self {
            extension: Some(ext.into()),
            limit: Some(10_000),
            ..Self::default()
        }
    }

    /// Folder search.
    pub fn folder(path: impl Into<PathBuf>, immediate: bool) -> Self {
        Self {
            folder: Some(path.into()),
            folder_immediate: immediate,
            limit: Some(10_000),
            ..Self::default()
        }
    }

    /// Structured metadata search (content fields and/or inventory browse).
    pub fn metadata(filters: MetadataFilters) -> Self {
        Self {
            metadata: filters,
            limit: Some(100),
            ..Self::default()
        }
    }

    /// Enable case-sensitive matching.
    pub fn with_case_sensitive(mut self, enabled: bool) -> Self {
        self.case_sensitive = enabled;
        self
    }

    /// Enable whole-word content matching.
    pub fn with_whole_word(mut self, enabled: bool) -> Self {
        self.whole_word = enabled;
        self
    }

    /// Enable regex content matching.
    pub fn with_regex(mut self, enabled: bool) -> Self {
        self.use_regex = enabled;
        self
    }

    /// Restrict free-text to filenames only.
    pub fn with_filename_only(mut self, enabled: bool) -> Self {
        self.filename_only = enabled;
        self
    }

    /// True when content-locate options require precise body scanning.
    pub fn needs_precise_content_match(&self) -> bool {
        self.use_regex || self.whole_word || self.case_sensitive
    }

    /// True when any primary search dimension is set.
    pub fn has_primary_dimension(&self) -> bool {
        self.free_text
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            || self
                .filename
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self
                .extension
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            || self.folder.is_some()
            || self.metadata.is_active()
    }
}
