//! Sprint B1.13.3 — multi-turn conversation history continuity.

use jaymi::{ConversationTurn, ExperienceSession};
use jaymi_reasoning::{
    ConversationRole, PromptBuilder, PromptBudget, PromptSectionDisposition, PromptSectionId,
    ReasoningRequest,
};

fn empty_llm_context() -> jaymi_context::LlmContext {
    jaymi_context::LlmContext::from_bundle(&jaymi_context::ContextBundleBuilder::new().build())
}

#[test]
fn single_turn_has_no_prior_history() {
    let session = ExperienceSession::new();
    let history = session.to_reasoning_history(None);
    assert!(history.is_empty());
}

#[test]
fn experience_history_reaches_prompt_builder() {
    let mut session = ExperienceSession::new();
    session.record_user_message("earlier question");
    session.append_turn(ConversationTurn::assistant("earlier answer"));
    let history = session.to_reasoning_history(None);
    assert_eq!(history.len(), 2);

    let request = ReasoningRequest::new("follow up", empty_llm_context()).with_history(history);
    let prompt = PromptBuilder::new().build_from_request(&request);
    assert!(prompt.text.contains("earlier question"));
    assert!(prompt.text.contains("earlier answer"));
    assert!(prompt.text.contains("follow up"));
    let conversation = prompt
        .diagnostics
        .sections
        .iter()
        .find(|section| section.id == PromptSectionId::Conversation)
        .expect("conversation section");
    assert_eq!(conversation.disposition, PromptSectionDisposition::Included);
}

#[test]
fn multi_turn_continuity_preserves_order_and_roles() {
    let mut session = ExperienceSession::new();
    session.record_user_message("first");
    session.append_turn(ConversationTurn::assistant("reply one"));
    session.record_user_message("second");
    session.append_turn(ConversationTurn::assistant("reply two"));
    let history = session.to_reasoning_history(None);
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].role, ConversationRole::User);
    assert_eq!(history[0].content, "first");
    assert_eq!(history[1].role, ConversationRole::Assistant);
    assert_eq!(history[2].content, "second");
    assert_eq!(history[3].content, "reply two");
}

#[test]
fn long_history_is_truncated_under_budget() {
    let mut history = Vec::new();
    for index in 0..50 {
        history.push(jaymi_reasoning::ConversationTurn::user(format!(
            "user message {index} with padding text for budget pressure on conversation history"
        )));
        history.push(jaymi_reasoning::ConversationTurn::assistant(format!(
            "assistant message {index} with padding text for budget pressure on conversation history"
        )));
    }
    let request = ReasoningRequest::new("summarize", empty_llm_context()).with_history(history);
    let prompt = PromptBuilder::new()
        .with_budget(PromptBudget::characters(600))
        .build_from_request(&request);
    let conversation = prompt
        .diagnostics
        .sections
        .iter()
        .find(|section| section.id == PromptSectionId::Conversation)
        .expect("conversation");
    assert!(matches!(
        conversation.disposition,
        PromptSectionDisposition::Truncated | PromptSectionDisposition::Budgeted
    ));
    assert!(prompt.section_ids().contains(&PromptSectionId::UserRequest));
}
