//! Structured search results returned by the Search Engine.

use std::path::PathBuf;

use crate::hybrid_rank::{fuse_relevance, RankSignals};
use crate::strategy::SearchStrategy;

/// Why a hit matched the request (deterministic labels).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchReason {
    /// Filename matched exactly (case-insensitive).
    FilenameExact,
    /// Filename contained the query substring.
    FilenameContains,
    /// Extension matched.
    Extension,
    /// Item is under / in the requested folder.
    Folder,
    /// Free text matched the filename or path.
    FreeTextFilename,
    /// Exact phrase matched in document body.
    FreeTextPhrase,
    /// Free text matched the document title.
    FreeTextTitle,
    /// Free text matched body content (words / frequency).
    FreeTextContent,
    /// Semantic embedding similarity match.
    Semantic,
    /// Matched inventory browse metadata filters only (recent, largest, …).
    Metadata,
    /// Matched content type metadata.
    MetadataContentType,
    /// Matched language metadata.
    MetadataLanguage,
    /// Matched author metadata.
    MetadataAuthor,
    /// Matched tag metadata.
    MetadataTag,
    /// Matched heading metadata.
    MetadataHeading,
    /// Matched title metadata (structured, not FTS).
    MetadataTitle,
    /// Matched date metadata.
    MetadataDate,
    /// Logical collection listing or membership.
    Collection,
    /// Combined multi-dimension match.
    Combined {
        /// Human-readable combined reason parts.
        parts: Vec<String>,
    },
}

impl MatchReason {
    /// Stable label for diagnostics and UI.
    pub fn as_str(&self) -> String {
        match self {
            Self::FilenameExact => "filename_exact".to_string(),
            Self::FilenameContains => "filename_contains".to_string(),
            Self::Extension => "extension".to_string(),
            Self::Folder => "folder".to_string(),
            Self::FreeTextFilename => "free_text_filename".to_string(),
            Self::FreeTextPhrase => "free_text_phrase".to_string(),
            Self::FreeTextTitle => "free_text_title".to_string(),
            Self::FreeTextContent => "free_text_content".to_string(),
            Self::Semantic => "semantic".to_string(),
            Self::Metadata => "metadata".to_string(),
            Self::MetadataContentType => "metadata_content_type".to_string(),
            Self::MetadataLanguage => "metadata_language".to_string(),
            Self::MetadataAuthor => "metadata_author".to_string(),
            Self::MetadataTag => "metadata_tag".to_string(),
            Self::MetadataHeading => "metadata_heading".to_string(),
            Self::MetadataTitle => "metadata_title".to_string(),
            Self::MetadataDate => "metadata_date".to_string(),
            Self::Collection => "collection".to_string(),
            Self::Combined { parts } => format!("combined:{}", parts.join("+")),
        }
    }
}

impl std::fmt::Display for MatchReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// One ranked search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Stable item identity (normalized path for inventory items).
    pub item_id: String,
    /// Display title (filename or content title when known).
    pub title: String,
    /// Absolute path when applicable.
    pub path: PathBuf,
    /// Normalized hybrid relevance on `[0, SCORE_SCALE]` (higher is better).
    pub score: u32,
    /// Independent ranking signals that produced `score`.
    pub signals: RankSignals,
    /// Why this hit matched.
    pub match_reason: MatchReason,
    /// Optional text preview when content is available.
    pub preview: Option<String>,
    /// Matching section title when content search localized a hit.
    pub matching_section: Option<String>,
    /// Snippet preview around the match in document content.
    pub snippet: Option<String>,
    /// True when the hit represents a directory / collection.
    pub is_directory: bool,
    /// Zero-based start line of the match, when known.
    pub line: Option<u32>,
    /// Zero-based start column of the match, when known.
    pub column: Option<u32>,
    /// Zero-based end line of the match, when known.
    pub end_line: Option<u32>,
    /// Zero-based end column of the match, when known.
    pub end_column: Option<u32>,
}

impl SearchHit {
    /// Rebuild `score` from the current signal set.
    pub fn recompute_score(&mut self) {
        self.score = fuse_relevance(&self.signals);
    }

    /// Merge another hit's independent signals and recompute relevance.
    pub fn merge_signals(&mut self, other: &RankSignals) {
        self.signals = self.signals.merge(other);
        self.recompute_score();
    }
}

/// Structured search response from the Search Engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResults {
    /// Ranked hits (hybrid relevance desc, path asc).
    pub hits: Vec<SearchHit>,
    /// Strategy selected for this request.
    pub strategy: SearchStrategy,
    /// Wall-clock query duration in milliseconds.
    pub duration_ms: u64,
    /// Total candidates considered before limit.
    pub candidate_count: usize,
}

impl SearchResults {
    /// Number of returned hits.
    pub fn len(&self) -> usize {
        self.hits.len()
    }

    /// True when no hits were returned.
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// Planner-facing citations for every hit (traceable provenance).
    pub fn citations(&self) -> Vec<jaymi_core::Citation> {
        crate::citation::hits_to_citations(&self.hits)
    }
}
