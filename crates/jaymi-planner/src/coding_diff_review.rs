//! Coding Diff Review (Sprint C3.5).
//!
//! Builds a multi-file [`ActionPreview`] and Review Card surface enrichment from
//! proposed [`CodeGeneration`] ops — still using the **same** ExecutionPlan /
//! ReviewCard / ReviewIntent pipeline. No coding-specific approval path.
//!
//! Constitutional:
//! - Preview / review only — never writes
//! - Diffs represent proposed changes before Approve
//! - Planner owns pause → Approve / Modify / Cancel

use std::path::Path;

use jaymi_core::{ActionPreview, PreviewKind};
use jaymi_tools::{unified_diff, write_file_preview};

use crate::code_generation::{CodeGeneration, GenerationOp, GenerationOpKind};
use crate::coding_plan::CodingPlan;
use crate::patch_generation::PatchGenerationReport;

/// Per-file proposed diff stats for the Review Card **Files affected** surface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodingFileDiffStat {
    /// Path shown to the user (usually basename or short relative).
    pub display_path: String,
    /// Full path string when known.
    pub path: String,
    /// Operation kind label (`modify_file`, …).
    pub operation: String,
    /// Lines added in the proposal.
    pub added: usize,
    /// Lines removed in the proposal.
    pub removed: usize,
}

impl CodingFileDiffStat {
    /// Example-style block:
    /// ```text
    /// planner.rs
    /// +18
    /// −7
    /// ```
    pub fn render_block(&self) -> String {
        format!("{}\n+{}\n−{}", self.display_path, self.added, self.removed)
    }

    /// Compact one-liner for diagnostics.
    pub fn render_compact(&self) -> String {
        format!(
            "{} (+{} / −{}) · {}",
            self.display_path, self.added, self.removed, self.operation
        )
    }
}

/// Presentation-only enrichment for the universal Review Card (not a new gate).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingDiffReviewSurface {
    /// Per-file +/− for Files affected.
    pub files: Vec<CodingFileDiffStat>,
    /// Total additions across proposed ops.
    pub total_added: usize,
    /// Total deletions across proposed ops.
    pub total_removed: usize,
    /// Proposed operation labels (create/modify/delete/rename).
    pub operations: Vec<String>,
    /// Why these mutations were proposed.
    pub rationale: Vec<String>,
    /// Risk notes (plan-level / op-level).
    pub risk_notes: Vec<String>,
    /// Dependencies called out for the review.
    pub dependencies: Vec<String>,
    /// Validation status lines (patch report / local checks).
    pub validation: Vec<String>,
}

impl CodingDiffReviewSurface {
    /// True when no enrichment was produced.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.operations.is_empty()
            && self.rationale.is_empty()
            && self.risk_notes.is_empty()
            && self.dependencies.is_empty()
            && self.validation.is_empty()
    }

    /// Diagnostics one-liner for Coding Diagnostics / lifecycle.
    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "coding_diff_review files={} +{} −{} ops={} rationale={} deps={} validation={}",
            self.files.len(),
            self.total_added,
            self.total_removed,
            self.operations.len(),
            self.rationale.len(),
            self.dependencies.len(),
            self.validation.len()
        )
    }
}

/// Built coding-diff ActionPreview plus Review Card surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingDiffReview {
    /// Multi-file unified diff preview attached to the ExecutionPlan.
    pub preview: ActionPreview,
    /// Optional Review Card sections (rationale / deps / validation / file stats).
    pub surface: CodingDiffReviewSurface,
}

/// Optional context used to enrich rationale / risk / deps / validation.
#[derive(Debug, Clone, Default)]
pub struct CodingDiffReviewContext<'a> {
    /// Last Coding Plan (C1.4 / C3.3), when any.
    pub coding_plan: Option<&'a CodingPlan>,
    /// Patch Generation report (C3.4), when any.
    pub patch_report: Option<&'a PatchGenerationReport>,
    /// Estimated risk label already chosen for the plan.
    pub risk_label: Option<&'a str>,
}

/// Build a multi-file coding diff preview from generation ops.
///
/// `read_before` must be read-only (Planner-mediated peek). Never writes.
pub fn build_coding_diff_review(
    generation: &CodeGeneration,
    read_before: impl Fn(&Path) -> Option<String>,
    context: CodingDiffReviewContext<'_>,
) -> CodingDiffReview {
    let mut surface = CodingDiffReviewSurface::default();
    let mut body_parts = Vec::new();
    let mut resources = Vec::new();
    let mut total_added = 0usize;
    let mut total_removed = 0usize;

    for op in &generation.operations {
        surface.operations.push(format!(
            "{} `{}`",
            op.kind.as_str(),
            display_name(&op.path)
        ));
        if !op.rationale.trim().is_empty() {
            push_unique(
                &mut surface.rationale,
                format!("{} — {}", display_name(&op.path), op.rationale.trim()),
            );
        }

        let path_str = op.path.display().to_string();
        if !resources.iter().any(|r| r == &path_str) {
            resources.push(path_str.clone());
        }
        if let Some(dest) = &op.destination {
            let dest_str = dest.display().to_string();
            if !resources.iter().any(|r| r == &dest_str) {
                resources.push(dest_str);
            }
        }

        let (added, removed, fragment) = preview_fragment_for_op(op, &read_before);
        total_added = total_added.saturating_add(added);
        total_removed = total_removed.saturating_add(removed);
        if let Some(fragment) = fragment {
            body_parts.push(fragment);
        }

        surface.files.push(CodingFileDiffStat {
            display_path: display_name(&op.path),
            path: path_str,
            operation: op.kind.as_str().to_string(),
            added,
            removed,
        });
    }

    surface.total_added = total_added;
    surface.total_removed = total_removed;
    enrich_surface_from_context(&mut surface, &context);

    let mut summary_lines = Vec::new();
    for file in &surface.files {
        summary_lines.push(file.display_path.clone());
        summary_lines.push(format!("+{}", file.added));
        summary_lines.push(format!("−{}", file.removed));
    }
    if !surface.operations.is_empty() {
        summary_lines.push(format!(
            "Proposed operations: {}",
            surface.operations.len()
        ));
    }
    summary_lines.push(format!(
        "Total +{} / −{} lines across {} file(s)",
        total_added,
        total_removed,
        surface.files.len()
    ));

    let body = if body_parts.is_empty() {
        None
    } else {
        Some(body_parts.join("\n\n"))
    };
    let total_lines = body.as_ref().map(|b| b.lines().count());

    let preview = ActionPreview {
        kind: PreviewKind::UnifiedDiff,
        title: if generation.goal.trim().is_empty() {
            "Coding Diff Review".into()
        } else {
            format!("Coding Diff Review — {}", generation.goal.trim())
        },
        summary_lines,
        body,
        truncated: false,
        total_lines,
        added_lines: Some(total_added),
        removed_lines: Some(total_removed),
        resources,
    };

    CodingDiffReview { preview, surface }
}

fn enrich_surface_from_context(
    surface: &mut CodingDiffReviewSurface,
    context: &CodingDiffReviewContext<'_>,
) {
    if let Some(plan) = context.coding_plan {
        for line in &plan.rationale {
            push_unique(&mut surface.rationale, line.clone());
        }
        for line in &plan.dependencies {
            push_unique(&mut surface.dependencies, line.clone());
        }
        for line in &plan.step_dependencies {
            push_unique(&mut surface.dependencies, format!("step: {line}"));
        }
        for line in &plan.estimated_risk {
            push_unique(&mut surface.risk_notes, line.clone());
        }
        if !plan.unresolved_questions.is_empty() {
            surface.validation.push(format!(
                "unresolved_questions={}",
                plan.unresolved_questions.len()
            ));
        }
        if !plan.constitutional_conflicts.is_empty() {
            surface.validation.push(format!(
                "constitutional_conflicts={}",
                plan.constitutional_conflicts.len()
            ));
        }
    }

    if let Some(report) = context.patch_report {
        surface.validation.push(format!(
            "patch_validation valid={} invalid/stale={} converted={}",
            report.valid_count(),
            report.invalid_count(),
            report.converted_to_execution
        ));
        for patch in &report.patches {
            let status = patch.status.as_str();
            let mut line = format!(
                "`{}` · {} · {}",
                display_name(&patch.proposal.file),
                patch.proposal.change_type.as_str(),
                status
            );
            if let Some(reason) = patch.status.reason() {
                line.push_str(&format!(" — {reason}"));
            }
            push_unique(&mut surface.validation, line);
            if !patch.proposal.rationale.trim().is_empty() {
                push_unique(
                    &mut surface.rationale,
                    format!(
                        "{} — {}",
                        display_name(&patch.proposal.file),
                        patch.proposal.rationale.trim()
                    ),
                );
            }
        }
        for failure in &report.failures {
            push_unique(&mut surface.validation, format!("failure: {failure}"));
        }
    }

    if let Some(risk) = context.risk_label {
        push_unique(&mut surface.risk_notes, format!("plan_risk: {risk}"));
    }

    if surface.validation.is_empty() {
        surface
            .validation
            .push("proposed_only — no filesystem writes until Approve".into());
    }
}

fn preview_fragment_for_op(
    op: &GenerationOp,
    read_before: &impl Fn(&Path) -> Option<String>,
) -> (usize, usize, Option<String>) {
    let path = op.path.display().to_string();
    match op.kind {
        GenerationOpKind::CreateFile => {
            let after = op.content.as_deref().unwrap_or("");
            let preview = write_file_preview(&path, None, after);
            (
                preview.added_lines.unwrap_or(0),
                preview.removed_lines.unwrap_or(0),
                preview.body,
            )
        }
        GenerationOpKind::ModifyFile => {
            let before = read_before(&op.path);
            let after = op.content.as_deref().unwrap_or("");
            let preview = write_file_preview(&path, before.as_deref(), after);
            (
                preview.added_lines.unwrap_or(0),
                preview.removed_lines.unwrap_or(0),
                preview.body,
            )
        }
        GenerationOpKind::DeleteFile => {
            let before = read_before(&op.path).unwrap_or_default();
            let stats = unified_diff(&path, &before, "");
            let body = format!(
                "--- a/{path}\n+++ /dev/null\n@@ delete @@\n{}",
                before
                    .lines()
                    .map(|line| format!("-{line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            (stats.added, stats.removed.max(before.lines().count()), Some(body))
        }
        GenerationOpKind::RenameMove => {
            let dest = op
                .destination
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(missing destination)".into());
            let body = format!("rename: {path} → {dest}");
            (0, 0, Some(body))
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn push_unique(target: &mut Vec<String>, value: String) {
    if value.trim().is_empty() {
        return;
    }
    if !target.iter().any(|existing| existing == &value) {
        target.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn multi_file_diff_counts_additions_and_deletions() {
        let planner = PathBuf::from("/tmp/proj/planner.rs");
        let context = PathBuf::from("/tmp/proj/context.rs");
        let generation = CodeGeneration {
            goal: "Adjust planner + context".into(),
            operations: vec![
                GenerationOp::modify_file(
                    &planner,
                    "fn a() {}\nfn b() {}\n",
                    "extend planner",
                ),
                GenerationOp::modify_file(&context, "use x;\nuse y;\n", "extend context"),
            ],
            coding_plan_summary: vec![],
        };
        let review = build_coding_diff_review(
            &generation,
            |path| {
                if path == planner.as_path() {
                    Some("fn a() {}\n".into())
                } else if path == context.as_path() {
                    Some("use x;\n".into())
                } else {
                    None
                }
            },
            CodingDiffReviewContext::default(),
        );

        assert_eq!(review.surface.files.len(), 2);
        assert_eq!(review.surface.files[0].display_path, "planner.rs");
        assert!(review.surface.files[0].added >= 1);
        assert_eq!(review.preview.kind, PreviewKind::UnifiedDiff);
        assert!(review
            .preview
            .summary_lines
            .iter()
            .any(|line| line == "planner.rs"));
        assert!(review.preview.body.as_ref().unwrap().contains("planner.rs"));
        assert!(review
            .surface
            .validation
            .iter()
            .any(|line| line.contains("no filesystem writes")));
    }

    #[test]
    fn create_file_counts_all_lines_as_additions() {
        let path = PathBuf::from("/tmp/proj/new.rs");
        let generation = CodeGeneration {
            goal: "Add file".into(),
            operations: vec![GenerationOp::create_file(&path, "one\ntwo\n", "create")],
            coding_plan_summary: vec![],
        };
        let review = build_coding_diff_review(
            &generation,
            |_| None,
            CodingDiffReviewContext::default(),
        );
        assert_eq!(review.surface.files[0].added, 2);
        assert_eq!(review.surface.files[0].removed, 0);
        assert_eq!(review.surface.total_added, 2);
    }

    #[test]
    fn patch_report_enriches_validation_and_rationale() {
        use crate::patch_generation::{
            PatchChangeType, PatchProposal, PatchValidationStatus, ValidatedPatch,
        };

        let path = PathBuf::from("src/lib.rs");
        let generation = CodeGeneration {
            goal: "Patch".into(),
            operations: vec![GenerationOp::modify_file(&path, "new\n", "from plan")],
            coding_plan_summary: vec![],
        };
        let report = PatchGenerationReport {
            goal: "Patch".into(),
            patches: vec![ValidatedPatch {
                proposal: PatchProposal {
                    file: path.clone(),
                    change_type: PatchChangeType::ModifyFile,
                    original_location: None,
                    destination: None,
                    original_content: Some("old\n".into()),
                    proposed_content: Some("new\n".into()),
                    rationale: "fix typo".into(),
                    observed_hash: None,
                },
                status: PatchValidationStatus::Valid,
            }],
            failures: vec![],
            reasoning_used: false,
            converted_to_execution: true,
            diagnostics: vec![],
        };
        let review = build_coding_diff_review(
            &generation,
            |_| Some("old\n".into()),
            CodingDiffReviewContext {
                coding_plan: None,
                patch_report: Some(&report),
                risk_label: Some("medium"),
            },
        );
        assert!(review
            .surface
            .validation
            .iter()
            .any(|line| line.contains("valid=1")));
        assert!(review
            .surface
            .rationale
            .iter()
            .any(|line| line.contains("fix typo")));
        assert!(review
            .surface
            .risk_notes
            .iter()
            .any(|line| line.contains("plan_risk: medium")));
    }
}
