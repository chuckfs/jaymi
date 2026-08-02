//! Search strategies and deterministic strategy selection.
//!
//! Strategy choice is rule-based. Semantic retrieval is selected for free-text
//! when the Search Engine has an embedding provider (engine upgrades FreeText).

use jaymi_core::SearchRequest;

/// Selected retrieval strategy for one search request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStrategy {
    /// Match free text against filenames / available previews / FTS body.
    FreeText,
    /// Semantic / embedding similarity retrieval.
    Semantic,
    /// Filename substring match.
    Filename,
    /// Extension filter.
    Extension,
    /// Folder / path-prefix filter.
    Folder,
    /// Inventory browse filters (recent, largest, hidden, collections, …).
    Metadata,
    /// Structured content-field metadata (author, tags, language, …) via SQL.
    StructuredMetadata,
    /// Multiple dimensions combined with deterministic ranking.
    Combined,
}

impl SearchStrategy {
    /// Stable label for diagnostics and tool messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FreeText => "free_text",
            Self::Semantic => "semantic",
            Self::Filename => "filename",
            Self::Extension => "extension",
            Self::Folder => "folder",
            Self::Metadata => "metadata",
            Self::StructuredMetadata => "structured_metadata",
            Self::Combined => "combined",
        }
    }
}

impl std::fmt::Display for SearchStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Select the appropriate search strategy for a request.
///
/// Selection is deterministic and independent of result content.
/// Structured content metadata is distinct from free-text FTS.
pub fn select_strategy(request: &SearchRequest) -> SearchStrategy {
    let has_text = request
        .free_text
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_filename = request
        .filename
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_extension = request
        .extension
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_folder = request.folder.is_some();
    let has_content_meta = request.metadata.has_content_filters();
    let has_inventory_meta = request.metadata.has_inventory_filters();

    let dimensions = [
        has_text,
        has_filename,
        has_extension,
        has_folder,
        has_content_meta,
        has_inventory_meta,
    ]
    .into_iter()
    .filter(|active| *active)
    .count();

    if dimensions > 1 {
        return SearchStrategy::Combined;
    }
    if has_text {
        return SearchStrategy::FreeText;
    }
    if has_filename {
        return SearchStrategy::Filename;
    }
    if has_extension {
        return SearchStrategy::Extension;
    }
    if has_folder {
        return SearchStrategy::Folder;
    }
    if has_content_meta {
        return SearchStrategy::StructuredMetadata;
    }
    SearchStrategy::Metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::MetadataFilters;
    use std::path::PathBuf;

    #[test]
    fn selects_each_primary_strategy() {
        assert_eq!(
            select_strategy(&SearchRequest::free_text("fungi")),
            SearchStrategy::FreeText
        );
        assert_eq!(
            select_strategy(&SearchRequest::filename("report")),
            SearchStrategy::Filename
        );
        assert_eq!(
            select_strategy(&SearchRequest::extension("pdf")),
            SearchStrategy::Extension
        );
        assert_eq!(
            select_strategy(&SearchRequest::folder(PathBuf::from("/tmp"), true)),
            SearchStrategy::Folder
        );
        assert_eq!(
            select_strategy(&SearchRequest {
                metadata: MetadataFilters {
                    recently_modified: true,
                    ..MetadataFilters::default()
                },
                limit: Some(10),
                ..SearchRequest::default()
            }),
            SearchStrategy::Metadata
        );
        assert_eq!(
            select_strategy(&SearchRequest::metadata(MetadataFilters {
                language: Some("en".into()),
                ..MetadataFilters::default()
            })),
            SearchStrategy::StructuredMetadata
        );
    }

    #[test]
    fn combined_when_multiple_dimensions() {
        let request = SearchRequest {
            free_text: Some("biology".into()),
            extension: Some("pdf".into()),
            limit: Some(10),
            ..SearchRequest::default()
        };
        assert_eq!(select_strategy(&request), SearchStrategy::Combined);

        let meta_and_text = SearchRequest {
            free_text: Some("fungi".into()),
            metadata: MetadataFilters {
                language: Some("en".into()),
                ..MetadataFilters::default()
            },
            limit: Some(10),
            ..SearchRequest::default()
        };
        assert_eq!(select_strategy(&meta_and_text), SearchStrategy::Combined);
    }

    #[test]
    fn structured_metadata_independent_of_free_text() {
        let request = SearchRequest::metadata(MetadataFilters {
            author: Some("Ada".into()),
            tag: Some("biology".into()),
            heading_contains: Some("Habitat".into()),
            ..MetadataFilters::default()
        });
        assert_eq!(
            select_strategy(&request),
            SearchStrategy::StructuredMetadata
        );
        assert!(request.free_text.is_none());
    }
}
