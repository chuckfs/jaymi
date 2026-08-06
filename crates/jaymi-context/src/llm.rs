//! LLM-facing Context API — structured views of a [`ContextBundle`].
//!
//! Converts an assembled bundle into a stable, deterministically serializable
//! representation for future language-model consumers.
//!
//! This module does **not** call models and does **not** build prompts.
//! LLMs should consume [`LlmContext`] (via [`ContextEngine::to_llm_context`] /
//! [`LlmContext::from_bundle`]) instead of querying Memory, Project, Search, or
//! other subsystems directly.

use std::collections::BTreeMap;

use jaymi_core::{JaymiError, JaymiResult};
use jaymi_memory_engine::PromotionAskDecision;
use serde::Serialize;

use crate::bundle::{
    ActiveCapabilitiesSection, ActiveProjectSection, ActiveWorkspaceSection, BudgetReport,
    ContextBundle, ContextSource, ConversationSection, CurrentFileSection,
    CurrentSelectionSection, DiagnosticsSection, MemoryResultsSection, OpenFilesSection,
    PermissionsSection, PlannerMetadataSection, SearchResultsSection, UserRequestMetadataSection,
};

/// Schema version for the LLM-facing Context API.
///
/// Bump when making breaking changes to [`LlmContext`] field layout or
/// section ids. Additive fields / extension keys do not require a bump when
/// consumers treat unknown keys as ignorable.
pub const LLM_CONTEXT_SCHEMA_VERSION: u32 = 1;

/// Canonical section identifiers in stable emission order.
///
/// Every [`LlmContext`] emits sections in this order. New sections append at
/// the end (and bump [`LLM_CONTEXT_SCHEMA_VERSION`] when introduced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmSectionId {
    /// Inbound user request metadata.
    UserRequest,
    /// Active conversation summary.
    Conversation,
    /// Open project identity (+ detail summary).
    ActiveProject,
    /// Active UX workspace kind.
    ActiveWorkspace,
    /// Focused editor file.
    CurrentFile,
    /// Editor selection.
    CurrentSelection,
    /// Open editor tabs.
    OpenFiles,
    /// Search coordination / attached hits.
    SearchResults,
    /// Retrieved memories and promotion suggestions.
    MemoryResults,
    /// Attached diagnostics.
    Diagnostics,
    /// Permission grants / decisions.
    Permissions,
    /// Active capability ids.
    ActiveCapabilities,
}

impl LlmSectionId {
    /// Stable string id used in serialized output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserRequest => "user_request",
            Self::Conversation => "conversation",
            Self::ActiveProject => "active_project",
            Self::ActiveWorkspace => "active_workspace",
            Self::CurrentFile => "current_file",
            Self::CurrentSelection => "current_selection",
            Self::OpenFiles => "open_files",
            Self::SearchResults => "search_results",
            Self::MemoryResults => "memory_results",
            Self::Diagnostics => "diagnostics",
            Self::Permissions => "permissions",
            Self::ActiveCapabilities => "active_capabilities",
        }
    }

    /// Fixed section emission order (schema v1).
    pub const ORDER: &'static [Self] = &[
        Self::UserRequest,
        Self::Conversation,
        Self::ActiveProject,
        Self::ActiveWorkspace,
        Self::CurrentFile,
        Self::CurrentSelection,
        Self::OpenFiles,
        Self::SearchResults,
        Self::MemoryResults,
        Self::Diagnostics,
        Self::Permissions,
        Self::ActiveCapabilities,
    ];
}

/// Structured ContextBundle representation for language-model consumers.
///
/// Stable field order, deterministic JSON, provider metadata, and an
/// extensions map for future additive payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmContext {
    /// Schema version ([`LLM_CONTEXT_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Assemble generation from the source bundle.
    pub assemble_generation: u64,
    /// Provider / source / budget metadata (not prompt text).
    pub providers: LlmProviderMetadata,
    /// Bundle sections in [`LlmSectionId::ORDER`].
    pub sections: Vec<LlmContextSection>,
    /// Reserved additive payloads keyed by stable extension ids.
    ///
    /// `BTreeMap` keeps extension key order deterministic in JSON.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl LlmContext {
    /// Convert a [`ContextBundle`] into the LLM-facing representation.
    ///
    /// Pure data transform — no model calls, no prompt construction.
    pub fn from_bundle(bundle: &ContextBundle) -> Self {
        Self {
            schema_version: LLM_CONTEXT_SCHEMA_VERSION,
            assemble_generation: bundle.assemble_generation(),
            providers: LlmProviderMetadata::from_planner(bundle.planner_metadata()),
            sections: build_sections(bundle),
            extensions: BTreeMap::new(),
        }
    }

    /// Deterministic JSON serialization (stable field / key order).
    pub fn to_json(&self) -> JaymiResult<String> {
        serde_json::to_string(self).map_err(|error| {
            JaymiError::new(format!("failed to serialize LlmContext: {error}"))
        })
    }

    /// Pretty-printed deterministic JSON (stable field / key order).
    pub fn to_json_pretty(&self) -> JaymiResult<String> {
        serde_json::to_string_pretty(self).map_err(|error| {
            JaymiError::new(format!("failed to serialize LlmContext: {error}"))
        })
    }

    /// JSON value form (useful for embedding into larger envelopes later).
    pub fn to_json_value(&self) -> JaymiResult<serde_json::Value> {
        serde_json::to_value(self).map_err(|error| {
            JaymiError::new(format!("failed to serialize LlmContext: {error}"))
        })
    }

    /// Insert a future extension payload under a stable key.
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }
}

/// Provider / assembly metadata accompanying the LLM context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmProviderMetadata {
    /// Contributing [`ContextSource`] labels (assemble order preserved).
    pub sources: Vec<String>,
    /// Provider ids / notes recorded during assemble (diagnostics transparency).
    pub notes: Vec<String>,
    /// Budget accounting when recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<LlmBudgetView>,
}

impl LlmProviderMetadata {
    fn from_planner(meta: &PlannerMetadataSection) -> Self {
        Self {
            sources: meta
                .sources
                .iter()
                .map(|source| source.as_str().to_string())
                .collect(),
            notes: meta.notes.clone(),
            budget: meta.budget.as_ref().map(LlmBudgetView::from_report),
        }
    }
}

/// Budget summary exposed to LLM consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmBudgetView {
    pub max_characters: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<usize>,
    pub used_characters: usize,
    pub estimated_tokens: usize,
    pub truncated_providers: Vec<String>,
    pub skipped_budget: Vec<String>,
    pub summaries: Vec<String>,
}

impl LlmBudgetView {
    fn from_report(report: &BudgetReport) -> Self {
        Self {
            max_characters: report.max_characters,
            max_tokens: report.max_tokens,
            used_characters: report.used_characters,
            estimated_tokens: report.estimated_tokens,
            truncated_providers: report.truncated_providers.clone(),
            skipped_budget: report.skipped_budget.clone(),
            summaries: report.summaries.clone(),
        }
    }
}

/// One ordered section in an [`LlmContext`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmContextSection {
    /// Stable section id.
    pub id: LlmSectionId,
    /// True when the section carries meaningful content.
    pub present: bool,
    /// Context sources associated with this section (sorted for stability).
    pub sources: Vec<String>,
    /// Typed section payload.
    pub content: LlmSectionContent,
}

/// Typed section payloads (tagged for extensible deserialization later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum LlmSectionContent {
    /// Empty / omitted payload when `present` is false.
    Empty,
    UserRequest(LlmUserRequest),
    Conversation(LlmConversation),
    ActiveProject(LlmActiveProject),
    ActiveWorkspace(LlmActiveWorkspace),
    CurrentFile(LlmCurrentFile),
    CurrentSelection(LlmCurrentSelection),
    OpenFiles(LlmOpenFiles),
    SearchResults(LlmSearchResults),
    MemoryResults(LlmMemoryResults),
    Diagnostics(LlmDiagnostics),
    Permissions(LlmPermissions),
    ActiveCapabilities(LlmActiveCapabilities),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmUserRequest {
    pub content_preview: String,
    pub has_directory: bool,
    pub has_file: bool,
    pub has_write_file: bool,
    pub has_search: bool,
    pub has_project_knowledge: bool,
    pub has_terminal: bool,
    pub has_git: bool,
    pub has_lsp: bool,
    pub has_discover_or_index: bool,
    pub has_project_session: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmConversation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmActiveProject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_directory: Option<String>,
    /// Compact project detail summary (not a full ProjectContext dump).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<LlmProjectDetailSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmProjectDetailSummary {
    pub is_open: bool,
    pub entry_count: usize,
    pub indexed_files: usize,
    pub conversations: usize,
    pub important_documents: usize,
    pub documentation: usize,
    pub recent_work: usize,
    pub architecture_documents: usize,
    pub parsed_content: usize,
    pub tasks: usize,
    pub decisions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmActiveWorkspace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmCurrentFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub dirty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmCurrentSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmOpenFiles {
    pub files: Vec<LlmOpenFileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmOpenFileEntry {
    pub path: String,
    pub dirty: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmSearchResults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<LlmSearchHint>,
    pub hits: Vec<LlmSearchHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmSearchHint {
    pub structured_query_pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_indexed_documents: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmSearchHit {
    pub item_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmMemoryResults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub candidate_count: usize,
    pub truncated: bool,
    pub memories: Vec<LlmMemoryItem>,
    pub promotion_suggestions: Vec<LlmPromotionSuggestion>,
    pub promotion_ask: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmMemoryItem {
    pub id: String,
    pub scope: String,
    pub summary: String,
    pub content: String,
    pub score: u32,
    pub reasons: Vec<String>,
    pub why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub importance: u32,
    pub confidence: u32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmPromotionSuggestion {
    pub memory_id: String,
    pub summary: String,
    pub from: String,
    pub to: String,
    pub reason: String,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmDiagnostics {
    pub diagnostics: Vec<LlmDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmDiagnostic {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmPermissions {
    pub entries: Vec<LlmPermissionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmPermissionEntry {
    pub category: String,
    pub action: String,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmActiveCapabilities {
    pub capability_ids: Vec<String>,
}

fn build_sections(bundle: &ContextBundle) -> Vec<LlmContextSection> {
    LlmSectionId::ORDER
        .iter()
        .copied()
        .map(|id| section_for(id, bundle))
        .collect()
}

fn section_for(id: LlmSectionId, bundle: &ContextBundle) -> LlmContextSection {
    let (present, content) = match id {
        LlmSectionId::UserRequest => {
            let section = bundle.user_request();
            let present = !section.content_preview.is_empty()
                || section.has_directory
                || section.has_file
                || section.has_write_file
                || section.has_search
                || section.has_project_knowledge
                || section.has_terminal
                || section.has_git
                || section.has_lsp
                || section.has_discover_or_index
                || section.has_project_session;
            (
                present,
                if present {
                    LlmSectionContent::UserRequest(map_user_request(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::Conversation => {
            let section = bundle.conversation();
            let present = section.id.is_some()
                || section.title.is_some()
                || section.message_count.is_some();
            (
                present,
                if present {
                    LlmSectionContent::Conversation(map_conversation(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::ActiveProject => {
            let section = bundle.active_project();
            let present = section.project_id.is_some() || section.detail.is_some();
            (
                present,
                if present {
                    LlmSectionContent::ActiveProject(map_active_project(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::ActiveWorkspace => {
            let section = bundle.active_workspace();
            let present = section.kind_id.is_some();
            (
                present,
                if present {
                    LlmSectionContent::ActiveWorkspace(map_workspace(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::CurrentFile => {
            let section = bundle.current_file();
            let present = section.path.is_some();
            (
                present,
                if present {
                    LlmSectionContent::CurrentFile(map_current_file(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::CurrentSelection => {
            let section = bundle.current_selection();
            let present = section.path.is_some() || section.text.is_some();
            (
                present,
                if present {
                    LlmSectionContent::CurrentSelection(map_selection(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::OpenFiles => {
            let section = bundle.open_files();
            let present = !section.files.is_empty();
            (
                present,
                if present {
                    LlmSectionContent::OpenFiles(map_open_files(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::SearchResults => {
            let section = bundle.search_results();
            let present = section.hint.is_some() || !section.hits.is_empty();
            (
                present,
                if present {
                    LlmSectionContent::SearchResults(map_search(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::MemoryResults => {
            let section = bundle.memory_results();
            let present = !section.memory.is_empty()
                || !section.promotion_suggestions.is_empty()
                || section.memory.candidate_count > 0;
            (
                present,
                if present {
                    LlmSectionContent::MemoryResults(map_memory(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::Diagnostics => {
            let section = bundle.diagnostics();
            let present = !section.diagnostics.is_empty();
            (
                present,
                if present {
                    LlmSectionContent::Diagnostics(map_diagnostics(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::Permissions => {
            let section = bundle.permissions();
            let present = !section.entries.is_empty();
            (
                present,
                if present {
                    LlmSectionContent::Permissions(map_permissions(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
        LlmSectionId::ActiveCapabilities => {
            let section = bundle.active_capabilities();
            let present = !section.capability_ids.is_empty();
            (
                present,
                if present {
                    LlmSectionContent::ActiveCapabilities(map_capabilities(section))
                } else {
                    LlmSectionContent::Empty
                },
            )
        }
    };

    LlmContextSection {
        id,
        present,
        sources: sources_for_section(id),
        content,
    }
}

fn sources_for_section(id: LlmSectionId) -> Vec<String> {
    let sources: &[ContextSource] = match id {
        LlmSectionId::UserRequest => &[ContextSource::UserRequest],
        LlmSectionId::Conversation => &[ContextSource::PreviousConversation],
        LlmSectionId::ActiveProject => &[ContextSource::ActiveProject],
        LlmSectionId::ActiveWorkspace => &[ContextSource::ActiveWorkspace],
        LlmSectionId::CurrentFile
        | LlmSectionId::CurrentSelection
        | LlmSectionId::OpenFiles => &[ContextSource::EditorState],
        LlmSectionId::SearchResults => &[ContextSource::SearchResults],
        LlmSectionId::MemoryResults => &[
            ContextSource::RetrievedMemories,
            ContextSource::PromotionSuggestions,
        ],
        LlmSectionId::Diagnostics => &[ContextSource::Diagnostics],
        LlmSectionId::Permissions => &[ContextSource::Permissions],
        LlmSectionId::ActiveCapabilities => &[ContextSource::ActiveCapabilities],
    };
    let mut labels: Vec<String> = sources.iter().map(|s| s.as_str().to_string()).collect();
    labels.sort();
    labels
}

fn map_user_request(section: &UserRequestMetadataSection) -> LlmUserRequest {
    LlmUserRequest {
        content_preview: section.content_preview.clone(),
        has_directory: section.has_directory,
        has_file: section.has_file,
        has_write_file: section.has_write_file,
        has_search: section.has_search,
        has_project_knowledge: section.has_project_knowledge,
        has_terminal: section.has_terminal,
        has_git: section.has_git,
        has_lsp: section.has_lsp,
        has_discover_or_index: section.has_discover_or_index,
        has_project_session: section.has_project_session,
    }
}

fn map_conversation(section: &ConversationSection) -> LlmConversation {
    LlmConversation {
        id: section.id.clone(),
        title: section.title.clone(),
        status: section.status.clone(),
        project_id: section.project_id.clone(),
        message_count: section.message_count,
    }
}

fn map_active_project(section: &ActiveProjectSection) -> LlmActiveProject {
    LlmActiveProject {
        project_id: section.project_id.clone(),
        name: section.name.clone(),
        root_directory: section.root_directory.clone(),
        detail: section.detail.as_ref().map(|detail| LlmProjectDetailSummary {
            is_open: detail.is_open,
            entry_count: detail.entry_count(),
            indexed_files: detail.indexed_files.len(),
            conversations: detail.conversations.len(),
            important_documents: detail.important_documents.len(),
            documentation: detail.documentation.len(),
            recent_work: detail.recent_work.len(),
            architecture_documents: detail.architecture_documents.len(),
            parsed_content: detail.parsed_content.len(),
            tasks: detail.tasks.len(),
            decisions: detail.decisions.len(),
        }),
    }
}

fn map_workspace(section: &ActiveWorkspaceSection) -> LlmActiveWorkspace {
    LlmActiveWorkspace {
        kind_id: section.kind_id.clone(),
    }
}

fn map_current_file(section: &CurrentFileSection) -> LlmCurrentFile {
    LlmCurrentFile {
        path: section.path.clone(),
        dirty: section.dirty,
        language: section.language.clone(),
    }
}

fn map_selection(section: &CurrentSelectionSection) -> LlmCurrentSelection {
    LlmCurrentSelection {
        path: section.path.clone(),
        start_line: section.start_line,
        start_column: section.start_column,
        end_line: section.end_line,
        end_column: section.end_column,
        text: section.text.clone(),
    }
}

fn map_open_files(section: &OpenFilesSection) -> LlmOpenFiles {
    LlmOpenFiles {
        files: section
            .files
            .iter()
            .map(|file| LlmOpenFileEntry {
                path: file.path.clone(),
                dirty: file.dirty,
                active: file.active,
            })
            .collect(),
    }
}

fn map_search(section: &SearchResultsSection) -> LlmSearchResults {
    LlmSearchResults {
        hint: section.hint.as_ref().map(|hint| LlmSearchHint {
            structured_query_pending: hint.structured_query_pending,
            query_preview: hint.query_preview.clone(),
            project_indexed_documents: hint.project_indexed_documents,
        }),
        hits: section
            .hits
            .iter()
            .map(|hit| LlmSearchHit {
                item_id: hit.item_id.clone(),
                title: hit.title.clone(),
                path: hit.path.clone(),
                score: hit.score,
                match_reason: hit.match_reason.clone(),
                preview: hit.preview.clone(),
                line: hit.line,
                column: hit.column,
            })
            .collect(),
    }
}

fn map_memory(section: &MemoryResultsSection) -> LlmMemoryResults {
    LlmMemoryResults {
        project_id: section.memory.project_id.clone(),
        conversation_id: section.memory.conversation_id.clone(),
        candidate_count: section.memory.candidate_count,
        truncated: section.memory.truncated,
        memories: section
            .memory
            .memories
            .iter()
            .map(|item| LlmMemoryItem {
                id: item.record.id.as_str().to_string(),
                scope: item.record.scope.as_str().to_string(),
                summary: item.record.summary.clone(),
                content: item.record.content.clone(),
                score: item.score,
                reasons: item
                    .reasons
                    .iter()
                    .map(|reason| reason.as_str().to_string())
                    .collect(),
                why: item.why.clone(),
                kind: item.record.kind.clone(),
                project_id: item.record.project_id.clone(),
                conversation_id: item.record.conversation_id.clone(),
                importance: item.record.importance,
                confidence: item.record.confidence,
                tags: item.record.tags.clone(),
            })
            .collect(),
        promotion_suggestions: section
            .promotion_suggestions
            .iter()
            .map(|suggestion| LlmPromotionSuggestion {
                memory_id: suggestion.memory_id.clone(),
                summary: suggestion.summary.clone(),
                from: suggestion.from.as_str().to_string(),
                to: suggestion.to.as_str().to_string(),
                reason: suggestion.reason.clone(),
                score: suggestion.score,
            })
            .collect(),
        promotion_ask: match section.promotion_ask {
            PromotionAskDecision::AskUser => "ask_user".to_string(),
            PromotionAskDecision::Defer => "defer".to_string(),
        },
    }
}

fn map_diagnostics(section: &DiagnosticsSection) -> LlmDiagnostics {
    LlmDiagnostics {
        diagnostics: section
            .diagnostics
            .iter()
            .map(|diag| LlmDiagnostic {
                path: diag.path.clone(),
                severity: diag.severity.clone(),
                message: diag.message.clone(),
                line: diag.line,
                column: diag.column,
                source: diag.source.clone(),
            })
            .collect(),
    }
}

fn map_permissions(section: &PermissionsSection) -> LlmPermissions {
    LlmPermissions {
        entries: section
            .entries
            .iter()
            .map(|entry| LlmPermissionEntry {
                category: entry.category.clone(),
                action: entry.action.clone(),
                decision: entry.decision.clone(),
                resource: entry.resource.clone(),
                explanation: entry.explanation.clone(),
            })
            .collect(),
    }
}

fn map_capabilities(section: &ActiveCapabilitiesSection) -> LlmActiveCapabilities {
    LlmActiveCapabilities {
        capability_ids: section.capability_ids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{
        ContextBundleBuilder, ContextSource, CurrentFileSection, OpenFileEntry, OpenFilesSection,
        PlannerMetadataSection, UserRequestMetadataSection,
    };

    fn sample_bundle() -> ContextBundle {
        ContextBundleBuilder::new()
            .user_request(UserRequestMetadataSection {
                content_preview: "hello llm".into(),
                has_file: true,
                ..UserRequestMetadataSection::default()
            })
            .current_file(CurrentFileSection {
                path: Some("/tmp/a.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            })
            .open_files(OpenFilesSection {
                files: vec![OpenFileEntry {
                    path: "/tmp/a.rs".into(),
                    dirty: true,
                    active: true,
                }],
            })
            .active_workspace(crate::ActiveWorkspaceSection {
                kind_id: Some("coding".into()),
            })
            .planner_metadata(PlannerMetadataSection {
                assemble_generation: 7,
                sources: vec![
                    ContextSource::UserRequest,
                    ContextSource::EditorState,
                    ContextSource::ActiveWorkspace,
                ],
                notes: vec!["providers contributed=3".into()],
                budget: Some(BudgetReport {
                    max_characters: 32_000,
                    max_tokens: None,
                    used_characters: 120,
                    estimated_tokens: 30,
                    truncated_providers: vec![],
                    skipped_budget: vec![],
                    summaries: vec![],
                }),
                policy: None,
            })
            .build()
    }

    #[test]
    fn sections_emit_in_stable_order() {
        let llm = LlmContext::from_bundle(&sample_bundle());
        let ids: Vec<_> = llm.sections.iter().map(|section| section.id).collect();
        assert_eq!(ids, LlmSectionId::ORDER.to_vec());
        assert_eq!(llm.schema_version, LLM_CONTEXT_SCHEMA_VERSION);
        assert_eq!(llm.assemble_generation, 7);
        assert!(llm.sections.iter().any(|s| {
            s.id == LlmSectionId::CurrentFile && s.present
        }));
        assert!(llm
            .providers
            .sources
            .iter()
            .any(|source| source == "editor_state"));
    }

    #[test]
    fn json_serialization_is_deterministic() {
        let llm = LlmContext::from_bundle(&sample_bundle())
            .with_extension("future.example", serde_json::json!({"a": 1, "b": 2}));
        let first = llm.to_json().unwrap();
        let second = llm.to_json().unwrap();
        assert_eq!(first, second);
        assert!(first.contains("\"schema_version\":1"));
        assert!(first.contains("\"id\":\"current_file\"") || first.contains("\"id\":\"CurrentFile\"") || first.contains("current_file"));
        // snake_case enum serialization
        assert!(first.contains("current_file"));
        assert!(first.contains("future.example"));
        // BTreeMap keeps extension object keys sorted
        let value = llm.to_json_value().unwrap();
        let extensions = value.get("extensions").unwrap().as_object().unwrap();
        let keys: Vec<&String> = extensions.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn section_source_labels_are_sorted() {
        let llm = LlmContext::from_bundle(&sample_bundle());
        let memory = llm
            .sections
            .iter()
            .find(|section| section.id == LlmSectionId::MemoryResults)
            .unwrap();
        assert_eq!(
            memory.sources,
            vec![
                "promotion_suggestions".to_string(),
                "retrieved_memories".to_string()
            ]
        );
    }
}
