//! Deterministic Context Selection profiles (Sprint B2.8).
//!
//! Context Policy uses these profiles to choose which workspace feeds and
//! candidate kinds participate for a request. Rules are ordered, documented in
//! `docs/context-selection.md`, and use **no AI scoring**.
//!
//! ## Ownership
//!
//! | Owner | Role |
//! |-------|------|
//! | Planner | Intent, Capabilities, optional `AssembleHints::complexity` |
//! | Context Selection | Maps complexity + RequestKind + documented lexical cues → profile |
//! | Context Policy | Enforces profile allowlists on providers / candidates |
//!
//! Context Selection **never** invents Intent or Complexity labels for the
//! Planner. When `AssembleHints.complexity` is present it is the coarse class;
//! lexical cues only refine within that class (e.g. coding → debug/compile) or
//! provide a fallback when no complexity hint is attached (tests / direct
//! assemble).

use crate::candidate::ContextCandidateKind;
use crate::relevance::{IntentTag, RelevanceSignals, RequestKind};

use jaymi_core::UserRequest;

/// Stable Context Selection profile for one assemble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextSelectionProfile {
    /// Short social hello — conversation + memory.
    Greeting,
    /// Thanks / goodbye — conversation (+ light memory).
    SmallTalk,
    /// Compile / error / "won't build" debug.
    DebugCompile,
    /// Summarize / overview this project.
    ProjectOverview,
    /// General coding question (broader than DebugCompile).
    CodingGeneral,
    /// Research / explain broadly.
    Research,
    /// Search / retrieval.
    Search,
    /// Git-focused.
    Git,
    /// Terminal-focused.
    Terminal,
    /// File read/write.
    FileEdit,
    /// Project open/close/switch.
    ProjectSession,
    /// Default chat / general question.
    GeneralChat,
}

impl ContextSelectionProfile {
    /// Stable id for diagnostics / PolicyReport.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Greeting => "greeting",
            Self::SmallTalk => "small_talk",
            Self::DebugCompile => "debug_compile",
            Self::ProjectOverview => "project_overview",
            Self::CodingGeneral => "coding_general",
            Self::Research => "research",
            Self::Search => "search",
            Self::Git => "git",
            Self::Terminal => "terminal",
            Self::FileEdit => "file_edit",
            Self::ProjectSession => "project_session",
            Self::GeneralChat => "general_chat",
        }
    }

    /// Providers allowed to participate under this profile.
    ///
    /// `permission` is always allowed separately by policy.
    pub fn allowed_providers(self) -> &'static [&'static str] {
        match self {
            Self::Greeting | Self::SmallTalk => &["conversation", "memory"],
            Self::DebugCompile => &[
                "conversation",
                "diagnostics",
                "editor",
                "runtime",
                "workspace_memory",
                "workspace",
            ],
            Self::ProjectOverview => &[
                "conversation",
                "project",
                "workspace",
                "workspace_inventory",
                "git_status",
                "file_summaries",
                "workspace_memory",
            ],
            Self::CodingGeneral => &[
                "conversation",
                "workspace",
                "diagnostics",
                "project",
                "editor",
                "runtime",
                "workspace_memory",
                "git_status",
                "file_summaries",
                "memory",
            ],
            Self::Research => &["conversation", "search", "memory", "project"],
            // `workspace` carries Planner capability ids (and optional kind).
            Self::Search => &[
                "conversation",
                "search",
                "memory",
                "workspace",
                "workspace_inventory",
            ],
            Self::Git => &["conversation", "git_status", "project", "workspace", "editor"],
            Self::Terminal => &[
                "conversation",
                "runtime",
                "workspace_memory",
                "workspace",
                "editor",
                "project",
            ],
            Self::FileEdit => &[
                "conversation",
                "editor",
                "workspace_memory",
                "file_summaries",
                "diagnostics",
                "project",
                "workspace",
            ],
            Self::ProjectSession => &[
                "conversation",
                "project",
                "workspace",
                "memory",
                "workspace_memory",
            ],
            Self::GeneralChat => &["conversation", "memory", "project", "workspace"],
        }
    }

    /// Candidate kinds preferred (boosted) under this profile.
    pub fn preferred_kinds(self) -> &'static [ContextCandidateKind] {
        match self {
            Self::Greeting | Self::SmallTalk => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::MemoryResults,
            ],
            Self::DebugCompile => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::Diagnostic,
                ContextCandidateKind::Diagnostics,
                ContextCandidateKind::CurrentFile,
                ContextCandidateKind::Selection,
                ContextCandidateKind::RuntimeIntelligence,
                ContextCandidateKind::WorkspaceMemory,
                ContextCandidateKind::EditorIntelligence,
            ],
            Self::ProjectOverview => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::ProjectIdentity,
                ContextCandidateKind::ProjectIntelligence,
                ContextCandidateKind::WorkspaceInventory,
                ContextCandidateKind::GitStatus,
                ContextCandidateKind::WorkspaceKind,
                ContextCandidateKind::FileSummaries,
                ContextCandidateKind::FileSummary,
                ContextCandidateKind::WorkspaceMemory,
            ],
            Self::CodingGeneral => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::CurrentFile,
                ContextCandidateKind::Selection,
                ContextCandidateKind::Diagnostic,
                ContextCandidateKind::Diagnostics,
                ContextCandidateKind::EditorIntelligence,
                ContextCandidateKind::RuntimeIntelligence,
                ContextCandidateKind::WorkspaceMemory,
                ContextCandidateKind::ProjectIdentity,
                ContextCandidateKind::GitStatus,
            ],
            Self::Research => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::SearchResults,
                ContextCandidateKind::MemoryResults,
                ContextCandidateKind::ProjectIdentity,
            ],
            Self::Search => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::SearchResults,
                ContextCandidateKind::WorkspaceInventory,
            ],
            Self::Git => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::GitStatus,
                ContextCandidateKind::ProjectIdentity,
                ContextCandidateKind::CurrentFile,
            ],
            Self::Terminal => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::RuntimeIntelligence,
                ContextCandidateKind::CurrentFile,
            ],
            Self::FileEdit => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::CurrentFile,
                ContextCandidateKind::Selection,
                ContextCandidateKind::OpenFile,
                ContextCandidateKind::OpenFiles,
                ContextCandidateKind::FileSummary,
                ContextCandidateKind::FileSummaries,
                ContextCandidateKind::Diagnostic,
            ],
            Self::ProjectSession => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::ProjectIdentity,
                ContextCandidateKind::ProjectIntelligence,
                ContextCandidateKind::WorkspaceKind,
                ContextCandidateKind::MemoryResults,
            ],
            Self::GeneralChat => &[
                ContextCandidateKind::Conversation,
                ContextCandidateKind::MemoryResults,
                ContextCandidateKind::ProjectIdentity,
                ContextCandidateKind::WorkspaceKind,
            ],
        }
    }

    /// Candidate kinds actively omitted under a selective profile.
    pub fn omitted_kinds(self) -> &'static [ContextCandidateKind] {
        match self {
            Self::Greeting | Self::SmallTalk => &[
                ContextCandidateKind::Diagnostic,
                ContextCandidateKind::Diagnostics,
                ContextCandidateKind::CurrentFile,
                ContextCandidateKind::Selection,
                ContextCandidateKind::OpenFile,
                ContextCandidateKind::OpenFiles,
                ContextCandidateKind::EditorIntelligence,
                ContextCandidateKind::RuntimeIntelligence,
                ContextCandidateKind::WorkspaceMemory,
                ContextCandidateKind::GitStatus,
                ContextCandidateKind::WorkspaceInventory,
                ContextCandidateKind::ProjectIntelligence,
                ContextCandidateKind::SearchResults,
                ContextCandidateKind::FileSummary,
                ContextCandidateKind::FileSummaries,
            ],
            Self::DebugCompile => &[
                ContextCandidateKind::ProjectIntelligence,
                ContextCandidateKind::WorkspaceInventory,
                ContextCandidateKind::GitStatus,
                ContextCandidateKind::SearchResults,
                ContextCandidateKind::FileSummaries,
                ContextCandidateKind::FileSummary,
                ContextCandidateKind::OpenFiles,
            ],
            Self::ProjectOverview => &[
                ContextCandidateKind::Diagnostic,
                ContextCandidateKind::Diagnostics,
                ContextCandidateKind::RuntimeIntelligence,
                ContextCandidateKind::Selection,
                ContextCandidateKind::SearchResults,
            ],
            _ => &[],
        }
    }

    /// True when `provider_id` may participate (`permission` always true).
    ///
    /// Strict profiles (greeting / debug / project overview / …) use an
    /// exclusive allowlist. Broader profiles still allow unknown providers for
    /// extensibility after the named allowlist check.
    pub fn allows_provider(self, provider_id: &str) -> bool {
        if provider_id == "permission" {
            return true;
        }
        if self
            .allowed_providers()
            .iter()
            .any(|id| *id == provider_id)
        {
            return true;
        }
        !self.is_strict_allowlist()
    }

    /// Exclusive allowlist profiles (unknown providers denied).
    pub fn is_strict_allowlist(self) -> bool {
        matches!(
            self,
            Self::Greeting
                | Self::SmallTalk
                | Self::DebugCompile
                | Self::ProjectOverview
                | Self::Search
                | Self::Git
                | Self::Terminal
                | Self::ProjectSession
        )
    }

    /// True when this kind is preferred.
    pub fn prefers_kind(self, kind: ContextCandidateKind) -> bool {
        self.preferred_kinds().contains(&kind)
    }

    /// True when this kind is omitted by the profile.
    pub fn omits_kind(self, kind: ContextCandidateKind) -> bool {
        self.omitted_kinds().contains(&kind)
    }
}

/// Explainable selection assessment for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSelectionAssessment {
    /// Chosen profile.
    pub profile: ContextSelectionProfile,
    /// Rule ids that matched (first is decisive for first-match).
    pub matched_rules: Vec<&'static str>,
}

impl ContextSelectionAssessment {
    /// Construct an assessment.
    pub fn new(profile: ContextSelectionProfile, matched_rules: Vec<&'static str>) -> Self {
        Self {
            profile,
            matched_rules,
        }
    }
}

/// Assess Context Selection for this request (deterministic, no AI).
pub fn assess_context_selection(
    request: &UserRequest,
    signals: &RelevanceSignals,
) -> ContextSelectionAssessment {
    if let Some(assessment) = from_request_kind(signals) {
        return assessment;
    }

    let text = normalize_selection_text(&request.content);
    let char_len = text.chars().count();

    if let Some(complexity) = signals.complexity_id() {
        return from_complexity(complexity, &text);
    }

    lexical_fallback(&text, char_len, signals)
}

fn from_request_kind(signals: &RelevanceSignals) -> Option<ContextSelectionAssessment> {
    match signals.request_kind {
        RequestKind::Search | RequestKind::Discover | RequestKind::Index => Some(
            ContextSelectionAssessment::new(
                ContextSelectionProfile::Search,
                vec!["request_kind_search"],
            ),
        ),
        RequestKind::Git => Some(ContextSelectionAssessment::new(
            ContextSelectionProfile::Git,
            vec!["request_kind_git"],
        )),
        RequestKind::Terminal => Some(ContextSelectionAssessment::new(
            ContextSelectionProfile::Terminal,
            vec!["request_kind_terminal"],
        )),
        RequestKind::FileRead | RequestKind::FileWrite => Some(ContextSelectionAssessment::new(
            ContextSelectionProfile::FileEdit,
            vec!["request_kind_file"],
        )),
        RequestKind::Lsp => Some(ContextSelectionAssessment::new(
            ContextSelectionProfile::DebugCompile,
            vec!["request_kind_lsp"],
        )),
        RequestKind::ProjectSession => Some(ContextSelectionAssessment::new(
            ContextSelectionProfile::ProjectSession,
            vec!["request_kind_project_session"],
        )),
        RequestKind::Chat => None,
    }
}

fn from_complexity(complexity: &str, text: &str) -> ContextSelectionAssessment {
    match complexity {
        "greeting" => ContextSelectionAssessment::new(
            ContextSelectionProfile::Greeting,
            vec!["complexity_greeting"],
        ),
        "small_talk" => ContextSelectionAssessment::new(
            ContextSelectionProfile::SmallTalk,
            vec!["complexity_small_talk"],
        ),
        "coding_question" if has_debug_compile_cues(text) => ContextSelectionAssessment::new(
            ContextSelectionProfile::DebugCompile,
            vec!["complexity_coding", "refine_debug_compile"],
        ),
        "coding_question" => ContextSelectionAssessment::new(
            ContextSelectionProfile::CodingGeneral,
            vec!["complexity_coding"],
        ),
        "project_question" => ContextSelectionAssessment::new(
            ContextSelectionProfile::ProjectOverview,
            vec!["complexity_project"],
        ),
        "research_question" => ContextSelectionAssessment::new(
            ContextSelectionProfile::Research,
            vec!["complexity_research"],
        ),
        "general_question" if has_debug_compile_cues(text) => ContextSelectionAssessment::new(
            ContextSelectionProfile::DebugCompile,
            vec!["complexity_general", "refine_debug_compile"],
        ),
        "general_question" if has_project_overview_cues(text) => ContextSelectionAssessment::new(
            ContextSelectionProfile::ProjectOverview,
            vec!["complexity_general", "refine_project_overview"],
        ),
        "general_question" => ContextSelectionAssessment::new(
            ContextSelectionProfile::GeneralChat,
            vec!["complexity_general"],
        ),
        _ => ContextSelectionAssessment::new(
            ContextSelectionProfile::GeneralChat,
            vec!["complexity_unknown"],
        ),
    }
}

fn lexical_fallback(
    text: &str,
    char_len: usize,
    signals: &RelevanceSignals,
) -> ContextSelectionAssessment {
    if is_greeting_cue(text, char_len) {
        return ContextSelectionAssessment::new(
            ContextSelectionProfile::Greeting,
            vec!["lexical_greeting"],
        );
    }
    if is_small_talk_cue(text, char_len) {
        return ContextSelectionAssessment::new(
            ContextSelectionProfile::SmallTalk,
            vec!["lexical_small_talk"],
        );
    }
    if has_debug_compile_cues(text) {
        return ContextSelectionAssessment::new(
            ContextSelectionProfile::DebugCompile,
            vec!["lexical_debug_compile"],
        );
    }
    if has_project_overview_cues(text) {
        return ContextSelectionAssessment::new(
            ContextSelectionProfile::ProjectOverview,
            vec!["lexical_project_overview"],
        );
    }
    if signals.has_intent(IntentTag::Code) || signals.coding_workspace() {
        return ContextSelectionAssessment::new(
            ContextSelectionProfile::CodingGeneral,
            vec!["fallback_coding_workspace"],
        );
    }
    ContextSelectionAssessment::new(
        ContextSelectionProfile::GeneralChat,
        vec!["fallback_general"],
    )
}

/// Lowercase, strip most punctuation (keep `?` `'`), collapse whitespace.
pub fn normalize_selection_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_space = true;
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || lower == '?' || lower == '\'' {
            out.push(lower);
            last_space = false;
        } else if lower.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        }
    }
    out.trim().to_string()
}

fn is_greeting_cue(lower: &str, char_len: usize) -> bool {
    if char_len == 0 || char_len > 48 {
        return false;
    }
    if has_debug_compile_cues(lower) || has_project_overview_cues(lower) {
        return false;
    }
    const EXACT: &[&str] = &[
        "hello",
        "hi",
        "hey",
        "howdy",
        "yo",
        "hiya",
        "greetings",
        "hello jaymi",
        "hi jaymi",
        "hey jaymi",
    ];
    if EXACT.iter().any(|phrase| lower == *phrase) {
        return true;
    }
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
            && !lower.contains('?')
            && lower.split_whitespace().count() <= 5
    })
}

fn is_small_talk_cue(lower: &str, char_len: usize) -> bool {
    if char_len == 0 || char_len > 72 {
        return false;
    }
    if has_debug_compile_cues(lower) || has_project_overview_cues(lower) {
        return false;
    }
    const PHRASES: &[&str] = &[
        "thanks",
        "thank you",
        "thx",
        "ty",
        "how are you",
        "what's up",
        "whats up",
        "goodbye",
        "bye",
        "see you",
        "take care",
        "good night",
    ];
    PHRASES.iter().any(|phrase| {
        lower == *phrase
            || lower.starts_with(&format!("{phrase} "))
            || lower.ends_with(&format!(" {phrase}"))
    })
}

/// Compile / build / type-error debug cues.
pub fn has_debug_compile_cues(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "won't compile",
        "wont compile",
        "will not compile",
        "doesn't compile",
        "does not compile",
        "failed to compile",
        "could not compile",
        "compile error",
        "compiler error",
        "build error",
        "build failed",
        "cargo check",
        "cargo build",
        "cargo test",
        "type error",
        "syntax error",
        "borrow checker",
        "stack trace",
        "stacktrace",
        "runtime error",
        "linker error",
        "why won't this compile",
        "why wont this compile",
        "won't build",
        "wont build",
        "failing test",
        "test failure",
        "panic",
        "segfault",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
        || (lower.contains("compile")
            && (lower.contains("why")
                || lower.contains("error")
                || lower.contains("fail")
                || lower.contains("broken")
                || lower.contains("fix")))
        || (lower.contains("error")
            && (lower.contains("rustc")
                || lower.contains("cargo")
                || lower.contains("typescript")
                || lower.contains("build")))
}

/// Project overview / summarize cues.
pub fn has_project_overview_cues(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "summarize this project",
        "summarise this project",
        "summary of this project",
        "summarize the project",
        "summarise the project",
        "summarize this repo",
        "summarize the repo",
        "summarize this codebase",
        "overview of this project",
        "project overview",
        "what is this project",
        "what's this project",
        "describe this project",
        "explain this project",
        "architecture of this project",
        "project architecture",
        "repo structure",
        "codebase structure",
        "filesystem layout",
    ];
    if CUES.iter().any(|cue| lower.contains(cue)) {
        return true;
    }
    let summarize = lower.contains("summarize")
        || lower.contains("summarise")
        || lower.contains("overview")
        || lower.contains("architecture");
    let project = lower.contains("project")
        || lower.contains("repo")
        || lower.contains("codebase")
        || lower.contains("workspace");
    summarize && project
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relevance::RelevanceSignals;
    use crate::AssembleHints;
    use crate::ContextSessionInputs;
    use jaymi_core::IntentId;

    fn signals_with_complexity(text: &str, complexity: &str) -> (UserRequest, RelevanceSignals) {
        let request = UserRequest::new(text);
        let session = ContextSessionInputs::default();
        let hints = AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity(complexity);
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        (request, signals)
    }

    #[test]
    fn hello_selects_greeting_conversation_and_memory() {
        let (request, signals) = signals_with_complexity("hello", "greeting");
        let assessment = assess_context_selection(&request, &signals);
        assert_eq!(assessment.profile, ContextSelectionProfile::Greeting);
        assert!(assessment.profile.allows_provider("conversation"));
        assert!(assessment.profile.allows_provider("memory"));
        assert!(!assessment.profile.allows_provider("diagnostics"));
        assert!(!assessment.profile.allows_provider("runtime"));
    }

    #[test]
    fn why_wont_compile_selects_debug_feeds() {
        let (request, signals) =
            signals_with_complexity("why won't this compile?", "coding_question");
        let assessment = assess_context_selection(&request, &signals);
        assert_eq!(assessment.profile, ContextSelectionProfile::DebugCompile);
        assert!(assessment.profile.allows_provider("conversation"));
        assert!(assessment.profile.allows_provider("diagnostics"));
        assert!(assessment.profile.allows_provider("editor"));
        assert!(assessment.profile.allows_provider("runtime"));
        assert!(!assessment.profile.allows_provider("git_status"));
        assert!(!assessment.profile.allows_provider("workspace_inventory"));
        assert!(assessment
            .profile
            .prefers_kind(ContextCandidateKind::CurrentFile));
        assert!(assessment
            .profile
            .prefers_kind(ContextCandidateKind::Selection));
        assert!(assessment
            .profile
            .prefers_kind(ContextCandidateKind::RuntimeIntelligence));
    }

    #[test]
    fn summarize_project_selects_overview_feeds() {
        let (request, signals) =
            signals_with_complexity("summarize this project", "project_question");
        let assessment = assess_context_selection(&request, &signals);
        assert_eq!(assessment.profile, ContextSelectionProfile::ProjectOverview);
        assert!(assessment.profile.allows_provider("project"));
        assert!(assessment.profile.allows_provider("workspace_inventory"));
        assert!(assessment.profile.allows_provider("git_status"));
        assert!(assessment
            .profile
            .prefers_kind(ContextCandidateKind::ProjectIntelligence));
        assert!(!assessment.profile.allows_provider("diagnostics"));
        assert!(!assessment.profile.allows_provider("runtime"));
    }

    #[test]
    fn lexical_fallback_hello_without_complexity_hint() {
        let request = UserRequest::new("hello");
        let session = ContextSessionInputs::default();
        let signals = RelevanceSignals::derive(&request, &session);
        let assessment = assess_context_selection(&request, &signals);
        assert_eq!(assessment.profile, ContextSelectionProfile::Greeting);
    }
}
