//! Prompt section identifiers and stable emission order.

use serde::{Deserialize, Serialize};

/// First-class prompt sections assembled by [`super::PromptBuilder`].
///
/// Order is architectural — see [`PromptSectionId::ORDER`].
///
/// Every [`jaymi_context::LlmSectionId`] maps to a prompt section (or an
/// intentional fold documented on [`PromptSectionId::llm_sources`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionId {
    /// Stable system / role instructions.
    SystemInstructions,
    /// Conversation continuity (history + bundle conversation metadata).
    Conversation,
    /// Retrieved memories relevant to the request.
    RelevantMemories,
    /// Active project identity and summary.
    ActiveProject,
    /// Intentional fold: `ActiveWorkspace` + `OpenFiles` + maintenance snapshots
    /// (`GitStatus` / `WorkspaceInventory` / `FileSummaries`).
    WorkspaceState,
    /// Focused editor file.
    CurrentFile,
    /// Editor selection.
    Selection,
    /// Search coordination / attached hits.
    SearchResults,
    /// Attached diagnostics.
    Diagnostics,
    /// Permission grants / decisions.
    Permissions,
    /// Active capability ids.
    Capabilities,
    /// Assemble / provider metadata from Context.
    PlannerMetadata,
    /// Inbound user request / goal.
    UserRequest,
}

impl PromptSectionId {
    /// Stable emission order for the default prompt layout.
    pub const ORDER: &'static [Self] = &[
        Self::SystemInstructions,
        Self::Conversation,
        Self::RelevantMemories,
        Self::ActiveProject,
        Self::WorkspaceState,
        Self::CurrentFile,
        Self::Selection,
        Self::SearchResults,
        Self::Diagnostics,
        Self::Permissions,
        Self::Capabilities,
        Self::PlannerMetadata,
        Self::UserRequest,
    ];

    /// Stable label used in formatted prompts and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SystemInstructions => "system_instructions",
            Self::Conversation => "conversation",
            Self::RelevantMemories => "relevant_memories",
            Self::ActiveProject => "active_project",
            Self::WorkspaceState => "workspace_state",
            Self::CurrentFile => "current_file",
            Self::Selection => "selection",
            Self::SearchResults => "search_results",
            Self::Diagnostics => "diagnostics",
            Self::Permissions => "permissions",
            Self::Capabilities => "capabilities",
            Self::PlannerMetadata => "planner_metadata",
            Self::UserRequest => "user_request",
        }
    }

    /// Human-readable section heading (provider-independent).
    pub fn heading(self) -> &'static str {
        match self {
            Self::SystemInstructions => "System Instructions",
            Self::Conversation => "Conversation",
            Self::RelevantMemories => "Relevant Memories",
            Self::ActiveProject => "Active Project",
            Self::WorkspaceState => "Workspace State",
            Self::CurrentFile => "Current File",
            Self::Selection => "Selection",
            Self::SearchResults => "Search Results",
            Self::Diagnostics => "Diagnostics",
            Self::Permissions => "Permissions",
            Self::Capabilities => "Capabilities",
            Self::PlannerMetadata => "Planner Metadata",
            Self::UserRequest => "User Request",
        }
    }

    /// Truncation priority — higher values are kept longer when budget is tight.
    pub fn retention_priority(self) -> u8 {
        match self {
            Self::SystemInstructions => 100,
            Self::UserRequest => 95,
            Self::Conversation => 85,
            Self::ActiveProject => 75,
            Self::CurrentFile => 70,
            Self::Selection => 65,
            Self::Capabilities => 55,
            Self::WorkspaceState => 50,
            Self::SearchResults => 45,
            Self::Diagnostics => 40,
            Self::Permissions => 35,
            Self::RelevantMemories => 30,
            Self::PlannerMetadata => 20,
        }
    }

    /// `LlmSectionId` sources that feed this prompt section (`as_str` labels).
    ///
    /// Empty for engine-owned sections (system instructions).
    pub fn llm_sources(self) -> &'static [&'static str] {
        match self {
            Self::SystemInstructions => &[],
            Self::Conversation => &["conversation"],
            Self::RelevantMemories => &["memory_results"],
            Self::ActiveProject => &["active_project"],
            Self::WorkspaceState => &["active_workspace", "open_files", "git_status", "workspace_inventory", "file_summaries"],
            Self::CurrentFile => &["current_file"],
            Self::Selection => &["current_selection"],
            Self::SearchResults => &["search_results"],
            Self::Diagnostics => &["diagnostics"],
            Self::Permissions => &["permissions"],
            Self::Capabilities => &["active_capabilities"],
            Self::PlannerMetadata => &[], // from LlmContext.providers, not a section
            Self::UserRequest => &["user_request"],
        }
    }
}

impl std::fmt::Display for PromptSectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Why a prompt section is or is not in the final prompt.
///
/// No silent drops: every section in [`PromptSectionId::ORDER`] receives one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionDisposition {
    /// Present in the final prompt at full (post-format) size.
    Included,
    /// Intentionally absent — no usable source data in `LlmContext` / history.
    Excluded,
    /// Present in the final prompt but body shortened to fit budget.
    Truncated,
    /// Source was present but formatter / policy dropped the content.
    Filtered,
    /// Omitted entirely because of prompt budget pressure.
    Budgeted,
}

impl PromptSectionDisposition {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Included => "included",
            Self::Excluded => "excluded",
            Self::Truncated => "truncated",
            Self::Filtered => "filtered",
            Self::Budgeted => "budgeted",
        }
    }
}

impl std::fmt::Display for PromptSectionDisposition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
