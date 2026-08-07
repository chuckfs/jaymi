//! [`PromptBuilder`] — assembles [`Prompt`] from [`LlmContext`].

use jaymi_context::{
    LlmActiveCapabilities, LlmActiveProject, LlmContext, LlmConversation, LlmCurrentFile,
    LlmCurrentSelection, LlmDiagnostics, LlmMemoryResults, LlmOpenFiles, LlmPermissions,
    LlmProviderMetadata, LlmSearchResults, LlmSectionContent, LlmSectionId, LlmUserRequest,
};

use crate::request::ReasoningRequest;
use crate::types::{ConversationRole, ConversationTurn};

use super::adapter::{ModelPromptAdapter, NullPromptAdapter};
use super::budget::{PromptBudget, PromptBudgetUsage};
use super::diagnostics::{PromptDiagnostics, PromptLlmCoverageEntry, PromptSectionContribution};
use super::format::{PlainTextFormatter, PromptFormatter};
use super::section::{PromptSectionDisposition, PromptSectionId};
use super::template::{DefaultPromptTemplate, PromptTemplate};
use super::types::{Prompt, PromptSection, PROMPT_SCHEMA_VERSION};

/// Builds provider-independent prompts from structured context.
///
/// The Planner and Reasoning providers must not concatenate prompt strings;
/// they call through the Reasoning Engine into this builder.
#[derive(Clone)]
pub struct PromptBuilder {
    budget: PromptBudget,
    system_instructions: Option<String>,
    section_order: Option<Vec<PromptSectionId>>,
    template_id: String,
    default_system: String,
    formatter_id: String,
    adapter_id: String,
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PromptBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptBuilder")
            .field("budget", &self.budget)
            .field("template_id", &self.template_id)
            .field("formatter_id", &self.formatter_id)
            .field("adapter_id", &self.adapter_id)
            .finish()
    }
}

impl PromptBuilder {
    /// Default builder (default template, plain-text formatter, null adapter).
    pub fn new() -> Self {
        let template = DefaultPromptTemplate;
        Self {
            budget: PromptBudget::default(),
            system_instructions: None,
            section_order: None,
            template_id: template.id().into(),
            default_system: template.default_system_instructions().into(),
            formatter_id: PlainTextFormatter.id().into(),
            adapter_id: NullPromptAdapter.id().into(),
        }
    }

    /// Apply a [`PromptTemplate`] (order + default system copy).
    pub fn with_template(mut self, template: &dyn PromptTemplate) -> Self {
        self.template_id = template.id().into();
        self.default_system = template.default_system_instructions().into();
        self.section_order = Some(template.section_order().to_vec());
        self
    }

    /// Override system instructions.
    pub fn with_system_instructions(mut self, text: impl Into<String>) -> Self {
        self.system_instructions = Some(text.into());
        self
    }

    /// Override section emission order.
    pub fn with_section_order(mut self, order: Vec<PromptSectionId>) -> Self {
        self.section_order = Some(order);
        self
    }

    /// Attach a token / character budget.
    pub fn with_budget(mut self, budget: PromptBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Current budget configuration.
    pub fn budget(&self) -> &PromptBudget {
        &self.budget
    }

    /// Build with an explicit budget without replacing the builder's stored default.
    pub fn build_with_budget(
        &self,
        context: &LlmContext,
        history: &[ConversationTurn],
        goal: &str,
        budget: PromptBudget,
    ) -> Prompt {
        self.clone().with_budget(budget).build(context, history, goal)
    }

    /// Record which formatter id will be reported in diagnostics.
    pub fn with_formatter_id(mut self, id: impl Into<String>) -> Self {
        self.formatter_id = id.into();
        self
    }

    /// Record which adapter id will be reported in diagnostics.
    pub fn with_adapter_id(mut self, id: impl Into<String>) -> Self {
        self.adapter_id = id.into();
        self
    }

    /// Build from a reasoning request (context + history + goal).
    pub fn build_from_request(&self, request: &ReasoningRequest) -> Prompt {
        self.build(&request.context, &request.history, &request.goal)
    }

    /// Build a prompt from structured context, conversation turns, and goal.
    pub fn build(
        &self,
        context: &LlmContext,
        history: &[ConversationTurn],
        goal: &str,
    ) -> Prompt {
        let build_started = std::time::Instant::now();
        let order = self
            .section_order
            .as_deref()
            .unwrap_or(PromptSectionId::ORDER);
        let system = self
            .system_instructions
            .as_deref()
            .unwrap_or(self.default_system.as_str());

        // Every ordered section gets an explicit disposition — no silent drops.
        let mut drafts: Vec<DraftSection> = order
            .iter()
            .map(|id| self.draft_section(*id, context, history, goal, system))
            .collect();

        let (truncated, truncation_notes) = apply_budget(&mut drafts, &self.budget);

        let sections: Vec<PromptSection> = drafts
            .iter()
            .filter(|draft| draft.included)
            .map(|draft| PromptSection {
                id: draft.id,
                heading: draft.id.heading().to_string(),
                body: draft.body.clone(),
            })
            .collect();

        let formatter = PlainTextFormatter;
        let text = formatter.format(&sections);
        // Provisional size from flat text — replaced by seal_for_delivery below.
        let used_characters = text.chars().count();
        let estimated_tokens = self.budget.estimate_tokens(used_characters);

        let contributions: Vec<PromptSectionContribution> = drafts
            .iter()
            .map(|draft| PromptSectionContribution {
                id: draft.id,
                characters: if draft.included {
                    draft.body.chars().count()
                } else {
                    0
                },
                estimated_tokens: self.budget.estimate_tokens(if draft.included {
                    draft.body.chars().count()
                } else {
                    0
                }),
                included: draft.included,
                truncated: draft.truncated,
                disposition: draft.disposition,
                note: draft.note.clone(),
                source_llm_sections: draft
                    .id
                    .llm_sources()
                    .iter()
                    .map(|label| (*label).to_string())
                    .collect(),
            })
            .collect();

        let llm_coverage = build_llm_coverage(context, &drafts);

        let diagnostics = PromptDiagnostics {
            prompt_size_characters: used_characters,
            prompt_size_tokens: estimated_tokens,
            assembled_prompt_size_characters: None,
            assembled_prompt_size_tokens: None,
            final_token_estimate: estimated_tokens,
            conversation_turns: history.len() as u64,
            budget: PromptBudgetUsage::from_budget(&self.budget, used_characters, truncated),
            sections: contributions,
            llm_coverage,
            truncated,
            truncation_notes,
            template_id: Some(self.template_id.clone()),
            formatter_id: Some(self.formatter_id.clone()),
            adapter_id: Some(self.adapter_id.clone()),
            build_duration_ms: None,
        };

        let prompt = Prompt {
            schema_version: PROMPT_SCHEMA_VERSION,
            sections,
            text,
            diagnostics,
        };

        let adapter = NullPromptAdapter;
        let mut adapted = adapter.adapt(prompt);
        adapted.diagnostics.adapter_id = Some(self.adapter_id.clone());
        adapted.diagnostics.template_id = Some(self.template_id.clone());
        adapted.diagnostics.formatter_id = Some(self.formatter_id.clone());
        // Retain assembled size before seal overwrites delivered size (Performance).
        adapted.diagnostics.assembled_prompt_size_characters =
            Some(adapted.diagnostics.prompt_size_characters);
        adapted.diagnostics.assembled_prompt_size_tokens =
            Some(adapted.diagnostics.prompt_size_tokens);
        // Diagnostics describe the prompt actually delivered to providers.
        adapted.seal_for_delivery(&self.budget, history.len());
        adapted.diagnostics.build_duration_ms =
            Some(build_started.elapsed().as_millis() as u64);
        adapted
    }

    fn draft_section(
        &self,
        id: PromptSectionId,
        context: &LlmContext,
        history: &[ConversationTurn],
        goal: &str,
        system: &str,
    ) -> DraftSection {
        match id {
            PromptSectionId::SystemInstructions => {
                let body = system.trim().to_string();
                if body.is_empty() {
                    DraftSection::excluded(id, "empty system instructions")
                } else {
                    DraftSection::included(id, body)
                }
            }
            PromptSectionId::Conversation => {
                let body = format_conversation(context, history);
                if body.trim().is_empty() {
                    DraftSection::excluded(id, "no conversation metadata or history")
                } else {
                    DraftSection::included(id, body)
                }
            }
            PromptSectionId::RelevantMemories => match find_content(context, LlmSectionId::MemoryResults)
            {
                Some(LlmSectionContent::MemoryResults(mem)) => match format_memories(mem) {
                    Some(body) => DraftSection::included(id, body),
                    None => DraftSection::filtered(
                        id,
                        "memory_results present but empty after format",
                    ),
                },
                Some(_) => DraftSection::filtered(id, "memory_results content kind mismatch"),
                None => DraftSection::excluded(id, "memory_results absent from LlmContext"),
            },
            PromptSectionId::ActiveProject => match find_content(context, LlmSectionId::ActiveProject)
            {
                Some(LlmSectionContent::ActiveProject(project)) => match format_project(project) {
                    Some(body) => DraftSection::included(id, body),
                    None => DraftSection::filtered(
                        id,
                        "active_project present but empty after format",
                    ),
                },
                Some(_) => DraftSection::filtered(id, "active_project content kind mismatch"),
                None => DraftSection::excluded(id, "active_project absent from LlmContext"),
            },
            PromptSectionId::WorkspaceState => {
                let body = format_workspace(context);
                if body.trim().is_empty() {
                    let ws = llm_section_present(context, LlmSectionId::ActiveWorkspace);
                    let files = llm_section_present(context, LlmSectionId::OpenFiles);
                    let git = llm_section_present(context, LlmSectionId::GitStatus);
                    let inventory = llm_section_present(context, LlmSectionId::WorkspaceInventory);
                    let summaries = llm_section_present(context, LlmSectionId::FileSummaries);
                    if ws || files || git || inventory || summaries {
                        DraftSection::filtered(
                            id,
                            "workspace maintenance sections present but empty after format",
                        )
                    } else {
                        DraftSection::excluded(
                            id,
                            "active_workspace/open_files/maintenance absent from LlmContext",
                        )
                    }
                } else {
                    DraftSection::included(id, body)
                }
            }
            PromptSectionId::CurrentFile => match find_content(context, LlmSectionId::CurrentFile) {
                Some(LlmSectionContent::CurrentFile(file)) => match format_current_file(file) {
                    Some(body) => DraftSection::included(id, body),
                    None => DraftSection::filtered(id, "current_file present but empty after format"),
                },
                Some(_) => DraftSection::filtered(id, "current_file content kind mismatch"),
                None => DraftSection::excluded(id, "current_file absent from LlmContext"),
            },
            PromptSectionId::Selection => {
                match find_content(context, LlmSectionId::CurrentSelection) {
                    Some(LlmSectionContent::CurrentSelection(selection)) => {
                        match format_selection(selection) {
                            Some(body) => DraftSection::included(id, body),
                            None => DraftSection::filtered(
                                id,
                                "current_selection present but empty after format",
                            ),
                        }
                    }
                    Some(_) => DraftSection::filtered(id, "current_selection content kind mismatch"),
                    None => DraftSection::excluded(id, "current_selection absent from LlmContext"),
                }
            }
            PromptSectionId::EditorIntelligence => {
                match find_content(context, LlmSectionId::EditorIntelligence) {
                    Some(LlmSectionContent::EditorIntelligence(intel)) => {
                        match format_editor_intelligence(intel) {
                            Some(body) => DraftSection::included(id, body),
                            None => DraftSection::filtered(
                                id,
                                "editor_intelligence present but empty after format",
                            ),
                        }
                    }
                    Some(_) => {
                        DraftSection::filtered(id, "editor_intelligence content kind mismatch")
                    }
                    None => DraftSection::excluded(id, "editor_intelligence absent from LlmContext"),
                }
            }
            PromptSectionId::ProjectIntelligence => {
                match find_content(context, LlmSectionId::ProjectIntelligence) {
                    Some(LlmSectionContent::ProjectIntelligence(intel)) => {
                        match format_project_intelligence(intel) {
                            Some(body) => DraftSection::included(id, body),
                            None => DraftSection::filtered(
                                id,
                                "project_intelligence present but empty after format",
                            ),
                        }
                    }
                    Some(_) => {
                        DraftSection::filtered(id, "project_intelligence content kind mismatch")
                    }
                    None => {
                        DraftSection::excluded(id, "project_intelligence absent from LlmContext")
                    }
                }
            }
            PromptSectionId::RuntimeIntelligence => {
                match find_content(context, LlmSectionId::RuntimeIntelligence) {
                    Some(LlmSectionContent::RuntimeIntelligence(intel)) => {
                        match format_runtime_intelligence(intel) {
                            Some(body) => DraftSection::included(id, body),
                            None => DraftSection::filtered(
                                id,
                                "runtime_intelligence present but empty after format",
                            ),
                        }
                    }
                    Some(_) => {
                        DraftSection::filtered(id, "runtime_intelligence content kind mismatch")
                    }
                    None => {
                        DraftSection::excluded(id, "runtime_intelligence absent from LlmContext")
                    }
                }
            }
            PromptSectionId::WorkspaceMemory => {
                match find_content(context, LlmSectionId::WorkspaceMemory) {
                    Some(LlmSectionContent::WorkspaceMemory(memory)) => {
                        match format_workspace_memory(memory) {
                            Some(body) => DraftSection::included(id, body),
                            None => DraftSection::filtered(
                                id,
                                "workspace_memory present but empty after format",
                            ),
                        }
                    }
                    Some(_) => DraftSection::filtered(id, "workspace_memory content kind mismatch"),
                    None => DraftSection::excluded(id, "workspace_memory absent from LlmContext"),
                }
            }
            PromptSectionId::EnvironmentalResolution => {
                match format_environmental_resolution(context) {
                    Some(body) => DraftSection::included(id, body),
                    None => DraftSection::excluded(
                        id,
                        "no planner environmental resolution bindings",
                    ),
                }
            }
            PromptSectionId::SearchResults => {
                match find_content(context, LlmSectionId::SearchResults) {
                    Some(LlmSectionContent::SearchResults(search)) => match format_search(search) {
                        Some(body) => DraftSection::included(id, body),
                        None => DraftSection::filtered(
                            id,
                            "search_results present but empty after format",
                        ),
                    },
                    Some(_) => DraftSection::filtered(id, "search_results content kind mismatch"),
                    None => DraftSection::excluded(id, "search_results absent from LlmContext"),
                }
            }
            PromptSectionId::Diagnostics => match find_content(context, LlmSectionId::Diagnostics) {
                Some(LlmSectionContent::Diagnostics(diagnostics)) => {
                    match format_diagnostics(diagnostics) {
                        Some(body) => DraftSection::included(id, body),
                        None => DraftSection::filtered(
                            id,
                            "diagnostics present but empty after format",
                        ),
                    }
                }
                Some(_) => DraftSection::filtered(id, "diagnostics content kind mismatch"),
                None => DraftSection::excluded(id, "diagnostics absent from LlmContext"),
            },
            PromptSectionId::Permissions => match find_content(context, LlmSectionId::Permissions) {
                Some(LlmSectionContent::Permissions(permissions)) => {
                    match format_permissions(permissions) {
                        Some(body) => DraftSection::included(id, body),
                        None => DraftSection::filtered(
                            id,
                            "permissions present but empty after format",
                        ),
                    }
                }
                Some(_) => DraftSection::filtered(id, "permissions content kind mismatch"),
                None => DraftSection::excluded(id, "permissions absent from LlmContext"),
            },
            PromptSectionId::Capabilities => {
                match find_content(context, LlmSectionId::ActiveCapabilities) {
                    Some(LlmSectionContent::ActiveCapabilities(caps)) => {
                        match format_capabilities(caps) {
                            Some(body) => DraftSection::included(id, body),
                            None => DraftSection::filtered(
                                id,
                                "active_capabilities present but empty after format",
                            ),
                        }
                    }
                    Some(_) => {
                        DraftSection::filtered(id, "active_capabilities content kind mismatch")
                    }
                    None => DraftSection::excluded(id, "active_capabilities absent from LlmContext"),
                }
            }
            PromptSectionId::PlannerMetadata => {
                let body = format_planner_metadata(context);
                if body.trim().is_empty() {
                    DraftSection::excluded(id, "empty planner metadata")
                } else {
                    DraftSection::included(id, body)
                }
            }
            PromptSectionId::UserRequest => {
                let body = format_user_request(context, goal);
                DraftSection::included(id, body)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct DraftSection {
    id: PromptSectionId,
    body: String,
    original_characters: usize,
    included: bool,
    truncated: bool,
    disposition: PromptSectionDisposition,
    note: Option<String>,
}

impl DraftSection {
    fn included(id: PromptSectionId, body: String) -> Self {
        let body = body.trim().to_string();
        let original_characters = body.chars().count();
        Self {
            id,
            body,
            original_characters,
            included: true,
            truncated: false,
            disposition: PromptSectionDisposition::Included,
            note: None,
        }
    }

    fn excluded(id: PromptSectionId, note: impl Into<String>) -> Self {
        Self {
            id,
            body: String::new(),
            original_characters: 0,
            included: false,
            truncated: false,
            disposition: PromptSectionDisposition::Excluded,
            note: Some(note.into()),
        }
    }

    fn filtered(id: PromptSectionId, note: impl Into<String>) -> Self {
        Self {
            id,
            body: String::new(),
            original_characters: 0,
            included: false,
            truncated: false,
            disposition: PromptSectionDisposition::Filtered,
            note: Some(note.into()),
        }
    }
}

fn apply_budget(drafts: &mut [DraftSection], budget: &PromptBudget) -> (bool, Vec<String>) {
    let Some(max_chars) = budget.effective_max_characters() else {
        return (false, Vec::new());
    };

    // Rough overhead: "## Heading\n" + blank lines between sections.
    let overhead_per_section = 32usize;
    let mut notes = Vec::new();
    let mut truncated = false;

    let total = |drafts: &[DraftSection]| -> usize {
        drafts
            .iter()
            .filter(|d| d.included)
            .map(|d| d.body.chars().count().saturating_add(overhead_per_section))
            .sum()
    };

    if total(drafts) <= max_chars {
        return (false, notes);
    }

    // Drop lowest-retention sections first (omit entirely).
    let mut order: Vec<usize> = (0..drafts.len()).collect();
    order.sort_by_key(|&index| drafts[index].id.retention_priority());

    for index in order {
        if total(drafts) <= max_chars {
            break;
        }
        let draft = &mut drafts[index];
        if !draft.included {
            continue;
        }
        // Never omit system instructions or user request — truncate instead.
        if matches!(
            draft.id,
            PromptSectionId::SystemInstructions | PromptSectionId::UserRequest
        ) {
            continue;
        }
        draft.included = false;
        draft.truncated = false;
        draft.disposition = PromptSectionDisposition::Budgeted;
        draft.note = Some("omitted to fit prompt budget".into());
        notes.push(format!(
            "budgeted section {} omitted to fit budget",
            draft.id.as_str()
        ));
        truncated = true;
    }

    // Truncate remaining bodies from lowest retention if still over.
    let mut order: Vec<usize> = (0..drafts.len()).collect();
    order.sort_by_key(|&index| drafts[index].id.retention_priority());

    for index in order {
        if total(drafts) <= max_chars {
            break;
        }
        if !drafts[index].included {
            continue;
        }
        let current = total(drafts);
        let overflow = current.saturating_sub(max_chars);
        let body_chars = drafts[index].body.chars().count();
        if body_chars == 0 {
            continue;
        }
        let keep = body_chars.saturating_sub(overflow).max(16);
        if keep >= body_chars {
            continue;
        }
        let original = drafts[index].original_characters;
        let truncated_body = truncate_chars(&drafts[index].body, keep);
        let new_len = truncated_body.chars().count();
        drafts[index].body = truncated_body;
        drafts[index].truncated = true;
        drafts[index].disposition = PromptSectionDisposition::Truncated;
        drafts[index].note = Some(format!(
            "truncated from {original} to {new_len} characters"
        ));
        notes.push(format!(
            "truncated section {} from {original} to {new_len} characters",
            drafts[index].id.as_str()
        ));
        truncated = true;
    }

    (truncated, notes)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

fn llm_section_present(context: &LlmContext, id: LlmSectionId) -> bool {
    context
        .sections
        .iter()
        .any(|section| section.id == id && section.present)
}

fn find_content<'a>(
    context: &'a LlmContext,
    id: LlmSectionId,
) -> Option<&'a LlmSectionContent> {
    context.sections.iter().find_map(|section| {
        if section.id == id && section.present {
            Some(&section.content)
        } else {
            None
        }
    })
}

fn prompt_section_for_llm(id: LlmSectionId) -> PromptSectionId {
    match id {
        LlmSectionId::UserRequest => PromptSectionId::UserRequest,
        LlmSectionId::Conversation => PromptSectionId::Conversation,
        LlmSectionId::ActiveProject => PromptSectionId::ActiveProject,
        LlmSectionId::ActiveWorkspace | LlmSectionId::OpenFiles | LlmSectionId::GitStatus
        | LlmSectionId::WorkspaceInventory | LlmSectionId::FileSummaries => {
            PromptSectionId::WorkspaceState
        }
        LlmSectionId::CurrentFile => PromptSectionId::CurrentFile,
        LlmSectionId::CurrentSelection => PromptSectionId::Selection,
        LlmSectionId::EditorIntelligence => PromptSectionId::EditorIntelligence,
        LlmSectionId::ProjectIntelligence => PromptSectionId::ProjectIntelligence,
        LlmSectionId::RuntimeIntelligence => PromptSectionId::RuntimeIntelligence,
        LlmSectionId::WorkspaceMemory => PromptSectionId::WorkspaceMemory,
        LlmSectionId::SearchResults => PromptSectionId::SearchResults,
        LlmSectionId::MemoryResults => PromptSectionId::RelevantMemories,
        LlmSectionId::Diagnostics => PromptSectionId::Diagnostics,
        LlmSectionId::Permissions => PromptSectionId::Permissions,
        LlmSectionId::ActiveCapabilities => PromptSectionId::Capabilities,
    }
}

fn build_llm_coverage(context: &LlmContext, drafts: &[DraftSection]) -> Vec<PromptLlmCoverageEntry> {
    LlmSectionId::ORDER
        .iter()
        .copied()
        .map(|llm_id| {
            let prompt_section = prompt_section_for_llm(llm_id);
            let llm_present = llm_section_present(context, llm_id);
            let draft = drafts.iter().find(|draft| draft.id == prompt_section);
            let folded = matches!(
                llm_id,
                LlmSectionId::OpenFiles
                    | LlmSectionId::ActiveWorkspace
                    | LlmSectionId::GitStatus
                    | LlmSectionId::WorkspaceInventory
                    | LlmSectionId::FileSummaries
            );
            let (disposition, note) = match draft {
                Some(draft) => {
                    // Folded Llm sections may be absent while the shared prompt
                    // section is still Included from a sibling source. Coverage
                    // reports Excluded for the absent sibling so diagnostics stay
                    // honest about which Llm payloads reached the prompt.
                    let disposition = if folded && !llm_present {
                        PromptSectionDisposition::Excluded
                    } else {
                        draft.disposition
                    };
                    let note = if folded {
                        Some(format!(
                            "folded into workspace_state · {}",
                            draft
                                .note
                                .clone()
                                .unwrap_or_else(|| draft.disposition.as_str().into())
                        ))
                    } else {
                        draft.note.clone()
                    };
                    (disposition, note)
                }
                None => (
                    PromptSectionDisposition::Excluded,
                    Some("prompt section not in emission order".into()),
                ),
            };
            PromptLlmCoverageEntry {
                llm_section: llm_id.as_str().into(),
                prompt_section: Some(prompt_section),
                disposition,
                llm_present,
                note,
            }
        })
        .collect()
}

fn format_conversation(context: &LlmContext, history: &[ConversationTurn]) -> String {
    let mut lines = Vec::new();
    if let Some(LlmSectionContent::Conversation(conv)) =
        find_content(context, LlmSectionId::Conversation)
    {
        if let Some(meta) = format_conversation_meta(conv) {
            lines.push(meta);
        }
    }
    for turn in history {
        let role = match turn.role {
            ConversationRole::User => "user",
            ConversationRole::Assistant => "assistant",
            ConversationRole::System => "system",
            ConversationRole::Tool => match turn.tool_name.as_deref() {
                Some(name) => {
                    lines.push(format!("tool[{name}]: {}", turn.content.trim()));
                    continue;
                }
                None => "tool",
            },
        };
        lines.push(format!("{role}: {}", turn.content.trim()));
    }
    lines.join("\n")
}

fn format_conversation_meta(conv: &LlmConversation) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(id) = &conv.id {
        parts.push(format!("id={id}"));
    }
    if let Some(title) = &conv.title {
        parts.push(format!("title={title}"));
    }
    if let Some(status) = &conv.status {
        parts.push(format!("status={status}"));
    }
    if let Some(count) = conv.message_count {
        parts.push(format!("messages={count}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("meta: {}", parts.join(" ")))
    }
}

fn format_memories(mem: &LlmMemoryResults) -> Option<String> {
    if mem.memories.is_empty() && mem.promotion_suggestions.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    if mem.truncated {
        lines.push("note: memory list truncated by context assemble".into());
    }
    for item in &mem.memories {
        lines.push(format!(
            "- [{}] score={} summary={} content={}",
            item.id,
            item.score,
            compact(&item.summary),
            compact(&item.content)
        ));
    }
    for promo in &mem.promotion_suggestions {
        lines.push(format!(
            "- promote {} ({} → {}): {}",
            promo.memory_id,
            promo.from,
            promo.to,
            compact(&promo.reason)
        ));
    }
    Some(lines.join("\n"))
}

fn format_project(project: &LlmActiveProject) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(id) = &project.project_id {
        lines.push(format!("project_id: {id}"));
    }
    if let Some(name) = &project.name {
        lines.push(format!("name: {name}"));
    }
    if let Some(root) = &project.root_directory {
        lines.push(format!("root: {root}"));
    }
    if let Some(detail) = &project.detail {
        lines.push(format!(
            "detail: open={} entries={} indexed_files={} conversations={}",
            detail.is_open, detail.entry_count, detail.indexed_files, detail.conversations
        ));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_workspace(context: &LlmContext) -> String {
    let mut lines = Vec::new();
    if let Some(LlmSectionContent::ActiveWorkspace(ws)) =
        find_content(context, LlmSectionId::ActiveWorkspace)
    {
        if let Some(kind) = &ws.kind_id {
            lines.push(format!("workspace: {kind}"));
        }
    }
    if let Some(LlmSectionContent::OpenFiles(files)) =
        find_content(context, LlmSectionId::OpenFiles)
    {
        if let Some(block) = format_open_files(files) {
            lines.push(block);
        }
    }
    if let Some(LlmSectionContent::GitStatus(git)) = find_content(context, LlmSectionId::GitStatus)
    {
        if let Some(block) = format_git_status(git) {
            lines.push(block);
        }
    }
    if let Some(LlmSectionContent::WorkspaceInventory(inventory)) =
        find_content(context, LlmSectionId::WorkspaceInventory)
    {
        if let Some(block) = format_workspace_inventory(inventory) {
            lines.push(block);
        }
    }
    if let Some(LlmSectionContent::FileSummaries(summaries)) =
        find_content(context, LlmSectionId::FileSummaries)
    {
        if let Some(block) = format_file_summaries(summaries) {
            lines.push(block);
        }
    }
    lines.join("\n")
}

fn format_git_status(git: &jaymi_context::LlmGitStatus) -> Option<String> {
    if !git.is_repository && git.summary.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    if let Some(branch) = &git.branch {
        lines.push(format!("git: branch={branch} summary={}", git.summary));
    } else {
        lines.push(format!("git: summary={}", git.summary));
    }
    if let Some(head) = git.head_short.as_ref().or(git.head_sha.as_ref()) {
        lines.push(format!("git_head: {head}"));
    }
    lines.push(format!(
        "git_counts: modified={} staged={} untracked={} conflicts={}",
        git.modified_count, git.staged_count, git.untracked_count, git.conflict_count
    ));
    if !git.conflict_paths.is_empty() {
        lines.push(format!(
            "git_conflicts: {}",
            git.conflict_paths.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !git.dirty_paths.is_empty() {
        lines.push(format!(
            "git_dirty: {}",
            git.dirty_paths.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !git.staged_paths.is_empty() {
        lines.push(format!(
            "git_staged: {}",
            git.staged_paths.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !git.untracked_paths.is_empty() {
        lines.push(format!(
            "git_untracked: {}",
            git.untracked_paths.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !git.recent_commits.is_empty() {
        lines.push(format!("git_recent_commits: {}", git.recent_commits.len()));
        for commit in git.recent_commits.iter().take(5) {
            let mut line = format!("- {} {}", commit.short_sha, commit.subject);
            if let Some(when) = &commit.relative_time {
                line.push_str(&format!(" ({when})"));
            }
            lines.push(line);
        }
    } else if !git.sample_paths.is_empty() {
        lines.push(format!("git_paths: {}", git.sample_paths.join(", ")));
    }
    Some(lines.join("\n"))
}

fn format_workspace_inventory(inventory: &jaymi_context::LlmWorkspaceInventory) -> Option<String> {
    if inventory.root.is_none()
        && inventory.file_count == 0
        && inventory.directory_count == 0
        && inventory.status.is_empty()
    {
        return None;
    }
    let mut lines = Vec::new();
    if let Some(root) = &inventory.root {
        lines.push(format!("inventory_root: {root}"));
    }
    lines.push(format!(
        "inventory: status={} files={} dirs={}",
        inventory.status, inventory.file_count, inventory.directory_count
    ));
    if !inventory.sample_paths.is_empty() {
        lines.push(format!("inventory_sample: {}", inventory.sample_paths.join(", ")));
    }
    Some(lines.join("\n"))
}

fn format_file_summaries(summaries: &jaymi_context::LlmFileSummaries) -> Option<String> {
    if summaries.entries.is_empty() {
        return None;
    }
    let mut lines = vec!["file_summaries:".to_string()];
    for entry in &summaries.entries {
        let language = entry.language.as_deref().unwrap_or("unknown");
        let lines_label = entry
            .line_count
            .map(|count| format!(" lines={count}"))
            .unwrap_or_default();
        lines.push(format!(
            "- {} ({language}{lines_label}): {}",
            entry.path,
            compact(&entry.summary)
        ));
    }
    Some(lines.join("\n"))
}

fn format_open_files(files: &LlmOpenFiles) -> Option<String> {
    if files.files.is_empty() {
        return None;
    }
    let mut lines = vec!["open_files:".to_string()];
    for file in &files.files {
        let mut flags = Vec::new();
        if file.active {
            flags.push("active");
        }
        if file.dirty {
            flags.push("dirty");
        }
        let flag = if flags.is_empty() {
            String::new()
        } else {
            format!(" ({})", flags.join(","))
        };
        lines.push(format!("- {}{flag}", file.path));
    }
    Some(lines.join("\n"))
}

fn format_current_file(file: &LlmCurrentFile) -> Option<String> {
    let path = file.path.as_ref()?;
    let mut lines = vec![format!("path: {path}")];
    lines.push(format!("dirty: {}", file.dirty));
    if let Some(language) = &file.language {
        lines.push(format!("language: {language}"));
    }
    Some(lines.join("\n"))
}

fn format_editor_intelligence(
    intel: &jaymi_context::LlmEditorIntelligence,
) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(symbol) = &intel.symbol {
        let mut line = format!("symbol: {}", symbol.name);
        if let Some(kind) = &symbol.kind {
            line.push_str(&format!(" ({kind})"));
        }
        lines.push(line);
        if let Some(detail) = &symbol.detail {
            lines.push(format!("symbol_detail: {detail}"));
        }
    }
    if let Some(func) = &intel.enclosing_function {
        lines.push(format!("enclosing_function: {}", func.name));
    }
    if let Some(ty) = &intel.enclosing_type {
        lines.push(format!("enclosing_type: {}", ty.name));
    }
    if intel.semantic_token_count > 0 {
        lines.push(format!(
            "semantic_tokens: {}",
            intel.semantic_token_count
        ));
    }
    if !intel.references.is_empty() {
        lines.push(format!("references: {}", intel.references.len()));
        for reference in intel.references.iter().take(8) {
            lines.push(format!(
                "- {}:{}:{}",
                reference.path, reference.start_line, reference.start_column
            ));
        }
    }
    if !intel.code_lens.is_empty() {
        lines.push(format!("code_lens: {}", intel.code_lens.len()));
        for lens in intel.code_lens.iter().take(8) {
            lines.push(format!("- {}", lens.title));
        }
    }
    if let Some(hover) = &intel.hover {
        let preview: String = hover.contents.chars().take(480).collect();
        lines.push(format!("hover:\n{preview}"));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_project_intelligence(
    intel: &jaymi_context::LlmProjectIntelligence,
) -> Option<String> {
    let mut lines = Vec::new();
    if !intel.languages.is_empty() {
        lines.push(format!("languages: {}", intel.languages.join(", ")));
    }
    if !intel.frameworks.is_empty() {
        lines.push(format!("frameworks: {}", intel.frameworks.join(", ")));
    }
    if let Some(pm) = &intel.package_manager {
        lines.push(format!("package_manager: {pm}"));
    }
    if let Some(build) = &intel.build_system {
        lines.push(format!("build_system: {build}"));
    }
    if intel.dependency_direct_count > 0 || !intel.dependency_top_level.is_empty() {
        lines.push(format!(
            "dependencies: {} direct",
            intel.dependency_direct_count
        ));
        for dep in intel.dependency_top_level.iter().take(16) {
            lines.push(format!("- {dep}"));
        }
    }
    if !intel.workspace_members.is_empty() {
        lines.push(format!(
            "workspace_members: {}",
            intel.workspace_members.join(", ")
        ));
    }
    if let Some(name) = &intel.cargo_package {
        lines.push(format!("cargo_package: {name}"));
    }
    if let Some(name) = &intel.npm_package {
        lines.push(format!("npm_package: {name}"));
    }
    if let Some(branch) = &intel.repository_branch {
        lines.push(format!("repository_branch: {branch}"));
    }
    if let Some(shape) = &intel.layout_shape {
        lines.push(format!("layout: {shape}"));
    }
    if !intel.top_level_dirs.is_empty() {
        lines.push(format!(
            "top_level_dirs: {}",
            intel.top_level_dirs.join(", ")
        ));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_runtime_intelligence(
    intel: &jaymi_context::LlmRuntimeIntelligence,
) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(check) = &intel.latest_cargo_check {
        lines.push(format!("latest_cargo_check: {check}"));
    }
    if let Some(build) = &intel.latest_build {
        lines.push(format!("latest_build: {build}"));
    }
    if let Some(tests) = &intel.latest_tests {
        lines.push(format!("latest_tests: {tests}"));
    }
    if intel.session_count > 0 || intel.alive_count > 0 {
        lines.push(format!(
            "terminal_sessions: {} ({} alive)",
            intel.session_count, intel.alive_count
        ));
    }
    if let Some(cmd) = &intel.last_command {
        lines.push(format!("last_command: {cmd}"));
    }
    if !intel.running.is_empty() {
        lines.push(format!("running: {}", intel.running.len()));
        for entry in intel.running.iter().take(8) {
            lines.push(format!("- {entry}"));
        }
    }
    if !intel.recent_failures.is_empty() {
        lines.push(format!("recent_failures: {}", intel.recent_failures.len()));
        for entry in intel.recent_failures.iter().take(8) {
            lines.push(format!("- {entry}"));
        }
    }
    if !intel.output_tail.trim().is_empty() {
        lines.push("output_tail:".into());
        lines.push(intel.output_tail.trim().to_string());
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_workspace_memory(memory: &jaymi_context::LlmWorkspaceMemory) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(objective) = &memory.coding_objective {
        lines.push(format!("coding_objective: {objective}"));
    }
    if !memory.recent_edits.is_empty() {
        lines.push(format!("recent_edits: {}", memory.recent_edits.len()));
        for path in memory.recent_edits.iter().take(8) {
            lines.push(format!("- {path}"));
        }
    }
    if !memory.recently_opened.is_empty() {
        lines.push(format!("recently_opened: {}", memory.recently_opened.len()));
        for path in memory.recently_opened.iter().take(8) {
            lines.push(format!("- {path}"));
        }
    }
    if !memory.recent_builds.is_empty() {
        lines.push(format!("recent_builds: {}", memory.recent_builds.len()));
        for entry in memory.recent_builds.iter().take(6) {
            lines.push(format!("- {entry}"));
        }
    }
    if !memory.recent_failures.is_empty() {
        lines.push(format!("recent_failures: {}", memory.recent_failures.len()));
        for entry in memory.recent_failures.iter().take(6) {
            lines.push(format!("- {entry}"));
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_environmental_resolution(context: &LlmContext) -> Option<String> {
    let env = context.providers.environmental.as_ref()?;
    let mut lines = Vec::new();
    lines.push(
        "Use only these Planner-resolved workspace references. Do not invent paths, files, or symbols."
            .into(),
    );
    if env.ambiguous {
        lines.push("ambiguous: true".into());
    }
    if let Some(path) = &env.primary_path {
        lines.push(format!("primary_path: {path}"));
    }
    if let Some(preview) = &env.selection_preview {
        lines.push(format!("selection: {preview}"));
    }
    if let Some(symbol) = &env.symbol {
        lines.push(format!("symbol: {symbol}"));
    }
    if let Some(diagnostic) = &env.diagnostic {
        lines.push(format!("diagnostic: {diagnostic}"));
    }
    if !env.bindings.is_empty() {
        lines.push("bindings:".into());
        for binding in &env.bindings {
            lines.push(format!("- {binding}"));
        }
    }
    if !env.rules.is_empty() {
        lines.push(format!("rules: {}", env.rules.join(", ")));
    }
    Some(lines.join("\n"))
}

fn format_selection(selection: &LlmCurrentSelection) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(path) = &selection.path {
        lines.push(format!("path: {path}"));
    }
    lines.push(format!(
        "range: {}:{}-{}:{}",
        selection.start_line, selection.start_column, selection.end_line, selection.end_column
    ));
    if let Some(text) = &selection.text {
        if !text.trim().is_empty() {
            lines.push(format!("text:\n{}", text.trim_end()));
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_search(search: &LlmSearchResults) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(hint) = &search.hint {
        let mut parts = Vec::new();
        if hint.structured_query_pending {
            parts.push("structured_query_pending".into());
        }
        if let Some(preview) = &hint.query_preview {
            parts.push(format!("query={preview}"));
        }
        if let Some(count) = hint.project_indexed_documents {
            parts.push(format!("indexed_documents={count}"));
        }
        if !parts.is_empty() {
            lines.push(format!("hint: {}", parts.join(" ")));
        }
    }
    for hit in &search.hits {
        let mut parts = vec![format!("[{}] {}", hit.item_id, hit.title)];
        if let Some(path) = &hit.path {
            parts.push(format!("path={path}"));
        }
        if let Some(score) = hit.score {
            parts.push(format!("score={score}"));
        }
        if let Some(reason) = &hit.match_reason {
            parts.push(format!("reason={reason}"));
        }
        if let Some(preview) = &hit.preview {
            parts.push(format!("preview={}", compact(preview)));
        }
        lines.push(format!("- {}", parts.join(" · ")));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_diagnostics(diagnostics: &LlmDiagnostics) -> Option<String> {
    if diagnostics.diagnostics.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for item in &diagnostics.diagnostics {
        let loc = match (&item.path, item.line, item.column) {
            (Some(path), Some(line), Some(col)) => format!("{path}:{line}:{col}"),
            (Some(path), Some(line), None) => format!("{path}:{line}"),
            (Some(path), None, _) => path.clone(),
            _ => "unknown".into(),
        };
        lines.push(format!("- [{}] {loc}: {}", item.severity, item.message));
    }
    Some(lines.join("\n"))
}

fn format_permissions(permissions: &LlmPermissions) -> Option<String> {
    if permissions.entries.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for entry in &permissions.entries {
        let mut line = format!(
            "- {}/{} → {}",
            entry.category, entry.action, entry.decision
        );
        if let Some(resource) = &entry.resource {
            line.push_str(&format!(" ({resource})"));
        }
        if let Some(explanation) = &entry.explanation {
            line.push_str(&format!(" — {explanation}"));
        }
        lines.push(line);
    }
    Some(lines.join("\n"))
}

fn format_capabilities(caps: &LlmActiveCapabilities) -> Option<String> {
    if caps.capability_ids.is_empty() {
        return None;
    }
    Some(caps.capability_ids.join(", "))
}

fn format_planner_metadata(context: &LlmContext) -> String {
    let mut lines = vec![
        format!("schema_version: {}", context.schema_version),
        format!("assemble_generation: {}", context.assemble_generation),
    ];
    append_provider_metadata(&mut lines, &context.providers);
    lines.join("\n")
}

fn append_provider_metadata(lines: &mut Vec<String>, meta: &LlmProviderMetadata) {
    if !meta.sources.is_empty() {
        lines.push(format!("sources: {}", meta.sources.join(", ")));
    }
    for note in &meta.notes {
        lines.push(format!("note: {note}"));
    }
    if let Some(budget) = &meta.budget {
        lines.push(format!(
            "context_budget: used={} max={} tokens~{}",
            budget.used_characters, budget.max_characters, budget.estimated_tokens
        ));
    }
}

fn format_user_request(context: &LlmContext, goal: &str) -> String {
    let mut lines = Vec::new();
    let goal = goal.trim();
    if !goal.is_empty() {
        lines.push(format!("goal: {goal}"));
    }
    if let Some(LlmSectionContent::UserRequest(req)) =
        find_content(context, LlmSectionId::UserRequest)
    {
        append_user_request_flags(&mut lines, req);
    }
    if lines.is_empty() {
        lines.push("goal:".into());
    }
    lines.join("\n")
}

fn append_user_request_flags(lines: &mut Vec<String>, req: &LlmUserRequest) {
    if !req.content_preview.trim().is_empty() {
        lines.push(format!("preview: {}", req.content_preview.trim()));
    }
    let mut flags = Vec::new();
    if req.has_directory {
        flags.push("directory");
    }
    if req.has_file {
        flags.push("file");
    }
    if req.has_write_file {
        flags.push("write_file");
    }
    if req.has_search {
        flags.push("search");
    }
    if req.has_project_knowledge {
        flags.push("project_knowledge");
    }
    if req.has_terminal {
        flags.push("terminal");
    }
    if req.has_git {
        flags.push("git");
    }
    if req.has_lsp {
        flags.push("lsp");
    }
    if req.has_discover_or_index {
        flags.push("discover_or_index");
    }
    if req.has_project_session {
        flags.push("project_session");
    }
    if !flags.is_empty() {
        lines.push(format!("structured: {}", flags.join(", ")));
    }
}

fn compact(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 160 {
        truncate_chars(&flat, 160)
    } else {
        flat
    }
}
