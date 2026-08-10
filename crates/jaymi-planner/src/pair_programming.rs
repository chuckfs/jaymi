//! Pair Programming Experience (Sprint C1.8).
//!
//! Make Jaymi feel like an engineering partner: Conversation First turns
//! naturally move between Understanding → Review → Planning → Generation →
//! Modification → Execution. **Suggested next actions are Planner-generated**
//! from Planner state + Workspace Intelligence — never hardcoded UI buttons.
//!
//! Mutations still require Execution Plans and Review Before Action.

use jaymi_context::ContextBundle;
use jaymi_core::CodingAction;

/// Conversational partnership phase inferred from the latest Planner turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairPhase {
    /// Orient / explain before acting.
    Understanding,
    /// Structured review without edits.
    Review,
    /// Generation planning (Coding Plan).
    Planning,
    /// Ops proposed / GenerationOps awaiting review.
    Generation,
    /// Multi-file or edit proposals (still Review-gated).
    Modification,
    /// Tools running or just finished under an Approved plan.
    Execution,
    /// Coding partner ready — no specialized scaffold this turn.
    Ready,
}

impl PairPhase {
    /// Stable id for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Understanding => "understanding",
            Self::Review => "review",
            Self::Planning => "planning",
            Self::Generation => "generation",
            Self::Modification => "modification",
            Self::Execution => "execution",
            Self::Ready => "ready",
        }
    }

    /// Human label for conversation / menu chrome.
    pub fn label(self) -> &'static str {
        match self {
            Self::Understanding => "Understanding",
            Self::Review => "Review",
            Self::Planning => "Planning",
            Self::Generation => "Generation",
            Self::Modification => "Modification",
            Self::Execution => "Execution",
            Self::Ready => "Ready",
        }
    }
}

/// One Planner-authored next action (Conversation First).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SuggestedAction {
    /// Stable id (`explain_architecture`, `review_changes`, …).
    pub id: String,
    /// Short Quick Action / chip label.
    pub label: String,
    /// Partnership phase this action advances.
    pub phase: PairPhase,
    /// User-turn text submitted into the conversation.
    pub conversation_text: String,
    /// Optional typed Coding Action (C0.1) when WI binding helps.
    pub coding_action: Option<CodingAction>,
    /// True when following through may eventually mutate (still Review-gated).
    pub mutates: bool,
}

impl SuggestedAction {
    /// Construct a free-text conversational suggestion.
    pub fn conversational(
        id: impl Into<String>,
        label: impl Into<String>,
        phase: PairPhase,
        conversation_text: impl Into<String>,
        mutates: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            phase,
            conversation_text: conversation_text.into(),
            coding_action: None,
            mutates,
        }
    }

    /// Construct a suggestion that also carries a typed Coding Action.
    pub fn with_coding_action(
        id: impl Into<String>,
        label: impl Into<String>,
        phase: PairPhase,
        conversation_text: impl Into<String>,
        coding_action: CodingAction,
        mutates: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            phase,
            conversation_text: conversation_text.into(),
            coding_action: Some(coding_action),
            mutates,
        }
    }
}

/// Planner-owned pair-programming surface for Conversation + Quick Actions.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PairProgrammingSurface {
    /// Current inferred partnership phase.
    pub phase: Option<PairPhase>,
    /// Ordered suggested next actions (UI renders these; does not invent them).
    pub suggested_actions: Vec<SuggestedAction>,
    /// Short observational notes (why these actions).
    pub notes: Vec<String>,
}

impl PairProgrammingSurface {
    /// True when there is nothing to show.
    pub fn is_empty(&self) -> bool {
        self.suggested_actions.is_empty()
    }

    /// Markdown block for conversation / More menu.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Pair Programming\n");
        if let Some(phase) = self.phase {
            out.push_str(&format!("\n### Phase\n- {}\n", phase.label()));
        }
        out.push_str("\n### Suggested Next Actions\n");
        if self.suggested_actions.is_empty() {
            out.push_str("- _(none)_\n");
        } else {
            for action in &self.suggested_actions {
                let gate = if action.mutates {
                    " — mutations still require Review"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "- **{}** ({}) — `{}`{gate}\n",
                    action.label,
                    action.phase.label(),
                    action.conversation_text.trim_end_matches('.')
                ));
            }
        }
        if !self.notes.is_empty() {
            out.push_str("\n### Notes\n");
            for note in &self.notes {
                out.push_str("- ");
                out.push_str(note);
                out.push('\n');
            }
        }
        out.push_str(
            "\nConversation First — pick a suggestion or type freely. Quick Actions mirror this list.\n",
        );
        out
    }
}

/// Snapshot of Planner-owned coding scaffolds used to bias next actions.
#[derive(Debug, Clone, Default)]
pub struct PairMemory {
    /// Last understanding scaffold present.
    pub had_understanding: bool,
    /// Last review scaffold present.
    pub had_review: bool,
    /// Last coding plan present.
    pub had_coding_plan: bool,
}

/// Inputs for synthesizing the pair-programming surface (no LLM).
#[derive(Debug, Clone, Default)]
pub struct PairSynthesisInput<'a> {
    /// Remembered Planner scaffolds.
    pub memory: PairMemory,
    /// Assembled WI when available.
    pub bundle: Option<&'a ContextBundle>,
    /// Coding Understanding ran this turn.
    pub understanding: bool,
    /// Coding Review ran this turn.
    pub review: bool,
    /// Coding Plan ran this turn.
    pub planning: bool,
    /// Code Generation ops proposed this turn.
    pub generation: bool,
    /// Multi-file plan this turn.
    pub multi_file: bool,
    /// Execution Plan awaiting review.
    pub awaiting_review: bool,
    /// Execution summary present (tools finished / cancelled).
    pub execution_summary: bool,
    /// Execution completed successfully.
    pub execution_completed: bool,
}

/// Baseline partner actions (Planner-authored defaults — not UI chrome).
pub fn baseline_actions() -> Vec<SuggestedAction> {
    vec![
        SuggestedAction::conversational(
            "explain_architecture",
            "Explain Architecture",
            PairPhase::Understanding,
            "Explain the architecture of this project.",
            false,
        ),
        SuggestedAction::conversational(
            "review_changes",
            "Review Changes",
            PairPhase::Review,
            "Review my changes.",
            false,
        ),
        SuggestedAction::conversational(
            "generate_tests",
            "Generate Tests",
            PairPhase::Planning,
            "Generate tests.",
            true,
        ),
        SuggestedAction::with_coding_action(
            "refactor",
            "Refactor",
            PairPhase::Modification,
            "Propose a refactoring for the current selection. Do not edit files yet — describe the proposal only.",
            CodingAction::RefactorSelection,
            true,
        ),
        SuggestedAction::with_coding_action(
            "run_build",
            "Run Build",
            PairPhase::Execution,
            "Run the project.",
            CodingAction::RunProject,
            true,
        ),
        SuggestedAction::conversational(
            "find_dead_code",
            "Find Dead Code",
            PairPhase::Understanding,
            "Find dead code.",
            false,
        ),
    ]
}

/// Synthesize phase + suggested actions from Planner state (deterministic).
pub fn synthesize(input: &PairSynthesisInput<'_>) -> PairProgrammingSurface {
    let (phase, mut actions, mut notes) = if input.awaiting_review && input.generation {
        (
            PairPhase::Generation,
            vec![
                SuggestedAction::conversational(
                    "review_plan",
                    "Review Plan",
                    PairPhase::Review,
                    "Review the proposed generation plan before approving.",
                    false,
                ),
                SuggestedAction::conversational(
                    "modify_plan",
                    "Modify Plan",
                    PairPhase::Planning,
                    "Modify the plan: keep the core files and drop optional ones.",
                    false,
                ),
            ],
            vec![
                "Generation ops are paused — Approve / Modify / Cancel on the Review Card.".into(),
            ],
        )
    } else if input.awaiting_review && input.multi_file {
        (
            PairPhase::Modification,
            vec![
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
                SuggestedAction::conversational(
                    "explain_architecture",
                    "Explain Architecture",
                    PairPhase::Understanding,
                    "Explain the architecture of this project.",
                    false,
                ),
            ],
            vec!["Multi-file mutations await Review Before Action.".into()],
        )
    } else if input.awaiting_review {
        (
            PairPhase::Execution,
            vec![
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
            ],
            vec!["An Execution Plan is awaiting Review Before Action.".into()],
        )
    } else if input.execution_completed || input.execution_summary {
        (
            PairPhase::Execution,
            vec![
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
                SuggestedAction::with_coding_action(
                    "run_build",
                    "Run Build",
                    PairPhase::Execution,
                    "Run the project.",
                    CodingAction::RunProject,
                    true,
                ),
                SuggestedAction::conversational(
                    "find_dead_code",
                    "Find Dead Code",
                    PairPhase::Understanding,
                    "Find dead code.",
                    false,
                ),
            ],
            vec!["Execution finished — next partner moves stay Conversation First.".into()],
        )
    } else if input.planning || input.memory.had_coding_plan {
        (
            PairPhase::Planning,
            vec![
                SuggestedAction::conversational(
                    "generate_code",
                    "Generate Code",
                    PairPhase::Generation,
                    "Generate the code.",
                    true,
                ),
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
                SuggestedAction::conversational(
                    "generate_tests",
                    "Generate Tests",
                    PairPhase::Planning,
                    "Generate tests.",
                    true,
                ),
            ],
            vec!["A Coding Plan is ready — generation still requires Review.".into()],
        )
    } else if input.review || input.memory.had_review {
        (
            PairPhase::Review,
            vec![
                SuggestedAction::conversational(
                    "generate_tests",
                    "Generate Tests",
                    PairPhase::Planning,
                    "Generate tests.",
                    true,
                ),
                SuggestedAction::with_coding_action(
                    "refactor",
                    "Refactor",
                    PairPhase::Modification,
                    "Propose a refactoring for the current selection. Do not edit files yet — describe the proposal only.",
                    CodingAction::RefactorSelection,
                    true,
                ),
                SuggestedAction::conversational(
                    "explain_architecture",
                    "Explain Architecture",
                    PairPhase::Understanding,
                    "Explain the architecture of this project.",
                    false,
                ),
            ],
            vec!["Review complete — plan or refactor next; no silent edits.".into()],
        )
    } else if input.understanding || input.memory.had_understanding {
        (
            PairPhase::Understanding,
            vec![
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
                SuggestedAction::conversational(
                    "generate_tests",
                    "Generate Tests",
                    PairPhase::Planning,
                    "Generate tests.",
                    true,
                ),
                SuggestedAction::with_coding_action(
                    "refactor",
                    "Refactor",
                    PairPhase::Modification,
                    "Propose a refactoring for the current selection. Do not edit files yet — describe the proposal only.",
                    CodingAction::RefactorSelection,
                    true,
                ),
            ],
            vec!["Understanding first — suggested moves stay observational until Review.".into()],
        )
    } else if input.multi_file {
        (
            PairPhase::Modification,
            vec![
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
                SuggestedAction::with_coding_action(
                    "run_build",
                    "Run Build",
                    PairPhase::Execution,
                    "Run the project.",
                    CodingAction::RunProject,
                    true,
                ),
            ],
            vec!["Multi-file intelligence proposed coordinated changes.".into()],
        )
    } else {
        (
            PairPhase::Ready,
            baseline_actions(),
            vec![
                "Baseline partner actions from Planner state (not hardcoded UI).".into(),
            ],
        )
    };

    enrich_from_workspace(input.bundle, &mut actions, &mut notes);
    dedupe_actions(&mut actions);
    // Cap menu length — Conversation First remains primary.
    actions.truncate(6);

    PairProgrammingSurface {
        phase: Some(phase),
        suggested_actions: actions,
        notes,
    }
}

/// Soft baseline when Coding opens before any turn (still Planner-authored).
pub fn baseline_surface() -> PairProgrammingSurface {
    PairProgrammingSurface {
        phase: Some(PairPhase::Ready),
        suggested_actions: baseline_actions(),
        notes: vec![
            "Conversation First — Quick Actions mirror Planner suggestions.".into(),
            "Mutating paths still pause for Review Before Action.".into(),
        ],
    }
}

fn enrich_from_workspace(
    bundle: Option<&ContextBundle>,
    actions: &mut Vec<SuggestedAction>,
    notes: &mut Vec<String>,
) {
    let Some(bundle) = bundle else {
        return;
    };
    let current = bundle.current_file();
    let open = bundle.open_files();
    let git = bundle.git_status();
    let has_file = current.path.as_ref().is_some_and(|p| !p.trim().is_empty())
        || open.files.iter().any(|f| !f.path.trim().is_empty());
    if has_file {
        notes.push("Active editor context observed in Workspace Intelligence.".into());
    } else {
        notes.push("Open a file to enrich Understanding / Refactor suggestions.".into());
    }
    let dirty = git.modified_count > 0
        || git.staged_count > 0
        || git.untracked_count > 0
        || !git.dirty_paths.is_empty();
    if dirty {
        if let Some(idx) = actions.iter().position(|a| a.id == "review_changes") {
            let action = actions.remove(idx);
            actions.insert(0, action);
        } else {
            actions.insert(
                0,
                SuggestedAction::conversational(
                    "review_changes",
                    "Review Changes",
                    PairPhase::Review,
                    "Review my changes.",
                    false,
                ),
            );
        }
        notes.push("Git working tree looks dirty — Review Changes promoted.".into());
    }
}

fn dedupe_actions(actions: &mut Vec<SuggestedAction>) {
    let mut seen = std::collections::BTreeSet::new();
    actions.retain(|action| seen.insert(action.id.clone()));
}

/// True when free-text asks for dead-code orientation (read-only).
pub fn looks_like_dead_code_request(lower: &str) -> bool {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '!');
    trimmed == "find dead code"
        || trimmed.starts_with("find dead code")
        || trimmed.contains("dead code")
        || trimmed.contains("unused code")
        || trimmed.contains("unused exports")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ActiveProjectSection, ContextBundle, CurrentFileSection, GitStatusSection,
    };

    #[test]
    fn baseline_covers_sprint_examples() {
        let actions = baseline_actions();
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
    fn understanding_transitions_toward_review_and_planning() {
        let surface = synthesize(&PairSynthesisInput {
            understanding: true,
            ..Default::default()
        });
        assert_eq!(surface.phase, Some(PairPhase::Understanding));
        assert!(surface
            .suggested_actions
            .iter()
            .any(|a| a.id == "review_changes"));
        assert!(surface
            .suggested_actions
            .iter()
            .any(|a| a.id == "generate_tests"));
        assert!(surface.to_markdown().contains("## Pair Programming"));
    }

    #[test]
    fn planning_suggests_generation() {
        let surface = synthesize(&PairSynthesisInput {
            planning: true,
            ..Default::default()
        });
        assert_eq!(surface.phase, Some(PairPhase::Planning));
        assert!(surface
            .suggested_actions
            .iter()
            .any(|a| a.id == "generate_code"));
        assert!(surface
            .suggested_actions
            .iter()
            .any(|a| a.mutates && a.id == "generate_code"));
    }

    #[test]
    fn dirty_git_promotes_review() {
        let bundle = ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("p".into()),
                name: Some("Demo".into()),
                root_directory: Some("/proj".into()),
                detail: None,
            })
            .current_file(CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            })
            .git_status(GitStatusSection {
                is_repository: true,
                modified_count: 1,
                dirty_paths: vec!["/proj/src/lib.rs".into()],
                ..Default::default()
            })
            .build();
        let surface = synthesize(&PairSynthesisInput {
            bundle: Some(&bundle),
            ..Default::default()
        });
        assert_eq!(
            surface.suggested_actions.first().map(|a| a.id.as_str()),
            Some("review_changes")
        );
    }

    #[test]
    fn dead_code_phrasing_detected() {
        assert!(looks_like_dead_code_request("find dead code"));
        assert!(looks_like_dead_code_request("Find dead code."));
        assert!(!looks_like_dead_code_request("explain this file"));
    }
}
