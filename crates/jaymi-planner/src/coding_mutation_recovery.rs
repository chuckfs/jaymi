//! Coding Mutation Recovery (Sprint C3.7).
//!
//! Makes coding mutations safe when source state drifts or an apply stops
//! mid-batch — using the existing ExecutionPlan / Approval History /
//! ExecutionSummary architecture (no parallel transaction system).
//!
//! Constitutional:
//! - Detect stale state before mutation; never silently overwrite
//! - No automatic destructive rollback / silent retries
//! - Recovery mutations require a new Planner-owned reviewed plan
//! - Preserve Approval History + Execution Summary

use std::path::{Path, PathBuf};

use crate::code_generation::{CodeGeneration, GenerationOpKind};
use crate::coding_apply::CodingOpResult;
use crate::execution_plan::{ExecutionPlan, ExecutionStatus, ExecutionSummary};
use crate::patch_generation::{content_fingerprint, snippet_preview};

/// Coarse lifecycle phase for recovery / diagnostics (maps onto ExecutionPlan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationPhase {
    /// Draft / Ready / AwaitingReview — proposed, not approved.
    Planned,
    /// Approved; tools may run.
    Approved,
    /// Tools are running.
    Executing,
    /// Batch completed successfully.
    Succeeded,
    /// Batch failed with no successful ops.
    Failed,
    /// Some ops succeeded; later ops stopped.
    PartiallyCompleted,
    /// User / Planner cancelled before or instead of completion.
    Cancelled,
}

impl MutationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::PartiallyCompleted => "partially_completed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Map plan status (+ partial flag) onto the recovery vocabulary.
    pub fn from_plan_status(status: ExecutionStatus, partial: bool) -> Self {
        match status {
            ExecutionStatus::Draft
            | ExecutionStatus::Ready
            | ExecutionStatus::AwaitingReview => Self::Planned,
            ExecutionStatus::Approved => Self::Approved,
            ExecutionStatus::Executing => Self::Executing,
            ExecutionStatus::Completed => Self::Succeeded,
            ExecutionStatus::Failed if partial => Self::PartiallyCompleted,
            ExecutionStatus::Failed => Self::Failed,
            ExecutionStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl std::fmt::Display for MutationPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Why recovery is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    /// On-disk bytes drifted since the plan was paused for review.
    StaleSource,
    /// Anchor / original context no longer matches (patch cannot apply safely).
    PatchNoLongerApplies,
    /// Tool / provider write failed.
    WriteFailed,
    /// Parent directory missing or removed.
    DirectoryMissing,
    /// Proposed patch content is empty / invalid at apply time.
    InvalidPatch,
    /// Earlier op(s) succeeded; later op stopped the batch.
    PartialBatchFailure,
    /// User cancelled a multi-file (or any) plan before completion.
    CancelledMultiFile,
}

impl RecoveryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StaleSource => "stale_source",
            Self::PatchNoLongerApplies => "patch_no_longer_applies",
            Self::WriteFailed => "write_failed",
            Self::DirectoryMissing => "directory_missing",
            Self::InvalidPatch => "invalid_patch",
            Self::PartialBatchFailure => "partial_batch_failure",
            Self::CancelledMultiFile => "cancelled_multi_file",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::StaleSource => "Source changed since review",
            Self::PatchNoLongerApplies => "Patch no longer applies",
            Self::WriteFailed => "Write failed",
            Self::DirectoryMissing => "Directory missing",
            Self::InvalidPatch => "Invalid patch",
            Self::PartialBatchFailure => "Partial batch failure",
            Self::CancelledMultiFile => "Cancelled multi-file plan",
        }
    }
}

/// Frozen expectation for one generation op (captured at pause / review time).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenerationBaseline {
    pub order: usize,
    pub path: PathBuf,
    pub op_kind: String,
    /// Expected FNV-1a fingerprint of file bytes when paused (`None` for create).
    pub expected_hash: Option<String>,
    /// Whether the path was expected to exist at pause time.
    pub expect_exists: bool,
    /// Parent directory must exist before write/create.
    pub parent_must_exist: bool,
    /// Bounded original snippet for recovery display / patch-apply checks.
    pub original_snippet: Option<String>,
    /// Destination for rename/move (must be absent).
    pub destination: Option<PathBuf>,
}

/// One preflight / apply blocker.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecoveryBlocker {
    pub order: usize,
    pub path: String,
    pub kind: RecoveryKind,
    pub detail: String,
}

/// Actionable recovery report (conversation + diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingMutationRecovery {
    pub phase: Option<MutationPhase>,
    pub kind: Option<RecoveryKind>,
    pub plan_id: String,
    pub blockers: Vec<RecoveryBlocker>,
    pub succeeded_paths: Vec<String>,
    pub failed_paths: Vec<String>,
    pub skipped_paths: Vec<String>,
    /// Original snippets preserved from baselines (never silently discarded).
    pub preserved_originals: Vec<String>,
    pub actions: Vec<String>,
    /// True when any further mutation needs a fresh reviewed plan.
    pub requires_new_plan: bool,
    pub notes: Vec<String>,
}

impl CodingMutationRecovery {
    pub fn is_empty(&self) -> bool {
        self.blockers.is_empty()
            && self.succeeded_paths.is_empty()
            && self.failed_paths.is_empty()
            && self.skipped_paths.is_empty()
            && self.actions.is_empty()
            && self.kind.is_none()
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "coding_mutation_recovery phase={} kind={} blockers={} succeeded={} failed={} skipped={} requires_new_plan={}",
            self.phase.map(|p| p.as_str()).unwrap_or("—"),
            self.kind.map(|k| k.as_str()).unwrap_or("—"),
            self.blockers.len(),
            self.succeeded_paths.len(),
            self.failed_paths.len(),
            self.skipped_paths.len(),
            self.requires_new_plan,
        )
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Mutation Recovery\n");
        if let Some(phase) = self.phase {
            out.push_str(&format!("\n### Phase\n- `{}`\n", phase.as_str()));
        }
        if let Some(kind) = self.kind {
            out.push_str(&format!(
                "\n### Issue\n- **{}** (`{}`)\n",
                kind.label(),
                kind.as_str()
            ));
        }
        if !self.plan_id.is_empty() {
            out.push_str(&format!("\n### Plan\n- `{}`\n", self.plan_id));
        }
        if !self.blockers.is_empty() {
            out.push_str("\n### Blockers\n");
            for b in &self.blockers {
                out.push_str(&format!(
                    "- op {} · `{}` · {} — {}\n",
                    b.order,
                    b.path,
                    b.kind.as_str(),
                    b.detail
                ));
            }
        }
        push_list(&mut out, "Succeeded", &self.succeeded_paths);
        push_list(&mut out, "Failed", &self.failed_paths);
        push_list(&mut out, "Skipped (not executed)", &self.skipped_paths);
        if !self.preserved_originals.is_empty() {
            out.push_str("\n### Preserved originals (review-time snapshots)\n");
            for line in &self.preserved_originals {
                out.push_str(&format!("- {line}\n"));
            }
        }
        out.push_str("\n### Required\n");
        if self.requires_new_plan {
            out.push_str(
                "- Further mutation requires a **new** Planner-owned plan and Review Before Action.\n",
            );
            out.push_str("- Jaymi will not silently retry or roll back destructively.\n");
        } else {
            out.push_str("- No further mutation required for this turn.\n");
        }
        if !self.actions.is_empty() {
            out.push_str("\n### Suggested recovery actions\n");
            for action in &self.actions {
                out.push_str(&format!("- {action}\n"));
            }
        }
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for note in &self.notes {
                out.push_str(&format!("- {note}\n"));
            }
        }
        out
    }
}

fn push_list(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n### {title}\n"));
    for item in items {
        out.push_str(&format!("- `{item}`\n"));
    }
}

/// Capture baselines for a generation batch at pause time (read-only).
pub fn baselines_for_generation(
    generation: &CodeGeneration,
    read_before: impl Fn(&Path) -> Option<String>,
) -> Vec<GenerationBaseline> {
    let mut out = Vec::new();
    for (index, op) in generation.operations.iter().enumerate() {
        let order = index + 1;
        let current = read_before(&op.path);
        let parent_must_exist = matches!(
            op.kind,
            GenerationOpKind::CreateFile | GenerationOpKind::ModifyFile | GenerationOpKind::RenameMove
        );
        let (expect_exists, expected_hash, original_snippet) = match op.kind {
            GenerationOpKind::CreateFile => (false, None, None),
            GenerationOpKind::ModifyFile
            | GenerationOpKind::DeleteFile
            | GenerationOpKind::RenameMove => {
                let hash = current.as_deref().map(content_fingerprint);
                let snippet = current.as_deref().map(snippet_preview);
                (true, hash, snippet)
            }
        };
        out.push(GenerationBaseline {
            order,
            path: op.path.clone(),
            op_kind: op.kind.as_str().to_string(),
            expected_hash,
            expect_exists,
            parent_must_exist,
            original_snippet,
            destination: op.destination.clone(),
        });
    }
    out
}

/// Result of a batch preflight (before any mutation).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreflightResult {
    pub blockers: Vec<RecoveryBlocker>,
}

impl PreflightResult {
    pub fn ok(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// Re-check frozen baselines against current disk (read-only).
pub fn preflight_baselines(
    baselines: &[GenerationBaseline],
    read_before: impl Fn(&Path) -> Option<String>,
    parent_exists: impl Fn(&Path) -> bool,
) -> PreflightResult {
    let mut blockers = Vec::new();
    for baseline in baselines {
        let path_str = baseline.path.display().to_string();
        if baseline.parent_must_exist {
            if let Some(parent) = baseline.path.parent() {
                if !parent.as_os_str().is_empty() && !parent_exists(parent) {
                    blockers.push(RecoveryBlocker {
                        order: baseline.order,
                        path: path_str.clone(),
                        kind: RecoveryKind::DirectoryMissing,
                        detail: format!(
                            "Parent directory `{}` is missing — refusing to write.",
                            parent.display()
                        ),
                    });
                    continue;
                }
            }
        }

        let current = read_before(&baseline.path);
        match (baseline.expect_exists, current.as_ref()) {
            (false, Some(_)) => {
                blockers.push(RecoveryBlocker {
                    order: baseline.order,
                    path: path_str.clone(),
                    kind: RecoveryKind::StaleSource,
                    detail: format!(
                        "Create target `{}` now exists — refusing silent overwrite.",
                        baseline.path.display()
                    ),
                });
            }
            (true, None) => {
                blockers.push(RecoveryBlocker {
                    order: baseline.order,
                    path: path_str.clone(),
                    kind: RecoveryKind::StaleSource,
                    detail: format!(
                        "Expected `{}` to exist for `{}` — file disappeared since review.",
                        baseline.path.display(),
                        baseline.op_kind
                    ),
                });
            }
            (true, Some(body)) => {
                if let Some(expected) = &baseline.expected_hash {
                    let now = content_fingerprint(body);
                    if &now != expected {
                        let kind = if baseline
                            .original_snippet
                            .as_ref()
                            .map(|s| !s.trim().is_empty() && !body.contains(s.trim()))
                            .unwrap_or(false)
                        {
                            RecoveryKind::PatchNoLongerApplies
                        } else {
                            RecoveryKind::StaleSource
                        };
                        blockers.push(RecoveryBlocker {
                            order: baseline.order,
                            path: path_str.clone(),
                            kind,
                            detail: format!(
                                "File `{}` changed since review (hash {expected} → {now}).",
                                baseline.path.display()
                            ),
                        });
                    } else if let Some(orig) = &baseline.original_snippet {
                        if !orig.trim().is_empty() && !body.contains(orig.trim()) {
                            // Hash matched but snippet missing — treat as apply failure.
                            // (Unusual; still refuse rather than overwrite.)
                            blockers.push(RecoveryBlocker {
                                order: baseline.order,
                                path: path_str.clone(),
                                kind: RecoveryKind::PatchNoLongerApplies,
                                detail: format!(
                                    "Original context no longer found in `{}`.",
                                    baseline.path.display()
                                ),
                            });
                        }
                    }
                }
            }
            (false, None) => {}
        }

        if let Some(dest) = &baseline.destination {
            if read_before(dest).is_some() {
                blockers.push(RecoveryBlocker {
                    order: baseline.order,
                    path: dest.display().to_string(),
                    kind: RecoveryKind::StaleSource,
                    detail: format!(
                        "Rename destination `{}` now exists — refusing overwrite.",
                        dest.display()
                    ),
                });
            }
        }
    }
    PreflightResult { blockers }
}

/// Validate tool input content for write ops (apply-time invalid patch guard).
pub fn invalid_write_content(tool_id: &str, content: Option<&str>) -> Option<String> {
    if tool_id != jaymi_tools::WRITE_FILE_TOOL_ID {
        return None;
    }
    match content {
        None => Some("Write op missing proposed content.".into()),
        Some(body) if body.is_empty() => {
            Some("Write op has empty proposed content — invalid patch.".into())
        }
        _ => None,
    }
}

/// Build recovery after a cancelled multi-file (or single-file) review plan.
pub fn recovery_for_cancel(
    plan: &ExecutionPlan,
    baselines: &[GenerationBaseline],
) -> CodingMutationRecovery {
    let multi = baselines.len() > 1 || plan.steps().len() > 1;
    let skipped: Vec<String> = if baselines.is_empty() {
        plan.affected_resources().to_vec()
    } else {
        baselines
            .iter()
            .map(|b| b.path.display().to_string())
            .collect()
    };
    let mut recovery = CodingMutationRecovery {
        phase: Some(MutationPhase::Cancelled),
        kind: Some(if multi {
            RecoveryKind::CancelledMultiFile
        } else {
            RecoveryKind::CancelledMultiFile
        }),
        plan_id: plan.id().as_str().to_string(),
        blockers: Vec::new(),
        succeeded_paths: Vec::new(),
        failed_paths: Vec::new(),
        skipped_paths: skipped,
        preserved_originals: preserved_from_baselines(baselines),
        actions: vec![
            "No files were written for this cancelled plan.".into(),
            "If you still want the change, ask Jaymi to propose a new reviewed plan.".into(),
        ],
        requires_new_plan: true,
        notes: vec![
            "Cancel discarded the proposed mutation — Approval History retains the decision.".into(),
        ],
    };
    if multi {
        recovery.notes.push(format!(
            "Multi-file plan cancelled — {} proposed path(s) were not executed.",
            recovery.skipped_paths.len()
        ));
    }
    recovery
}

/// Build recovery when preflight blocks the entire batch (no writes).
pub fn recovery_for_preflight(
    plan: &ExecutionPlan,
    baselines: &[GenerationBaseline],
    preflight: &PreflightResult,
) -> CodingMutationRecovery {
    let kind = preflight
        .blockers
        .first()
        .map(|b| b.kind)
        .unwrap_or(RecoveryKind::StaleSource);
    CodingMutationRecovery {
        phase: Some(MutationPhase::Failed),
        kind: Some(kind),
        plan_id: plan.id().as_str().to_string(),
        blockers: preflight.blockers.clone(),
        succeeded_paths: Vec::new(),
        failed_paths: preflight
            .blockers
            .iter()
            .map(|b| b.path.clone())
            .collect(),
        skipped_paths: baselines
            .iter()
            .map(|b| b.path.display().to_string())
            .filter(|p| {
                !preflight
                    .blockers
                    .iter()
                    .any(|b| &b.path == p)
            })
            .collect(),
        preserved_originals: preserved_from_baselines(baselines),
        actions: default_actions(kind, true),
        requires_new_plan: true,
        notes: vec![
            "No filesystem writes were performed — preflight refused the batch.".into(),
            "Original review-time snapshots are listed under preserved originals.".into(),
        ],
    }
}

/// Build recovery from applied op results (success / fail / partial).
pub fn recovery_from_op_results(
    plan: &ExecutionPlan,
    baselines: &[GenerationBaseline],
    results: &[CodingOpResult],
    summary: &ExecutionSummary,
    stop_kind: Option<RecoveryKind>,
) -> CodingMutationRecovery {
    let phase = MutationPhase::from_plan_status(summary.status, summary.partial);
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for result in results {
        let path = result
            .path
            .clone()
            .unwrap_or_else(|| format!("op-{}", result.order));
        if result.success {
            succeeded.push(path);
        } else {
            failed.push(path);
        }
    }
    let executed_orders: Vec<usize> = results.iter().map(|r| r.order).collect();
    let skipped: Vec<String> = baselines
        .iter()
        .filter(|b| !executed_orders.contains(&b.order))
        .map(|b| b.path.display().to_string())
        .collect();

    let kind = if summary.partial {
        Some(RecoveryKind::PartialBatchFailure)
    } else {
        stop_kind.or_else(|| {
            if matches!(summary.status, ExecutionStatus::Failed) {
                Some(RecoveryKind::WriteFailed)
            } else {
                None
            }
        })
    };

    let requires_new_plan = matches!(
        phase,
        MutationPhase::Failed | MutationPhase::PartiallyCompleted
    );

    let mut blockers = Vec::new();
    if let Some(failed_result) = results.iter().find(|r| !r.success) {
        blockers.push(RecoveryBlocker {
            order: failed_result.order,
            path: failed_result
                .path
                .clone()
                .unwrap_or_else(|| format!("op-{}", failed_result.order)),
            kind: kind.unwrap_or(RecoveryKind::WriteFailed),
            detail: failed_result.message.clone(),
        });
    }

    CodingMutationRecovery {
        phase: Some(phase),
        kind,
        plan_id: plan.id().as_str().to_string(),
        blockers,
        succeeded_paths: succeeded,
        failed_paths: failed,
        skipped_paths: skipped,
        preserved_originals: preserved_from_baselines(baselines),
        actions: kind
            .map(|k| default_actions(k, requires_new_plan))
            .unwrap_or_default(),
        requires_new_plan,
        notes: if summary.partial {
            vec![
                "Earlier successful ops were left in place — Jaymi does not auto-rollback.".into(),
                "Ask for a new reviewed plan if you need compensating changes.".into(),
            ]
        } else {
            Vec::new()
        },
    }
}

fn preserved_from_baselines(baselines: &[GenerationBaseline]) -> Vec<String> {
    baselines
        .iter()
        .filter_map(|b| {
            b.original_snippet.as_ref().map(|s| {
                format!(
                    "`{}` (hash={}) — {}",
                    b.path.display(),
                    b.expected_hash.as_deref().unwrap_or("—"),
                    s
                )
            })
        })
        .collect()
}

fn default_actions(kind: RecoveryKind, requires_new_plan: bool) -> Vec<String> {
    let mut actions = match kind {
        RecoveryKind::StaleSource | RecoveryKind::PatchNoLongerApplies => vec![
            "Re-read the current file(s) / re-investigate the change.".into(),
            "Ask Jaymi for a fresh Coding Plan + patches against current contents.".into(),
        ],
        RecoveryKind::DirectoryMissing => vec![
            "Restore or create the missing directory, then request a new reviewed plan.".into(),
        ],
        RecoveryKind::InvalidPatch => vec![
            "Ask Jaymi to regenerate valid patch content, then review again.".into(),
        ],
        RecoveryKind::WriteFailed => vec![
            "Inspect the tool error, fix the environment, then propose a new plan.".into(),
        ],
        RecoveryKind::PartialBatchFailure => vec![
            "Review files that already changed.".into(),
            "Ask Jaymi for a new plan covering only the remaining / compensating edits.".into(),
        ],
        RecoveryKind::CancelledMultiFile => vec![
            "If you still want the change, ask Jaymi to propose the plan again.".into(),
        ],
    };
    if requires_new_plan {
        actions.push(
            "Any new mutation must go through Planner → Permission → Review → Approve.".into(),
        );
    }
    actions
}

/// Enrich an ExecutionSummary with recovery next-actions (no destructive rollback).
pub fn enrich_summary_with_recovery(
    summary: &mut ExecutionSummary,
    recovery: &CodingMutationRecovery,
) {
    for action in &recovery.actions {
        if !summary.next_suggested_actions.iter().any(|a| a == action) {
            summary.next_suggested_actions.push(action.clone());
        }
    }
    if recovery.requires_new_plan {
        let note = "Recovery: further mutation requires a new reviewed plan (no silent retry)."
            .to_string();
        if !summary.warnings.iter().any(|w| w == &note) {
            summary.warnings.push(note);
        }
    }
    for blocker in &recovery.blockers {
        let line = format!(
            "recovery[{}] op {} `{}`: {}",
            blocker.kind.as_str(),
            blocker.order,
            blocker.path,
            blocker.detail
        );
        if !summary.errors.iter().any(|e| e == &line) {
            // Prefer keeping primary tool errors; append recovery detail as warning if error exists.
            if summary.errors.is_empty() {
                summary.errors.push(line.clone());
                if summary.error.is_none() {
                    summary.error = Some(line);
                }
            } else if !summary.warnings.iter().any(|w| w == &line) {
                summary.warnings.push(line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_generation::{CodeGeneration, GenerationOp};

    #[test]
    fn preflight_detects_stale_modify_hash() {
        let path = PathBuf::from("/tmp/proj/a.rs");
        let baselines = vec![GenerationBaseline {
            order: 1,
            path: path.clone(),
            op_kind: "modify_file".into(),
            expected_hash: Some(content_fingerprint("old\n")),
            expect_exists: true,
            parent_must_exist: true,
            original_snippet: Some("old".into()),
            destination: None,
        }];
        let result = preflight_baselines(
            &baselines,
            |p| {
                if p == path.as_path() {
                    Some("changed\n".into())
                } else {
                    None
                }
            },
            |_| true,
        );
        assert!(!result.ok());
        assert_eq!(result.blockers[0].kind, RecoveryKind::PatchNoLongerApplies);
    }

    #[test]
    fn preflight_refuses_create_when_file_appeared() {
        let path = PathBuf::from("/tmp/proj/new.rs");
        let baselines = vec![GenerationBaseline {
            order: 1,
            path: path.clone(),
            op_kind: "create_file".into(),
            expected_hash: None,
            expect_exists: false,
            parent_must_exist: true,
            original_snippet: None,
            destination: None,
        }];
        let result = preflight_baselines(
            &baselines,
            |_| Some("surprise\n".into()),
            |_| true,
        );
        assert_eq!(result.blockers[0].kind, RecoveryKind::StaleSource);
    }

    #[test]
    fn preflight_detects_missing_parent_directory() {
        let path = PathBuf::from("/tmp/proj/gone/child.rs");
        let baselines = vec![GenerationBaseline {
            order: 1,
            path: path.clone(),
            op_kind: "create_file".into(),
            expected_hash: None,
            expect_exists: false,
            parent_must_exist: true,
            original_snippet: None,
            destination: None,
        }];
        let result = preflight_baselines(&baselines, |_| None, |_| false);
        assert_eq!(result.blockers[0].kind, RecoveryKind::DirectoryMissing);
    }

    #[test]
    fn baselines_capture_modify_hash() {
        let path = PathBuf::from("/tmp/x.rs");
        let generation = CodeGeneration {
            goal: "g".into(),
            operations: vec![GenerationOp::modify_file(&path, "new\n", "m")],
            coding_plan_summary: vec![],
        };
        let baselines = baselines_for_generation(&generation, |_| Some("old\n".into()));
        assert_eq!(baselines.len(), 1);
        assert_eq!(
            baselines[0].expected_hash.as_deref(),
            Some(content_fingerprint("old\n").as_str())
        );
        assert!(baselines[0].expect_exists);
    }

    #[test]
    fn phase_maps_partial_failed() {
        assert_eq!(
            MutationPhase::from_plan_status(ExecutionStatus::Failed, true),
            MutationPhase::PartiallyCompleted
        );
        assert_eq!(
            MutationPhase::from_plan_status(ExecutionStatus::Completed, false),
            MutationPhase::Succeeded
        );
        assert_eq!(
            MutationPhase::from_plan_status(ExecutionStatus::AwaitingReview, false),
            MutationPhase::Planned
        );
    }

    #[test]
    fn invalid_write_content_detected() {
        assert!(invalid_write_content(jaymi_tools::WRITE_FILE_TOOL_ID, Some("")).is_some());
        assert!(invalid_write_content(jaymi_tools::WRITE_FILE_TOOL_ID, Some("ok")).is_none());
    }
}
