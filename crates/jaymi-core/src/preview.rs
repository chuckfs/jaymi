//! Structured previews for mutating actions (Preview Before Action).
//!
//! Tools produce [`ActionPreview`] metadata. The Planner attaches previews to
//! Execution Plans and Review Cards. Providers never render UI.

use serde::{Deserialize, Serialize};

/// Default maximum body lines shown before truncation.
pub const PREVIEW_MAX_BODY_LINES: usize = 40;
/// Default maximum body characters shown before truncation.
pub const PREVIEW_MAX_BODY_CHARS: usize = 8_000;

/// Kind of structured preview a tool can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    /// Unified text diff (write / edit).
    UnifiedDiff,
    /// Rename: before path → after path.
    PathRename,
    /// Move: source → destination.
    PathMove,
    /// Directory or file create.
    PathCreate,
    /// Delete (Trash or permanent).
    PathDelete,
    /// Git mutation impact (modified / staged / …).
    GitImpact,
    /// Language-server workspace edits (e.g. rename).
    LspWorkspaceEdit,
    /// Future image editing preview.
    ImageStub,
    /// Preview could not be produced.
    Unavailable,
}

impl PreviewKind {
    /// Stable label for diagnostics and serialization keys.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnifiedDiff => "unified_diff",
            Self::PathRename => "path_rename",
            Self::PathMove => "path_move",
            Self::PathCreate => "path_create",
            Self::PathDelete => "path_delete",
            Self::GitImpact => "git_impact",
            Self::LspWorkspaceEdit => "lsp_workspace_edit",
            Self::ImageStub => "image_stub",
            Self::Unavailable => "unavailable",
        }
    }

    /// Review Card section title (Sprint C1.6 Coding Execution Plans).
    ///
    /// Diff-like previews use **Diff Preview**; path / git / unavailable keep
    /// a shorter **Preview** label so the universal card stays readable.
    pub fn review_section_title(self) -> &'static str {
        match self {
            Self::UnifiedDiff | Self::LspWorkspaceEdit => "Diff Preview",
            Self::PathRename
            | Self::PathMove
            | Self::PathCreate
            | Self::PathDelete
            | Self::GitImpact
            | Self::ImageStub
            | Self::Unavailable => "Preview",
        }
    }
}

impl std::fmt::Display for PreviewKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Structured preview of what a mutating tool would change.
///
/// Always includes short [`Self::summary_lines`]. Optional [`Self::body`] holds
/// a unified diff or detailed listing; [`Self::truncated`] means the body was
/// shortened for review and can be expanded in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPreview {
    /// Preview kind.
    pub kind: PreviewKind,
    /// Short title (e.g. "Write notes.txt", "Rename a → b").
    pub title: String,
    /// Always-visible short lines (paths, counts, hunks summary).
    pub summary_lines: Vec<String>,
    /// Optional detailed body (unified diff, file lists, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// True when `body` was truncated for display.
    #[serde(default)]
    pub truncated: bool,
    /// Full line count of the untruncated body, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<usize>,
    /// Added line count for text diffs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_lines: Option<usize>,
    /// Removed line count for text diffs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_lines: Option<usize>,
    /// Resources the preview describes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
}

impl ActionPreview {
    /// Build an unavailable preview with a reason.
    pub fn unavailable(title: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: PreviewKind::Unavailable,
            title: title.into(),
            summary_lines: vec![reason.into()],
            body: None,
            truncated: false,
            total_lines: None,
            added_lines: None,
            removed_lines: None,
            resources: Vec::new(),
        }
    }

    /// Apply display truncation to the body (summary stays intact).
    pub fn truncate_for_display(
        mut self,
        max_lines: usize,
        max_chars: usize,
    ) -> Self {
        let Some(body) = self.body.take() else {
            return self;
        };
        let line_count = body.lines().count();
        let truncated_body = truncate_preview_body(&body, max_lines, max_chars);
        let was_truncated = truncated_body.len() < body.len()
            || truncated_body.lines().count() < line_count;
        self.total_lines = Some(line_count.max(self.total_lines.unwrap_or(0)));
        self.truncated = self.truncated || was_truncated;
        self.body = Some(truncated_body);
        self
    }

    /// Plain-text render for chat / Review Card bodies.
    pub fn render_text(&self, expanded: bool) -> String {
        let mut lines = Vec::new();
        lines.push(format!("{} · {}", self.kind.review_section_title(), self.title));
        for summary in &self.summary_lines {
            lines.push(format!("• {summary}"));
        }
        if let (Some(added), Some(removed)) = (self.added_lines, self.removed_lines) {
            if !self.summary_lines.iter().any(|line| line.contains('+') && line.contains('-')) {
                lines.push(format!("• +{added} / −{removed} lines"));
            }
        }
        if let Some(body) = &self.body {
            lines.push(String::new());
            if expanded || !self.truncated {
                lines.push(body.clone());
            } else {
                lines.push(body.clone());
                if let Some(total) = self.total_lines {
                    lines.push(format!(
                        "… preview truncated ({total} lines total). Expand to see the full preview."
                    ));
                } else {
                    lines.push("… preview truncated. Expand to see the full preview.".into());
                }
            }
        } else if self.truncated {
            lines.push("… preview truncated. Expand to see the full preview.".into());
        }
        lines.join("\n")
    }
}

/// Truncate a preview body to at most `max_lines` / `max_chars`.
pub fn truncate_preview_body(body: &str, max_lines: usize, max_chars: usize) -> String {
    if max_lines == 0 || max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut lines = 0usize;
    for line in body.lines() {
        if lines >= max_lines {
            break;
        }
        let candidate = if out.is_empty() {
            line.to_string()
        } else {
            format!("{out}\n{line}")
        };
        if candidate.len() > max_chars {
            if out.is_empty() {
                let end = max_chars.min(line.len());
                return line[..end].to_string();
            }
            break;
        }
        out = candidate;
        lines += 1;
    }
    out
}
