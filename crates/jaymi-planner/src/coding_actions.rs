//! Sprint C0.1 — Coding Action Planner responses.
//!
//! Deterministic / honest replies for toolbar Coding Actions when tools are not
//! yet appropriate. Conversational Explain / Edit / Refactor still use Reasoning.
//! Sprint **C1.8** — More / OpenCodingActions lists Planner-generated pair actions.

use jaymi_core::CodingAction;

use crate::pair_programming::{baseline_surface, PairProgrammingSurface};
use crate::PlannerResponse;
use jaymi_reasoning::StreamingLifecycle;

/// Deterministic Coding Actions menu (More → OpenCodingActions).
///
/// Prefer [`open_coding_actions_from_surface`] so the menu mirrors Planner state.
pub fn open_coding_actions_content() -> String {
    open_coding_actions_from_surface(&baseline_surface())
}

/// Render the More menu from a Planner-authored pair-programming surface (C1.8).
pub fn open_coding_actions_from_surface(surface: &PairProgrammingSurface) -> String {
    let mut out = String::from(
        "Here are suggested next actions from the Planner (Conversation First):\n\n",
    );
    if let Some(phase) = surface.phase {
        out.push_str(&format!("**Phase:** {}\n\n", phase.label()));
    }
    if surface.suggested_actions.is_empty() {
        out.push_str("• _(none yet — ask me anything in chat)_\n");
    } else {
        for action in &surface.suggested_actions {
            let gate = if action.mutates {
                " _(mutations require Review)_"
            } else {
                ""
            };
            out.push_str(&format!(
                "• **{}** — {}{}\n",
                action.label, action.conversation_text, gate
            ));
        }
    }
    out.push_str(
        "\nUse a Quick Action, or tell me which move you want next. Conversation First remains primary.",
    );
    out
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

/// Soft dead-code orientation (C1.8) — observational, no tools / edits.
pub fn find_dead_code_content() -> &'static str {
    "## Find Dead Code\n\n\
     I can help hunt unused code as an **Understanding** pass — no edits and no tools yet.\n\n\
     ### What I need\n\
     - An open project / file so Workspace Intelligence can orient\n\
     - Optional: a module or symbol you suspect is unused\n\n\
     ### Suggested Next Actions\n\
     - **Explain Architecture** — map modules before pruning\n\
     - **Review Changes** — check whether recent edits orphaned callers\n\
     - **Search** — look for a symbol name across the workspace\n\n\
     Tell me a symbol or path to inspect, or ask me to explain the architecture first."
}

/// When a Coding Action can be answered without Reasoning / tools.
pub fn deterministic_coding_action_response(
    action: CodingAction,
) -> Option<PlannerResponse> {
    deterministic_coding_action_response_with_surface(action, &baseline_surface())
}

/// Like [`deterministic_coding_action_response`], with a Planner-authored surface.
pub fn deterministic_coding_action_response_with_surface(
    action: CodingAction,
    surface: &PairProgrammingSurface,
) -> Option<PlannerResponse> {
    let content = match action {
        CodingAction::OpenCodingActions => open_coding_actions_from_surface(surface),
        CodingAction::SearchWorkspace => search_needs_query_content().to_string(),
        CodingAction::RunProject => run_needs_command_content().to_string(),
        _ => return None,
    };
    Some(PlannerResponse {
        content,
        reasoning_used: false,
        stream_lifecycle: Some(StreamingLifecycle::Idle),
        pair_programming: Some(surface.clone()),
        ..PlannerResponse::default()
    })
}

/// Soft response builder for missing selection / file (honest, never silent).
pub fn soft_coding_action_response(content: impl Into<String>) -> PlannerResponse {
    PlannerResponse {
        content: content.into(),
        reasoning_used: false,
        stream_lifecycle: Some(StreamingLifecycle::Idle),
        pair_programming: Some(baseline_surface()),
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
        assert!(
            response.content.contains("Explain Architecture") || response.content.contains("Explain")
        );
        assert!(response.content.contains("Run Build") || response.content.contains("Run"));
        assert!(!response.reasoning_used);
        assert!(response.pair_programming.is_some());
    }

    #[test]
    fn explain_edit_are_not_deterministic() {
        assert!(deterministic_coding_action_response(CodingAction::ExplainFile).is_none());
        assert!(deterministic_coding_action_response(CodingAction::EditSelection).is_none());
    }
}
