//! Citation generation from ranked search hits.
//!
//! Guarantees every hit can be cited with title, location, preview,
//! why-matched, and confidence — without exposing Search Engine internals.

use jaymi_core::Citation;

use crate::hybrid_rank::SCORE_SCALE;
use crate::result::{MatchReason, SearchHit};

impl MatchReason {
    /// Human-readable explanation suitable for citations.
    pub fn why_matched(&self) -> String {
        match self {
            Self::FilenameExact => "filename matched exactly".into(),
            Self::FilenameContains => "filename contains the query".into(),
            Self::Extension => "file extension matched".into(),
            Self::Folder => "item is under the requested folder".into(),
            Self::FreeTextFilename => "filename or path matched free text".into(),
            Self::FreeTextPhrase => "exact phrase matched in document body".into(),
            Self::FreeTextTitle => "document title matched free text".into(),
            Self::FreeTextContent => "document body matched free text".into(),
            Self::Semantic => "semantic similarity to the query".into(),
            Self::Metadata => "matched inventory metadata filters".into(),
            Self::MetadataContentType => "matched content type metadata".into(),
            Self::MetadataLanguage => "matched language metadata".into(),
            Self::MetadataAuthor => "matched author metadata".into(),
            Self::MetadataTag => "matched tag metadata".into(),
            Self::MetadataHeading => "matched heading metadata".into(),
            Self::MetadataTitle => "matched title metadata".into(),
            Self::MetadataDate => "matched date metadata".into(),
            Self::Collection => "matched logical collection".into(),
            Self::Combined { parts } => {
                if parts.is_empty() {
                    "matched multiple search signals".into()
                } else {
                    format!("matched via {}", parts.join(" + "))
                }
            }
        }
    }
}

impl SearchHit {
    /// Build a Planner-facing citation with guaranteed provenance fields.
    pub fn to_citation(&self) -> Citation {
        Citation {
            title: if self.title.trim().is_empty() {
                self.path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.item_id.clone())
            } else {
                self.title.clone()
            },
            location: self.path.clone(),
            preview: ensure_preview(self),
            why_matched: self.match_reason.why_matched(),
            confidence: self.score.min(SCORE_SCALE),
            matching_section: self.matching_section.clone(),
            line: self.line,
            column: self.column,
            end_line: self.end_line,
            end_column: self.end_column,
        }
    }

    /// True when this hit has all required citation provenance fields.
    pub fn has_traceable_provenance(&self) -> bool {
        let citation = self.to_citation();
        !citation.title.trim().is_empty()
            && !citation.location.as_os_str().is_empty()
            && !citation.preview.trim().is_empty()
            && !citation.why_matched.trim().is_empty()
    }
}

/// Convert ranked hits into citations (one per hit, order preserved).
pub fn hits_to_citations(hits: &[SearchHit]) -> Vec<Citation> {
    hits.iter().map(SearchHit::to_citation).collect()
}

/// Ensure every hit has a non-empty preview before leaving the Search Engine.
pub fn ensure_hit_previews(hits: &mut [SearchHit]) {
    for hit in hits.iter_mut() {
        if hit
            .preview
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
        {
            hit.preview = Some(ensure_preview(hit));
        }
        if hit
            .snippet
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
        {
            hit.snippet = hit.preview.clone();
        }
    }
}

fn ensure_preview(hit: &SearchHit) -> String {
    if let Some(snippet) = hit
        .snippet
        .as_ref()
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
    {
        return snippet.to_string();
    }
    if let Some(preview) = hit
        .preview
        .as_ref()
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
    {
        return preview.to_string();
    }
    if let Some(section) = hit
        .matching_section
        .as_ref()
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
    {
        return format!("Section: {section}");
    }
    if !hit.title.trim().is_empty() {
        return hit.title.clone();
    }
    hit.path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid_rank::RankSignals;
    use std::path::PathBuf;

    #[test]
    fn citation_fills_missing_preview_from_title() {
        let hit = SearchHit {
            item_id: "/tmp/a.md".into(),
            title: "a.md".into(),
            path: PathBuf::from("/tmp/a.md"),
            score: 2_500,
            signals: RankSignals::default(),
            match_reason: MatchReason::FilenameExact,
            preview: None,
            matching_section: None,
            snippet: None,
            is_directory: false,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        };
        let citation = hit.to_citation();
        assert_eq!(citation.title, "a.md");
        assert_eq!(citation.location, PathBuf::from("/tmp/a.md"));
        assert_eq!(citation.preview, "a.md");
        assert_eq!(citation.why_matched, "filename matched exactly");
        assert_eq!(citation.confidence, 2_500);
        assert!(hit.has_traceable_provenance());
    }
}
