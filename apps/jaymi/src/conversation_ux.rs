//! Conversation UX helpers — polish without parallel state machines.
//!
//! All indicators and actions derive from Planner [`ConversationState`] and
//! turn [`StreamingLifecycle`]. Experience / UI never invent a third lifecycle.
//!
//! **Sprint B1.11**

use jaymi_memory::MessageRole;
use jaymi_planner::ConversationState;
use jaymi_reasoning::StreamingLifecycle;

use crate::experience::ConversationTurn;

/// Actions available on an assistant turn (Conversation First chrome).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConversationTurnActions {
    /// Copy response text to the clipboard.
    pub copy: bool,
    /// Retry after cancel / failure (same stream request).
    pub retry: bool,
    /// Regenerate a completed (or terminal) assistant reply from the last user turn.
    pub regenerate: bool,
}

impl ConversationTurnActions {
    /// True when any action should be shown.
    pub fn any(self) -> bool {
        self.copy || self.retry || self.regenerate
    }
}

/// Derive turn actions from role + lifecycle (no hidden rules).
pub fn turn_actions(turn: &ConversationTurn) -> ConversationTurnActions {
    if !matches!(turn.role, MessageRole::Assistant) {
        return ConversationTurnActions::default();
    }
    if turn.is_streaming() {
        return ConversationTurnActions::default();
    }
    let lifecycle = turn
        .stream_lifecycle
        .unwrap_or(StreamingLifecycle::Completed);
    match lifecycle {
        StreamingLifecycle::Cancelled | StreamingLifecycle::Failed => ConversationTurnActions {
            copy: !turn.content.trim().is_empty(),
            retry: true,
            regenerate: true,
        },
        StreamingLifecycle::Completed | StreamingLifecycle::Idle => ConversationTurnActions {
            copy: !turn.content.trim().is_empty(),
            retry: false,
            regenerate: true,
        },
        StreamingLifecycle::Thinking | StreamingLifecycle::Streaming => {
            ConversationTurnActions::default()
        }
    }
}

/// Streaming caret glyph (left half block — readable in proportional UI fonts).
pub fn caret_glyph() -> &'static str {
    "\u{258C}"
}

/// Content plus caret when streaming and the blink phase is on.
pub fn smooth_streaming_text(content: &str, streaming: bool, caret_on: bool) -> String {
    if streaming && caret_on {
        format!("{content}{}", caret_glyph())
    } else {
        content.to_string()
    }
}

/// Body text for display, with an optional streaming caret.
pub fn display_content(turn: &ConversationTurn, caret_on: bool) -> String {
    smooth_streaming_text(&turn.content, turn.is_streaming(), caret_on)
}

/// Whether the typing / loading indicator row should show (not the in-bubble caret).
pub fn show_typing_indicator(state: ConversationState, has_streaming_turn: bool) -> bool {
    if has_streaming_turn {
        // Tokens are visible on the assistant bubble — keep the row for Thinking /
        // Preparing only so the conversation stays primary and uncluttered.
        return matches!(
            state,
            ConversationState::PreparingContext | ConversationState::Reasoning
        );
    }
    state.shows_progress_indicator()
}

/// Accessibility label for the conversation progress region.
pub fn progress_accessibility_label(state: ConversationState) -> String {
    let label = state.status_label();
    if label.is_empty() {
        match state {
            ConversationState::Idle => "Conversation idle".into(),
            ConversationState::Completed => "Response completed".into(),
            ConversationState::Cancelled => "Generation cancelled".into(),
            ConversationState::Failed => "Generation failed".into(),
            _ => "Jaymi is working".into(),
        }
    } else {
        format!("Jaymi status: {label}")
    }
}

/// Accessibility label for an assistant turn action button.
pub fn action_accessibility_label(action: &str, preview: &str) -> String {
    let trimmed = preview.trim();
    if trimmed.is_empty() {
        action.to_string()
    } else {
        let snippet: String = trimmed.chars().take(48).collect();
        format!("{action}: {snippet}")
    }
}

/// Loading transition opacity (0..1) from a 0..1 eased progress.
pub fn loading_opacity(progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    // Ease-out cubic — matches workspace expand motion language.
    let inv = 1.0 - t;
    0.35 + 0.65 * (1.0 - inv * inv * inv)
}

/// Blink phase for the streaming caret (period ≈ 1s).
pub fn caret_blink_on(secs: f64) -> bool {
    (secs.fract() * 2.0) < 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_for_completed_assistant() {
        let mut turn = ConversationTurn::assistant("Hello world");
        turn.stream_lifecycle = Some(StreamingLifecycle::Completed);
        let actions = turn_actions(&turn);
        assert!(actions.copy);
        assert!(!actions.retry);
        assert!(actions.regenerate);
    }

    #[test]
    fn actions_for_cancelled_offer_retry() {
        let mut turn = ConversationTurn::assistant("partial");
        turn.stream_lifecycle = Some(StreamingLifecycle::Cancelled);
        let actions = turn_actions(&turn);
        assert!(actions.copy);
        assert!(actions.retry);
        assert!(actions.regenerate);
    }

    #[test]
    fn no_actions_while_streaming() {
        let mut turn = ConversationTurn::assistant("Hi");
        turn.stream_lifecycle = Some(StreamingLifecycle::Streaming);
        assert!(!turn_actions(&turn).any());
    }

    #[test]
    fn smooth_streaming_appends_caret() {
        let text = smooth_streaming_text("Hello", true, true);
        assert!(text.ends_with(caret_glyph()));
        assert_eq!(smooth_streaming_text("Hello", true, false), "Hello");
        assert_eq!(smooth_streaming_text("Hello", false, true), "Hello");
    }

    #[test]
    fn typing_indicator_hidden_when_tokens_visible() {
        assert!(!show_typing_indicator(ConversationState::Streaming, true));
        assert!(show_typing_indicator(
            ConversationState::PreparingContext,
            false
        ));
        assert!(show_typing_indicator(ConversationState::Reasoning, true));
    }

    #[test]
    fn accessibility_labels_are_descriptive() {
        let label = progress_accessibility_label(ConversationState::Reasoning);
        assert!(label.contains("Thinking") || label.contains("Jaymi"));
        let action = action_accessibility_label("Copy response", "The quick brown fox");
        assert!(action.starts_with("Copy response:"));
    }

    #[test]
    fn loading_opacity_eases_in() {
        assert!((loading_opacity(0.0) - 0.35).abs() < f32::EPSILON);
        assert!((loading_opacity(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(loading_opacity(0.5) > loading_opacity(0.25));
    }

    #[test]
    fn caret_blink_toggles() {
        assert!(caret_blink_on(0.1));
        assert!(!caret_blink_on(0.6));
    }
}
