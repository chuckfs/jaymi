//! Execution inspection data for developer Diagnostics.
//!
//! Aggregates the live Planner pause store, conversation Review Cards,
//! Approval History, and Execution Summaries so Coding / Developer Diagnostics
//! can explain why execution is paused or resumed.

use jaymi_planner::{
    ApprovalDecision, ApprovalHistoryView, ExecutionSummary, PausedPlanSnapshot, ReviewCardModel,
    ReviewCardState,
};

use crate::experience::ExperienceSession;

/// Aggregated execution-inspection snapshot for Diagnostics panels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionInspection {
    /// Plans currently paused awaiting review.
    pub paused: Vec<PausedPlanSnapshot>,
    /// Pending Review Cards still in the conversation.
    pub pending_reviews: Vec<PendingReviewDiag>,
    /// Completed Approve decisions (newest first).
    pub completed_approvals: Vec<ApprovalHistoryView>,
    /// Full Approval History views (newest first).
    pub approval_history: Vec<ApprovalHistoryView>,
    /// Recent Execution Summaries from the conversation.
    pub execution_summaries: Vec<ExecutionSummaryDiag>,
    /// Last recorded resume / cancel note for developers.
    pub last_resume_note: Option<String>,
}

/// Compact pending Review Card row for Diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReviewDiag {
    pub plan_id: String,
    pub state: String,
    pub summary: String,
    pub risk: String,
    pub permissions: Vec<String>,
    pub revision: u32,
}

/// Compact Execution Summary row for Diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSummaryDiag {
    pub plan_id: String,
    pub status: String,
    pub goal: String,
    pub duration_ms: u64,
    pub tools: Vec<String>,
    pub files_edited: Vec<String>,
    pub files_moved_to_trash: Vec<String>,
    pub files_permanently_deleted: Vec<String>,
    pub recovery_available: Option<bool>,
    pub deletion_method: Option<String>,
    pub partial: bool,
    pub error: Option<String>,
}

impl ExecutionSummaryDiag {
    fn from_summary(summary: &ExecutionSummary) -> Self {
        Self {
            plan_id: summary.plan_id.as_str().to_string(),
            status: summary.status.as_str().to_string(),
            goal: summary.goal.clone(),
            duration_ms: summary.duration_ms,
            tools: summary.tools_executed.clone(),
            files_edited: summary.files_edited.clone(),
            files_moved_to_trash: summary.files_moved_to_trash.clone(),
            files_permanently_deleted: summary.files_permanently_deleted.clone(),
            recovery_available: summary.recovery_available,
            deletion_method: summary
                .deletion_method
                .map(|method| method.as_str().to_string()),
            partial: summary.partial,
            error: summary.error.clone(),
        }
    }
}

impl PendingReviewDiag {
    fn from_card(card: &ReviewCardModel) -> Self {
        let state = match &card.state {
            ReviewCardState::Pending => "pending".to_string(),
            ReviewCardState::Resolved { intent } => format!("resolved:{}", intent.as_str()),
        };
        Self {
            plan_id: card.plan_id.as_str().to_string(),
            state,
            summary: card.summary.clone(),
            risk: card.risk_level.as_str().to_string(),
            permissions: card.permissions.clone(),
            revision: card.revision,
        }
    }
}

/// Build an [`ExecutionInspection`] from Planner + conversation + history.
pub fn build_execution_inspection(
    paused: Vec<PausedPlanSnapshot>,
    experience: &ExperienceSession,
    approval_history: Vec<ApprovalHistoryView>,
) -> ExecutionInspection {
    let pending_reviews: Vec<_> = experience
        .conversation()
        .iter()
        .rev()
        .filter_map(|turn| turn.review.as_ref())
        .filter(|card| card.state.is_pending())
        .map(PendingReviewDiag::from_card)
        .collect();

    let execution_summaries: Vec<_> = experience
        .conversation()
        .iter()
        .rev()
        .filter_map(|turn| turn.execution_summary.as_ref())
        .take(8)
        .map(ExecutionSummaryDiag::from_summary)
        .collect();

    let completed_approvals: Vec<_> = approval_history
        .iter()
        .filter(|entry| entry.decision == ApprovalDecision::Approve.as_str())
        .cloned()
        .collect();

    let last_resume_note = approval_history.first().map(|entry| {
        let execution = entry
            .execution_status
            .as_deref()
            .unwrap_or("no execution");
        match entry.decision.as_str() {
            "approve" => format!(
                "Last resume: Approve on plan {} → execution={execution}",
                entry.plan_id
            ),
            "modify" => format!(
                "Last modify: plan {} → child {} (re-paused for approval)",
                entry.plan_id,
                entry.modified_plan_id.as_deref().unwrap_or("—")
            ),
            "cancel" => format!(
                "Last cancel: plan {} dropped; nothing executed",
                entry.plan_id
            ),
            other => format!("Last decision={other} on plan {}", entry.plan_id),
        }
    });

    ExecutionInspection {
        paused,
        pending_reviews,
        completed_approvals,
        approval_history,
        execution_summaries,
        last_resume_note,
    }
}

/// Titles of execution-inspection sections (stable for tests / UI filters).
pub const EXECUTION_INSPECTION_SECTION_TITLES: &[&str] = &[
    "Current Execution Plan",
    "Review state",
    "Risk",
    "Deletion strategy",
    "Permissions",
    "Planner pause state",
    "Pending approvals",
    "Completed approvals",
    "Execution summaries",
    "Approval history",
];

/// Build the execution-inspection Diagnostics sections.
pub fn execution_inspection_sections(
    inspection: &ExecutionInspection,
) -> Vec<crate::coding_workspace::CodingDiagnosticsSection> {
    use crate::coding_workspace::CodingDiagnosticsSection;

    let current = inspection.paused.first();
    let mut sections = Vec::new();

    sections.push(CodingDiagnosticsSection {
        title: "Current Execution Plan".into(),
        lines: match current {
            Some(plan) => {
                let mut lines = vec![
                    format!("id={} · status={} · rev={}", plan.plan_id, plan.plan_status, plan.revision),
                    format!("goal={}", truncate(&plan.originating_request, 140)),
                    format!(
                        "intent={} · capability={} · tool={}",
                        plan.planner_intent, plan.capability_id, plan.tool_id
                    ),
                ];
                if let Some(parent) = &plan.parent_plan_id {
                    lines.push(format!("parent={parent}"));
                }
                if !plan.steps.is_empty() {
                    lines.push("steps:".into());
                    for step in &plan.steps {
                        lines.push(format!("  {step}"));
                    }
                }
                if !plan.affected_resources.is_empty() {
                    lines.push(format!(
                        "resources={}",
                        plan.affected_resources.join(", ")
                    ));
                }
                if !plan.revision_changes.is_empty() {
                    lines.push("revision changes:".into());
                    for change in &plan.revision_changes {
                        lines.push(format!("  • {change}"));
                    }
                }
                lines
            }
            None => vec![
                "none (no Execution Plan currently paused)".into(),
                inspection
                    .last_resume_note
                    .clone()
                    .unwrap_or_else(|| "No recent review decision.".into()),
            ],
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Review state".into(),
        lines: {
            let mut lines = Vec::new();
            if let Some(plan) = current {
                lines.push(format!(
                    "paused plan {} · review={}",
                    plan.plan_id, plan.review_requirement
                ));
                lines.push("card state=pending (AwaitingReview)".into());
            } else if let Some(pending) = inspection.pending_reviews.first() {
                lines.push(format!(
                    "conversation card plan={} · state={}",
                    pending.plan_id, pending.state
                ));
            } else {
                lines.push("no review pending".into());
            }
            if let Some(note) = &inspection.last_resume_note {
                lines.push(note.clone());
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Risk".into(),
        lines: match current {
            Some(plan) => vec![
                format!("estimated_risk={}", plan.risk),
                format!("reversibility={}", plan.reversibility),
                format!("tool={}", plan.tool_id),
            ],
            None => inspection
                .pending_reviews
                .first()
                .map(|pending| vec![format!("pending review risk={}", pending.risk)])
                .unwrap_or_else(|| vec!["no active plan risk".into()]),
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Deletion strategy".into(),
        lines: match current.and_then(|plan| plan.deletion_method.as_deref()) {
            Some(method) => vec![
                format!("deletion_method={method}"),
                match method {
                    "trash" => "Recoverable via OS Trash / Recycle Bin.".into(),
                    "permanent" => "Permanent delete — not recoverable via Trash.".into(),
                    other => format!("method={other}"),
                },
            ],
            None => {
                let from_summary = inspection.execution_summaries.iter().find_map(|summary| {
                    summary.deletion_method.as_ref().map(|method| {
                        vec![
                            format!("last summary deletion_method={method}"),
                            format!(
                                "moved={} · permanent={} · recovery={}",
                                summary.files_moved_to_trash.len(),
                                summary.files_permanently_deleted.len(),
                                summary
                                    .recovery_available
                                    .map(|value| if value { "yes" } else { "no" })
                                    .unwrap_or("n/a")
                            ),
                        ]
                    })
                });
                from_summary.unwrap_or_else(|| vec!["no deletion strategy on the active plan".into()])
            }
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Permissions".into(),
        lines: match current {
            Some(plan) => {
                let mut lines = Vec::new();
                lines.push(format!(
                    "required={}",
                    if plan.permissions.is_empty() {
                        "none".into()
                    } else {
                        plan.permissions.join(", ")
                    }
                ));
                if let Some(decision) = &plan.permission_decision {
                    lines.push(format!("gate decision={decision}"));
                }
                if let Some(explanation) = &plan.permission_explanation {
                    lines.push(format!("explanation={}", truncate(explanation, 160)));
                }
                if let Some(decision) = &plan.policy_decision {
                    lines.push(format!("policy decision={decision}"));
                }
                if let Some(explanation) = &plan.policy_explanation {
                    lines.push(format!("policy={}", truncate(explanation, 160)));
                }
                lines
            }
            None => vec!["no plan permissions (nothing paused)".into()],
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Planner pause state".into(),
        lines: {
            let mut lines = vec![format!("paused_count={}", inspection.paused.len())];
            if inspection.paused.is_empty() {
                lines.push("running freely — no pause store entries".into());
                if let Some(note) = &inspection.last_resume_note {
                    lines.push(note.clone());
                }
            } else {
                for plan in &inspection.paused {
                    lines.push(format!(
                        "plan={} · paused_for={}s · tool={}",
                        plan.plan_id, plan.paused_for_secs, plan.tool_id
                    ));
                    for line in plan.pause_explanation.lines() {
                        lines.push(line.to_string());
                    }
                    lines.push("How to resume:".into());
                    for line in plan.resume_explanation.lines() {
                        lines.push(format!("  {line}"));
                    }
                }
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Pending approvals".into(),
        lines: if inspection.pending_reviews.is_empty() && inspection.paused.is_empty() {
            vec!["none".into()]
        } else {
            let mut lines = Vec::new();
            for plan in &inspection.paused {
                lines.push(format!(
                    "paused {} · risk={} · perms=[{}]",
                    plan.plan_id,
                    plan.risk,
                    plan.permissions.join(", ")
                ));
            }
            for pending in &inspection.pending_reviews {
                if inspection
                    .paused
                    .iter()
                    .any(|plan| plan.plan_id == pending.plan_id)
                {
                    continue;
                }
                lines.push(format!(
                    "card {} · {} · risk={} · rev={}",
                    pending.plan_id, pending.state, pending.risk, pending.revision
                ));
            }
            if lines.is_empty() {
                lines.push("none".into());
            }
            lines
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Completed approvals".into(),
        lines: if inspection.completed_approvals.is_empty() {
            vec!["none".into()]
        } else {
            inspection
                .completed_approvals
                .iter()
                .take(8)
                .map(|entry| {
                    format!(
                        "approve · plan={} · execution={} · ts={}",
                        entry.plan_id,
                        entry.execution_status.as_deref().unwrap_or("—"),
                        entry.timestamp
                    )
                })
                .collect()
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Execution summaries".into(),
        lines: if inspection.execution_summaries.is_empty() {
            vec!["none".into()]
        } else {
            inspection
                .execution_summaries
                .iter()
                .flat_map(|summary| {
                    let mut lines = vec![format!(
                        "{} · plan={} · {}ms · partial={}",
                        summary.status, summary.plan_id, summary.duration_ms, summary.partial
                    )];
                    lines.push(format!("  goal={}", truncate(&summary.goal, 100)));
                    if !summary.tools.is_empty() {
                        lines.push(format!("  tools={}", summary.tools.join(", ")));
                    }
                    if !summary.files_edited.is_empty() {
                        lines.push(format!("  files={}", summary.files_edited.join(", ")));
                    }
                    if let Some(error) = &summary.error {
                        lines.push(format!("  error={}", truncate(error, 120)));
                    }
                    lines
                })
                .collect()
        },
    });

    sections.push(CodingDiagnosticsSection {
        title: "Approval history".into(),
        lines: if inspection.approval_history.is_empty() {
            vec!["none".into()]
        } else {
            let mut lines = vec![format!("entries={}", inspection.approval_history.len())];
            for entry in inspection.approval_history.iter().take(10) {
                let modified = entry
                    .modified_plan_id
                    .as_deref()
                    .map(|id| format!(" → {id}"))
                    .unwrap_or_default();
                let execution = entry.execution_status.as_deref().unwrap_or("—");
                let reason = entry
                    .reason
                    .as_deref()
                    .map(|reason| format!(" · reason={}", truncate(reason, 60)))
                    .unwrap_or_default();
                lines.push(format!(
                    "{} · plan={}{} · execution={}{reason}",
                    entry.decision, entry.plan_id, modified, execution
                ));
            }
            lines
        },
    });

    sections
}

fn truncate(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let shortened: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{shortened}…")
}
