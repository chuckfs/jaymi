//! Approval History — durable record of Review Card decisions.
//!
//! Stores what the user decided (Approve / Modify / Cancel), optional reason,
//! modified child plan, and eventual execution result. Used for transparency,
//! Memory retrieval, Planner reasoning, and diagnostics.
//!
//! Content stays searchable in-session via [`ApprovalHistoryStore`]. Application
//! persists entries to the Memory Engine (`kind = approval_history`). Sensitive
//! fields are redacted when exposed outside the user's permission boundary.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::execution_plan::{ExecutionPlanId, ExecutionSummary};
use crate::review_card::ReviewIntent;

/// User decision recorded from a Review Card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// User accepted the plan.
    Approve,
    /// User requested changes; a child plan may follow.
    Modify,
    /// User rejected the plan; nothing executed.
    Cancel,
}

impl ApprovalDecision {
    /// Stable label for storage, search, and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Modify => "modify",
            Self::Cancel => "cancel",
        }
    }

    /// Parse a stored decision label.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approve" => Some(Self::Approve),
            "modify" => Some(Self::Modify),
            "cancel" => Some(Self::Cancel),
            _ => None,
        }
    }
}

impl From<&ReviewIntent> for ApprovalDecision {
    fn from(intent: &ReviewIntent) -> Self {
        match intent {
            ReviewIntent::Approve { .. } => Self::Approve,
            ReviewIntent::Modify { .. } => Self::Modify,
            ReviewIntent::Cancel { .. } => Self::Cancel,
        }
    }
}

/// How much Approval History detail a caller may see.
///
/// Full is for the local user (UI / diagnostics). Restricted is for Context
/// assembly, Planner reasoning exports, and any surface that must not leak
/// paths or free-text notes outside the user's permission boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApprovalHistoryAccess {
    /// Local user may see reasons, resources, and execution detail.
    Full,
    /// Redact reasons, resource paths, and execution detail; keep decision metadata.
    Restricted,
}

/// One Approval History row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalHistoryEntry {
    /// Execution Plan under review when the decision was made.
    pub plan_id: ExecutionPlanId,
    /// Unix seconds when the decision was recorded.
    pub timestamp: i64,
    /// Approve / Modify / Cancel.
    pub decision: ApprovalDecision,
    /// Optional free-text reason (Modify note, cancel explanation).
    pub reason: Option<String>,
    /// Child plan produced by Modify, when any.
    pub modified_plan_id: Option<ExecutionPlanId>,
    /// Parent plan when the reviewed plan was itself a revision.
    pub parent_plan_id: Option<ExecutionPlanId>,
    /// High-level execution outcome after Approve (or cancel summary).
    pub execution_result: Option<ApprovalExecutionResult>,
    /// Resources named on the plan (may be redacted for Restricted access).
    pub affected_resources: Vec<String>,
    /// Originating request / goal (may be redacted for Restricted access).
    pub goal: Option<String>,
    /// Conversation association when known.
    pub conversation_id: Option<String>,
    /// Project association when known.
    pub project_id: Option<String>,
}

/// Compact execution outcome linked to an approval decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalExecutionResult {
    /// Terminal plan status (`completed`, `failed`, `cancelled`, …).
    pub status: String,
    /// Whether the run was partial.
    pub partial: bool,
    /// Duration in milliseconds when known.
    pub duration_ms: u64,
    /// Tools that ran (empty when nothing executed).
    pub tools_executed: Vec<String>,
    /// Files edited (redacted under Restricted access).
    pub files_edited: Vec<String>,
    /// Short error line when failed / cancelled.
    pub error: Option<String>,
}

impl ApprovalExecutionResult {
    /// Build from a full [`ExecutionSummary`].
    pub fn from_summary(summary: &ExecutionSummary) -> Self {
        Self {
            status: summary.status.as_str().to_string(),
            partial: summary.partial,
            duration_ms: summary.duration_ms,
            tools_executed: summary.tools_executed.clone(),
            files_edited: summary.files_edited.clone(),
            error: summary.error.clone(),
        }
    }
}

/// Search filters for Approval History.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovalHistoryQuery {
    /// Free-text match against decision, reason, plan ids, goal, resources.
    pub text: Option<String>,
    /// Restrict to one plan id.
    pub plan_id: Option<ExecutionPlanId>,
    /// Restrict to one decision kind.
    pub decision: Option<ApprovalDecision>,
    /// Restrict to a conversation.
    pub conversation_id: Option<String>,
    /// Restrict to a project.
    pub project_id: Option<String>,
    /// Maximum rows (newest first when applied by the store).
    pub limit: Option<usize>,
}

/// Permission-aware view of an Approval History entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalHistoryView {
    pub plan_id: String,
    pub timestamp: i64,
    pub decision: String,
    pub reason: Option<String>,
    pub modified_plan_id: Option<String>,
    pub parent_plan_id: Option<String>,
    pub execution_status: Option<String>,
    pub execution_partial: Option<bool>,
    pub tools_executed: Vec<String>,
    pub files_edited: Vec<String>,
    pub affected_resources: Vec<String>,
    pub goal: Option<String>,
    pub conversation_id: Option<String>,
    pub project_id: Option<String>,
    /// True when sensitive fields were stripped for the access level.
    pub redacted: bool,
}

impl ApprovalHistoryEntry {
    /// Build an entry from a Review intent and Planner response.
    pub fn from_intent_and_response(
        intent: &ReviewIntent,
        plan_id: &ExecutionPlanId,
        modified_plan_id: Option<ExecutionPlanId>,
        parent_plan_id: Option<ExecutionPlanId>,
        affected_resources: Vec<String>,
        goal: Option<String>,
        execution_summary: Option<&ExecutionSummary>,
        conversation_id: Option<String>,
        project_id: Option<String>,
    ) -> Self {
        let reason = match intent {
            ReviewIntent::Modify { note, .. } => note
                .as_ref()
                .map(|note| note.trim().to_string())
                .filter(|note| !note.is_empty()),
            ReviewIntent::Cancel { .. } => Some("cancelled by user".into()),
            ReviewIntent::Approve { .. } => None,
        };
        Self {
            plan_id: plan_id.clone(),
            timestamp: unix_now(),
            decision: ApprovalDecision::from(intent),
            reason,
            modified_plan_id,
            parent_plan_id,
            execution_result: execution_summary.map(ApprovalExecutionResult::from_summary),
            affected_resources,
            goal,
            conversation_id,
            project_id,
        }
    }

    /// Whether this entry matches a search query.
    pub fn matches(&self, query: &ApprovalHistoryQuery) -> bool {
        if let Some(plan_id) = &query.plan_id {
            if &self.plan_id != plan_id {
                return false;
            }
        }
        if let Some(decision) = query.decision {
            if self.decision != decision {
                return false;
            }
        }
        if let Some(conversation_id) = &query.conversation_id {
            if self.conversation_id.as_deref() != Some(conversation_id.as_str()) {
                return false;
            }
        }
        if let Some(project_id) = &query.project_id {
            if self.project_id.as_deref() != Some(project_id.as_str()) {
                return false;
            }
        }
        if let Some(text) = query.text.as_ref().map(|t| t.trim().to_ascii_lowercase()) {
            if text.is_empty() {
                return true;
            }
            let haystack = self.search_blob().to_ascii_lowercase();
            if !haystack.contains(&text) {
                return false;
            }
        }
        true
    }

    /// Concatenated searchable text.
    pub fn search_blob(&self) -> String {
        let mut parts = vec![
            self.plan_id.as_str().to_string(),
            self.decision.as_str().to_string(),
        ];
        if let Some(reason) = &self.reason {
            parts.push(reason.clone());
        }
        if let Some(modified) = &self.modified_plan_id {
            parts.push(modified.as_str().to_string());
        }
        if let Some(parent) = &self.parent_plan_id {
            parts.push(parent.as_str().to_string());
        }
        if let Some(goal) = &self.goal {
            parts.push(goal.clone());
        }
        parts.extend(self.affected_resources.iter().cloned());
        if let Some(result) = &self.execution_result {
            parts.push(result.status.clone());
            parts.extend(result.tools_executed.iter().cloned());
            parts.extend(result.files_edited.iter().cloned());
            if let Some(error) = &result.error {
                parts.push(error.clone());
            }
        }
        parts.join(" ")
    }

    /// Permission-aware view of this entry.
    pub fn view_for(&self, access: ApprovalHistoryAccess) -> ApprovalHistoryView {
        match access {
            ApprovalHistoryAccess::Full => ApprovalHistoryView {
                plan_id: self.plan_id.as_str().to_string(),
                timestamp: self.timestamp,
                decision: self.decision.as_str().to_string(),
                reason: self.reason.clone(),
                modified_plan_id: self.modified_plan_id.as_ref().map(|id| id.as_str().into()),
                parent_plan_id: self.parent_plan_id.as_ref().map(|id| id.as_str().into()),
                execution_status: self
                    .execution_result
                    .as_ref()
                    .map(|result| result.status.clone()),
                execution_partial: self.execution_result.as_ref().map(|result| result.partial),
                tools_executed: self
                    .execution_result
                    .as_ref()
                    .map(|result| result.tools_executed.clone())
                    .unwrap_or_default(),
                files_edited: self
                    .execution_result
                    .as_ref()
                    .map(|result| result.files_edited.clone())
                    .unwrap_or_default(),
                affected_resources: self.affected_resources.clone(),
                goal: self.goal.clone(),
                conversation_id: self.conversation_id.clone(),
                project_id: self.project_id.clone(),
                redacted: false,
            },
            ApprovalHistoryAccess::Restricted => ApprovalHistoryView {
                plan_id: self.plan_id.as_str().to_string(),
                timestamp: self.timestamp,
                decision: self.decision.as_str().to_string(),
                reason: None,
                modified_plan_id: self.modified_plan_id.as_ref().map(|id| id.as_str().into()),
                parent_plan_id: self.parent_plan_id.as_ref().map(|id| id.as_str().into()),
                execution_status: self
                    .execution_result
                    .as_ref()
                    .map(|result| result.status.clone()),
                execution_partial: self.execution_result.as_ref().map(|result| result.partial),
                tools_executed: self
                    .execution_result
                    .as_ref()
                    .map(|result| {
                        result
                            .tools_executed
                            .iter()
                            .map(|_| "tool".to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                files_edited: Vec::new(),
                affected_resources: Vec::new(),
                goal: None,
                conversation_id: self.conversation_id.clone(),
                project_id: self.project_id.clone(),
                redacted: true,
            },
        }
    }

    /// Short Memory `summary` line (safe; no paths or notes).
    pub fn memory_summary_line(&self) -> String {
        format!(
            "Approval {}: plan {}{}",
            self.decision.as_str(),
            self.plan_id,
            self.modified_plan_id
                .as_ref()
                .map(|id| format!(" → {id}"))
                .unwrap_or_default()
        )
    }

    /// Memory content blob. Full detail for local persistence; retrieval
    /// surfaces must still apply [`Self::view_for`] before exporting.
    pub fn memory_content(&self) -> String {
        let mut lines = vec![
            format!("Decision: {}", self.decision.as_str()),
            format!("Plan: {}", self.plan_id),
            format!("Timestamp: {}", self.timestamp),
        ];
        if let Some(reason) = &self.reason {
            lines.push(format!("Reason: {reason}"));
        }
        if let Some(modified) = &self.modified_plan_id {
            lines.push(format!("Modified plan: {modified}"));
        }
        if let Some(parent) = &self.parent_plan_id {
            lines.push(format!("Parent plan: {parent}"));
        }
        if let Some(goal) = &self.goal {
            lines.push(format!("Goal: {goal}"));
        }
        if !self.affected_resources.is_empty() {
            lines.push(format!(
                "Resources: {}",
                self.affected_resources.join(", ")
            ));
        }
        if let Some(result) = &self.execution_result {
            lines.push(format!(
                "Execution: {} (partial={}, {} ms)",
                result.status, result.partial, result.duration_ms
            ));
            if !result.tools_executed.is_empty() {
                lines.push(format!("Tools: {}", result.tools_executed.join(", ")));
            }
            if !result.files_edited.is_empty() {
                lines.push(format!("Files: {}", result.files_edited.join(", ")));
            }
            if let Some(error) = &result.error {
                lines.push(format!("Error: {error}"));
            }
        }
        lines.join("\n")
    }

    /// JSON metadata for Memory filters (ids + decision only — no free text).
    pub fn memory_metadata_json(&self) -> String {
        let modified = self
            .modified_plan_id
            .as_ref()
            .map(|id| format!("\"{}\"", escape_json(id.as_str())))
            .unwrap_or_else(|| "null".into());
        let parent = self
            .parent_plan_id
            .as_ref()
            .map(|id| format!("\"{}\"", escape_json(id.as_str())))
            .unwrap_or_else(|| "null".into());
        let status = self
            .execution_result
            .as_ref()
            .map(|result| format!("\"{}\"", escape_json(&result.status)))
            .unwrap_or_else(|| "null".into());
        format!(
            r#"{{"kind":"approval_history","plan_id":"{}","decision":"{}","timestamp":{},"modified_plan_id":{},"parent_plan_id":{},"execution_status":{},"sensitivity":"private"}}"#,
            escape_json(self.plan_id.as_str()),
            self.decision.as_str(),
            self.timestamp,
            modified,
            parent,
            status
        )
    }

    /// Diagnostic one-liner.
    pub fn summary_line(&self) -> String {
        format!(
            "approval {} · plan={} · ts={} · modified={} · execution={}",
            self.decision.as_str(),
            self.plan_id,
            self.timestamp,
            self.modified_plan_id
                .as_ref()
                .map(|id| id.as_str())
                .unwrap_or("—"),
            self.execution_result
                .as_ref()
                .map(|result| result.status.as_str())
                .unwrap_or("—")
        )
    }

    /// Reconstruct from Memory metadata + content when possible.
    pub fn from_memory_record(
        summary: &str,
        content: &str,
        metadata_json: &str,
        conversation_id: Option<String>,
        project_id: Option<String>,
        created_at: i64,
    ) -> Option<Self> {
        let plan_id = extract_json_string(metadata_json, "plan_id")
            .or_else(|| extract_plan_from_summary(summary))?;
        let decision = extract_json_string(metadata_json, "decision")
            .and_then(|value| ApprovalDecision::parse(&value))
            .or_else(|| {
                content.lines().find_map(|line| {
                    line.strip_prefix("Decision: ")
                        .and_then(ApprovalDecision::parse)
                })
            })?;
        let timestamp = extract_json_number(metadata_json, "timestamp").unwrap_or(created_at);
        let modified_plan_id = extract_json_string(metadata_json, "modified_plan_id")
            .filter(|value| value != "null")
            .map(ExecutionPlanId::from_existing);
        let parent_plan_id = extract_json_string(metadata_json, "parent_plan_id")
            .filter(|value| value != "null")
            .map(ExecutionPlanId::from_existing);
        let reason = content.lines().find_map(|line| {
            line.strip_prefix("Reason: ")
                .map(str::to_string)
                .filter(|value| !value.is_empty())
        });
        let goal = content.lines().find_map(|line| {
            line.strip_prefix("Goal: ").map(str::to_string)
        });
        let affected_resources = content
            .lines()
            .find_map(|line| {
                line.strip_prefix("Resources: ").map(|resources| {
                    resources
                        .split(", ")
                        .filter(|part| !part.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        let execution_status = extract_json_string(metadata_json, "execution_status")
            .filter(|value| value != "null")
            .or_else(|| {
                content.lines().find_map(|line| {
                    line.strip_prefix("Execution: ")
                        .and_then(|rest| rest.split(' ').next())
                        .map(str::to_string)
                })
            });
        let execution_result = execution_status.map(|status| ApprovalExecutionResult {
            status,
            partial: content.contains("partial=true"),
            duration_ms: 0,
            tools_executed: content
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Tools: ").map(|tools| {
                        tools
                            .split(", ")
                            .filter(|part| !part.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                })
                .unwrap_or_default(),
            files_edited: content
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Files: ").map(|files| {
                        files
                            .split(", ")
                            .filter(|part| !part.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                })
                .unwrap_or_default(),
            error: content.lines().find_map(|line| {
                line.strip_prefix("Error: ").map(str::to_string)
            }),
        });

        Some(Self {
            plan_id: ExecutionPlanId::from_existing(plan_id),
            timestamp,
            decision,
            reason,
            modified_plan_id,
            parent_plan_id,
            execution_result,
            affected_resources,
            goal,
            conversation_id,
            project_id,
        })
    }
}

/// In-session searchable Approval History store (oldest → newest).
#[derive(Debug, Default, Clone)]
pub struct ApprovalHistoryStore {
    entries: Vec<ApprovalHistoryEntry>,
}

impl ApprovalHistoryStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an entry.
    pub fn record(&mut self, entry: ApprovalHistoryEntry) {
        self.entries.push(entry);
    }

    /// All entries (oldest first).
    pub fn entries(&self) -> &[ApprovalHistoryEntry] {
        &self.entries
    }

    /// Search newest-first with optional limit.
    pub fn search(&self, query: &ApprovalHistoryQuery) -> Vec<ApprovalHistoryEntry> {
        let mut matched: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.matches(query))
            .cloned()
            .collect();
        matched.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        if let Some(limit) = query.limit {
            matched.truncate(limit);
        }
        matched
    }

    /// Permission-aware search.
    pub fn search_views(
        &self,
        query: &ApprovalHistoryQuery,
        access: ApprovalHistoryAccess,
    ) -> Vec<ApprovalHistoryView> {
        self.search(query)
            .into_iter()
            .map(|entry| entry.view_for(access))
            .collect()
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn extract_json_number(json: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\":");
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let digits: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect();
    digits.parse().ok()
}

fn extract_plan_from_summary(summary: &str) -> Option<String> {
    // "Approval approve: plan <id>"
    let after = summary.split("plan ").nth(1)?;
    Some(
        after
            .split_whitespace()
            .next()?
            .trim_matches(|c| c == '→' || c == ',')
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{
        EstimatedReversibility, EstimatedRisk, ExecutionPlan, ExecutionPlanParams, ExecutionStatus,
        ExecutionStep, PlanPermissionRequirement, ReviewRequirement,
    };
    use jaymi_capabilities::Capability;
    use jaymi_core::IntentId;

    fn sample_plan() -> ExecutionPlan {
        ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Write secret notes".into(),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec!["write_file".into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Write".into(),
                tool_id: Some("write_file".into()),
                resource: Some("/private/notes.md".into()),
            }],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec!["/private/notes.md".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["written".into()],
            lineage: Default::default(),
        })
    }

    #[test]
    fn stores_and_searches_decisions() {
        let plan = sample_plan();
        let mut store = ApprovalHistoryStore::new();
        let entry = ApprovalHistoryEntry::from_intent_and_response(
            &ReviewIntent::Modify {
                plan_id: plan.id().clone(),
                note: Some("Skip README".into()),
            },
            plan.id(),
            Some(ExecutionPlanId::from_existing("child-1")),
            None,
            plan.affected_resources().to_vec(),
            Some(plan.originating_request().into()),
            None,
            Some("conv-1".into()),
            Some("proj-1".into()),
        );
        store.record(entry);

        let by_decision = store.search(&ApprovalHistoryQuery {
            decision: Some(ApprovalDecision::Modify),
            ..Default::default()
        });
        assert_eq!(by_decision.len(), 1);
        assert_eq!(by_decision[0].reason.as_deref(), Some("Skip README"));

        let by_text = store.search(&ApprovalHistoryQuery {
            text: Some("readme".into()),
            ..Default::default()
        });
        assert_eq!(by_text.len(), 1);

        let miss = store.search(&ApprovalHistoryQuery {
            decision: Some(ApprovalDecision::Approve),
            ..Default::default()
        });
        assert!(miss.is_empty());
    }

    #[test]
    fn restricted_view_redacts_sensitive_fields() {
        let plan = sample_plan();
        let mut summary = ExecutionSummary::from_plan(&plan, Vec::new(), Vec::new(), None);
        summary.status = ExecutionStatus::Completed;
        summary.files_edited = vec!["/private/notes.md".into()];
        summary.tools_executed = vec!["write_file".into()];

        let entry = ApprovalHistoryEntry::from_intent_and_response(
            &ReviewIntent::Approve {
                plan_id: plan.id().clone(),
            },
            plan.id(),
            None,
            None,
            plan.affected_resources().to_vec(),
            Some(plan.originating_request().into()),
            Some(&summary),
            None,
            None,
        );

        let full = entry.view_for(ApprovalHistoryAccess::Full);
        assert!(!full.redacted);
        assert_eq!(full.affected_resources, vec!["/private/notes.md"]);
        assert_eq!(full.files_edited, vec!["/private/notes.md"]);
        assert_eq!(full.goal.as_deref(), Some("Write secret notes"));

        let restricted = entry.view_for(ApprovalHistoryAccess::Restricted);
        assert!(restricted.redacted);
        assert!(restricted.reason.is_none());
        assert!(restricted.affected_resources.is_empty());
        assert!(restricted.files_edited.is_empty());
        assert!(restricted.goal.is_none());
        assert_eq!(restricted.decision, "approve");
        assert_eq!(restricted.execution_status.as_deref(), Some("completed"));
    }

    #[test]
    fn memory_roundtrip_preserves_decision_and_plan() {
        let plan = sample_plan();
        let entry = ApprovalHistoryEntry::from_intent_and_response(
            &ReviewIntent::Cancel {
                plan_id: plan.id().clone(),
            },
            plan.id(),
            None,
            None,
            plan.affected_resources().to_vec(),
            Some(plan.originating_request().into()),
            None,
            Some("c1".into()),
            Some("p1".into()),
        );
        let restored = ApprovalHistoryEntry::from_memory_record(
            &entry.memory_summary_line(),
            &entry.memory_content(),
            &entry.memory_metadata_json(),
            entry.conversation_id.clone(),
            entry.project_id.clone(),
            entry.timestamp,
        )
        .expect("restore");
        assert_eq!(restored.plan_id, entry.plan_id);
        assert_eq!(restored.decision, ApprovalDecision::Cancel);
        assert_eq!(restored.reason.as_deref(), Some("cancelled by user"));
    }
}
