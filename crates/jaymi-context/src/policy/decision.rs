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
    /// Whether surfacing this context requires prior user approval.
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
            exclude_sensitive: false,
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
    /// Drop current selection text (keep range metadata).
    pub redact_selection_text: bool,
}

impl ContributionConstraints {
    /// Merge with another set (OR for exclusions).
    pub fn merge(&mut self, other: &Self) {
        self.exclude_open_files = self.exclude_open_files || other.exclude_open_files;
        self.permission_summary_only =
            self.permission_summary_only || other.permission_summary_only;
        self.redact_memory_content = self.redact_memory_content || other.redact_memory_content;
        self.redact_search_previews = self.redact_search_previews || other.redact_search_previews;
        self.redact_selection_text = self.redact_selection_text || other.redact_selection_text;
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
        if self.redact_selection_text {
            labels.push("redact_selection_text".into());
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
                entry.resource = None;
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
    if constraints.redact_selection_text {
        if let Some(selection) = contribution.current_selection.as_mut() {
            selection.text = None;
        }
    }
    contribution
}

/// Apply a full policy decision to a contribution (constraints + exclude_sensitive).
///
/// Returns the shaped contribution and the constraint labels that actually ran.
pub fn apply_policy_to_contribution(
    contribution: ContextContribution,
    decision: &ContextPolicyDecision,
) -> (ContextContribution, Vec<String>) {
    let mut constraints = decision.constraints.clone();
    if decision.exclude_sensitive {
        constraints.redact_memory_content = true;
        constraints.redact_search_previews = true;
        constraints.redact_selection_text = true;
    }
    let labels = constraints.labels();
    (
        apply_contribution_constraints(contribution, &constraints),
        labels,
    )
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
    /// Sprint B2.7 candidate selection explainability.
    #[serde(default)]
    pub candidate_selection: crate::candidate::CandidateSelectionReport,
    /// Sprint B2.8 Context Selection profile id.
    #[serde(default)]
    pub selection_profile: Option<String>,
    /// Sprint B2.8 matched selection rule ids.
    #[serde(default)]
    pub selection_rules: Vec<String>,
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

    /// Providers omitted pending user approval.
    pub fn pending_approval_providers(&self) -> Vec<&str> {
        self.decisions
            .iter()
            .filter(|decision| decision.approval_status == "pending")
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
            "included=[{}] · excluded=[{}] · pending_approval=[{}]",
            self.included_providers().join(","),
            self.excluded_providers().join(","),
            self.pending_approval_providers().join(",")
        ));
        if self.candidate_selection.proposed > 0 {
            lines.push(format!(
                "candidates proposed={} selected={} rejected_policy={} rejected_budget={}",
                self.candidate_selection.proposed,
                self.candidate_selection.selected,
                self.candidate_selection.rejected_policy,
                self.candidate_selection.rejected_budget
            ));
        }
        if let Some(profile) = &self.selection_profile {
            lines.push(format!(
                "selection_profile={} rules=[{}]",
                profile,
                self.selection_rules.join(",")
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "{:<14} {:<10} {:<10} {:<12} {}",
            "Provider", "Status", "Approval", "Constraints", "Reason"
        ));
        lines.push("-".repeat(96));
        for decision in &self.decisions {
            let status = if decision.included {
                "Included"
            } else if decision.approval_status == "pending" {
                "Pending"
            } else {
                "Excluded"
            };
            let constraints = if decision.constraints.is_empty() {
                "-".to_string()
            } else {
                decision.constraints.join(",")
            };
            let mut reason = decision.reason.clone();
            if decision.exclude_sensitive {
                reason.push_str(" · exclude_sensitive");
            }
            if let Some(truncation) = &decision.truncation_reason {
                reason.push_str(" · truncated=");
                reason.push_str(truncation);
            }
            lines.push(format!(
                "{:<14} {:<10} {:<10} {:<12} {}",
                decision.provider_id, status, decision.approval_status, constraints, reason
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
    /// Included in the assembled bundle (after approval / relevance gates).
    pub included: bool,
    /// Explanation (exclusion, approval, or allow reason).
    pub reason: String,
    /// Sensitivity label.
    pub sensitivity: String,
    /// Effective priority.
    pub priority: u8,
    /// Whether truncation is allowed by policy.
    pub can_truncate: bool,
    /// Whether user approval is required before contribution.
    pub requires_user_approval: bool,
    /// Approval gate status: `not_required`, `approved`, `pending`, or `n/a`.
    pub approval_status: String,
    /// Whether high-sensitivity fields must be stripped.
    pub exclude_sensitive: bool,
    /// Whether relevance threshold was bypassed.
    pub bypass_relevance: bool,
    /// Whether the contribution was truncated during fitting.
    pub truncated: bool,
    /// Why truncation happened (or why it was refused), when applicable.
    pub truncation_reason: Option<String>,
    /// Enforced contribution constraint labels.
    pub constraints: Vec<String>,
}

impl PolicyDecisionSummary {
    /// Build from a full decision record (enforcement fields filled during assemble).
    pub fn from_record(record: &ContextPolicyDecisionRecord) -> Self {
        let approval_status = if !record.decision.participate {
            "n/a".to_string()
        } else if record.decision.requires_user_approval {
            "pending".to_string() // updated to approved when session allows
        } else {
            "not_required".to_string()
        };
        let mut constraints = record.decision.constraints.labels();
        if record.decision.exclude_sensitive {
            for label in [
                "redact_memory_content",
                "redact_search_previews",
                "redact_selection_text",
            ] {
                if !constraints.iter().any(|existing| existing == label) {
                    constraints.push(label.to_string());
                }
            }
        }
        Self {
            provider_id: record.provider_id.clone(),
            included: record.decision.participate,
            reason: record.decision.reason.clone(),
            sensitivity: record.sensitivity.as_str().to_string(),
            priority: record.decision.priority.value(),
            can_truncate: record.decision.can_truncate,
            requires_user_approval: record.decision.requires_user_approval,
            approval_status,
            exclude_sensitive: record.decision.exclude_sensitive,
            bypass_relevance: record.decision.bypass_relevance,
            truncated: false,
            truncation_reason: None,
            constraints,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::ProviderPriority;
    use crate::provider::ContextContribution;
    use crate::{
        BundlePermissionEntry, BundleSearchHit, CurrentSelectionSection, MemoryResultsSection,
        PermissionsSection, SearchResultsSection,
    };
    use jaymi_memory_engine::{
        AssembledMemoryContext, MemoryRecord, MemoryScope, MemoryStatus, RelevantMemory,
    };

    fn sample_memory_contribution(content: &str) -> ContextContribution {
        ContextContribution {
            memory_results: Some(MemoryResultsSection {
                memory: AssembledMemoryContext {
                    memories: vec![RelevantMemory {
                        record: MemoryRecord {
                            id: jaymi_core::EntityId::new("m1"),
                            scope: MemoryScope::Personal,
                            summary: "sum".into(),
                            content: content.into(),
                            conversation_id: None,
                            project_id: None,
                            importance: 50,
                            confidence: 50,
                            tags: Vec::new(),
                            source: None,
                            kind: None,
                            metadata_json: "{}".into(),
                            status: MemoryStatus::Active,
                            created_at: 0,
                            updated_at: 0,
                            archived_at: None,
                        },
                        score: 10,
                        reasons: Vec::new(),
                        why: "test".into(),
                    }],
                    project_id: None,
                    conversation_id: None,
                    candidate_count: 1,
                    truncated: false,
                },
                promotion_suggestions: Vec::new(),
                promotion_ask: jaymi_memory_engine::PromotionAskDecision::Defer,
            }),
            ..ContextContribution::default()
        }
    }

    #[test]
    fn exclude_sensitive_redacts_memory_search_and_selection() {
        let mut contribution = sample_memory_contribution("SECRET BODY");
        contribution.search_results = Some(SearchResultsSection {
            hint: None,
            hits: vec![BundleSearchHit {
                item_id: "1".into(),
                title: "t".into(),
                path: None,
                score: None,
                match_reason: None,
                preview: Some("preview secret".into()),
                line: None,
                column: None,
            }],
        });
        contribution.current_selection = Some(CurrentSelectionSection {
            path: Some("/a.rs".into()),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 4,
            text: Some("sel".into()),
        });

        let mut decision = ContextPolicyDecision::allow("test", ProviderPriority::MEMORY);
        decision.exclude_sensitive = true;
        let (shaped, labels) = apply_policy_to_contribution(contribution, &decision);
        assert!(labels.iter().any(|l| l == "redact_memory_content"));
        assert!(labels.iter().any(|l| l == "redact_search_previews"));
        assert!(labels.iter().any(|l| l == "redact_selection_text"));
        assert_eq!(
            shaped
                .memory_results
                .as_ref()
                .unwrap()
                .memory
                .memories[0]
                .record
                .content,
            ""
        );
        assert!(shaped
            .search_results
            .as_ref()
            .unwrap()
            .hits[0]
            .preview
            .is_none());
        assert!(shaped
            .current_selection
            .as_ref()
            .unwrap()
            .text
            .is_none());
    }

    #[test]
    fn permission_summary_only_clears_resource_and_explanation() {
        let contribution = ContextContribution {
            permissions: Some(PermissionsSection {
                entries: vec![BundlePermissionEntry {
                    category: "fs".into(),
                    action: "read".into(),
                    decision: "allowed".into(),
                    resource: Some("/secret".into()),
                    explanation: Some("why".into()),
                }],
            }),
            ..ContextContribution::default()
        };
        let mut decision = ContextPolicyDecision::allow("perm", ProviderPriority::PERMISSION);
        decision.constraints.permission_summary_only = true;
        let (shaped, labels) = apply_policy_to_contribution(contribution, &decision);
        assert!(labels.iter().any(|l| l == "permission_summary_only"));
        let entry = &shaped.permissions.as_ref().unwrap().entries[0];
        assert!(entry.resource.is_none());
        assert!(entry.explanation.is_none());
        assert_eq!(entry.decision, "allowed");
    }
}
