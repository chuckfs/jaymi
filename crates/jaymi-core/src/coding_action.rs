//! Typed Coding Actions — Planner entry points from the Coding toolbar.
//!
//! The UI emits [`CodingAction`] only. The Planner owns routing; Workspace
//! Intelligence supplies context. These are request-level typed intents, not a
//! parallel [`crate::IntentId`] taxonomy — Decision Engine maps them onto
//! conversational Reasoning, Search, or reviewed terminal execution.

use serde::{Deserialize, Serialize};

/// First-class Coding toolbar / Planner coding intents (Sprint C0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingAction {
    /// Explain the current editor selection (WI-bound).
    ExplainSelection,
    /// Explain the active file when there is no selection.
    ExplainFile,
    /// Start an editing conversation; ask what change is desired.
    EditSelection,
    /// Propose a refactoring without applying edits.
    RefactorSelection,
    /// Semantic workspace search (query from selection or conversation).
    SearchWorkspace,
    /// Propose a reviewed project run before tools execute.
    RunProject,
    /// Planner-generated Coding Actions menu (More).
    OpenCodingActions,
}

impl CodingAction {
    /// Stable snake_case id for diagnostics and tests.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplainSelection => "explain_selection",
            Self::ExplainFile => "explain_file",
            Self::EditSelection => "edit_selection",
            Self::RefactorSelection => "refactor_selection",
            Self::SearchWorkspace => "search_workspace",
            Self::RunProject => "run_project",
            Self::OpenCodingActions => "open_coding_actions",
        }
    }

    /// Conversation-visible user turn text (Conversation First).
    pub fn conversation_text(self) -> &'static str {
        match self {
            Self::ExplainSelection => "Explain the current selection.",
            Self::ExplainFile => "Explain the current file.",
            Self::EditSelection => {
                "I'd like to edit the current selection. What change should I make?"
            }
            Self::RefactorSelection => {
                "Propose a refactoring for the current selection. Do not edit files yet — describe the proposal only."
            }
            Self::SearchWorkspace => "Search the workspace.",
            Self::RunProject => "Run the project.",
            Self::OpenCodingActions => "Show Coding Actions.",
        }
    }

    /// True when this action is answered by conversational Reasoning (not tools).
    pub fn is_conversational(self) -> bool {
        matches!(
            self,
            Self::ExplainSelection
                | Self::ExplainFile
                | Self::EditSelection
                | Self::RefactorSelection
                | Self::OpenCodingActions
        )
    }
}

impl std::fmt::Display for CodingAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_stable() {
        assert_eq!(CodingAction::ExplainSelection.as_str(), "explain_selection");
        assert_eq!(CodingAction::OpenCodingActions.as_str(), "open_coding_actions");
        assert!(CodingAction::EditSelection.is_conversational());
        assert!(!CodingAction::SearchWorkspace.is_conversational());
    }
}
