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
        // Diagnostics describe the prompt actually delivered to providers.
        adapted.seal_for_delivery(&self.budget, history.len());
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
                    if ws || files {
                        DraftSection::filtered(
                            id,
                            "active_workspace/open_files present but empty after format",
                        )
                    } else {
                        DraftSection::excluded(
                            id,
                            "active_workspace and open_files absent from LlmContext",
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
        LlmSectionId::ActiveWorkspace | LlmSectionId::OpenFiles => PromptSectionId::WorkspaceState,
        LlmSectionId::CurrentFile => PromptSectionId::CurrentFile,
        LlmSectionId::CurrentSelection => PromptSectionId::Selection,
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
            let (disposition, note) = match draft {
                Some(draft) => {
                    let note = if llm_id == LlmSectionId::OpenFiles
                        || llm_id == LlmSectionId::ActiveWorkspace
                    {
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
                    (draft.disposition, note)
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
    lines.join("\n")
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
