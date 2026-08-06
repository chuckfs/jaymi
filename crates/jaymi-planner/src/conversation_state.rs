//! Conversation runtime state machine — owned by the Planner.
//!
//! The conversation is the primary interface. This machine describes what the
//! conversation is doing **now**, independent of workspace expansion.
//!
//! ```text
//! Idle
//!   → PreparingContext
//!   → Reasoning | Streaming | WaitingForReview | Executing
//!   → Completed | Cancelled | Failed
//!   → Idle (next request)
//! ```
//!
//! [`crate::reasoning::StreamingLifecycle`] remains the generation sub-machine
//! nested under Reasoning / Streaming. [`crate::ExecutionStatus`] remains
//! plan-scoped. This enum is the user-visible conversation phase.

use serde::{Deserialize, Serialize};

/// User-visible conversation runtime phase.
///
/// The Planner owns all transitions. Experience / UI mirror this state only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConversationState {
    /// No active request.
    #[default]
    Idle,
    /// Intent resolved; Context Policy / providers / assemble in progress.
    PreparingContext,
    /// Reasoning Engine started; waiting for first visible token (may include thoughts).
    Reasoning,
    /// Visible tokens are streaming into the conversation.
    Streaming,
    /// Execution Plan paused for conversational review.
    WaitingForReview,
    /// Approved plan is executing tools.
    Executing,
    /// Request finished successfully.
    Completed,
    /// Request or generation was cancelled.
    Cancelled,
    /// Request or generation failed.
    Failed,
}

impl ConversationState {
    /// Stable label for diagnostics / logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::PreparingContext => "preparing_context",
            Self::Reasoning => "reasoning",
            Self::Streaming => "streaming",
            Self::WaitingForReview => "waiting_for_review",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    /// Short UI status line.
    pub fn status_label(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::PreparingContext => "Preparing context…",
            Self::Reasoning => "Thinking…",
            Self::Streaming => "Jaymi is typing…",
            Self::WaitingForReview => "Waiting for review…",
            Self::Executing => "Executing…",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Failed",
        }
    }

    /// True when no further work is expected for this turn.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }

    /// True when the conversation is actively working (not idle / terminal).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::PreparingContext
                | Self::Reasoning
                | Self::Streaming
                | Self::WaitingForReview
                | Self::Executing
        )
    }

    /// True when the UI should show an in-progress indicator.
    pub fn shows_progress_indicator(self) -> bool {
        matches!(
            self,
            Self::PreparingContext | Self::Reasoning | Self::Streaming | Self::Executing
        )
    }

    /// Whether `from → to` is a legal Planner transition.
    pub fn can_transition(from: Self, to: Self) -> bool {
        use ConversationState::*;
        if from == to {
            // Idempotent (e.g. Modify keeps WaitingForReview).
            return matches!(
                from,
                PreparingContext | Reasoning | Streaming | WaitingForReview | Executing
            );
        }
        match (from, to) {
            // New request.
            (Idle, PreparingContext) => true,
            (Completed | Cancelled | Failed, PreparingContext) => true,
            // After context assemble.
            (PreparingContext, Reasoning) => true,
            (PreparingContext, Streaming) => true,
            (PreparingContext, WaitingForReview) => true,
            (PreparingContext, Executing) => true,
            (PreparingContext, Completed) => true,
            (PreparingContext, Cancelled) => true,
            (PreparingContext, Failed) => true,
            // Reasoning / streaming.
            (Reasoning, Streaming) => true,
            (Reasoning, Completed) => true,
            (Reasoning, Cancelled) => true,
            (Reasoning, Failed) => true,
            (Streaming, Completed) => true,
            (Streaming, Cancelled) => true,
            (Streaming, Failed) => true,
            // Retry / reconnect after cancel, failure, or mid-stream reset (B1.13.7).
            (Streaming | Cancelled | Failed, Reasoning) => true,
            // Review / execute.
            (WaitingForReview, Executing) => true,
            (WaitingForReview, Cancelled) => true,
            (WaitingForReview, Failed) => true,
            (WaitingForReview, Completed) => true,
            (Executing, Completed) => true,
            (Executing, Cancelled) => true,
            (Executing, Failed) => true,
            // Return to idle after a terminal turn (optional explicit reset).
            (Completed | Cancelled | Failed, Idle) => true,
            _ => false,
        }
    }

    /// Map generation sub-lifecycle into conversation phases.
    pub fn from_streaming_lifecycle(
        lifecycle: jaymi_reasoning::StreamingLifecycle,
    ) -> Option<Self> {
        use jaymi_reasoning::StreamingLifecycle;
        match lifecycle {
            StreamingLifecycle::Idle => Some(Self::Idle),
            StreamingLifecycle::Thinking => Some(Self::Reasoning),
            StreamingLifecycle::Streaming => Some(Self::Streaming),
            StreamingLifecycle::Completed => Some(Self::Completed),
            StreamingLifecycle::Cancelled => Some(Self::Cancelled),
            StreamingLifecycle::Failed => Some(Self::Failed),
        }
    }
}

/// Error when a conversation state transition is illegal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTransitionError {
    /// State before the attempted transition.
    pub from: ConversationState,
    /// Requested next state.
    pub to: ConversationState,
}

impl std::fmt::Display for ConversationTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "illegal conversation transition {} → {}",
            self.from.as_str(),
            self.to.as_str()
        )
    }
}

impl std::error::Error for ConversationTransitionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_reasoning::StreamingLifecycle;

    #[test]
    fn happy_path_conversational_transitions() {
        assert!(ConversationState::can_transition(
            ConversationState::Idle,
            ConversationState::PreparingContext
        ));
        assert!(ConversationState::can_transition(
            ConversationState::PreparingContext,
            ConversationState::Reasoning
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Reasoning,
            ConversationState::Streaming
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Streaming,
            ConversationState::Completed
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Completed,
            ConversationState::PreparingContext
        ));
    }

    #[test]
    fn review_and_execute_transitions() {
        assert!(ConversationState::can_transition(
            ConversationState::PreparingContext,
            ConversationState::WaitingForReview
        ));
        assert!(ConversationState::can_transition(
            ConversationState::WaitingForReview,
            ConversationState::Executing
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Executing,
            ConversationState::Completed
        ));
        assert!(ConversationState::can_transition(
            ConversationState::WaitingForReview,
            ConversationState::Cancelled
        ));
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        assert!(!ConversationState::can_transition(
            ConversationState::Idle,
            ConversationState::Streaming
        ));
        assert!(!ConversationState::can_transition(
            ConversationState::Completed,
            ConversationState::Executing
        ));
        assert!(!ConversationState::can_transition(
            ConversationState::Streaming,
            ConversationState::WaitingForReview
        ));
    }

    #[test]
    fn retry_recovery_transitions() {
        assert!(ConversationState::can_transition(
            ConversationState::Streaming,
            ConversationState::Reasoning
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Cancelled,
            ConversationState::Reasoning
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Failed,
            ConversationState::Reasoning
        ));
        assert!(ConversationState::can_transition(
            ConversationState::Cancelled,
            ConversationState::PreparingContext
        ));
    }

    #[test]
    fn streaming_lifecycle_maps() {
        assert_eq!(
            ConversationState::from_streaming_lifecycle(StreamingLifecycle::Thinking),
            Some(ConversationState::Reasoning)
        );
        assert_eq!(
            ConversationState::from_streaming_lifecycle(StreamingLifecycle::Streaming),
            Some(ConversationState::Streaming)
        );
    }

    #[test]
    fn status_labels_are_user_facing() {
        assert!(ConversationState::PreparingContext
            .status_label()
            .contains("Preparing"));
        assert!(ConversationState::WaitingForReview
            .status_label()
            .contains("review"));
    }
}
