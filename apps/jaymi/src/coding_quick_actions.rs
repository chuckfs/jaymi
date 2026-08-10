//! Coding Workspace Quick Action Bar — Planner-generated suggestions (C1.8).
//!
//! The bar renders [`jaymi_planner::SuggestedAction`] values from Planner state.
//! UI never invents the action list. Clicks submit Conversation First turns
//! (typed [`CodingAction`] when present, otherwise free-text). No direct
//! editor / tool / provider calls.

use eframe::egui;
use jaymi_core::CodingAction;
use jaymi_planner::{baseline_actions, SuggestedAction};

use crate::theme::{type_size, Theme};

/// Height reserved for legacy layout constants (actions live in the header menu).
pub const QUICK_ACTION_BAR_HEIGHT: f32 = 40.0;

/// Result of activating a suggested action — Conversation First only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickActionEffect {
    /// Submit a typed Coding Action through the Planner.
    SubmitCodingAction(CodingAction),
    /// Explain — Application picks selection vs file from CodingState / WI.
    SubmitExplain,
    /// Submit free-text conversation turn (Planner-authored phrasing).
    SubmitConversation(String),
}

/// Map a Planner suggestion to a Conversation First effect (pure).
pub fn dispatch_suggested_action(action: &SuggestedAction) -> QuickActionEffect {
    if action.id == "explain_architecture"
        || action
            .coding_action
            .is_some_and(|a| matches!(a, CodingAction::ExplainFile | CodingAction::ExplainSelection))
    {
        // Architecture is free-text understanding; Explain* still use typed path.
        if action.coding_action.is_some() {
            return QuickActionEffect::SubmitExplain;
        }
    }
    if let Some(coding) = action.coding_action {
        match coding {
            CodingAction::ExplainSelection | CodingAction::ExplainFile => {
                return QuickActionEffect::SubmitExplain;
            }
            other => return QuickActionEffect::SubmitCodingAction(other),
        }
    }
    QuickActionEffect::SubmitConversation(action.conversation_text.clone())
}

/// Legacy QuickAction enum kept for tests / transitional callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuickAction {
    /// Explain selection or active file.
    Explain,
    /// Edit selection (conversational).
    Edit,
    /// Refactor selection (proposal only).
    Refactor,
    /// Semantic workspace search.
    Search,
    /// Reviewed project run.
    Run,
    /// Planner-generated Coding Actions menu.
    More,
}

impl QuickAction {
    /// Canonical left-to-right order (fallback labels only).
    pub const ALL: [QuickAction; 6] = [
        QuickAction::Explain,
        QuickAction::Refactor,
        QuickAction::Run,
        QuickAction::Edit,
        QuickAction::Search,
        QuickAction::More,
    ];

    /// Visible button label.
    pub fn label(self) -> &'static str {
        match self {
            QuickAction::Explain => "Explain",
            QuickAction::Edit => "Edit",
            QuickAction::Refactor => "Refactor",
            QuickAction::Search => "Search",
            QuickAction::Run => "Run",
            QuickAction::More => "More",
        }
    }

    /// Hover / accessibility hint.
    pub fn hint(self) -> &'static str {
        match self {
            QuickAction::Explain => "Ask Jaymi to explain the selection or current file",
            QuickAction::Edit => "Ask Jaymi what to change in the selection",
            QuickAction::Refactor => "Ask Jaymi for a refactoring proposal (no edits yet)",
            QuickAction::Search => "Search the workspace (uses selection as query when present)",
            QuickAction::Run => "Propose a reviewed project run command",
            QuickAction::More => "Show Coding Actions menu",
        }
    }
}

/// Map a legacy bar button to a typed Coding Action (pure; no UI side effects).
pub fn dispatch_quick_action(action: QuickAction) -> QuickActionEffect {
    match action {
        QuickAction::Explain => QuickActionEffect::SubmitExplain,
        QuickAction::Edit => QuickActionEffect::SubmitCodingAction(CodingAction::EditSelection),
        QuickAction::Refactor => {
            QuickActionEffect::SubmitCodingAction(CodingAction::RefactorSelection)
        }
        QuickAction::Search => QuickActionEffect::SubmitCodingAction(CodingAction::SearchWorkspace),
        QuickAction::Run => QuickActionEffect::SubmitCodingAction(CodingAction::RunProject),
        QuickAction::More => QuickActionEffect::SubmitCodingAction(CodingAction::OpenCodingActions),
    }
}

/// Actions shown in the menu — Planner list, else Planner baseline (never UI-invented).
pub fn actions_for_menu(planner_actions: &[SuggestedAction]) -> Vec<SuggestedAction> {
    if planner_actions.is_empty() {
        baseline_actions()
    } else {
        planner_actions.to_vec()
    }
}

/// Events the Quick Action Bar can emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickActionBarEvent {
    /// User activated a Planner-suggested action.
    Suggested(SuggestedAction),
    /// Legacy hardcoded button (tests / transitional).
    Action(QuickAction),
}

/// Ctx-level memory key for "is the Coding Actions '···' menu currently open".
fn quick_actions_menu_open_id() -> egui::Id {
    egui::Id::new("jaymi_coding_quick_actions_menu_open")
}

/// Read back whether the Coding Actions menu is open this frame.
pub fn quick_actions_menu_is_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp(quick_actions_menu_open_id()).unwrap_or(false))
}

/// Paint the "···" Coding Actions trigger from Planner-generated suggestions.
pub fn render_actions_menu_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    planner_actions: &[SuggestedAction],
) -> (egui::Response, Vec<QuickActionBarEvent>) {
    let mut events = Vec::new();
    let actions = actions_for_menu(planner_actions);

    let ellipsis_button = egui::Button::new(
        egui::RichText::new("···")
            .size(type_size::UI)
            .color(theme.text_secondary),
    )
    .frame(false);
    let menu_response = egui::menu::menu_custom_button(ui, ellipsis_button, |ui| {
        for action in &actions {
            let hint = if action.mutates {
                format!("{} (Review required before writes)", action.conversation_text)
            } else {
                action.conversation_text.clone()
            };
            let clicked = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(&action.label)
                            .size(type_size::UI)
                            .color(theme.text_primary),
                    )
                    .frame(false)
                    .min_size(egui::vec2(148.0, 24.0)),
                )
                .on_hover_text(hint)
                .clicked();
            if clicked {
                events.push(QuickActionBarEvent::Suggested(action.clone()));
                ui.close_menu();
            }
        }
    });
    let menu_open = menu_response.inner.is_some();
    ui.ctx().data_mut(|data| {
        data.insert_temp(quick_actions_menu_open_id(), menu_open);
    });

    (menu_response.response, events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_planner::PairPhase;

    #[test]
    fn quick_action_dispatch_is_typed_coding_action() {
        assert_eq!(
            dispatch_quick_action(QuickAction::Explain),
            QuickActionEffect::SubmitExplain
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Edit),
            QuickActionEffect::SubmitCodingAction(CodingAction::EditSelection)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Refactor),
            QuickActionEffect::SubmitCodingAction(CodingAction::RefactorSelection)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Search),
            QuickActionEffect::SubmitCodingAction(CodingAction::SearchWorkspace)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Run),
            QuickActionEffect::SubmitCodingAction(CodingAction::RunProject)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::More),
            QuickActionEffect::SubmitCodingAction(CodingAction::OpenCodingActions)
        );
    }

    #[test]
    fn menu_falls_back_to_planner_baseline() {
        let actions = actions_for_menu(&[]);
        let labels: Vec<&str> = actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Explain Architecture",
                "Review Changes",
                "Generate Tests",
                "Refactor",
                "Run Build",
                "Find Dead Code",
            ]
        );
    }

    #[test]
    fn suggested_refactor_uses_coding_action() {
        let action = SuggestedAction::with_coding_action(
            "refactor",
            "Refactor",
            PairPhase::Modification,
            "Propose a refactoring.",
            CodingAction::RefactorSelection,
            true,
        );
        assert_eq!(
            dispatch_suggested_action(&action),
            QuickActionEffect::SubmitCodingAction(CodingAction::RefactorSelection)
        );
    }

    #[test]
    fn suggested_architecture_is_conversation() {
        let action = SuggestedAction::conversational(
            "explain_architecture",
            "Explain Architecture",
            PairPhase::Understanding,
            "Explain the architecture of this project.",
            false,
        );
        assert_eq!(
            dispatch_suggested_action(&action),
            QuickActionEffect::SubmitConversation(
                "Explain the architecture of this project.".into()
            )
        );
    }
}
