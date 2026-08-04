//! Memory promotion — intentional moves up the scope ladder.
//!
//! Working → Conversation → Project → Personal
//!
//! The Memory Engine suggests. The Planner decides whether to ask.
//! Promotion never happens automatically.

use crate::types::{MemoryRecord, MemoryScope};

/// Rank on the promotion ladder (higher = more durable).
pub fn scope_rank(scope: MemoryScope) -> u8 {
    match scope {
        MemoryScope::Working => 0,
        MemoryScope::Conversation => 1,
        MemoryScope::Project => 2,
        MemoryScope::Personal => 3,
    }
}

/// Next scope up the ladder, when any.
pub fn next_scope(scope: MemoryScope) -> Option<MemoryScope> {
    match scope {
        MemoryScope::Working => Some(MemoryScope::Conversation),
        MemoryScope::Conversation => Some(MemoryScope::Project),
        MemoryScope::Project => Some(MemoryScope::Personal),
        MemoryScope::Personal => None,
    }
}

/// True when `to` is a valid upward promotion from `from`.
pub fn is_upward_promotion(from: MemoryScope, to: MemoryScope) -> bool {
    scope_rank(to) > scope_rank(from)
}

/// Intentional promotion request (never implied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteMemoryRequest {
    /// Memory to promote.
    pub memory_id: String,
    /// Target scope (must be higher on the ladder).
    pub to: MemoryScope,
    /// Conversation association (required for Conversation when missing on the record).
    pub conversation_id: Option<String>,
    /// Project association (required for Project when missing on the record).
    pub project_id: Option<String>,
    /// Optional structured kind after promotion.
    pub kind: Option<String>,
}

/// A conservative promotion suggestion produced by the Memory Engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionSuggestion {
    /// Candidate memory id.
    pub memory_id: String,
    /// Short summary for user-facing prompts.
    pub summary: String,
    /// Current scope.
    pub from: MemoryScope,
    /// Suggested target scope (always the next ladder step).
    pub to: MemoryScope,
    /// Why this candidate was suggested.
    pub reason: String,
    /// Suggestion strength `0..=100` (conservative thresholds apply).
    pub score: u32,
}

/// Filters for generating promotion suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromotionSuggestQuery {
    /// Prefer candidates tied to this conversation.
    pub conversation_id: Option<String>,
    /// Prefer / enable project promotions for this project.
    pub project_id: Option<String>,
    /// Minimum importance to consider (default 70).
    pub min_importance: Option<u32>,
    /// Maximum suggestions to return.
    pub limit: Option<usize>,
}

/// Planner decision about whether to surface suggestions to the user.
///
/// The Planner never auto-applies promotions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromotionAskDecision {
    /// Ask the user whether to promote.
    AskUser,
    /// Keep suggestions available but do not interrupt.
    #[default]
    Defer,
}

impl PromotionAskDecision {
    /// Decide conservatively from suggestion scores.
    pub fn from_suggestions(suggestions: &[PromotionSuggestion]) -> Self {
        if suggestions.iter().any(|suggestion| suggestion.score >= 80) {
            Self::AskUser
        } else {
            Self::Defer
        }
    }
}

/// Build a human-readable reason for a ladder step.
pub fn suggestion_reason(record: &MemoryRecord, to: MemoryScope) -> String {
    match to {
        MemoryScope::Conversation => format!(
            "Working memory looks durable enough to keep for this conversation (importance={}).",
            record.importance
        ),
        MemoryScope::Project => format!(
            "Conversation memory may belong with the project (importance={}).",
            record.importance
        ),
        MemoryScope::Personal => format!(
            "Project memory may be a long-term personal preference (importance={}).",
            record.importance
        ),
        MemoryScope::Working => "Working is not a promotion target.".into(),
    }
}

/// Score a candidate for the next ladder step (0 when not eligible).
pub fn score_promotion_candidate(
    record: &MemoryRecord,
    to: MemoryScope,
    query: &PromotionSuggestQuery,
) -> u32 {
    if !is_upward_promotion(record.scope, to) {
        return 0;
    }
    if next_scope(record.scope) != Some(to) {
        // Suggestions only recommend one step at a time.
        return 0;
    }
    let min_importance = query.min_importance.unwrap_or(70);
    if record.importance < min_importance {
        return 0;
    }

    let mut score = record.importance.min(100);
    // Slight confidence contribution.
    score = score.saturating_add(record.confidence / 10).min(100);

    match to {
        MemoryScope::Conversation => {
            if query.conversation_id.is_some() || record.conversation_id.is_some() {
                score = score.saturating_add(5).min(100);
            }
        }
        MemoryScope::Project => {
            let has_project = query.project_id.is_some() || record.project_id.is_some();
            if !has_project {
                return 0;
            }
            if record.importance < 80 {
                return 0;
            }
        }
        MemoryScope::Personal => {
            if record.importance < 90 {
                return 0;
            }
            let preference_like = record
                .kind
                .as_deref()
                .map(|kind| {
                    matches!(
                        kind,
                        "preferred_name"
                            | "writing_style"
                            | "code_style"
                            | "favorite_editor"
                            | "preferred_theme"
                            | "coding_preference"
                    )
                })
                .unwrap_or(false)
                || record.tags.iter().any(|tag| {
                    tag.contains("preference") || tag.contains("style") || tag.contains("theme")
                });
            if !preference_like {
                return 0;
            }
            score = score.saturating_add(5).min(100);
        }
        MemoryScope::Working => return 0,
    }

    score
}
