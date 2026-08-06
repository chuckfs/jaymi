//! Context policy decisions, candidates, and explainability reports.

use serde::Serialize;

use crate::budget::{BudgetEstimate, ProviderPriority};
use crate::provider::ContextContribution;
use crate::relevance::{RelevanceScore, RelevanceSignals};
use crate::ContextSessionInputs;

use super::Sensitivity;

use jaymi_core::UserRequest;

/// Read-only inputs available to context policies during evaluation.
#[derive(Debug, Clone, Copy)]
pub struct ContextPolicyInputs<'a> {
    /// Inbound user request.
    pub request: &'a UserRequest,
    /// Host session snapshot.
    pub session: &'a ContextSessionInputs,
    /// Deterministic relevance cues.
    pub signals: &'a RelevanceSignals,
    /// True when a project is session-open.
    pub project_open: bool,
    /// Maximum sensitivity allowed for this request without special need.
    pub max_sensitivity: Sensitivity,
}

/// Snapshot of one provider presented to policies (never mutated).
#[derive(Debug, Clone, Copy)]
pub struct ContextPolicyCandidate<'a> {
    /// Provider id.
    pub provider_id: &'static str,
    /// Provider's declared budget priority.
    pub provider_priority: ProviderPriority,
    /// Relevance score for this request.
    pub relevance: RelevanceScore,
    /// Provider sensitivity metadata.
    pub sensitivity: Sensitivity,
    /// Size estimate before contribute.
    pub estimate: BudgetEstimate,
    /// Shared policy inputs.
    pub inputs: &'a ContextPolicyInputs<'a>,
}

/// Decision returned by one [`super::ContextPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPolicyDecision {
    /// Whether the provider may participate in assembly.
    pub participate: bool,
    /// Human-readable explanation (Context Inspector / transparency).
    pub reason: String,
    /// Effective priority for budget ordering.
    pub priority: ProviderPriority,
    /// Whether the engine may truncate this contribution to fit budget.
    pub can_truncate: bool,
    /// Whether surfacing this context should ask the user (future UI).
    pub requires_user_approval: bool,
    /// Whether high-sensitivity fields must be stripped from contributions.
    pub exclude_sensitive: bool,
    /// When true, the provider may participate even below the relevance threshold.
    pub bypass_relevance: bool,
    /// Contribution shaping constraints (policies never gather; only constrain).
    pub constraints: ContributionConstraints,
}

impl ContextPolicyDecision {
    /// Allow participation with the given reason and priority.
    pub fn allow(reason: impl Into<String>, priority: ProviderPriority) -> Self {
        Self {
            participate: true,
            reason: reason.into(),
            priority,
            can_truncate: true,
            requires_user_approval: false,
            exclude_sensitive: false,
            bypass_relevance: false,
            constraints: ContributionConstraints::default(),
        }
    }

    /// Deny participation with an explanation.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            participate: false,
            reason: reason.into(),
            priority: ProviderPriority::new(0),
            can_truncate: true,
            requires_user_approval: false,
            exclude_sensitive: true,
            bypass_relevance: false,
            constraints: ContributionConstraints::default(),
        }
    }
}

/// Constraints applied to a contribution after `contribute` returns.
///
/// Policies do not gather data — they only shape what may remain.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ContributionConstraints {
    /// Drop open-editor tabs (keep current file / selection only).
    pub exclude_open_files: bool,
    /// Drop permission explanation / resource detail (keep category/action/decision).
    pub permission_summary_only: bool,
    /// Drop memory bodies that exceed sensitivity (keep ids/summaries when possible).
    pub redact_memory_content: bool,
    /// Drop search hit previews.
    pub redact_search_previews: bool,
}

impl ContributionConstraints {
    /// Merge with another set (OR for exclusions).
    pub fn merge(&mut self, other: &Self) {
        self.exclude_open_files = self.exclude_open_files || other.exclude_open_files;
        self.permission_summary_only =
            self.permission_summary_only || other.permission_summary_only;
        self.redact_memory_content = self.redact_memory_content || other.redact_memory_content;
        self.redact_search_previews = self.redact_search_previews || other.redact_search_previews;
    }

    /// Labels of active constraints for explainability.
    pub fn labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        if self.exclude_open_files {
            labels.push("exclude_open_files".into());
        }
        if self.permission_summary_only {
            labels.push("permission_summary_only".into());
        }
        if self.redact_memory_content {
            labels.push("redact_memory_content".into());
        }
        if self.redact_search_previews {
            labels.push("redact_search_previews".into());
        }
        labels
    }
}

/// Apply contribution constraints without mutating the provider.
pub fn apply_contribution_constraints(
    mut contribution: ContextContribution,
    constraints: &ContributionConstraints,
) -> ContextContribution {
    if constraints.exclude_open_files {
        contribution.open_files = None;
    }
    if constraints.permission_summary_only {
        if let Some(permissions) = contribution.permissions.as_mut() {
            for entry in &mut permissions.entries {
                entry.explanation = None;
            }
        }
    }
    if constraints.redact_memory_content {
        if let Some(memory) = contribution.memory_results.as_mut() {
            for item in &mut memory.memory.memories {
                item.record.content.clear();
            }
        }
    }
    if constraints.redact_search_previews {
        if let Some(search) = contribution.search_results.as_mut() {
            for hit in &mut search.hits {
                hit.preview = None;
            }
        }
    }
    contribution
}

/// Per-provider policy evaluation record for explainability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextPolicyDecisionRecord {
    /// Provider id.
    pub provider_id: String,
    /// Provider sensitivity.
    pub sensitivity: Sensitivity,
    /// Relevance score at evaluation time.
    pub relevance: u8,
    /// Estimated characters before contribute.
    pub estimate_characters: usize,
    /// Merged decision.
    #[serde(skip)]
    pub decision: ContextPolicyDecision,
    /// Policy ids that evaluated this provider.
    pub applied_policies: Vec<String>,
}

impl ContextPolicyDecisionRecord {
    /// True when the provider may participate.
    pub fn included(&self) -> bool {
        self.decision.participate
    }

    /// Explainability status label.
    pub fn status(&self) -> &'static str {
        if self.decision.participate {
            "Included"
        } else {
            "Excluded"
        }
    }

    /// One-line summary for diagnostics.
    pub fn summary(&self) -> String {
        format!(
            "{} · {} · reason: {} · sensitivity={} · priority={}",
            self.provider_id,
            self.status(),
            self.decision.reason,
            self.sensitivity.as_str(),
            self.decision.priority.value()
        )
    }
}

/// Explainability report recorded on the ContextBundle / Inspector.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct PolicyReport {
    /// Active context policy ids.
    pub active_policies: Vec<String>,
    /// Per-provider decisions (registration order).
    pub decisions: Vec<PolicyDecisionSummary>,
    /// Estimated characters before policy filtering (relevance-pass providers).
    pub size_before_characters: usize,
    /// Estimated characters after policy filtering (allowed providers).
    pub size_after_characters: usize,
    /// Final assembled characters after budget fitting.
    pub size_assembled_characters: usize,
}

impl PolicyReport {
    /// Included provider ids.
    pub fn included_providers(&self) -> Vec<&str> {
        self.decisions
            .iter()
            .filter(|decision| decision.included)
            .map(|decision| decision.provider_id.as_str())
            .collect()
    }

    /// Excluded provider ids.
    pub fn excluded_providers(&self) -> Vec<&str> {
        self.decisions
            .iter()
            .filter(|decision| !decision.included)
            .map(|decision| decision.provider_id.as_str())
            .collect()
    }

    /// Plain-text render for diagnostics.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Context Policy".to_string());
        lines.push(format!(
            "active=[{}] · before={} chars · after={} chars · assembled={} chars",
            self.active_policies.join(","),
            self.size_before_characters,
            self.size_after_characters,
            self.size_assembled_characters
        ));
        lines.push(format!(
            "included=[{}] · excluded=[{}]",
            self.included_providers().join(","),
            self.excluded_providers().join(",")
        ));
        lines.push(String::new());
        lines.push(format!(
            "{:<14} {:<10} {}",
            "Provider", "Status", "Reason"
        ));
        lines.push("-".repeat(72));
        for decision in &self.decisions {
            lines.push(format!(
                "{:<14} {:<10} {}",
                decision.provider_id,
                if decision.included {
                    "Included"
                } else {
                    "Excluded"
                },
                decision.reason
            ));
        }
        lines.join("\n")
    }
}

/// Serializable decision summary (no non-serde fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyDecisionSummary {
    /// Provider id.
    pub provider_id: String,
    /// Included or excluded.
    pub included: bool,
    /// Explanation.
    pub reason: String,
    /// Sensitivity label.
    pub sensitivity: String,
    /// Effective priority.
    pub priority: u8,
    /// Whether truncation is allowed.
    pub can_truncate: bool,
    /// Whether user approval would be required.
    pub requires_user_approval: bool,
    /// Whether the contribution was truncated during fitting.
    pub truncated: bool,
    /// Applied contribution constraint labels.
    pub constraints: Vec<String>,
}

impl PolicyDecisionSummary {
    /// Build from a full decision record (truncated filled later).
    pub fn from_record(record: &ContextPolicyDecisionRecord) -> Self {
        Self {
            provider_id: record.provider_id.clone(),
            included: record.decision.participate,
            reason: record.decision.reason.clone(),
            sensitivity: record.sensitivity.as_str().to_string(),
            priority: record.decision.priority.value(),
            can_truncate: record.decision.can_truncate,
            requires_user_approval: record.decision.requires_user_approval,
            truncated: false,
            constraints: record.decision.constraints.labels(),
        }
    }
}
