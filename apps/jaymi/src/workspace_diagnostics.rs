//! Workspace Intelligence diagnostics (Sprint B2.11).
//!
//! Read-only aggregate for **Developer Diagnostics** only. Surfaces snapshot
//! freshness, provider timings, maintenance status, candidate selection,
//! policy decisions, and context budget.
//!
//! Never writes to the conversation transcript, Memory turns, or Planner
//! routing. Assembling this report must not schedule maintenance or re-assemble
//! Context.

use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_context::{
    BudgetReport, CandidateDecisionSummary, CandidateSelectionReport, ContextInspectorReport,
    PolicyDecisionSummary, PolicyReport,
};

use crate::context_maintenance::{
    CompletedMaintenanceSnapshots, ContextMaintenance, MaintenanceKind,
};

/// One Workspace Intelligence snapshot freshness row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFreshnessRow {
    /// Snapshot kind label (`workspace`, `editor`, …).
    pub kind: String,
    /// Whether a completed snapshot is present.
    pub present: bool,
    /// Capture unix seconds when known.
    pub timestamp_unix: Option<i64>,
    /// Age in seconds relative to `now`, when timestamp known.
    pub age_seconds: Option<u64>,
    /// Human freshness label (`fresh`, `stale`, `missing`, …).
    pub freshness: String,
}

/// One maintenance job status row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceStatusRow {
    /// Maintenance kind id.
    pub kind: String,
    /// Whether a worker is currently running this kind.
    pub inflight: bool,
    /// Whether a completed snapshot exists for this kind.
    pub has_completed: bool,
}

/// Read-only Workspace Intelligence diagnostics report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceDiagnosticsReport {
    /// Maintenance generation counter.
    pub maintenance_generation: u64,
    /// Jobs started (process lifetime).
    pub jobs_started: u64,
    /// Jobs completed (process lifetime).
    pub jobs_completed: u64,
    /// Snapshot freshness rows.
    pub snapshot_freshness: Vec<SnapshotFreshnessRow>,
    /// Maintenance kind status rows.
    pub maintenance_status: Vec<MaintenanceStatusRow>,
    /// Last assemble provider timing rows (`id`, `detail`).
    pub provider_timings: Vec<(String, String)>,
    /// Last assemble duration (ms).
    pub assemble_duration_ms: Option<u64>,
    /// Last assemble cache status.
    pub assemble_cache: Option<String>,
    /// Context Selection profile id.
    pub selection_profile: Option<String>,
    /// Context Selection matched rules.
    pub selection_rules: Vec<String>,
    /// Candidate selection summary.
    pub candidate_selection: CandidateSelectionReport,
    /// Policy decision rows (capped for UI).
    pub policy_decisions: Vec<PolicyDecisionSummary>,
    /// Active context policy ids.
    pub active_policies: Vec<String>,
    /// Context budget, when recorded.
    pub budget: Option<BudgetReport>,
    /// Unix seconds used as “now” for age computation.
    pub observed_at_unix: i64,
}

/// Inputs for assembling Workspace Diagnostics (read-only).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceDiagnosticsInput {
    /// Latest Context Inspector (last assemble).
    pub context_inspector: Option<ContextInspectorReport>,
    /// Maintenance generation.
    pub maintenance_generation: u64,
    /// Jobs started.
    pub jobs_started: u64,
    /// Jobs completed.
    pub jobs_completed: u64,
    /// Per-kind inflight flags (parallel to [`MaintenanceKind`] order below).
    pub inflight: Vec<(MaintenanceKind, bool)>,
    /// Latest completed snapshots.
    pub completed: CompletedMaintenanceSnapshots,
    /// Optional override for “now” (tests).
    pub now_unix: Option<i64>,
}

impl WorkspaceDiagnosticsReport {
    /// Assemble from maintenance + last Context Inspector (pure observation).
    pub fn assemble(input: WorkspaceDiagnosticsInput) -> Self {
        let now = input.now_unix.unwrap_or_else(now_unix);
        let mut report = Self {
            maintenance_generation: input.maintenance_generation,
            jobs_started: input.jobs_started,
            jobs_completed: input.jobs_completed,
            observed_at_unix: now,
            ..Self::default()
        };

        let completed = &input.completed;
        report.snapshot_freshness = vec![
            freshness_row(
                "workspace",
                completed
                    .workspace_snapshot
                    .as_ref()
                    .map(|s| s.timestamp),
                now,
            ),
            freshness_row(
                "editor",
                completed.editor_snapshot.as_ref().map(|s| s.timestamp),
                now,
            ),
            freshness_row(
                "project",
                completed.project_snapshot.as_ref().map(|s| s.timestamp),
                now,
            ),
            freshness_row(
                "git",
                completed.git_snapshot.as_ref().map(|s| s.timestamp),
                now,
            ),
            freshness_row(
                "runtime",
                completed.runtime_snapshot.as_ref().map(|s| s.timestamp),
                now,
            ),
            presence_row("git_status", completed.git_status.is_some()),
            presence_row(
                "workspace_inventory",
                completed.workspace_inventory.is_some(),
            ),
            presence_row("diagnostics", completed.diagnostics.is_some()),
            presence_row("file_summaries", completed.file_summaries.is_some()),
        ];

        report.maintenance_status = input
            .inflight
            .into_iter()
            .map(|(kind, inflight)| MaintenanceStatusRow {
                kind: kind.as_str().to_string(),
                inflight,
                has_completed: match kind {
                    MaintenanceKind::GitStatus => completed.git_status.is_some()
                        || completed.git_snapshot.is_some(),
                    MaintenanceKind::WorkspaceInventory => completed.workspace_inventory.is_some(),
                    MaintenanceKind::Diagnostics => completed.diagnostics.is_some(),
                    MaintenanceKind::FileSummaries => completed.file_summaries.is_some(),
                    MaintenanceKind::WorkspaceSnapshot => completed.workspace_snapshot.is_some(),
                    MaintenanceKind::EditorSnapshot => completed.editor_snapshot.is_some(),
                    MaintenanceKind::ProjectSnapshot => completed.project_snapshot.is_some(),
                    MaintenanceKind::RuntimeSnapshot => completed.runtime_snapshot.is_some(),
                },
            })
            .collect();

        if let Some(inspector) = input.context_inspector.as_ref() {
            report.assemble_duration_ms = Some(inspector.duration_ms);
            report.assemble_cache = Some(inspector.cache_status().to_string());
            report.provider_timings = inspector.provider_timing_rows();
            report.budget = inspector.budget.clone();
            if let Some(policy) = inspector.policy.as_ref() {
                fill_policy(&mut report, policy);
            }
        }

        report
    }

    /// Assemble from live Application maintenance + optional inspector.
    pub fn from_maintenance(
        maintenance: &ContextMaintenance,
        context_inspector: Option<ContextInspectorReport>,
    ) -> Self {
        let kinds = [
            MaintenanceKind::GitStatus,
            MaintenanceKind::WorkspaceInventory,
            MaintenanceKind::Diagnostics,
            MaintenanceKind::FileSummaries,
            MaintenanceKind::WorkspaceSnapshot,
            MaintenanceKind::EditorSnapshot,
            MaintenanceKind::ProjectSnapshot,
            MaintenanceKind::RuntimeSnapshot,
        ];
        Self::assemble(WorkspaceDiagnosticsInput {
            context_inspector,
            maintenance_generation: maintenance.generation(),
            jobs_started: maintenance.jobs_started(),
            jobs_completed: maintenance.jobs_completed(),
            inflight: kinds
                .into_iter()
                .map(|kind| (kind, maintenance.is_inflight(kind)))
                .collect(),
            completed: maintenance.latest_completed(),
            now_unix: None,
        })
    }

    /// True when there is anything useful to show.
    pub fn has_content(&self) -> bool {
        !self.snapshot_freshness.is_empty()
            || !self.maintenance_status.is_empty()
            || !self.provider_timings.is_empty()
            || self.budget.is_some()
            || !self.policy_decisions.is_empty()
            || self.candidate_selection.proposed > 0
            || self.assemble_duration_ms.is_some()
    }

    /// Flat labeled rows for grids / text dashboards.
    pub fn labeled_values(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        rows.push((
            "Maintenance Generation".into(),
            self.maintenance_generation.to_string(),
        ));
        rows.push((
            "Maintenance Jobs".into(),
            format!(
                "started={} completed={}",
                self.jobs_started, self.jobs_completed
            ),
        ));
        if let Some(cache) = &self.assemble_cache {
            rows.push(("Assemble Cache".into(), cache.clone()));
        }
        if let Some(ms) = self.assemble_duration_ms {
            rows.push(("Assemble Duration".into(), format!("{ms} ms")));
        }
        if let Some(profile) = &self.selection_profile {
            rows.push(("Selection Profile".into(), profile.clone()));
        }
        if !self.selection_rules.is_empty() {
            rows.push((
                "Selection Rules".into(),
                self.selection_rules.join(", "),
            ));
        }
        rows.push((
            "Candidates".into(),
            format!(
                "proposed={} selected={} rejected_policy={} rejected_budget={}",
                self.candidate_selection.proposed,
                self.candidate_selection.selected,
                self.candidate_selection.rejected_policy,
                self.candidate_selection.rejected_budget
            ),
        ));
        if let Some(budget) = &self.budget {
            rows.push((
                "Context Budget".into(),
                format!(
                    "{} / {} chars (≈{} tok)",
                    budget.used_characters, budget.max_characters, budget.estimated_tokens
                ),
            ));
            if !budget.truncated_providers.is_empty() {
                rows.push((
                    "Budget Truncated".into(),
                    budget.truncated_providers.join(", "),
                ));
            }
            if !budget.skipped_budget.is_empty() {
                rows.push((
                    "Budget Skipped".into(),
                    budget.skipped_budget.join(", "),
                ));
            }
        }
        if !self.active_policies.is_empty() {
            rows.push((
                "Active Context Policies".into(),
                self.active_policies.join(", "),
            ));
        }
        rows
    }

    /// Candidate decision rows for UI (capped).
    pub fn candidate_rows(&self) -> &[CandidateDecisionSummary] {
        &self.candidate_selection.decisions
    }

    /// Plain-text render for headless / CLI dashboards.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Workspace Intelligence Diagnostics (developer-only)".to_string());
        lines.push(
            "Observational only — never written to conversation transcript.".into(),
        );
        lines.push(String::new());
        for (label, value) in self.labeled_values() {
            lines.push(format!("{label}: {value}"));
        }
        lines.push(String::new());
        lines.push("Snapshot freshness:".into());
        for row in &self.snapshot_freshness {
            let age = row
                .age_seconds
                .map(|s| format!("{s}s"))
                .unwrap_or_else(|| "-".into());
            lines.push(format!(
                "  {} present={} freshness={} age={}",
                row.kind, row.present, row.freshness, age
            ));
        }
        lines.push(String::new());
        lines.push("Maintenance status:".into());
        for row in &self.maintenance_status {
            lines.push(format!(
                "  {} inflight={} completed={}",
                row.kind, row.inflight, row.has_completed
            ));
        }
        if !self.provider_timings.is_empty() {
            lines.push(String::new());
            lines.push("Provider timings:".into());
            for (id, detail) in &self.provider_timings {
                lines.push(format!("  {id}: {detail}"));
            }
        }
        if !self.candidate_selection.decisions.is_empty() {
            lines.push(String::new());
            lines.push("Candidate selection:".into());
            for decision in self.candidate_selection.decisions.iter().take(32) {
                lines.push(format!(
                    "  {} {} kind={} selected={} rel={} rec={} imp={} reason={}",
                    decision.provider_id,
                    decision.candidate_id,
                    decision.kind,
                    decision.selected,
                    decision.relevance,
                    decision.recency,
                    decision.importance,
                    decision.reason
                ));
            }
        }
        if !self.policy_decisions.is_empty() {
            lines.push(String::new());
            lines.push("Policy decisions:".into());
            for decision in self.policy_decisions.iter().take(32) {
                lines.push(format!(
                    "  {} included={} reason={}",
                    decision.provider_id, decision.included, decision.reason
                ));
            }
        }
        lines.join("\n")
    }
}

fn fill_policy(report: &mut WorkspaceDiagnosticsReport, policy: &PolicyReport) {
    report.selection_profile = policy.selection_profile.clone();
    report.selection_rules = policy.selection_rules.clone();
    report.candidate_selection = policy.candidate_selection.clone();
    report.policy_decisions = policy.decisions.clone();
    report.active_policies = policy.active_policies.clone();
}

fn freshness_row(kind: &str, timestamp: Option<i64>, now: i64) -> SnapshotFreshnessRow {
    match timestamp {
        Some(ts) if ts > 0 => {
            let age = now.saturating_sub(ts).max(0) as u64;
            SnapshotFreshnessRow {
                kind: kind.into(),
                present: true,
                timestamp_unix: Some(ts),
                age_seconds: Some(age),
                freshness: freshness_label(age).into(),
            }
        }
        _ => SnapshotFreshnessRow {
            kind: kind.into(),
            present: false,
            timestamp_unix: None,
            age_seconds: None,
            freshness: "missing".into(),
        },
    }
}

fn presence_row(kind: &str, present: bool) -> SnapshotFreshnessRow {
    SnapshotFreshnessRow {
        kind: kind.into(),
        present,
        timestamp_unix: None,
        age_seconds: None,
        freshness: if present { "present" } else { "missing" }.into(),
    }
}

fn freshness_label(age_seconds: u64) -> &'static str {
    match age_seconds {
        0..=30 => "fresh",
        31..=120 => "warm",
        121..=600 => "aging",
        _ => "stale",
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        BudgetReport, CandidateDecisionSummary, CandidateSelectionReport, ContextInspectorReport,
        PolicyDecisionSummary, PolicyReport,
    };

    #[test]
    fn assemble_exposes_freshness_maintenance_candidates_budget() {
        let now = 1_700_000_100;
        let mut completed = CompletedMaintenanceSnapshots::default();
        completed.workspace_snapshot = Some({
            let mut snap = jaymi_context::WorkspaceSnapshot::empty();
            snap.timestamp = now - 10;
            snap
        });
        completed.runtime_snapshot = Some({
            let mut snap = jaymi_context::RuntimeSnapshot::empty();
            snap.timestamp = now - 400;
            snap
        });

        let inspector = ContextInspectorReport {
            duration_ms: 12,
            cache_hit: false,
            budget: Some(BudgetReport {
                max_characters: 32_000,
                max_tokens: None,
                used_characters: 1_200,
                estimated_tokens: 300,
                truncated_providers: vec![],
                skipped_budget: vec![],
                summaries: vec![],
            }),
            policy: Some(PolicyReport {
                active_policies: vec!["jaymi_default_context".into()],
                decisions: vec![PolicyDecisionSummary {
                    provider_id: "editor".into(),
                    included: true,
                    reason: "ok".into(),
                    sensitivity: "normal".into(),
                    priority: 80,
                    can_truncate: true,
                    requires_user_approval: false,
                    approval_status: "not_required".into(),
                    exclude_sensitive: false,
                    bypass_relevance: false,
                    truncated: false,
                    truncation_reason: None,
                    constraints: vec![],
                }],
                size_before_characters: 2000,
                size_after_characters: 1500,
                size_assembled_characters: 1200,
                candidate_selection: CandidateSelectionReport {
                    proposed: 4,
                    selected: 2,
                    rejected_policy: 1,
                    rejected_budget: 1,
                    decisions: vec![CandidateDecisionSummary {
                        candidate_id: "editor/current_file".into(),
                        provider_id: "editor".into(),
                        kind: "current_file".into(),
                        selected: true,
                        reason: "selected".into(),
                        relevance: 90,
                        recency: 80,
                        importance: 70,
                        estimated_chars: 40,
                    }],
                },
                selection_profile: Some("coding_general".into()),
                selection_rules: vec!["complexity_coding".into()],
            }),
            providers: vec![],
            ..ContextInspectorReport::default()
        };

        let report = WorkspaceDiagnosticsReport::assemble(WorkspaceDiagnosticsInput {
            context_inspector: Some(inspector),
            maintenance_generation: 3,
            jobs_started: 5,
            jobs_completed: 4,
            inflight: vec![
                (MaintenanceKind::WorkspaceSnapshot, false),
                (MaintenanceKind::RuntimeSnapshot, true),
            ],
            completed,
            now_unix: Some(now),
        });

        assert!(report.has_content());
        assert_eq!(report.maintenance_generation, 3);
        assert_eq!(
            report
                .snapshot_freshness
                .iter()
                .find(|r| r.kind == "workspace")
                .map(|r| r.freshness.as_str()),
            Some("fresh")
        );
        assert_eq!(
            report
                .snapshot_freshness
                .iter()
                .find(|r| r.kind == "runtime")
                .map(|r| r.freshness.as_str()),
            Some("aging")
        );
        assert!(report
            .maintenance_status
            .iter()
            .any(|r| r.kind == "runtime_snapshot" && r.inflight));
        assert_eq!(report.selection_profile.as_deref(), Some("coding_general"));
        assert_eq!(report.candidate_selection.proposed, 4);
        assert_eq!(
            report.budget.as_ref().map(|b| b.used_characters),
            Some(1_200)
        );
        let rendered = report.render();
        assert!(rendered.contains("developer-only"));
        assert!(rendered.contains("never written to conversation transcript"));
        assert!(rendered.contains("Snapshot freshness"));
        assert!(rendered.contains("Candidate selection"));
        assert!(rendered.contains("Context Budget"));
    }

    #[test]
    fn empty_input_still_lists_maintenance_kinds() {
        let report = WorkspaceDiagnosticsReport::assemble(WorkspaceDiagnosticsInput {
            inflight: vec![(MaintenanceKind::GitStatus, false)],
            ..WorkspaceDiagnosticsInput::default()
        });
        assert!(!report.snapshot_freshness.is_empty());
        assert_eq!(report.maintenance_status.len(), 1);
    }
}
