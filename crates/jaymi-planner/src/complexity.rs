//! Deterministic conversational complexity assessment (Planner-owned).
//!
//! Classifies free-text conversational requests **before** Context assemble so
//! [`jaymi_context::AssembleHints`] can carry a complexity label. This never
//! changes Intent routing or Capability selection — those stay on
//! [`crate::decision::DecisionEngine`].
//!
//! No AI / model classification. Rules are ordered and documented in
//! `docs/complexity.md`.

use jaymi_core::UserRequest;

/// Conversational complexity class for Context assemble hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationalComplexity {
    /// Short social greeting.
    Greeting,
    /// Polite / phatic chat without a task.
    SmallTalk,
    /// General question without project/coding/research markers.
    GeneralQuestion,
    /// Question about the open project / codebase / workspace.
    ProjectQuestion,
    /// Question about code, tools, errors, or implementation.
    CodingQuestion,
    /// Broader research / explain / compare question.
    ResearchQuestion,
}

impl ConversationalComplexity {
    /// Stable id written onto [`jaymi_context::AssembleHints::complexity`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Greeting => "greeting",
            Self::SmallTalk => "small_talk",
            Self::GeneralQuestion => "general_question",
            Self::ProjectQuestion => "project_question",
            Self::CodingQuestion => "coding_question",
            Self::ResearchQuestion => "research_question",
        }
    }

    /// Parse a stable id (Context / diagnostics).
    pub fn from_str_id(id: &str) -> Option<Self> {
        match id {
            "greeting" => Some(Self::Greeting),
            "small_talk" => Some(Self::SmallTalk),
            "general_question" => Some(Self::GeneralQuestion),
            "project_question" => Some(Self::ProjectQuestion),
            "coding_question" => Some(Self::CodingQuestion),
            "research_question" => Some(Self::ResearchQuestion),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConversationalComplexity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lightweight deterministic assessment produced by the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexityAssessment {
    /// Selected class.
    pub class: ConversationalComplexity,
    /// Rule ids that matched (diagnostics; first is decisive for first-match).
    pub matched_rules: Vec<&'static str>,
}

impl ComplexityAssessment {
    /// Construct an assessment.
    pub fn new(class: ConversationalComplexity, matched_rules: Vec<&'static str>) -> Self {
        Self {
            class,
            matched_rules,
        }
    }

    /// Stable class id for AssembleHints.
    pub fn class_id(&self) -> &'static str {
        self.class.as_str()
    }
}

/// Assess conversational complexity from request text (and optional workspace).
///
/// `workspace_kind` is an optional host hint (e.g. `"coding"`). It may break
/// ties toward [`ConversationalComplexity::CodingQuestion`] only when the
/// content already looks like a question — it never invents Intent or Capabilities.
pub fn assess_conversational_complexity(
    request: &UserRequest,
    workspace_kind: Option<&str>,
) -> ComplexityAssessment {
    assess_text(request.content.as_str(), workspace_kind)
}

/// Core classifier over raw text (testable without a full request).
pub fn assess_text(content: &str, workspace_kind: Option<&str>) -> ComplexityAssessment {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return ComplexityAssessment::new(
            ConversationalComplexity::GeneralQuestion,
            vec!["empty_default"],
        );
    }

    let lower = normalize(trimmed);
    let char_len = lower.chars().count();
    let coding_workspace = matches!(
        workspace_kind.map(str::to_ascii_lowercase).as_deref(),
        Some("coding") | Some("code") | Some("development")
    );

    // Ordered first-match — see docs/complexity.md.
    if is_greeting(&lower, char_len) {
        return ComplexityAssessment::new(ConversationalComplexity::Greeting, vec!["greeting"]);
    }
    if is_small_talk(&lower, char_len) {
        return ComplexityAssessment::new(ConversationalComplexity::SmallTalk, vec!["small_talk"]);
    }
    if has_coding_markers(&lower) {
        return ComplexityAssessment::new(
            ConversationalComplexity::CodingQuestion,
            vec!["coding_markers"],
        );
    }
    if has_project_markers(&lower) {
        return ComplexityAssessment::new(
            ConversationalComplexity::ProjectQuestion,
            vec!["project_markers"],
        );
    }
    if has_research_markers(&lower) {
        return ComplexityAssessment::new(
            ConversationalComplexity::ResearchQuestion,
            vec!["research_markers"],
        );
    }
    if coding_workspace && looks_like_question(&lower) {
        return ComplexityAssessment::new(
            ConversationalComplexity::CodingQuestion,
            vec!["coding_workspace_question"],
        );
    }
    if looks_like_question(&lower) {
        return ComplexityAssessment::new(
            ConversationalComplexity::GeneralQuestion,
            vec!["general_question"],
        );
    }

    ComplexityAssessment::new(
        ConversationalComplexity::GeneralQuestion,
        vec!["general_default"],
    )
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_whitespace() || ch == '?' || ch == '\'' {
                ch.to_ascii_lowercase()
            } else if ch == '’' {
                '\''
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_greeting(lower: &str, char_len: usize) -> bool {
    if char_len > 48 {
        return false;
    }
    // Strong coding/project markers disqualify greeting even when short.
    if has_coding_markers(lower) || has_project_markers(lower) || has_research_markers(lower) {
        return false;
    }
    const EXACT: &[&str] = &[
        "hi",
        "hello",
        "hey",
        "howdy",
        "yo",
        "hiya",
        "greetings",
        "good morning",
        "good afternoon",
        "good evening",
        "hello there",
        "hi there",
        "hey there",
        "hello jaymi",
        "hi jaymi",
        "hey jaymi",
    ];
    if EXACT.iter().any(|phrase| lower == *phrase) {
        return true;
    }
    // "hello!" / "hi jaymi" already normalized; allow trailing soft tokens.
    const PREFIXES: &[&str] = &[
        "hello ",
        "hi ",
        "hey ",
        "good morning",
        "good afternoon",
        "good evening",
    ];
    PREFIXES.iter().any(|prefix| {
        lower.starts_with(prefix)
            && char_len <= 32
            && !looks_like_question(lower)
            && lower.split_whitespace().count() <= 5
    })
}

fn is_small_talk(lower: &str, char_len: usize) -> bool {
    if char_len > 72 {
        return false;
    }
    if has_coding_markers(lower) || has_project_markers(lower) || has_research_markers(lower) {
        return false;
    }
    const PHRASES: &[&str] = &[
        "thanks",
        "thank you",
        "thank you so much",
        "thx",
        "ty",
        "cheers",
        "how are you",
        "how're you",
        "how's it going",
        "how is it going",
        "what's up",
        "whats up",
        "sup",
        "nice to meet you",
        "good to meet you",
        "goodbye",
        "good bye",
        "bye",
        "see you",
        "see ya",
        "take care",
        "you're welcome",
        "you are welcome",
        "good night",
        "goodnight",
        "have a good day",
        "have a nice day",
    ];
    PHRASES.iter().any(|phrase| {
        lower == *phrase
            || lower.starts_with(&format!("{phrase} "))
            || lower.ends_with(&format!(" {phrase}"))
    })
}

fn has_coding_markers(lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "compile",
        "compiler",
        "rustc",
        "cargo ",
        "cargo.",
        "borrow checker",
        "segfault",
        "stack trace",
        "stacktrace",
        "panic",
        "null pointer",
        "type error",
        "syntax error",
        "linker",
        "refactor",
        "unit test",
        "integration test",
        "failing test",
        "test failure",
        "debugger",
        "breakpoint",
        "lsp",
        "language server",
        "typescript",
        "javascript",
        "python",
        "golang",
        "rust ",
        " rust",
        "java ",
        "npm ",
        "webpack",
        "async await",
        "impl ",
        "trait ",
        "fn ",
        "function ",
        "method ",
        "class ",
        "bug in",
        "fix the bug",
        "code review",
        "pull request",
        "merge conflict",
        "git commit",
        "git diff",
        "git status",
        "clippy",
        "eslint",
        "compiler error",
        "runtime error",
        "build error",
        "undefined reference",
        "segmentation fault",
        "memory leak",
        "race condition",
        "deadlock",
        "api endpoint",
        "http handler",
        "sql query",
        "database schema",
        "write a function",
        "implement a",
        "implement the",
        "add a test",
        "fix this code",
        "explain this code",
        "what does this code",
    ];
    MARKERS.iter().any(|marker| lower.contains(marker))
}

fn has_project_markers(lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "this project",
        "this repo",
        "this repository",
        "this codebase",
        "our project",
        "our repo",
        "our codebase",
        "the project",
        "the codebase",
        "open project",
        "close project",
        "switch project",
        "continue project",
        "project structure",
        "workspace root",
        "monorepo",
        "which files",
        "what files",
        "in the workspace",
        "in this workspace",
        "active project",
        "current project",
    ];
    MARKERS.iter().any(|marker| lower.contains(marker))
}

fn has_research_markers(lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "research",
        "investigate",
        "literature",
        "whitepaper",
        "white paper",
        "scientific",
        "according to",
        "compare and contrast",
        "what is the history",
        "history of",
        "origin of",
        "explain the concept",
        "theoretical",
        "survey of",
        "state of the art",
        "peer reviewed",
        "academic",
        "encyclopedia",
    ];
    if MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    // "compare X and Y" / "difference between X and Y" when not coding-framed.
    (lower.contains("compare ") || lower.contains("difference between "))
        && !has_coding_markers(lower)
        && !has_project_markers(lower)
}

fn looks_like_question(lower: &str) -> bool {
    if lower.contains('?') {
        return true;
    }
    const STARTERS: &[&str] = &[
        "what ",
        "what's ",
        "whats ",
        "why ",
        "how ",
        "when ",
        "where ",
        "who ",
        "which ",
        "can you ",
        "could you ",
        "would you ",
        "is there ",
        "are there ",
        "do you ",
        "does ",
        "did ",
        "should ",
        "explain ",
        "tell me ",
        "help me ",
    ];
    STARTERS.iter().any(|starter| lower.starts_with(starter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_deterministically() {
        let a = assess_text("Hello!", None);
        assert_eq!(a.class, ConversationalComplexity::Greeting);
        let b = assess_text("Hello!", None);
        assert_eq!(a, b);
        assert_eq!(assess_text("hi jaymi", None).class, ConversationalComplexity::Greeting);
    }

    #[test]
    fn small_talk_thanks() {
        assert_eq!(
            assess_text("Thanks!", None).class,
            ConversationalComplexity::SmallTalk
        );
        assert_eq!(
            assess_text("how are you", None).class,
            ConversationalComplexity::SmallTalk
        );
    }

    #[test]
    fn coding_beats_greeting_when_markers_present() {
        assert_eq!(
            assess_text("hi, fix the borrow checker error", None).class,
            ConversationalComplexity::CodingQuestion
        );
    }

    #[test]
    fn project_question() {
        assert_eq!(
            assess_text("What files are in this project?", None).class,
            ConversationalComplexity::ProjectQuestion
        );
    }

    #[test]
    fn research_question() {
        assert_eq!(
            assess_text("What is the history of cryptography?", None).class,
            ConversationalComplexity::ResearchQuestion
        );
    }

    #[test]
    fn general_question_default() {
        assert_eq!(
            assess_text("What time is it in Tokyo?", None).class,
            ConversationalComplexity::GeneralQuestion
        );
    }

    #[test]
    fn coding_workspace_tie_break() {
        assert_eq!(
            assess_text("How should I structure this?", Some("coding")).class,
            ConversationalComplexity::CodingQuestion
        );
        // Same text without coding workspace stays general.
        assert_eq!(
            assess_text("How should I structure this?", None).class,
            ConversationalComplexity::GeneralQuestion
        );
    }

    #[test]
    fn does_not_invent_capabilities_or_intents() {
        // Classifier only returns a class — no capability ids.
        let assessment = assess_text("refactor this function", None);
        assert_eq!(assessment.class, ConversationalComplexity::CodingQuestion);
        assert_eq!(assessment.class_id(), "coding_question");
    }
}
