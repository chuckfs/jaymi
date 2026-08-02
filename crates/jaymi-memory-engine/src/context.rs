//! Dynamic memory context assembly.
//!
//! Planner → Memory Engine → Relevant Memories → Planner
//!
//! Assembly considers active project, active conversation, the current request,
//! and recent work. It never loads every memory.

use crate::types::{MemoryRecord, MemoryScope};

/// Default overall budget for assembled context.
pub const DEFAULT_CONTEXT_LIMIT: usize = 12;

/// Default per-source candidate pool sizes (before global ranking).
pub const DEFAULT_PERSONAL_LIMIT: usize = 8;
pub const DEFAULT_PROJECT_LIMIT: usize = 16;
pub const DEFAULT_CONVERSATION_LIMIT: usize = 12;
pub const DEFAULT_WORKING_LIMIT: usize = 8;
pub const DEFAULT_RECENT_LIMIT: usize = 6;

/// Why a memory was selected into assembled context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryRelevanceKind {
    /// Matched the current request text.
    RequestMatch,
    /// Belongs to the active project.
    ActiveProject,
    /// Belongs to the active conversation.
    ActiveConversation,
    /// Curated personal preference.
    PersonalPreference,
    /// Recently updated working / session memory.
    RecentWork,
}

impl MemoryRelevanceKind {
    /// Stable label for diagnostics and tests.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RequestMatch => "request_match",
            Self::ActiveProject => "active_project",
            Self::ActiveConversation => "active_conversation",
            Self::PersonalPreference => "personal_preference",
            Self::RecentWork => "recent_work",
        }
    }
}

impl std::fmt::Display for MemoryRelevanceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Request for dynamic context assembly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssembleContextRequest {
    /// Current request / user goal text.
    pub text: String,
    /// Active conversation, when any.
    pub conversation_id: Option<String>,
    /// Project override; when unset the engine uses its active project.
    pub project_id: Option<String>,
    /// Hard cap on returned memories (default [`DEFAULT_CONTEXT_LIMIT`]).
    pub limit: Option<usize>,
    /// Max personal candidates considered.
    pub personal_limit: Option<usize>,
    /// Max active-project candidates considered.
    pub project_limit: Option<usize>,
    /// Max active-conversation candidates considered.
    pub conversation_limit: Option<usize>,
    /// Max working-scope candidates considered.
    pub working_limit: Option<usize>,
    /// Max recent-work candidates considered (by `updated_at`).
    pub recent_limit: Option<usize>,
}

/// A memory selected for the current request, with explainable relevance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevantMemory {
    /// Selected memory record.
    pub record: MemoryRecord,
    /// Relevance score `0..=100`.
    pub score: u32,
    /// Why this memory was included.
    pub reasons: Vec<MemoryRelevanceKind>,
    /// Human-readable explanation.
    pub why: String,
}

/// Assembled memory context for one Planner request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssembledMemoryContext {
    /// Relevant memories only (never a full dump).
    pub memories: Vec<RelevantMemory>,
    /// Project used during assembly, when any.
    pub project_id: Option<String>,
    /// Conversation used during assembly, when any.
    pub conversation_id: Option<String>,
    /// Total candidates scored before the limit was applied.
    pub candidate_count: usize,
    /// True when more candidates existed than the limit allowed.
    pub truncated: bool,
}

impl AssembledMemoryContext {
    /// Memory records only (Planner convenience).
    pub fn records(&self) -> Vec<MemoryRecord> {
        self.memories
            .iter()
            .map(|item| item.record.clone())
            .collect()
    }

    /// Number of selected memories.
    pub fn len(&self) -> usize {
        self.memories.len()
    }

    /// True when nothing was selected.
    pub fn is_empty(&self) -> bool {
        self.memories.is_empty()
    }
}

/// Tokenize request text for lightweight relevance matching.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
        .collect()
}

/// Count how many query tokens appear in a haystack.
pub fn token_overlap(tokens: &[String], haystack: &str) -> usize {
    if tokens.is_empty() {
        return 0;
    }
    let hay = haystack.to_ascii_lowercase();
    tokens.iter().filter(|token| hay.contains(token.as_str())).count()
}

/// Score a candidate and collect relevance reasons.
pub fn score_candidate(
    record: &MemoryRecord,
    tokens: &[String],
    project_id: Option<&str>,
    conversation_id: Option<&str>,
    now: i64,
    recent_cutoff: i64,
) -> Option<(u32, Vec<MemoryRelevanceKind>)> {
    let mut reasons = Vec::new();
    let mut score: u32 = record.importance.min(100) / 2;
    score = score.saturating_add(record.confidence.min(100) / 10);

    let overlap = token_overlap(tokens, &record.summary)
        + token_overlap(tokens, &record.content)
        + record
            .tags
            .iter()
            .map(|tag| token_overlap(tokens, tag))
            .sum::<usize>();
    if overlap > 0 {
        reasons.push(MemoryRelevanceKind::RequestMatch);
        score = score.saturating_add((overlap as u32).saturating_mul(12).min(40));
    }

    if record.scope == MemoryScope::Personal {
        reasons.push(MemoryRelevanceKind::PersonalPreference);
        score = score.saturating_add(15);
        if overlap == 0 {
            // Curated prefs stay eligible at a modest floor even without a text hit.
            score = score.max(25);
        }
    }

    if let Some(active) = project_id {
        if record.project_id.as_deref() == Some(active) {
            reasons.push(MemoryRelevanceKind::ActiveProject);
            score = score.saturating_add(20);
        } else if record.scope == MemoryScope::Project {
            // Foreign project memory must never enter context.
            return None;
        }
    } else if record.scope == MemoryScope::Project {
        return None;
    }

    if let Some(active) = conversation_id {
        if record.conversation_id.as_deref() == Some(active) {
            reasons.push(MemoryRelevanceKind::ActiveConversation);
            score = score.saturating_add(18);
        } else if record.scope == MemoryScope::Conversation {
            return None;
        }
    } else if record.scope == MemoryScope::Conversation {
        return None;
    }

    if record.scope == MemoryScope::Working || record.updated_at >= recent_cutoff {
        if record.scope == MemoryScope::Working || now.saturating_sub(record.updated_at) <= 86_400 {
            reasons.push(MemoryRelevanceKind::RecentWork);
            let age = now.saturating_sub(record.updated_at).max(0);
            let recency_boost = if age <= 3_600 {
                20
            } else if age <= 86_400 {
                10
            } else {
                4
            };
            score = score.saturating_add(recency_boost);
        }
    }

    // Require at least one explicit reason; never return arbitrary global rows.
    if reasons.is_empty() {
        return None;
    }

    // Deduplicate reason tags while preserving order.
    let mut unique = Vec::new();
    for reason in reasons {
        if !unique.contains(&reason) {
            unique.push(reason);
        }
    }

    Some((score.min(100), unique))
}

/// Build a short human explanation from relevance reasons.
pub fn explain_reasons(reasons: &[MemoryRelevanceKind], score: u32) -> String {
    let labels: Vec<&str> = reasons.iter().map(MemoryRelevanceKind::as_str).collect();
    format!("score={score}; reasons={}", labels.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::EntityId;

    fn record(scope: MemoryScope, summary: &str, content: &str, importance: u32) -> MemoryRecord {
        MemoryRecord {
            id: EntityId::new("mem-1"),
            scope,
            summary: summary.into(),
            content: content.into(),
            conversation_id: None,
            project_id: None,
            importance,
            confidence: 80,
            tags: vec![],
            source: None,
            kind: None,
            status: crate::types::MemoryStatus::Active,
            created_at: 100,
            updated_at: 100,
            archived_at: None,
        }
    }

    #[test]
    fn foreign_project_memory_is_rejected() {
        let mut foreign = record(
            MemoryScope::Project,
            "Other architecture",
            "secret-other-token",
            99,
        );
        foreign.project_id = Some("project:other".into());
        let scored = score_candidate(
            &foreign,
            &tokenize("architecture"),
            Some("project:jaymi"),
            None,
            200,
            0,
        );
        assert!(scored.is_none());
    }

    #[test]
    fn request_match_and_personal_are_scored() {
        let personal = record(
            MemoryScope::Personal,
            "Preferred name",
            "Charlie",
            90,
        );
        let (score, reasons) =
            score_candidate(&personal, &tokenize("hello"), None, None, 200, 0).unwrap();
        assert!(score > 0);
        assert!(reasons.contains(&MemoryRelevanceKind::PersonalPreference));

        let working = record(
            MemoryScope::Working,
            "Promotion ladder",
            "Keep promotions intentional",
            80,
        );
        let (score, reasons) = score_candidate(
            &working,
            &tokenize("promotion ladder"),
            None,
            None,
            200,
            0,
        )
        .unwrap();
        assert!(reasons.contains(&MemoryRelevanceKind::RequestMatch));
        assert!(score >= 40);
    }
}
