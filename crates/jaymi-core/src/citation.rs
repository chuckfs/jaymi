//! Explainable search citations for Planner-facing provenance.

use std::path::PathBuf;

/// Traceable provenance for one retrieved search result.
///
/// Every field is required so the Planner can cite retrieved information
/// without inspecting Search Engine internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// Display title (filename or document title).
    pub title: String,
    /// Absolute location / path of the source.
    pub location: PathBuf,
    /// Short preview snippet (always non-empty).
    pub preview: String,
    /// Human-readable explanation of why this result matched.
    pub why_matched: String,
    /// Normalized confidence / relevance on `[0, 10_000]` (higher is better).
    pub confidence: u32,
    /// Optional section title that localized the match.
    pub matching_section: Option<String>,
    /// Zero-based start line of the match in the source, when known.
    pub line: Option<u32>,
    /// Zero-based start column of the match, when known.
    pub column: Option<u32>,
    /// Zero-based end line of the match, when known.
    pub end_line: Option<u32>,
    /// Zero-based end column of the match, when known.
    pub end_column: Option<u32>,
}

impl Citation {
    /// Confidence as a percentage in `[0, 100]`.
    pub fn confidence_percent(&self) -> u32 {
        // SCORE_SCALE is 10_000 in jaymi-search; keep core free of that dep.
        ((u64::from(self.confidence) * 100) / 10_000).min(100) as u32
    }

    /// One-line cite suitable for Planner summaries.
    pub fn cite_line(&self) -> String {
        let section = self
            .matching_section
            .as_ref()
            .map(|value| format!(" §{value}"))
            .unwrap_or_default();
        format!(
            "{}{} ({}) — {} — confidence {}%",
            self.title,
            section,
            self.location.display(),
            self.why_matched,
            self.confidence_percent()
        )
    }
}

/// Format citations as a Planner-readable block.
pub fn format_citations(citations: &[Citation]) -> String {
    if citations.is_empty() {
        return String::new();
    }
    let mut lines = Vec::with_capacity(citations.len().saturating_mul(3).saturating_add(1));
    lines.push("Citations:".to_string());
    for (index, citation) in citations.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, citation.cite_line()));
        lines.push(format!("   preview: {}", truncate_preview(&citation.preview, 160)));
    }
    lines.join("\n")
}

fn truncate_preview(preview: &str, max_chars: usize) -> String {
    let trimmed = preview.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let short: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{short}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_citations_includes_provenance_fields() {
        let citations = vec![Citation {
            title: "report.md".into(),
            location: PathBuf::from("/tmp/report.md"),
            preview: "Fungi grow in damp soil.".into(),
            why_matched: "exact phrase in body".into(),
            confidence: 7_500,
            matching_section: Some("Habitat".into()),
            line: Some(2),
            column: Some(0),
            end_line: Some(2),
            end_column: Some(5),
        }];
        let text = format_citations(&citations);
        assert!(text.contains("Citations:"));
        assert!(text.contains("report.md"));
        assert!(text.contains("/tmp/report.md"));
        assert!(text.contains("exact phrase in body"));
        assert!(text.contains("confidence 75%"));
        assert!(text.contains("Fungi grow"));
        assert!(text.contains("§Habitat"));
    }
}
