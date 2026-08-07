//! Sprint C0.1 — Coding Action Planner responses.
//!
//! Deterministic / honest replies for toolbar Coding Actions when tools are not
//! yet appropriate. Conversational Explain / Edit / Refactor still use Reasoning.

use jaymi_core::CodingAction;

use crate::PlannerResponse;
use jaymi_reasoning::StreamingLifecycle;

/// Deterministic Coding Actions menu (More → OpenCodingActions).
pub fn open_coding_actions_content() -> &'static str {
    "Here are Coding Actions available from the toolbar:\n\n\
     • **Explain** — explain the current selection, or the active file when nothing is selected\n\
     • **Edit** — start an editing conversation; I'll ask what change you want\n\
     • **Refactor** — propose a refactoring without editing files yet\n\
     • **Search** — semantic workspace search (uses the selection as the query when present)\n\
     • **Run** — propose a project run command as a reviewed execution request\n\n\
     Use a toolbar button, or tell me which action you want next."
}

/// Honest reply when Search has no query yet.
pub fn search_needs_query_content() -> &'static str {
    "What should I search for in the workspace? Select text in the editor and click Search again, or tell me the query here."
}

/// Honest reply when Run has no suggested command / project cwd.
pub fn run_needs_command_content() -> &'static str {
    "I don't have a project run command ready yet. Open a project (or tell me the command, e.g. `cargo test` / `npm test`), and I'll prepare a reviewed execution request before anything runs."
}

/// Honest reply when ExplainFile has no active file.
pub fn explain_needs_file_content() -> &'static str {
    "There's no active file to explain yet. Open a file in the Coding editor, or select code and use Explain again."
}

/// Honest reply when selection-backed actions lack a selection.
pub fn selection_needed_content(action: CodingAction) -> String {
    match action {
        CodingAction::ExplainSelection => {
            "There's no text selection to explain. Select code in the editor, or open a file and I'll explain the file instead.".into()
        }
        CodingAction::EditSelection | CodingAction::RefactorSelection => {
            format!(
                "There's no text selection for {} yet. Select code in the editor, then use the toolbar again — or describe what you want in chat.",
                action.as_str().replace('_', " ")
            )
        }
        other => format!(
            "Coding Action `{}` isn't ready for this workspace state yet. Tell me what you'd like to do in chat.",
            other.as_str()
        ),
    }
}

/// When a Coding Action can be answered without Reasoning / tools.
pub fn deterministic_coding_action_response(
    action: CodingAction,
) -> Option<PlannerResponse> {
    let content = match action {
        CodingAction::OpenCodingActions => open_coding_actions_content().to_string(),
        CodingAction::SearchWorkspace => search_needs_query_content().to_string(),
        CodingAction::RunProject => run_needs_command_content().to_string(),
        _ => return None,
    };
    Some(PlannerResponse {
        content,
        reasoning_used: false,
        stream_lifecycle: Some(StreamingLifecycle::Idle),
        ..PlannerResponse::default()
    })
}

/// Soft response builder for missing selection / file (honest, never silent).
pub fn soft_coding_action_response(content: impl Into<String>) -> PlannerResponse {
    PlannerResponse {
        content: content.into(),
        reasoning_used: false,
        stream_lifecycle: Some(StreamingLifecycle::Idle),
        ..PlannerResponse::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_is_deterministic() {
        let response =
            deterministic_coding_action_response(CodingAction::OpenCodingActions).unwrap();
        assert!(response.content.contains("Explain"));
        assert!(response.content.contains("Run"));
        assert!(!response.reasoning_used);
    }

    #[test]
    fn explain_edit_are_not_deterministic() {
        assert!(deterministic_coding_action_response(CodingAction::ExplainFile).is_none());
        assert!(deterministic_coding_action_response(CodingAction::EditSelection).is_none());
    }
}
