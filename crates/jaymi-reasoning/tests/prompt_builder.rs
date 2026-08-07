//! Sprint B1.2 — Prompt Builder contract tests.

use jaymi_context::{
    LlmActiveCapabilities, LlmActiveProject, LlmActiveWorkspace, LlmContext, LlmContextSection,
    LlmConversation, LlmCurrentFile, LlmCurrentSelection, LlmDiagnostic, LlmDiagnostics,
    LlmMemoryItem, LlmMemoryResults, LlmOpenFileEntry, LlmOpenFiles, LlmProviderMetadata,
    LlmSectionContent, LlmSectionId, LlmUserRequest, LLM_CONTEXT_SCHEMA_VERSION,
};
use jaymi_reasoning::{
    ConversationTurn, DefaultPromptTemplate, Prompt, PromptBudget, PromptBuilder, PromptSectionId,
    PromptTemplate, ReasoningRequest,
};

fn rich_context() -> LlmContext {
    let section = |id: LlmSectionId, content: LlmSectionContent| LlmContextSection {
        id,
        present: !matches!(content, LlmSectionContent::Empty),
        sources: vec![],
        content,
    };

    LlmContext {
        schema_version: LLM_CONTEXT_SCHEMA_VERSION,
        assemble_generation: 3,
        providers: LlmProviderMetadata {
            sources: vec![
                "user_request".into(),
                "editor_state".into(),
                "retrieved_memories".into(),
            ],
            notes: vec!["assembled".into()],
            budget: None,
            environmental: None,
        },
        sections: vec![
            section(
                LlmSectionId::UserRequest,
                LlmSectionContent::UserRequest(LlmUserRequest {
                    content_preview: "fix the bug".into(),
                    has_directory: false,
                    has_file: true,
                    has_write_file: false,
                    has_search: false,
                    has_project_knowledge: false,
                    has_terminal: false,
                    has_git: false,
                    has_lsp: false,
                    has_discover_or_index: false,
                    has_project_session: false,
                }),
            ),
            section(
                LlmSectionId::Conversation,
                LlmSectionContent::Conversation(LlmConversation {
                    id: Some("c1".into()),
                    title: Some("Bug".into()),
                    status: Some("active".into()),
                    project_id: Some("proj".into()),
                    message_count: Some(2),
                }),
            ),
            section(
                LlmSectionId::ActiveProject,
                LlmSectionContent::ActiveProject(LlmActiveProject {
                    project_id: Some("proj".into()),
                    name: Some("Demo".into()),
                    root_directory: Some("/tmp/demo".into()),
                    detail: None,
                }),
            ),
            section(
                LlmSectionId::ActiveWorkspace,
                LlmSectionContent::ActiveWorkspace(LlmActiveWorkspace {
                    kind_id: Some("coding".into()),
                }),
            ),
            section(
                LlmSectionId::CurrentFile,
                LlmSectionContent::CurrentFile(LlmCurrentFile {
                    path: Some("/tmp/main.rs".into()),
                    dirty: true,
                    language: Some("rust".into()),
                }),
            ),
            section(
                LlmSectionId::CurrentSelection,
                LlmSectionContent::CurrentSelection(LlmCurrentSelection {
                    path: Some("/tmp/main.rs".into()),
                    start_line: 1,
                    start_column: 1,
                    end_line: 2,
                    end_column: 4,
                    text: Some("fn main() {}".into()),
                }),
            ),
            section(
                LlmSectionId::OpenFiles,
                LlmSectionContent::OpenFiles(LlmOpenFiles {
                    files: vec![LlmOpenFileEntry {
                        path: "/tmp/main.rs".into(),
                        dirty: true,
                        active: true,
                    }],
                }),
            ),
            section(
                LlmSectionId::SearchResults,
                LlmSectionContent::SearchResults(jaymi_context::LlmSearchResults {
                    hint: Some(jaymi_context::LlmSearchHint {
                        structured_query_pending: false,
                        query_preview: Some("bug".into()),
                        project_indexed_documents: Some(12),
                    }),
                    hits: vec![jaymi_context::LlmSearchHit {
                        item_id: "hit1".into(),
                        title: "main.rs".into(),
                        path: Some("/tmp/main.rs".into()),
                        score: Some(90),
                        match_reason: Some("content".into()),
                        preview: Some("fn main".into()),
                        line: Some(1),
                        column: Some(1),
                    }],
                }),
            ),
            section(
                LlmSectionId::MemoryResults,
                LlmSectionContent::MemoryResults(LlmMemoryResults {
                    project_id: Some("proj".into()),
                    conversation_id: None,
                    candidate_count: 1,
                    truncated: false,
                    memories: vec![LlmMemoryItem {
                        id: "m1".into(),
                        scope: "project".into(),
                        summary: "prefers rust".into(),
                        content: "User prefers Rust for tools.".into(),
                        score: 10,
                        reasons: vec!["tag".into()],
                        why: "matched".into(),
                        kind: None,
                        project_id: Some("proj".into()),
                        conversation_id: None,
                        importance: 5,
                        confidence: 8,
                        tags: vec!["lang".into()],
                    }],
                    promotion_suggestions: vec![],
                    promotion_ask: "none".into(),
                }),
            ),
            section(
                LlmSectionId::Diagnostics,
                LlmSectionContent::Diagnostics(LlmDiagnostics {
                    diagnostics: vec![LlmDiagnostic {
                        path: Some("/tmp/main.rs".into()),
                        severity: "error".into(),
                        message: "missing semicolon".into(),
                        line: Some(1),
                        column: Some(12),
                        source: Some("rustc".into()),
                    }],
                }),
            ),
            section(
                LlmSectionId::Permissions,
                LlmSectionContent::Permissions(jaymi_context::LlmPermissions {
                    entries: vec![jaymi_context::LlmPermissionEntry {
                        category: "filesystem".into(),
                        action: "read".into(),
                        decision: "allow".into(),
                        resource: Some("/tmp".into()),
                        explanation: Some("workspace read".into()),
                    }],
                }),
            ),
            section(
                LlmSectionId::ActiveCapabilities,
                LlmSectionContent::ActiveCapabilities(LlmActiveCapabilities {
                    capability_ids: vec!["code".into(), "search".into()],
                }),
            ),
        ],
        extensions: Default::default(),
    }
}

fn build_default(goal: &str) -> Prompt {
    PromptBuilder::new().build(
        &rich_context(),
        &[
            ConversationTurn::user("earlier"),
            ConversationTurn::assistant("ok"),
        ],
        goal,
    )
}

#[test]
fn prompt_generation_is_deterministic() {
    let a = build_default("fix it");
    let b = build_default("fix it");
    assert_eq!(a, b);
    assert_eq!(a.text, b.text);
    let json_a = serde_json::to_string(&a).unwrap();
    let json_b = serde_json::to_string(&b).unwrap();
    assert_eq!(json_a, json_b);
}

#[test]
fn section_ordering_matches_template() {
    let prompt = build_default("fix it");
    let expected: Vec<_> = DefaultPromptTemplate
        .section_order()
        .iter()
        .copied()
        .filter(|id| prompt.section_ids().contains(id))
        .collect();
    assert_eq!(prompt.section_ids(), expected);

    let ids = prompt.section_ids();
    let positions: Vec<_> = PromptSectionId::ORDER
        .iter()
        .filter_map(|id| ids.iter().position(|present| present == id))
        .collect();
    let mut sorted = positions.clone();
    sorted.sort();
    assert_eq!(positions, sorted, "included sections must keep ORDER");

    assert_eq!(
        ids.first().copied(),
        Some(PromptSectionId::SystemInstructions)
    );
    assert_eq!(ids.last().copied(), Some(PromptSectionId::UserRequest));
}

#[test]
fn assembles_required_section_kinds() {
    let prompt = build_default("fix it");
    for required in [
        PromptSectionId::SystemInstructions,
        PromptSectionId::Conversation,
        PromptSectionId::RelevantMemories,
        PromptSectionId::ActiveProject,
        PromptSectionId::WorkspaceState,
        PromptSectionId::CurrentFile,
        PromptSectionId::Selection,
        PromptSectionId::SearchResults,
        PromptSectionId::Diagnostics,
        PromptSectionId::Permissions,
        PromptSectionId::Capabilities,
        PromptSectionId::PlannerMetadata,
        PromptSectionId::UserRequest,
    ] {
        assert!(
            prompt.section_ids().contains(&required),
            "missing section {required}"
        );
    }
    assert!(prompt.text.contains("## System Instructions"));
    assert!(prompt.text.contains("## Relevant Memories"));
    assert!(prompt.text.contains("## Active Project"));
    assert!(prompt.text.contains("## Workspace State"));
    assert!(prompt.text.contains("## Current File"));
    assert!(prompt.text.contains("## Selection"));
    assert!(prompt.text.contains("## Search Results"));
    assert!(prompt.text.contains("## Diagnostics"));
    assert!(prompt.text.contains("## Permissions"));
    assert!(prompt.text.contains("## Capabilities"));
    assert!(prompt.text.contains("## Planner Metadata"));
    assert!(prompt.text.contains("## User Request"));
    assert!(prompt.text.contains("goal: fix it"));
}

#[test]
fn truncation_fits_budget_and_reports_diagnostics() {
    let prompt = PromptBuilder::new()
        .with_budget(PromptBudget::characters(280))
        .build(&rich_context(), &[], "investigate a long failure mode");
    assert!(prompt.was_truncated());
    assert!(prompt.diagnostics.budget.truncated);
    assert!(prompt.size_characters() <= 280);
    assert!(!prompt.diagnostics.truncation_notes.is_empty());
    assert!(prompt
        .diagnostics
        .sections
        .iter()
        .any(|s| matches!(
            s.disposition,
            jaymi_reasoning::PromptSectionDisposition::Truncated
                | jaymi_reasoning::PromptSectionDisposition::Budgeted
        )));
    assert!(prompt
        .section_ids()
        .contains(&PromptSectionId::SystemInstructions));
    assert!(prompt.section_ids().contains(&PromptSectionId::UserRequest));
}

#[test]
fn prompt_equality_is_structural() {
    let left = build_default("same");
    let right = build_default("same");
    assert_eq!(left, right);
    let different = build_default("other");
    assert_ne!(left, different);
}

#[test]
fn diagnostics_expose_size_contribution_budget() {
    let prompt = build_default("metrics");
    assert!(prompt.diagnostics.prompt_size_characters > 0);
    assert!(prompt.diagnostics.prompt_size_tokens > 0);
    assert_eq!(
        prompt.diagnostics.budget.used_characters,
        prompt.diagnostics.prompt_size_characters
    );
    assert!(!prompt.diagnostics.sections.is_empty());
    assert_eq!(
        prompt.diagnostics.included_section_count(),
        prompt.sections.len()
    );
    assert_eq!(
        prompt.diagnostics.template_id.as_deref(),
        Some("jaymi.default.v1")
    );
}

#[test]
fn custom_section_order_is_honored() {
    let prompt = PromptBuilder::new()
        .with_section_order(vec![
            PromptSectionId::UserRequest,
            PromptSectionId::SystemInstructions,
        ])
        .with_budget(PromptBudget::unlimited())
        .build(&rich_context(), &[], "ordered");
    assert_eq!(
        prompt.section_ids(),
        vec![
            PromptSectionId::UserRequest,
            PromptSectionId::SystemInstructions
        ]
    );
}

#[test]
fn engine_request_path_builds_same_prompt() {
    let builder = PromptBuilder::new();
    let request = ReasoningRequest::new("via request", rich_context());
    let from_builder = builder.build_from_request(&request);
    let from_parts = builder.build(&request.context, &request.history, &request.goal);
    assert_eq!(from_builder, from_parts);
}

#[test]
fn no_provider_symbols_in_prompt_public_api() {
    let names = [
        "Prompt",
        "PromptBuilder",
        "PromptSectionId",
        "PromptBudget",
        "PromptDiagnostics",
        "PromptTemplate",
        "ModelPromptAdapter",
    ];
    for name in names {
        let lower = name.to_ascii_lowercase();
        for bad in ["ollama", "openai", "anthropic", "gguf", "llama"] {
            assert!(
                !lower.contains(bad),
                "{name} must stay provider-independent"
            );
        }
    }
}

// --- Sprint B1.8: model-aware budgeting ---

fn large_conversation_history(turns: usize) -> Vec<ConversationTurn> {
    (0..turns)
        .map(|i| {
            if i % 2 == 0 {
                ConversationTurn::user(format!(
                    "user turn {i}: {}",
                    "please remember this detail about the feature. ".repeat(20)
                ))
            } else {
                ConversationTurn::assistant(format!(
                    "assistant turn {i}: {}",
                    "here is a long explanation of the prior request. ".repeat(20)
                ))
            }
        })
        .collect()
}

fn large_project_context() -> LlmContext {
    let mut ctx = rich_context();
    for section in &mut ctx.sections {
        if section.id == LlmSectionId::CurrentFile {
            if let LlmSectionContent::CurrentFile(file) = &mut section.content {
                file.path = Some(format!(
                    "/tmp/{}main.rs",
                    "very_long_module_path_segment_".repeat(40)
                ));
            }
        }
        if section.id == LlmSectionId::MemoryResults {
            if let LlmSectionContent::MemoryResults(memories) = &mut section.content {
                for i in 0..40 {
                    memories.memories.push(LlmMemoryItem {
                        id: format!("m{i}"),
                        scope: "project".into(),
                        summary: format!("memory {i}"),
                        content: format!(
                            "memory {i}: {}",
                            "long retrieved note body for budgeting. ".repeat(10)
                        ),
                        score: 5,
                        reasons: vec!["tag".into()],
                        why: "matched".into(),
                        kind: None,
                        project_id: Some("proj".into()),
                        conversation_id: None,
                        importance: 5,
                        confidence: 8,
                        tags: vec!["note".into()],
                    });
                }
                memories.candidate_count = memories.memories.len();
            }
        }
        if section.id == LlmSectionId::OpenFiles {
            if let LlmSectionContent::OpenFiles(open) = &mut section.content {
                for i in 0..30 {
                    open.files.push(LlmOpenFileEntry {
                        path: format!("/tmp/src/module_{i}/file_{i}.rs"),
                        dirty: false,
                        active: false,
                    });
                }
            }
        }
    }
    ctx
}

#[test]
fn budget_overflow_under_tiny_window() {
    let limits = jaymi_reasoning::ModelLimits::new(256).with_max_output_tokens(64);
    let budget = PromptBudget::from_model_limits(&limits, 64);
    let prompt = PromptBuilder::new()
        .with_budget(budget)
        .build(&rich_context(), &large_conversation_history(8), "summarize everything");
    assert!(prompt.was_truncated());
    assert!(prompt.diagnostics.budget.truncated);
    let max_chars = prompt.diagnostics.budget.max_characters.expect("ceiling");
    assert!(prompt.size_characters() <= max_chars);
    assert!(prompt.diagnostics.tokens_remaining().is_some());
    assert!(!prompt.diagnostics.truncated_sections().is_empty());
}

#[test]
fn section_prioritization_omits_low_retention_first() {
    let prompt = PromptBuilder::new()
        .with_budget(PromptBudget::characters(320))
        .build(&rich_context(), &[], "keep the essentials");
    assert!(prompt.was_truncated());
    let ids = prompt.section_ids();
    assert!(ids.contains(&PromptSectionId::SystemInstructions));
    assert!(ids.contains(&PromptSectionId::UserRequest));
    // Planner metadata / memories are lowest retention and should go first.
    let truncated = prompt.diagnostics.truncated_sections();
    assert!(
        truncated.contains(&PromptSectionId::PlannerMetadata)
            || truncated.contains(&PromptSectionId::RelevantMemories)
            || truncated.contains(&PromptSectionId::Diagnostics)
    );
}

#[test]
fn large_conversations_fit_deterministically() {
    let history = large_conversation_history(60);
    let budget = PromptBudget::from_model_limits(&jaymi_reasoning::ModelLimits::new(4_096), 512);
    let left = PromptBuilder::new()
        .with_budget(budget.clone())
        .build(&rich_context(), &history, "catch me up");
    let right = PromptBuilder::new()
        .with_budget(budget)
        .build(&rich_context(), &history, "catch me up");
    assert_eq!(left.text, right.text);
    assert_eq!(left.diagnostics, right.diagnostics);
    assert!(left.was_truncated());
    assert!(left.diagnostics.budget.context_window_tokens == Some(4_096));
    assert_eq!(left.diagnostics.budget.reserved_completion_tokens, 512);
}

#[test]
fn large_projects_fit_under_model_window() {
    let budget = PromptBudget::from_model_limits(&jaymi_reasoning::ModelLimits::new(2_048), 256);
    let prompt = PromptBuilder::new()
        .with_budget(budget)
        .build(&large_project_context(), &[], "what is this project?");
    assert!(prompt.was_truncated());
    let max_chars = prompt.diagnostics.budget.max_characters.expect("ceiling");
    assert!(prompt.size_characters() <= max_chars);
    assert!(prompt.diagnostics.context_efficiency().is_some());
    assert!(prompt
        .diagnostics
        .budget
        .context_efficiency_bps
        .unwrap_or(0)
        > 0);
}

#[test]
fn deterministic_truncation_is_stable() {
    let budget = PromptBudget::characters(400);
    let inputs = (0..5).map(|_| {
        PromptBuilder::new()
            .with_budget(budget.clone())
            .build(&rich_context(), &large_conversation_history(12), "stable")
    });
    let first = inputs.clone().next().unwrap();
    for prompt in inputs {
        assert_eq!(prompt.text, first.text);
        assert_eq!(
            prompt.diagnostics.truncated_sections(),
            first.diagnostics.truncated_sections()
        );
    }
}

#[test]
fn long_context_model_budget_scales_automatically() {
    let short = PromptBudget::from_model_limits(&jaymi_reasoning::ModelLimits::new(8_192), 1_024);
    let long = PromptBudget::from_model_limits(&jaymi_reasoning::ModelLimits::new(131_072), 1_024);
    assert!(long.effective_max_tokens().unwrap() > short.effective_max_tokens().unwrap());
    assert_eq!(long.max_tokens, Some(131_072 - 1_024));
}

#[test]
fn budget_diagnostics_expose_usage_tokens_remaining_efficiency() {
    let budget = PromptBudget::tokens(200);
    let prompt = PromptBuilder::new()
        .with_budget(budget)
        .build(&rich_context(), &[], "metrics");
    let usage = &prompt.diagnostics.budget;
    assert_eq!(usage.tokens_used(), prompt.diagnostics.tokens_used());
    assert_eq!(usage.remaining_tokens, prompt.diagnostics.tokens_remaining());
    assert!(usage.remaining_tokens.is_some());
    assert!(usage.context_efficiency_bps.is_some());
    assert!(prompt.diagnostics.context_efficiency().unwrap() <= 1.0);
}

fn assert_section_included(prompt: &Prompt, id: PromptSectionId, needle: &str) {
    assert!(
        prompt.section_ids().contains(&id),
        "expected {id} in final prompt sections"
    );
    assert!(
        prompt.text.contains(needle),
        "expected prompt text to contain {needle:?} for {id}"
    );
    let contribution = prompt
        .diagnostics
        .sections
        .iter()
        .find(|section| section.id == id)
        .unwrap_or_else(|| panic!("missing diagnostics for {id}"));
    assert_eq!(
        contribution.disposition,
        jaymi_reasoning::PromptSectionDisposition::Included
    );
    assert!(contribution.included);
}

#[test]
fn memory_reaches_prompt() {
    let prompt = build_default("remember");
    assert_section_included(&prompt, PromptSectionId::RelevantMemories, "prefers rust");
}

#[test]
fn project_reaches_prompt() {
    let prompt = build_default("project");
    assert_section_included(&prompt, PromptSectionId::ActiveProject, "Demo");
}

#[test]
fn workspace_reaches_prompt() {
    let prompt = build_default("workspace");
    assert_section_included(&prompt, PromptSectionId::WorkspaceState, "coding");
}

#[test]
fn conversation_reaches_prompt() {
    let prompt = build_default("chat");
    assert_section_included(&prompt, PromptSectionId::Conversation, "c1");
    assert!(prompt.text.contains("user: earlier"));
}

#[test]
fn capabilities_reach_prompt() {
    let prompt = build_default("caps");
    assert_section_included(&prompt, PromptSectionId::Capabilities, "code");
}

#[test]
fn planner_metadata_reaches_prompt() {
    let prompt = build_default("meta");
    assert_section_included(&prompt, PromptSectionId::PlannerMetadata, "assemble_generation");
    assert!(prompt.text.contains("sources:"));
}

#[test]
fn search_reaches_prompt() {
    let prompt = build_default("search");
    assert_section_included(&prompt, PromptSectionId::SearchResults, "hit1");
    assert!(prompt.text.contains("query=bug"));
}

#[test]
fn every_llm_section_has_explicit_coverage() {
    let prompt = build_default("coverage");
    assert_eq!(
        prompt.diagnostics.sections.len(),
        PromptSectionId::ORDER.len(),
        "diagnostics must list every prompt section disposition"
    );
    assert_eq!(
        prompt.diagnostics.llm_coverage.len(),
        jaymi_context::LlmSectionId::ORDER.len(),
        "every LlmSectionId must appear in coverage"
    );
    for entry in &prompt.diagnostics.llm_coverage {
        assert!(entry.prompt_section.is_some(), "{}", entry.llm_section);
        assert!(
            entry.llm_present
                || entry.disposition == jaymi_reasoning::PromptSectionDisposition::Excluded,
            "absent Llm section {} must be excluded, got {:?}",
            entry.llm_section,
            entry.disposition
        );
    }
    let summary = prompt.diagnostics.disposition_summary();
    assert!(summary.contains("included="));
    assert!(summary.contains("excluded="));
}

#[test]
fn absent_sections_are_excluded_not_silent() {
    let empty = LlmContext {
        schema_version: LLM_CONTEXT_SCHEMA_VERSION,
        assemble_generation: 1,
        providers: LlmProviderMetadata {
            sources: vec![],
            notes: vec![],
            budget: None,
            environmental: None,
        },
        sections: LlmSectionId::ORDER
            .iter()
            .copied()
            .map(|id| LlmContextSection {
                id,
                present: false,
                sources: vec![],
                content: LlmSectionContent::Empty,
            })
            .collect(),
        extensions: Default::default(),
    };
    let prompt = PromptBuilder::new().build(&empty, &[], "only goal");
    assert!(prompt.section_ids().contains(&PromptSectionId::UserRequest));
    assert!(prompt.section_ids().contains(&PromptSectionId::SystemInstructions));
    assert!(prompt.section_ids().contains(&PromptSectionId::PlannerMetadata));
    for id in [
        PromptSectionId::RelevantMemories,
        PromptSectionId::ActiveProject,
        PromptSectionId::SearchResults,
        PromptSectionId::Permissions,
        PromptSectionId::Capabilities,
    ] {
        let contribution = prompt
            .diagnostics
            .sections
            .iter()
            .find(|section| section.id == id)
            .expect("disposition recorded");
        assert_eq!(
            contribution.disposition,
            jaymi_reasoning::PromptSectionDisposition::Excluded
        );
        assert!(!contribution.included);
        assert!(contribution.note.is_some());
    }
}

#[test]
fn coding_understanding_extension_becomes_prompt_section() {
    let mut context = rich_context();
    context.extensions.insert(
        "coding_understanding".into(),
        serde_json::json!({
            "focus": "file",
            "instruction": "Respond with Coding Understanding for focus=`file`.\n### Purpose\n- demo",
        }),
    );
    let prompt = PromptBuilder::new().build(&context, &[], "Explain this file.");
    assert!(
        prompt
            .section_ids()
            .contains(&PromptSectionId::CodingUnderstanding),
        "section ids: {:?}",
        prompt.section_ids()
    );
    assert!(prompt.text.contains("Coding Understanding"));
    assert!(prompt.text.contains("focus: file"));
    let contribution = prompt
        .diagnostics
        .sections
        .iter()
        .find(|section| section.id == PromptSectionId::CodingUnderstanding)
        .expect("disposition");
    assert_eq!(
        contribution.disposition,
        jaymi_reasoning::PromptSectionDisposition::Included
    );
}

#[test]
fn coding_review_extension_becomes_prompt_section() {
    let mut context = rich_context();
    context.extensions.insert(
        "coding_review".into(),
        serde_json::json!({
            "focus": "file",
            "instruction": "Respond with Coding Review for focus=`file`.\n### Strengths\n- demo",
        }),
    );
    let prompt = PromptBuilder::new().build(&context, &[], "Review this file.");
    assert!(
        prompt
            .section_ids()
            .contains(&PromptSectionId::CodingReview),
        "section ids: {:?}",
        prompt.section_ids()
    );
    assert!(prompt.text.contains("Coding Review"));
    assert!(prompt.text.contains("focus: file"));
}

#[test]
fn coding_plan_extension_becomes_prompt_section() {
    let mut context = rich_context();
    context.extensions.insert(
        "coding_plan".into(),
        serde_json::json!({
            "kind": "new_project",
            "goal": "Build Pong.",
            "instruction": "Respond with a Coding Plan (kind=`new_project`).\n### Plan\n- demo",
        }),
    );
    let prompt = PromptBuilder::new().build(&context, &[], "Build Pong.");
    assert!(
        prompt.section_ids().contains(&PromptSectionId::CodingPlan),
        "section ids: {:?}",
        prompt.section_ids()
    );
    assert!(prompt.text.contains("Coding Plan"));
    assert!(prompt.text.contains("kind: new_project"));
}
