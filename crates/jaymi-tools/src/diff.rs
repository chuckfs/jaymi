//! Unified text-diff helpers for Preview Before Action.
//!
//! Pure functions — no providers, no UI. Tools call these when building
//! [`jaymi_core::ActionPreview`] for write / edit previews.

use jaymi_core::{ActionPreview, PreviewKind};

/// Maximum source lines considered for a full unified diff before summarizing.
pub const DIFF_MAX_SOURCE_LINES: usize = 2_000;

/// Result of comparing two text buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStats {
    /// Unified diff body (may be empty when identical).
    pub unified: String,
    /// Lines added in `after`.
    pub added: usize,
    /// Lines removed from `before`.
    pub removed: usize,
    /// True when source was too large and only a summary was produced.
    pub summarized: bool,
}

/// Build a unified diff between `before` and `after` for `path`.
pub fn unified_diff(path: &str, before: &str, after: &str) -> DiffStats {
        let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    if before_lines.len() + after_lines.len() > DIFF_MAX_SOURCE_LINES {
        let (added, removed) = if before_lines.len() == after_lines.len() {
            let changed = before_lines
                .iter()
                .zip(after_lines.iter())
                .filter(|(a, b)| a != b)
                .count();
            (changed, changed)
        } else {
            (after_lines.len(), before_lines.len())
        };
        return DiffStats {
            unified: format!(
                "--- a/{path}\n+++ b/{path}\n@@ large change summarized @@\n\
                 before: {} lines · after: {} lines · +{added} / −{removed} (approx)",
                before_lines.len(),
                after_lines.len()
            ),
            added,
            removed,
            summarized: true,
        };
    }

    let ops = myers_diff(&before_lines, &after_lines);
    let mut added = 0usize;
    let mut removed = 0usize;
    for op in &ops {
        match op {
            DiffOp::Insert(_) => added += 1,
            DiffOp::Delete(_) => removed += 1,
            DiffOp::Equal(_) => {}
        }
    }

    if added == 0 && removed == 0 {
        return DiffStats {
            unified: format!("--- a/{path}\n+++ b/{path}\n(no textual changes)"),
            added: 0,
            removed: 0,
            summarized: false,
        };
    }

    let mut out = String::new();
    out.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
    out.push_str(&format!("@@ -{},{} +{},{} @@\n", 1, before_lines.len().max(1), 1, after_lines.len().max(1)));
    for op in ops {
        match op {
            DiffOp::Equal(line) => {
                out.push(' ');
                out.push_str(line);
                out.push('\n');
            }
            DiffOp::Delete(line) => {
                out.push('-');
                out.push_str(line);
                out.push('\n');
            }
            DiffOp::Insert(line) => {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    // Trim trailing newline for stable tests when body is empty of ops — keep one.
    if out.ends_with('\n') {
        out.pop();
    }

    DiffStats {
        unified: out,
        added,
        removed,
        summarized: false,
    }
}

/// Build a write-file [`ActionPreview`] from old/new text.
pub fn write_file_preview(path: &str, before: Option<&str>, after: &str) -> ActionPreview {
    let creating = before.is_none();
    let before = before.unwrap_or("");
    let stats = unified_diff(path, before, after);
    let mut summary = Vec::new();
    if creating {
        summary.push(format!("Create {path}"));
    } else {
        summary.push(format!("Overwrite {path}"));
    }
    summary.push(format!("+{} / −{} lines", stats.added, stats.removed));
    if stats.summarized {
        summary.push("Large change summarized for preview".into());
    }

    let body = stats.unified.clone();
    let total_lines = body.lines().count();

    ActionPreview {
        kind: PreviewKind::UnifiedDiff,
        title: if creating {
            format!("Create {path}")
        } else {
            format!("Write {path}")
        },
        summary_lines: summary,
        body: Some(body),
        truncated: stats.summarized,
        total_lines: Some(total_lines),
        added_lines: Some(stats.added),
        removed_lines: Some(stats.removed),
        resources: vec![path.to_string()],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffOp<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

/// Myers O(ND) line diff (simple DP for modest inputs).
fn myers_diff<'a>(before: &[&'a str], after: &[&'a str]) -> Vec<DiffOp<'a>> {
    let n = before.len();
    let m = after.len();
    if n == 0 && m == 0 {
        return Vec::new();
    }
    // DP LCS table
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            if before[i - 1] == after[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    let mut ops = Vec::new();
    let mut i = n;
    let mut j = m;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && before[i - 1] == after[j - 1] {
            ops.push(DiffOp::Equal(before[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(DiffOp::Insert(after[j - 1]));
            j -= 1;
        } else if i > 0 {
            ops.push(DiffOp::Delete(before[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_render() {
        let stats = unified_diff(
            "notes.txt",
            "hello\nworld\n",
            "hello\nthere\nworld\n",
        );
        assert!(stats.unified.contains("--- a/notes.txt"));
        assert!(stats.unified.contains("+++ b/notes.txt"));
        assert!(stats.unified.contains("+there"));
        assert_eq!(stats.added, 1);
        assert_eq!(stats.removed, 0);
        assert!(!stats.summarized);
    }

    #[test]
    fn preview_generation() {
        let preview = write_file_preview("a.txt", Some("one\n"), "one\ntwo\n");
        assert_eq!(preview.kind, PreviewKind::UnifiedDiff);
        assert!(preview.summary_lines.iter().any(|line| line.contains("Overwrite")));
        assert_eq!(preview.added_lines, Some(1));
        assert!(preview.body.as_ref().is_some_and(|body| body.contains("+two")));

        let create = write_file_preview("new.txt", None, "hi\n");
        assert!(create.summary_lines.iter().any(|line| line.contains("Create")));
    }

    #[test]
    fn preview_truncation() {
        let mut long = String::new();
        for i in 0..200 {
            long.push_str(&format!("line {i}\n"));
        }
        let preview = write_file_preview("big.txt", Some(""), &long).truncate_for_display(10, 400);
        assert!(preview.truncated);
        let body = preview.body.as_deref().unwrap_or("");
        assert!(body.lines().count() <= 10);
        assert!(body.len() <= 400);
    }

    #[test]
    fn large_preview() {
        let before: String = (0..1500).map(|i| format!("old {i}\n")).collect();
        let after: String = (0..1500).map(|i| format!("new {i}\n")).collect();
        let stats = unified_diff("huge.txt", &before, &after);
        assert!(stats.summarized);
        assert!(stats.unified.contains("summarized"));
        let preview = write_file_preview("huge.txt", Some(&before), &after);
        assert!(preview.truncated || preview.summary_lines.iter().any(|l| l.contains("summarized")));
    }
}
