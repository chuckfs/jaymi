//! Conversation-first experience state.
//!
//! Conversation is permanent. Workspaces are temporary expansions controlled
//! by capabilities. Closing a workspace never destroys the conversation, and
//! capability runtime state disappears with the workspace unless promoted.

use jaymi_capabilities::{
    CapabilityState, CodingState, CreationState, ResearchState, WorkspaceExpansion, WorkspaceKind,
    WorkspacePanel,
};
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_memory::{ConversationMessage, MessageRole};
use jaymi_planner::PlannerResponse;

/// Persistent conversation turn kept across workspace open/close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTurn {
    /// Speaker role.
    pub role: MessageRole,
    /// Message text.
    pub content: String,
}

impl ConversationTurn {
    /// Create a user turn.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    /// Create an assistant turn.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
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

    /// Immutable conversation transcript.
    pub fn conversation(&self) -> &[ConversationTurn] {
        &self.conversation
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

    /// Apply a Planner response: append assistant content and honor workspace.
    pub fn apply_planner_response(&mut self, response: &PlannerResponse) {
        if !response.content.trim().is_empty() {
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
            let role = message.role;
            let content = message.content.clone();
            self.conversation.push(ConversationTurn { role, content });
        }
    }
}
