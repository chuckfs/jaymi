//! Conversation-first experience state.
//!
//! Conversation is permanent. Workspaces are temporary expansions controlled
//! by capabilities. Closing a workspace never destroys the conversation, and
//! capability runtime state disappears with the workspace unless promoted.

use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_capabilities::{
    CapabilityState, CodingState, CreationState, ResearchState, WorkspaceExpansion, WorkspaceKind,
    WorkspacePanel,
};
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_memory::{ConversationMessage, MessageRole};
use jaymi_planner::{ExecutionSummary, PlannerResponse, ReviewCardModel, ReviewIntent};

/// Persistent conversation turn kept across workspace open/close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTurn {
    /// Speaker role.
    pub role: MessageRole,
    /// Message text.
    pub content: String,
    /// Unix seconds when the turn was created (for separators / timestamps).
    pub created_at: i64,
    /// Optional in-conversation Review Card (structured; not a modal).
    pub review: Option<ReviewCardModel>,
    /// Optional structured Execution Summary embedded in the conversation.
    pub execution_summary: Option<ExecutionSummary>,
}

impl ConversationTurn {
    /// Create a user turn stamped with the current time.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            created_at: unix_now(),
            review: None,
            execution_summary: None,
        }
    }

    /// Create an assistant turn stamped with the current time.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            created_at: unix_now(),
            review: None,
            execution_summary: None,
        }
    }

    /// Assistant turn that embeds a Review Card inside the conversation.
    pub fn assistant_with_review(content: impl Into<String>, review: ReviewCardModel) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            created_at: unix_now(),
            review: Some(review),
            execution_summary: None,
        }
    }

    /// Assistant turn that embeds an Execution Summary inside the conversation.
    pub fn assistant_with_summary(
        content: impl Into<String>,
        summary: ExecutionSummary,
    ) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            created_at: unix_now(),
            review: None,
            execution_summary: Some(summary),
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Session experience: one conversation plus an optional expanded workspace.
#[derive(Debug, Clone, Default)]
pub struct ExperienceSession {
    /// Turns that survive workspace transitions.
    conversation: Vec<ConversationTurn>,
    /// Active expanded workspace, when any (`None` ⇒ conversation-only).
    active_workspace: Option<WorkspaceExpansion>,
    /// Temporary capability state for the active workspace (cleared on close).
    capability_state: Option<CapabilityState>,
    /// Entries explicitly promoted out of capability state (survive close).
    promoted: Vec<String>,
    /// Stable conversation id when bound to Memory Engine transcripts.
    conversation_id: Option<String>,
    /// Most recent Review Card intent communicated by the user (not executed here).
    last_review_intent: Option<ReviewIntent>,
}

impl ExperienceSession {
    /// Create an empty conversation-first session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bound conversation id, when any.
    pub fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    /// Bind this session to a persisted conversation id.
    pub fn set_conversation_id(&mut self, conversation_id: Option<String>) {
        self.conversation_id = conversation_id;
    }

    /// Replace the in-memory transcript without touching the workspace.
    ///
    /// Used when switching between persisted conversations from the History rail.
    pub fn replace_transcript(
        &mut self,
        conversation_id: Option<String>,
        turns: Vec<ConversationTurn>,
    ) {
        self.conversation_id = conversation_id;
        self.conversation = turns;
    }

    /// Immutable conversation transcript.
    pub fn conversation(&self) -> &[ConversationTurn] {
        &self.conversation
    }

    /// Display title for the conversation surface.
    ///
    /// Prefers the first user message (truncated); falls back to "Conversation".
    pub fn conversation_title(&self) -> String {
        self.conversation
            .iter()
            .find(|turn| matches!(turn.role, MessageRole::User))
            .map(|turn| {
                let trimmed = turn.content.trim();
                let chars: Vec<char> = trimmed.chars().collect();
                if chars.len() <= 42 {
                    trimmed.to_string()
                } else {
                    format!("{}…", chars.into_iter().take(41).collect::<String>())
                }
            })
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "Conversation".to_string())
    }

    /// Append a conversation turn (never cleared by workspace close).
    pub fn append_turn(&mut self, turn: ConversationTurn) {
        self.conversation.push(turn);
    }

    /// Number of conversation turns.
    pub fn turn_count(&self) -> usize {
        self.conversation.len()
    }

    /// Active workspace expansion, when any.
    pub fn active_workspace(&self) -> Option<&WorkspaceExpansion> {
        self.active_workspace.as_ref()
    }

    /// Active workspace kind, when expanded.
    pub fn active_workspace_kind(&self) -> Option<WorkspaceKind> {
        self.active_workspace
            .as_ref()
            .map(|workspace| workspace.kind)
    }

    /// True when a non-conversation workspace is open.
    pub fn workspace_expanded(&self) -> bool {
        self.active_workspace
            .as_ref()
            .map(WorkspaceExpansion::expands)
            .unwrap_or(false)
    }

    /// Panels for the active workspace.
    pub fn active_panels(&self) -> Vec<WorkspacePanel> {
        self.active_workspace
            .as_ref()
            .map(|workspace| workspace.panels.clone())
            .unwrap_or_default()
    }

    /// Temporary capability state for the active workspace.
    pub fn capability_state(&self) -> Option<&CapabilityState> {
        self.capability_state.as_ref()
    }

    /// Mutable temporary capability state for the active workspace.
    pub fn capability_state_mut(&mut self) -> Option<&mut CapabilityState> {
        self.capability_state.as_mut()
    }

    /// Summaries promoted out of capability state (survive workspace close).
    pub fn promoted_entries(&self) -> &[String] {
        &self.promoted
    }

    /// Expand (or replace) the workspace from a capability request.
    ///
    /// Conversation turns are left untouched. Switching workspace kinds
    /// replaces capability state so workspaces stay isolated.
    pub fn expand_workspace(&mut self, expansion: WorkspaceExpansion) -> JaymiResult<()> {
        if !expansion.expands() {
            return Err(JaymiError::new(
                "conversation workspace does not expand the UI",
            ));
        }
        if expansion.expands_from != jaymi_capabilities::WorkspaceEdge::Right {
            return Err(JaymiError::new(
                "workspaces must expand from the right of the conversation",
            ));
        }

        let next_kind = expansion.kind;
        let switching = self
            .active_workspace
            .as_ref()
            .map(|current| current.kind != next_kind)
            .unwrap_or(true);

        self.active_workspace = Some(expansion);
        if switching || self.capability_state.is_none() {
            self.capability_state = CapabilityState::empty_for(next_kind);
        } else if let Some(state) = &self.capability_state {
            if state.workspace_kind() != next_kind {
                self.capability_state = CapabilityState::empty_for(next_kind);
            }
        }
        Ok(())
    }

    /// Close the expanded workspace without destroying the conversation.
    ///
    /// Capability runtime state is discarded unless previously promoted.
    pub fn close_workspace(&mut self) -> Option<WorkspaceExpansion> {
        self.capability_state = None;
        self.active_workspace.take()
    }

    /// Update coding workspace state (isolated to Coding).
    pub fn with_coding_state<R>(
        &mut self,
        update: impl FnOnce(&mut CodingState) -> R,
    ) -> JaymiResult<R> {
        let state = self.capability_state_mut().ok_or_else(|| {
            JaymiError::new("no active capability state — expand a coding workspace first")
        })?;
        let coding = state
            .coding_mut()
            .ok_or_else(|| JaymiError::new("active capability state is not a coding workspace"))?;
        Ok(update(coding))
    }

    /// Update creation workspace state (isolated to Creation).
    pub fn with_creation_state<R>(
        &mut self,
        update: impl FnOnce(&mut CreationState) -> R,
    ) -> JaymiResult<R> {
        let state = self.capability_state_mut().ok_or_else(|| {
            JaymiError::new("no active capability state — expand a creation workspace first")
        })?;
        let creation = state.creation_mut().ok_or_else(|| {
            JaymiError::new("active capability state is not a creation workspace")
        })?;
        Ok(update(creation))
    }

    /// Update research workspace state (isolated to Research).
    pub fn with_research_state<R>(
        &mut self,
        update: impl FnOnce(&mut ResearchState) -> R,
    ) -> JaymiResult<R> {
        let state = self.capability_state_mut().ok_or_else(|| {
            JaymiError::new("no active capability state — expand a research workspace first")
        })?;
        let research = state.research_mut().ok_or_else(|| {
            JaymiError::new("active capability state is not a research workspace")
        })?;
        Ok(update(research))
    }

    /// Promote one capability-state entry into durable session notes.
    ///
    /// The entry remains in temporary state until the workspace closes; the
    /// promoted summary survives close and can be stored elsewhere by callers.
    pub fn promote_capability_entry(&mut self, entry_id: &str) -> JaymiResult<String> {
        let summary = self
            .capability_state
            .as_ref()
            .and_then(|state| state.promote_summary(entry_id))
            .ok_or_else(|| {
                JaymiError::new(format!(
                    "capability state entry not found for promotion: {entry_id}"
                ))
            })?;
        self.promoted.push(summary.clone());
        self.append_turn(ConversationTurn::assistant(format!(
            "Promoted from workspace:\n{summary}"
        )));
        Ok(summary)
    }

    /// Most recent Review Card intent, when any (intent only — not executed).
    pub fn last_review_intent(&self) -> Option<&ReviewIntent> {
        self.last_review_intent.as_ref()
    }

    /// Record a Review Card intent against the matching pending card, when any.
    ///
    /// Updates the card and appends an acknowledgement when a conversation
    /// Review Card is pending. Coding / Git / Terminal gestures may have no
    /// card — they still record [`Self::last_review_intent`] so approval
    /// semantics stay unified. Does **not** execute the Execution Plan.
    pub fn record_review_intent(&mut self, intent: ReviewIntent) -> ReviewIntent {
        let plan_id = intent.plan_id().clone();
        if let Some(recorded) = self.conversation.iter_mut().rev().find_map(|turn| {
            turn.review
                .as_mut()
                .filter(|card| card.plan_id == plan_id && card.state.is_pending())
                .and_then(|card| card.communicate(intent.clone()))
        }) {
            self.last_review_intent = Some(recorded.clone());
            self.append_turn(ConversationTurn::user(recorded.acknowledgement()));
            recorded
        } else {
            self.last_review_intent = Some(intent.clone());
            intent
        }
    }

    /// Record a Review Card intent against the matching pending card in-conversation.
    ///
    /// Requires a pending card (conversation path). Prefer
    /// [`Self::record_review_intent`] when gestures may omit a card.
    /// Does **not** approve, cancel, or execute the underlying Execution Plan.
    pub fn communicate_review_intent(
        &mut self,
        intent: ReviewIntent,
    ) -> JaymiResult<ReviewIntent> {
        let plan_id = intent.plan_id().clone();
        let before = self.last_review_intent.clone();
        let recorded = self.record_review_intent(intent);
        let card_resolved = self.conversation.iter().rev().any(|turn| {
            turn.review.as_ref().is_some_and(|card| {
                card.plan_id == plan_id && !card.state.is_pending()
            })
        });
        if !card_resolved {
            self.last_review_intent = before;
            return Err(JaymiError::new(format!(
                "no pending review card for plan {}",
                plan_id.as_str()
            )));
        }
        Ok(recorded)
    }

    /// Apply a Planner response: append assistant content and honor workspace.
    ///
    /// Completed / failed / cancelled / partial Execution Summaries are folded
    /// into the conversation naturally (structured field + readable text).
    pub fn apply_planner_response(&mut self, response: &PlannerResponse) {
        if response.awaiting_review {
            if let Some(plan) = &response.execution_plan {
                let explanation = response
                    .permission_result
                    .as_ref()
                    .map(|result| result.explanation.as_str());
                let review = ReviewCardModel::from_plan(plan, explanation);
                // Always prefer the conversational Review Card body so the
                // assistant turn matches the card (not a diagnostic pause line).
                let content = review.render_text();
                self.append_turn(ConversationTurn::assistant_with_review(content, review));
            } else if !response.content.trim().is_empty() {
                self.append_turn(ConversationTurn::assistant(response.content.clone()));
            }
        } else if let Some(summary) = response
            .execution_summary
            .as_ref()
            .filter(|summary| summary.should_surface_in_conversation())
        {
            let rendered = summary.render_conversation();
            let content = if response.content.trim().is_empty() {
                rendered
            } else if response.content.contains("Execution summary") {
                response.content.clone()
            } else {
                format!("{}\n\n{rendered}", response.content.trim_end())
            };
            self.append_turn(ConversationTurn::assistant_with_summary(
                content,
                summary.clone(),
            ));
        } else if !response.content.trim().is_empty() {
            self.append_turn(ConversationTurn::assistant(response.content.clone()));
        }
        if let Some(workspace) = &response.workspace {
            if workspace.expands() {
                let _ = self.expand_workspace(workspace.clone());
            }
        }
    }

    /// Record a user message in the durable conversation.
    pub fn record_user_message(&mut self, content: impl Into<String>) {
        self.append_turn(ConversationTurn::user(content));
    }

    /// Seed turns from a loaded conversation transcript.
    pub fn load_messages(&mut self, messages: &[ConversationMessage]) {
        self.conversation.clear();
        for message in messages {
            self.conversation.push(ConversationTurn {
                role: message.role,
                content: message.content.clone(),
                created_at: message.created_at,
                review: None,
                execution_summary: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::Capability;
    use jaymi_core::IntentId;
    use jaymi_planner::{
        EstimatedReversibility, EstimatedRisk, ExecutionPlan, ExecutionPlanParams, ExecutionStep,
        ExecutionStatus, PlanPermissionRequirement, ReviewRequirement,
    };

    fn awaiting_plan() -> ExecutionPlan {
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Delete scratch".into(),
            planner_intent: IntentId::ManagePath,
            capability: Capability::FileManagement,
            proposed_tools: vec!["manage_path".into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Delete path".into(),
                tool_id: Some("manage_path".into()),
                resource: Some("/tmp/scratch".into()),
            }],
            estimated_risk: EstimatedRisk::High,
            affected_resources: vec!["/tmp/scratch".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "delete".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::Irreversible,
            expected_outputs: vec!["managed path".into()],
        deletion_method: None,
        action_preview: None,
        lineage: Default::default(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        plan
    }

    #[test]
    fn awaiting_review_response_embeds_review_card_in_conversation() {
        let plan = awaiting_plan();
        let plan_id = plan.id().clone();
        let status = plan.status();
        let mut session = ExperienceSession::new();
        session.apply_planner_response(&PlannerResponse {
            content: "diagnostic pause line should be ignored".into(),
            awaiting_review: true,
            execution_plan: Some(plan),
            ..PlannerResponse::default()
        });
        assert_eq!(session.turn_count(), 1);
        let turn = &session.conversation()[0];
        let review = turn.review.as_ref().expect("review card");
        assert_eq!(review.plan_id, plan_id);
        assert!(review.state.is_pending());
        assert!(review.asking_to_do.contains("Delete scratch"));
        assert!(turn.content.starts_with("I can do that."));
        assert!(turn.content.contains("Plan"));
        assert!(turn.content.contains("You can:"));
        assert!(turn.content.contains("Modify the plan"));
        assert_eq!(status, ExecutionStatus::AwaitingReview);
    }

    #[test]
    fn completed_summary_appears_in_conversation() {
        use jaymi_planner::{ExecutionStatus, ExecutionSummary};
        let mut session = ExperienceSession::new();
        let summary = ExecutionSummary {
            plan_id: jaymi_planner::ExecutionPlanId::from_existing("plan-test"),
            status: ExecutionStatus::Completed,
            goal: "List /tmp".into(),
            actions_performed: vec!["Listed 2 entries".into()],
            resources_changed: vec!["/tmp".into()],
            files_edited: Vec::new(),
            duration_ms: 12,
            warnings: Vec::new(),
            errors: Vec::new(),
            error: None,
            next_suggested_actions: vec!["Open a listed file".into()],
            tools_executed: vec!["search_files".into()],
            outputs: vec!["directory listing".into()],
            partial: false,
            files_moved_to_trash: Vec::new(),
            files_permanently_deleted: Vec::new(),
            recovery_available: None,
            deletion_method: None,
        };
        session.apply_planner_response(&PlannerResponse {
            content: "Listed 2 entries.".into(),
            execution_summary: Some(summary.clone()),
            ..PlannerResponse::default()
        });
        assert_eq!(session.turn_count(), 1);
        let turn = &session.conversation()[0];
        assert!(turn.content.contains("Execution summary"));
        assert!(turn.content.contains("Goal: List /tmp"));
        assert_eq!(
            turn.execution_summary.as_ref().map(|s| s.status),
            Some(ExecutionStatus::Completed)
        );
    }

    #[test]
    fn review_intent_is_recorded_without_executing() {
        let plan = awaiting_plan();
        let plan_id = plan.id().clone();
        let mut session = ExperienceSession::new();
        session.apply_planner_response(&PlannerResponse {
            content: "Review needed.".into(),
            awaiting_review: true,
            execution_plan: Some(plan),
            ..PlannerResponse::default()
        });
        let recorded = session
            .communicate_review_intent(ReviewIntent::Cancel {
                plan_id: plan_id.clone(),
            })
            .expect("cancel intent");
        assert_eq!(recorded.as_str(), "cancel");
        assert_eq!(
            session.last_review_intent().map(ReviewIntent::as_str),
            Some("cancel")
        );
        assert!(!session.conversation()[0]
            .review
            .as_ref()
            .unwrap()
            .state
            .is_pending());
        assert_eq!(session.turn_count(), 2);
        assert!(session.conversation()[1].content.contains("Cancelled"));
    }

    #[test]
    fn gesture_approve_records_intent_without_review_card() {
        let mut session = ExperienceSession::new();
        let plan_id = jaymi_planner::ExecutionPlanId::from_existing("plan-gesture");
        let recorded = session.record_review_intent(ReviewIntent::Approve {
            plan_id: plan_id.clone(),
        });
        assert_eq!(recorded.as_str(), "approve");
        assert_eq!(
            session.last_review_intent().map(ReviewIntent::as_str),
            Some("approve")
        );
        assert_eq!(
            session.turn_count(),
            0,
            "gesture approval must not invent conversation turns"
        );
        assert!(session
            .communicate_review_intent(ReviewIntent::Approve { plan_id })
            .is_err());
    }
}
