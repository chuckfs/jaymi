//! Review Card — conversational approval surface for Execution Plans.
//!
//! Review happens **inside the conversation**, never as a modal. The card
//! speaks like Jaymi: a short lead-in, a clear Plan, an approval notice, and
//! Approve / Modify / Cancel as [`ReviewIntent`] only — with example modify
//! phrases the user can type.
//!
//! **Invariant:** ReviewCard never executes tools and never talks to
//! providers. Callers map intents to [`crate::Planner::resolve_review`], which
//! resumes, revises, or invalidates the paused plan.

use serde::{Deserialize, Serialize};

use crate::execution_plan::{
    EstimatedReversibility, EstimatedRisk, ExecutionPlan, ExecutionPlanId, ExecutionStatus,
};

/// User intent emitted by a Review Card. Never implies execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ReviewIntent {
    /// User accepts the proposed plan as-is.
    Approve {
        /// Plan under review.
        plan_id: ExecutionPlanId,
    },
    /// User wants the plan changed before any execution.
    Modify {
        /// Plan under review.
        plan_id: ExecutionPlanId,
        /// Optional free-text guidance for how to change the plan.
        note: Option<String>,
    },
    /// User rejects the plan; nothing should run.
    Cancel {
        /// Plan under review.
        plan_id: ExecutionPlanId,
    },
}

impl ReviewIntent {
    /// Plan this intent refers to.
    pub fn plan_id(&self) -> &ExecutionPlanId {
        match self {
            Self::Approve { plan_id } | Self::Modify { plan_id, .. } | Self::Cancel { plan_id } => {
                plan_id
            }
        }
    }

    /// Stable label for diagnostics and chat acknowledgements.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Approve { .. } => "approve",
            Self::Modify { .. } => "modify",
            Self::Cancel { .. } => "cancel",
        }
    }

    /// Short human acknowledgement suitable for a conversation turn.
    pub fn acknowledgement(&self) -> String {
        match self {
            Self::Approve { plan_id } => {
                format!("Approved plan {plan_id}. Waiting for the Planner to proceed.")
            }
            Self::Modify { plan_id, note } => {
                if let Some(note) = note.as_ref().filter(|n| !n.trim().is_empty()) {
                    format!("Requested changes to plan {plan_id}: {note}")
                } else {
                    format!("Requested changes to plan {plan_id}.")
                }
            }
            Self::Cancel { plan_id } => {
                format!("Cancelled plan {plan_id}. No action will run.")
            }
        }
    }
}

/// Coarse estimate of how long the plan would take once approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimatedDuration {
    /// Essentially immediate (typical reads).
    Instant,
    /// A few seconds.
    Seconds,
    /// May take a minute or longer.
    Longer,
}

impl EstimatedDuration {
    /// Stable label for display.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Seconds => "a few seconds",
            Self::Longer => "may take longer",
        }
    }

    /// Derive a coarse duration from risk and step count.
    pub fn from_plan(risk: EstimatedRisk, step_count: usize) -> Self {
        match risk {
            EstimatedRisk::Low if step_count <= 1 => Self::Instant,
            EstimatedRisk::Low | EstimatedRisk::Medium if step_count <= 3 => Self::Seconds,
            _ => Self::Longer,
        }
    }
}

impl std::fmt::Display for EstimatedDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Whether the card is still waiting on the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ReviewCardState {
    /// Buttons are interactive; no intent recorded yet.
    Pending,
    /// User already chose; further clicks must be ignored.
    Resolved {
        /// Intent the user communicated.
        intent: ReviewIntent,
    },
}

impl ReviewCardState {
    /// True while Approve / Modify / Cancel may still be chosen.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}

/// Reusable Review Card view-model for desktop UI and future chat rendering.
///
/// Built from an [`ExecutionPlan`]. Content mirrors the plan; the card never
/// mutates the plan. Primary surface is conversational (opening + Plan +
/// notice + choices); the four transparency questions remain available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCardModel {
    /// Plan this card reviews.
    pub plan_id: ExecutionPlanId,
    /// Plan status at card creation (informational).
    pub plan_status: ExecutionStatus,
    /// Conversational lead-in (e.g. "I can do that.").
    pub opening: String,
    /// Ordered plan bullets shown under **Plan**.
    pub plan_items: Vec<String>,
    /// Why approval is required (destructive / irreversible / policy).
    pub approval_notice: String,
    /// Example modify phrases the user may type.
    pub modify_examples: Vec<String>,
    /// 1. What are you asking me to do?
    pub asking_to_do: String,
    /// 2. What exactly will change?
    pub what_will_change: String,
    /// 3. Why is this the proposed plan?
    pub why_proposed: String,
    /// 4. What happens after approval?
    pub after_approval: String,
    /// Short summary line.
    pub summary: String,
    /// Resources the plan would touch.
    pub affected_resources: Vec<String>,
    /// Estimated risk.
    pub risk_level: EstimatedRisk,
    /// Permission labels (`filesystem:write`, …).
    pub permissions: Vec<String>,
    /// Coarse duration estimate.
    pub estimated_duration: EstimatedDuration,
    /// Reversibility of effects.
    pub reversibility: EstimatedReversibility,
    /// Deletion method when this card reviews a delete plan.
    pub deletion_method: Option<jaymi_core::DeletionMethod>,
    /// Structured preview of what will change (Preview Before Action).
    pub action_preview: Option<jaymi_core::ActionPreview>,
    /// Interactive vs resolved.
    pub state: ReviewCardState,
    /// 1-based revision number.
    pub revision: u32,
    /// Parent plan when this card reviews a Modify revision.
    pub parent_plan_id: Option<ExecutionPlanId>,
    /// Diff vs the previous plan revision.
    pub revision_changes: Vec<String>,
}

impl ReviewCardModel {
    /// Build a pending Review Card from an execution plan.
    ///
    /// `explanation` may carry permission / policy context shown under “why”
    /// and folded into the approval notice when present.
    pub fn from_plan(plan: &ExecutionPlan, explanation: Option<&str>) -> Self {
        let primary_tool = plan.primary_tool_id().unwrap_or("tool");
        let primary_resource = plan
            .affected_resources()
            .first()
            .cloned()
            .unwrap_or_else(|| "the selected resource".into());
        let display_resource = display_resource_label(&primary_resource);

        let opening = if plan.revision() > 1 {
            "I revised the plan. Please review it again.".to_string()
        } else {
            "I can do that.".to_string()
        };

        let plan_items = conversational_plan_items(plan, &display_resource, primary_tool);
        let modify_examples = modify_examples_for(plan, primary_tool);
        let approval_notice = approval_notice_for(plan, explanation);

        let asking_to_do = {
            let request = plan.originating_request().trim();
            if request.is_empty() {
                format!(
                    "Run `{}` for intent `{}`.",
                    primary_tool,
                    plan.planner_intent().as_str()
                )
            } else {
                request.to_string()
            }
        };

        let what_will_change = if plan_items.is_empty() {
            "No concrete steps were proposed.".to_string()
        } else {
            plan_items
                .iter()
                .map(|item| format!("• {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let why_proposed = {
            let mut parts = Vec::new();
            parts.push(format!(
                "This matches your request using `{}` via capability `{}`.",
                primary_tool,
                plan.capability().id()
            ));
            if let Some(explanation) = explanation.map(str::trim).filter(|s| !s.is_empty()) {
                parts.push(explanation.to_string());
            }
            if plan.review_requirement()
                == crate::execution_plan::ReviewRequirement::Required
            {
                parts.push("Nothing runs until you approve.".into());
            }
            parts.join(" ")
        };

        let after_approval = match plan.deletion_method() {
            Some(jaymi_core::DeletionMethod::Trash) => {
                "After approval, Jaymi will move the selected files to the Trash. You can restore them from Trash.".into()
            }
            Some(jaymi_core::DeletionMethod::Permanent) => {
                "After approval, Jaymi will permanently delete these files. This cannot be undone from Trash.".into()
            }
            None => match plan.estimated_reversibility() {
                EstimatedReversibility::Irreversible => {
                    "After approval, Jaymi will execute this plan. The change may be difficult or impossible to undo.".into()
                }
                EstimatedReversibility::PartiallyReversible => {
                    "After approval, Jaymi will execute this plan. Some effects can be undone; others may not.".into()
                }
                EstimatedReversibility::FullyReversible => {
                    "After approval, Jaymi will execute this plan. Effects should be reversible.".into()
                }
            },
        };

        let permissions = plan
            .permissions_required()
            .iter()
            .map(|permission| permission.label())
            .collect();

        Self {
            plan_id: plan.id().clone(),
            plan_status: plan.status(),
            opening,
            plan_items,
            approval_notice,
            modify_examples,
            asking_to_do,
            what_will_change,
            why_proposed,
            after_approval,
            summary: plan.summary(),
            affected_resources: plan.affected_resources().to_vec(),
            risk_level: plan.estimated_risk(),
            permissions,
            estimated_duration: EstimatedDuration::from_plan(
                plan.estimated_risk(),
                plan.steps().len().max(1),
            ),
            reversibility: plan.estimated_reversibility(),
            deletion_method: plan.deletion_method(),
            action_preview: plan.action_preview().cloned(),
            state: ReviewCardState::Pending,
            revision: plan.revision(),
            parent_plan_id: plan.parent_plan_id().cloned(),
            revision_changes: plan.revision_changes().to_vec(),
        }
    }

    /// Record a user intent. Returns `None` if the card was already resolved
    /// or the intent targets a different plan. Does **not** execute anything.
    pub fn communicate(&mut self, intent: ReviewIntent) -> Option<ReviewIntent> {
        if !self.state.is_pending() {
            return None;
        }
        if intent.plan_id() != &self.plan_id {
            return None;
        }
        self.state = ReviewCardState::Resolved {
            intent: intent.clone(),
        };
        Some(intent)
    }

    /// Resolved intent, when the user has already chosen.
    pub fn resolved_intent(&self) -> Option<&ReviewIntent> {
        match &self.state {
            ReviewCardState::Resolved { intent } => Some(intent),
            ReviewCardState::Pending => None,
        }
    }

    /// Conversational plain-text body for chat bubbles and future clients.
    pub fn render_text(&self) -> String {
        self.render_text_with_preview(false)
    }

    /// Conversational body with optional expanded preview.
    pub fn render_text_with_preview(&self, expand_preview: bool) -> String {
        let mut lines = Vec::new();
        lines.push(self.opening.clone());
        lines.push(String::new());
        lines.push("Plan".into());
        if self.plan_items.is_empty() {
            lines.push("• (no concrete steps)".into());
        } else {
            for item in &self.plan_items {
                lines.push(format!("• {item}"));
            }
        }
        if let Some(preview) = &self.action_preview {
            lines.push(String::new());
            let display = if expand_preview {
                preview.clone()
            } else {
                preview.clone().truncate_for_display(
                    jaymi_core::PREVIEW_MAX_BODY_LINES,
                    jaymi_core::PREVIEW_MAX_BODY_CHARS,
                )
            };
            lines.push(display.render_text(expand_preview));
        }
        if self.revision > 1 || !self.revision_changes.is_empty() {
            lines.push(String::new());
            lines.push(format!("Changes in revision {}", self.revision));
            if let Some(parent) = &self.parent_plan_id {
                lines.push(format!("Supersedes plan {}", parent.as_str()));
            }
            for change in &self.revision_changes {
                lines.push(format!("• {change}"));
            }
        }
        lines.push(String::new());
        lines.push(self.approval_notice.clone());
        lines.push(String::new());
        lines.push("You can:".into());
        lines.push("• Approve".into());
        lines.push("• Cancel".into());
        lines.push("• Modify the plan".into());
        if !self.modify_examples.is_empty() {
            lines.push("  For example:".into());
            for example in &self.modify_examples {
                lines.push(format!("  • \"{example}\""));
            }
        }
        match &self.state {
            ReviewCardState::Pending => {}
            ReviewCardState::Resolved { intent } => {
                lines.push(String::new());
                lines.push(format!("Resolved: {}", intent.as_str()));
            }
        }
        lines.join("\n")
    }
}

fn display_resource_label(resource: &str) -> String {
    let trimmed = resource.trim();
    if trimmed.is_empty() || trimmed == "unspecified" {
        return "the selected path".into();
    }
    // Prefer a short trailing path segment for conversation, keep full when short.
    let path = std::path::Path::new(trimmed);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if trimmed.chars().count() > 48 {
            return name.to_string();
        }
    }
    trimmed.to_string()
}

fn conversational_plan_items(
    plan: &ExecutionPlan,
    display_resource: &str,
    primary_tool: &str,
) -> Vec<String> {
    let mut items = Vec::new();
    let resource = display_resource;

    // Prefer human step descriptions when they already read conversationally.
    let step_descs: Vec<String> = plan
        .steps()
        .iter()
        .map(|step| step.description.trim().to_string())
        .filter(|description| !description.is_empty())
        .collect();

    let looks_generic = step_descs.iter().all(|description| {
        matches!(
            description.as_str(),
            "Manage path" | "Write file" | "Execute tool" | "Write" | "Delete path"
        ) || description.starts_with("Write file")
            || description == "Write"
    });

    if !step_descs.is_empty() && !looks_generic {
        items.extend(step_descs);
    } else {
        match primary_tool {
            "manage_path" => {
                let action = plan
                    .permissions_required()
                    .iter()
                    .find(|permission| permission.action == "delete")
                    .map(|_| "delete")
                    .or_else(|| {
                        plan.originating_request()
                            .to_ascii_lowercase()
                            .contains("delete")
                            .then_some("delete")
                    })
                    .unwrap_or("change");
                if action == "delete" {
                    match plan.deletion_method() {
                        Some(jaymi_core::DeletionMethod::Trash) => {
                            items.push(format!("Delete {resource}"));
                            items.push("Deletion Method: Trash".into());
                            items.push("Move the selected files to the Trash".into());
                            items.push("Update the project index afterward".into());
                        }
                        Some(jaymi_core::DeletionMethod::Permanent) => {
                            items.push(format!("Permanently delete {resource}"));
                            items.push("Deletion Method: Permanent".into());
                            items.push("Remove the folder or file with no Trash recovery".into());
                            items.push("Update the project index afterward".into());
                        }
                        None => {
                            items.push(format!("Delete {resource}"));
                            items.push("Remove the folder or file and everything inside it".into());
                            items.push("Update the project index afterward".into());
                        }
                    }
                } else if plan.originating_request().to_ascii_lowercase().contains("rename")
                    || plan
                        .steps()
                        .iter()
                        .any(|step| step.description.to_ascii_lowercase().contains("rename"))
                {
                    items.push(format!("Rename {resource}"));
                    items.push("Update references that depend on this path".into());
                } else {
                    items.push(format!("Change {resource}"));
                }
            }
            "write_file" => {
                items.push(format!("Write {resource}"));
                items.push("Overwrite existing contents if the file already exists".into());
            }
            "terminal" => {
                items.push("Run a terminal command".into());
                items.push(format!("Working context: {resource}"));
                items.push("Capture command output for the conversation".into());
            }
            "git" => {
                items.push("Perform a Git operation".into());
                items.push(format!("Repository: {resource}"));
            }
            _ => {
                items.push(format!("Run `{primary_tool}` on {resource}"));
            }
        }
    }

    if matches!(
        plan.estimated_risk(),
        EstimatedRisk::High | EstimatedRisk::Medium
    ) && !items.iter().any(|item| item.to_ascii_lowercase().contains("destructive")
        || item.to_ascii_lowercase().contains("irreversible"))
        && matches!(
            plan.estimated_reversibility(),
            EstimatedReversibility::Irreversible
        )
    {
        // Keep impact in the notice; plan stays action-focused.
    }

    items
}

fn approval_notice_for(plan: &ExecutionPlan, explanation: Option<&str>) -> String {
    let mut notice = if let Some(method) = plan.deletion_method() {
        match method {
            jaymi_core::DeletionMethod::Trash => {
                "This action moves the selected files to the Trash and can be undone.".to_string()
            }
            jaymi_core::DeletionMethod::Permanent => {
                "This action permanently deletes these files.".to_string()
            }
        }
    } else {
        match (plan.estimated_risk(), plan.estimated_reversibility()) {
            (EstimatedRisk::High, EstimatedReversibility::Irreversible)
            | (_, EstimatedReversibility::Irreversible) => {
                "This action is destructive and requires your approval.".to_string()
            }
            (EstimatedRisk::High, _) => {
                "This action has high risk and requires your approval.".to_string()
            }
            (EstimatedRisk::Medium, EstimatedReversibility::PartiallyReversible) => {
                "This will change your workspace and requires your approval.".to_string()
            }
            _ => "This action requires your approval before anything runs.".to_string(),
        }
    };
    if let Some(explanation) = explanation.map(str::trim).filter(|s| !s.is_empty()) {
        notice = format!("{notice} {explanation}");
    }
    notice
}

fn modify_examples_for(plan: &ExecutionPlan, primary_tool: &str) -> Vec<String> {
    let is_delete = plan
        .permissions_required()
        .iter()
        .any(|permission| permission.action == "delete")
        || plan
            .originating_request()
            .to_ascii_lowercase()
            .contains("delete")
        || primary_tool == "manage_path"
            && plan
                .steps()
                .iter()
                .any(|step| step.description.to_ascii_lowercase().contains("delete"));

    if is_delete {
        return vec![
            "Keep today's files.".into(),
            "Only delete the cache.".into(),
            "Show me the files first.".into(),
        ];
    }

    match primary_tool {
        "write_file" => vec![
            "Rename instead of overwrite.".into(),
            "Skip README.".into(),
            "Show me the diff first.".into(),
        ],
        "terminal" => vec![
            "Run it dry-run / with --help first.".into(),
            "Use a safer flag.".into(),
            "Show me the exact command.".into(),
        ],
        "git" => vec![
            "Stage only these files.".into(),
            "Don't discard — show me the diff.".into(),
            "Commit without amending.".into(),
        ],
        _ => vec![
            "Skip README.".into(),
            "Do less — only the temporary files.".into(),
            "Show me what would change first.".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{
        ExecutionPlanParams, ExecutionStep, PlanPermissionRequirement, ReviewRequirement,
    };
    use jaymi_capabilities::Capability;
    use jaymi_core::IntentId;

    fn sample_plan() -> ExecutionPlan {
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Delete the build folder".into(),
            planner_intent: IntentId::ManagePath,
            capability: Capability::FileManagement,
            proposed_tools: vec!["manage_path".into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Delete build/".into(),
                tool_id: Some("manage_path".into()),
                resource: Some("/tmp/project/build".into()),
            }],
            estimated_risk: EstimatedRisk::High,
            affected_resources: vec!["/tmp/project/build".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "delete".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::Irreversible,
            expected_outputs: vec!["deleted path".into()],
            deletion_method: None,
            action_preview: None,
            lineage: Default::default(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        plan
    }

    #[test]
    fn from_plan_answers_four_questions_and_display_fields() {
        let plan = sample_plan();
        let card = ReviewCardModel::from_plan(&plan, Some("Permission requires approval"));
        assert!(card.state.is_pending());
        assert_eq!(card.opening, "I can do that.");
        assert!(card.plan_items.iter().any(|item| item.contains("Delete")));
        assert!(card.approval_notice.to_lowercase().contains("destructive"));
        assert!(card.modify_examples.iter().any(|ex| ex.contains("cache")));
        assert!(card.asking_to_do.contains("Delete the build folder"));
        assert!(card.what_will_change.contains("Delete"));
        assert!(card.why_proposed.contains("Nothing runs until you approve"));
        assert!(!card.after_approval.is_empty());
        assert_eq!(card.risk_level, EstimatedRisk::High);
        assert!(card.permissions.iter().any(|p| p.contains("delete")));
    }

    #[test]
    fn communicate_records_intent_without_mutating_plan() {
        let plan = sample_plan();
        let plan_id = plan.id().clone();
        let mut card = ReviewCardModel::from_plan(&plan, None);
        let intent = ReviewIntent::Approve {
            plan_id: plan_id.clone(),
        };
        assert_eq!(card.communicate(intent.clone()).as_ref(), Some(&intent));
        assert!(!card.state.is_pending());
        assert!(
            card.communicate(ReviewIntent::Cancel { plan_id }).is_none(),
            "second intent must be ignored"
        );
        assert_eq!(plan.status(), ExecutionStatus::AwaitingReview);
    }

    #[test]
    fn render_text_is_conversational() {
        let plan = sample_plan();
        let card = ReviewCardModel::from_plan(&plan, None);
        let text = card.render_text();
        assert!(text.starts_with("I can do that."));
        assert!(text.contains("Plan"));
        assert!(text.contains("Delete"));
        assert!(text.contains("destructive"));
        assert!(text.contains("You can:"));
        assert!(text.contains("Approve"));
        assert!(text.contains("Modify the plan"));
        assert!(text.contains("Keep today's files."));
        assert!(text.contains("Only delete the cache."));
        assert!(text.contains("Show me the files first."));
    }

    #[test]
    fn serialization_roundtrip() {
        let plan = sample_plan();
        let mut card = ReviewCardModel::from_plan(&plan, None);
        card.communicate(ReviewIntent::Modify {
            plan_id: plan.id().clone(),
            note: Some("Only delete the cache.".into()),
        });
        let json = serde_json::to_string(&card).unwrap();
        let restored: ReviewCardModel = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.plan_id, card.plan_id);
        assert_eq!(restored.opening, "I can do that.");
        assert!(!restored.modify_examples.is_empty());
        assert_eq!(restored.resolved_intent().map(ReviewIntent::as_str), Some("modify"));
    }
}
