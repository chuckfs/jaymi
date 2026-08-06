//! Request-scoped conversation history for Reasoning.
//!
//! Experience owns the durable transcript. The Planner does **not** store a
//! parallel history — it accepts prior turns per request, normalizes them, and
//! places them on [`jaymi_reasoning::ReasoningRequest::history`] for
//! PromptBuilder.

use jaymi_reasoning::{ConversationRole, ConversationTurn};

/// Prepare prior turns for [`ReasoningRequest::history`].
///
/// * Drops empty content
/// * Drops a trailing user turn that duplicates `goal` (goal is the latest utterance)
/// * Preserves user / assistant / system / tool roles in order
///
/// PromptBuilder owns formatting and budget truncation of the Conversation section.
pub fn prepare_reasoning_history(
    history: Vec<ConversationTurn>,
    goal: &str,
) -> Vec<ConversationTurn> {
    let goal = goal.trim();
    let mut turns: Vec<ConversationTurn> = history
        .into_iter()
        .filter(|turn| !turn.content.trim().is_empty())
        .collect();
    if !goal.is_empty() {
        if let Some(last) = turns.last() {
            if matches!(last.role, ConversationRole::User) && last.content.trim() == goal {
                turns.pop();
            }
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_duplicate_trailing_goal() {
        let history = vec![
            ConversationTurn::user("earlier"),
            ConversationTurn::assistant("ok"),
            ConversationTurn::user("latest"),
        ];
        let prepared = prepare_reasoning_history(history, "latest");
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].content, "earlier");
        assert_eq!(prepared[1].content, "ok");
    }

    #[test]
    fn keeps_distinct_prior_user_turns() {
        let history = vec![
            ConversationTurn::user("one"),
            ConversationTurn::assistant("a"),
            ConversationTurn::user("two"),
        ];
        let prepared = prepare_reasoning_history(history, "three");
        assert_eq!(prepared.len(), 3);
    }

    #[test]
    fn drops_empty_turns() {
        let history = vec![
            ConversationTurn::user("hi"),
            ConversationTurn::assistant(""),
            ConversationTurn::system("be brief"),
        ];
        let prepared = prepare_reasoning_history(history, "next");
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[1].role, ConversationRole::System);
    }
}
