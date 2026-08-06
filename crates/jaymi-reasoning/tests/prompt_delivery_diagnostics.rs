//! Sprint B1.13.5 — prompt diagnostics describe the prompt actually delivered.

use jaymi_context::{
    ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
    UserRequestMetadataSection,
};
use jaymi_reasoning::{
    ConversationTurn, PromptBuilder, PromptBudget, PromptChatRole, PromptSectionDisposition,
    PromptSectionId, ReasoningRequest,
};

fn sample_context(preview: &str) -> LlmContext {
    let bundle = ContextBundleBuilder::new()
        .user_request(UserRequestMetadataSection {
            content_preview: preview.into(),
            ..UserRequestMetadataSection::default()
        })
        .planner_metadata(PlannerMetadataSection {
            assemble_generation: 1,
            sources: vec![ContextSource::UserRequest],
            notes: vec![],
            budget: None,
            policy: None,
        })
        .build();
    LlmContext::from_bundle(&bundle)
}

#[test]
fn prompt_inspection_matches_delivered_messages() {
    let history = vec![
        ConversationTurn::user("prior"),
        ConversationTurn::assistant("answer"),
    ];
    let request =
        ReasoningRequest::new("inspect me", sample_context("inspect me")).with_history(history);
    let prompt = PromptBuilder::new()
        .with_system_instructions("Be helpful.")
        .build_from_request(&request);

    let delivered_chars = prompt.delivered_character_count();
    let d = &prompt.diagnostics;
    assert_eq!(d.prompt_size_characters, delivered_chars);
    assert_eq!(d.budget.used_characters, delivered_chars);
    assert_eq!(d.final_token_estimate, d.prompt_size_tokens);
    assert_eq!(d.conversation_turns, 2);
    assert!(d.included_section_count() > 0);
    // Excluded / budgeted / filtered still listed — but never counted in size.
    for section in &d.sections {
        if !section.included {
            assert_eq!(section.characters, 0);
        }
    }

    // Flat Prompt::text framing is not what providers send.
    assert_ne!(d.prompt_size_characters, prompt.text.chars().count());
}

#[test]
fn unused_sections_never_count_toward_prompt_size() {
    let prompt = PromptBuilder::new()
        .with_system_instructions("sys")
        .with_budget(PromptBudget::characters(80))
        .build_from_request(&ReasoningRequest::new(
            "tiny budget forces omissions",
            sample_context("tiny budget forces omissions"),
        ));

    let unused: Vec<_> = prompt
        .diagnostics
        .sections
        .iter()
        .filter(|section| !section.included)
        .collect();
    assert!(
        !unused.is_empty(),
        "expected some sections excluded/budgeted under tight budget"
    );
    for section in &unused {
        assert_eq!(section.characters, 0);
        assert_eq!(section.estimated_tokens, 0);
        assert!(matches!(
            section.disposition,
            PromptSectionDisposition::Excluded
                | PromptSectionDisposition::Budgeted
                | PromptSectionDisposition::Filtered
        ));
    }
    assert_eq!(
        prompt.diagnostics.prompt_size_characters,
        prompt.delivered_character_count()
    );
}

#[test]
fn prompt_equality_is_stable_for_identical_inputs() {
    let request = ReasoningRequest::new("same", sample_context("same")).with_history(vec![
        ConversationTurn::user("a"),
        ConversationTurn::assistant("b"),
    ]);
    let left = PromptBuilder::new()
        .with_system_instructions("sys")
        .build_from_request(&request);
    let right = PromptBuilder::new()
        .with_system_instructions("sys")
        .build_from_request(&request);
    assert_eq!(left, right);
    assert_eq!(left.diagnostics, right.diagnostics);
    assert_eq!(
        left.diagnostics.prompt_size_characters,
        left.delivered_character_count()
    );
}

#[test]
fn prompt_integrity_sections_match_delivery_framing() {
    let prompt = PromptBuilder::new()
        .with_system_instructions("integrity")
        .build_from_request(&ReasoningRequest::new(
            "check integrity",
            sample_context("check integrity"),
        ));
    let messages = prompt.to_chat_messages();
    let user = messages
        .iter()
        .find(|message| message.role == PromptChatRole::User)
        .expect("user");
    let user_section = prompt
        .diagnostics
        .sections
        .iter()
        .find(|section| section.id == PromptSectionId::UserRequest)
        .expect("user section");
    assert!(user_section.included);
    assert_eq!(user_section.characters, user.content.chars().count());
    // User delivery omits the "## User Request" heading present in prompt.text.
    assert!(!user.content.contains("## User Request"));
    assert!(prompt.text.contains("## User Request"));
}
