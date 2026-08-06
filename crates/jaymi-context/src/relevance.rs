//! Deterministic request-aware relevance for [`crate::ContextProvider`]s.
//!
//! No AI / model scoring. Heuristics use structured request fields, coarse
//! intent tags, active capability ids, and workspace kind.

use jaymi_core::UserRequest;

use crate::ContextSessionInputs;

/// Inclusive maximum relevance score.
pub const RELEVANCE_MAX: u8 = 100;

/// Default minimum score required for a provider to be asked to contribute.
pub const DEFAULT_RELEVANCE_THRESHOLD: u8 = 40;

/// Lightweight 0..=100 relevance score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelevanceScore(u8);

impl RelevanceScore {
    /// Clamp `value` into `0..=RELEVANCE_MAX`.
    pub fn new(value: u8) -> Self {
        Self(value.min(RELEVANCE_MAX))
    }

    /// Saturating sum of weighted heuristic parts, clamped to [`RELEVANCE_MAX`].
    pub fn from_parts(parts: impl IntoIterator<Item = u8>) -> Self {
        let mut total: u16 = 0;
        for part in parts {
            total = total.saturating_add(u16::from(part));
        }
        Self::new(total.min(u16::from(RELEVANCE_MAX)) as u8)
    }

    /// Raw score value.
    pub fn value(self) -> u8 {
        self.0
    }

    /// True when this score meets or exceeds `threshold`.
    pub fn meets(self, threshold: u8) -> bool {
        self.0 >= threshold
    }

    /// Irrelevant / should skip.
    pub const NONE: Self = Self(0);

    /// Strongly relevant.
    pub const HIGH: Self = Self(80);

    /// Marginally relevant.
    pub const LOW: Self = Self(25);
}

impl std::fmt::Display for RelevanceScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Coarse request classification derived from structured [`UserRequest`] fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestKind {
    /// Free-text / chat with no structured tool payload.
    Chat,
    /// Structured or implied file read.
    FileRead,
    /// Structured write / path management.
    FileWrite,
    /// Search / project-knowledge search.
    Search,
    /// Open / close / switch project session.
    ProjectSession,
    /// Terminal operation.
    Terminal,
    /// Git operation.
    Git,
    /// Language server operation.
    Lsp,
    /// Discovery inventory query.
    Discover,
    /// Index / scan roots.
    Index,
}

impl RequestKind {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::Search => "search",
            Self::ProjectSession => "project_session",
            Self::Terminal => "terminal",
            Self::Git => "git",
            Self::Lsp => "lsp",
            Self::Discover => "discover",
            Self::Index => "index",
        }
    }
}

/// Deterministic intent tags used by relevance heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentTag {
    /// Conversational / free-text intent.
    Chat,
    /// Search / find intent.
    Search,
    /// Read document / file intent.
    Read,
    /// Write / mutate filesystem intent.
    Write,
    /// Project open / switch / close intent.
    Project,
    /// Terminal intent.
    Terminal,
    /// Git intent.
    Git,
    /// Language server / coding assistance intent.
    Lsp,
    /// Discovery inventory intent.
    Discover,
    /// Indexing intent.
    Index,
    /// Software development / coding intent.
    Code,
}

impl IntentTag {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Search => "search",
            Self::Read => "read",
            Self::Write => "write",
            Self::Project => "project",
            Self::Terminal => "terminal",
            Self::Git => "git",
            Self::Lsp => "lsp",
            Self::Discover => "discover",
            Self::Index => "index",
            Self::Code => "code",
        }
    }
}

/// Precomputed, deterministic cues for provider relevance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevanceSignals {
    /// Primary structured request kind.
    pub request_kind: RequestKind,
    /// Coarse intent tags derived from the request.
    pub intent_tags: Vec<IntentTag>,
    /// Active UX workspace kind id, when set.
    pub workspace_kind: Option<String>,
    /// Active capability ids from the session (and inferred from request kind).
    pub active_capabilities: Vec<String>,
}

impl RelevanceSignals {
    /// Build signals from the inbound request and session (no AI).
    pub fn derive(request: &UserRequest, session: &ContextSessionInputs) -> Self {
        let mut intent_tags = Vec::new();
        let mut request_kind = RequestKind::Chat;

        if request.close_project || request.open_project_id.is_some() {
            request_kind = RequestKind::ProjectSession;
            push_unique(&mut intent_tags, IntentTag::Project);
        }
        if request.project_knowledge.is_some() || request.search.is_some() {
            request_kind = RequestKind::Search;
            push_unique(&mut intent_tags, IntentTag::Search);
        }
        if request.discover || request.discovery_kind.is_some() {
            request_kind = RequestKind::Discover;
            push_unique(&mut intent_tags, IntentTag::Discover);
        }
        if request.index_root.is_some() {
            request_kind = RequestKind::Index;
            push_unique(&mut intent_tags, IntentTag::Index);
        }
        if request.lsp.is_some() {
            request_kind = RequestKind::Lsp;
            push_unique(&mut intent_tags, IntentTag::Lsp);
            push_unique(&mut intent_tags, IntentTag::Code);
        }
        if request.git.is_some() {
            request_kind = RequestKind::Git;
            push_unique(&mut intent_tags, IntentTag::Git);
            push_unique(&mut intent_tags, IntentTag::Code);
        }
        if request.terminal.is_some() {
            request_kind = RequestKind::Terminal;
            push_unique(&mut intent_tags, IntentTag::Terminal);
            push_unique(&mut intent_tags, IntentTag::Code);
        }
        if request.write_file.is_some() || request.manage_path.is_some() {
            request_kind = RequestKind::FileWrite;
            push_unique(&mut intent_tags, IntentTag::Write);
            push_unique(&mut intent_tags, IntentTag::Code);
        }
        if request.file.is_some() {
            request_kind = RequestKind::FileRead;
            push_unique(&mut intent_tags, IntentTag::Read);
        }
        if request.directory.is_some() || request.project_tree.is_some() {
            if matches!(request_kind, RequestKind::Chat) {
                request_kind = RequestKind::Search;
            }
            push_unique(&mut intent_tags, IntentTag::Search);
        }

        let content = request.content.to_ascii_lowercase();
        if !content.trim().is_empty() {
            if contains_any(
                &content,
                &[
                    "search ",
                    "find ",
                    "look for",
                    "where is",
                    "locate ",
                    "grep ",
                ],
            ) {
                push_unique(&mut intent_tags, IntentTag::Search);
            }
            if contains_any(
                &content,
                &[
                    "continue working",
                    "switch to project",
                    "open project",
                    "close project",
                    "leave the project",
                ],
            ) {
                push_unique(&mut intent_tags, IntentTag::Project);
            }
            if contains_any(&content, &["commit ", "git ", "pull request", "branch "]) {
                push_unique(&mut intent_tags, IntentTag::Git);
                push_unique(&mut intent_tags, IntentTag::Code);
            }
            if contains_any(&content, &["terminal", "shell", "run command"]) {
                push_unique(&mut intent_tags, IntentTag::Terminal);
                push_unique(&mut intent_tags, IntentTag::Code);
            }
            if contains_any(
                &content,
                &["refactor", "compile", "type error", "lsp", "rust analyzer"],
            ) {
                push_unique(&mut intent_tags, IntentTag::Code);
                push_unique(&mut intent_tags, IntentTag::Lsp);
            }
            if matches!(request_kind, RequestKind::Chat)
                && intent_tags.is_empty()
                && request.search.is_none()
                && request.file.is_none()
            {
                push_unique(&mut intent_tags, IntentTag::Chat);
            }
        }

        if intent_tags.is_empty() {
            push_unique(&mut intent_tags, IntentTag::Chat);
        }

        let workspace_kind = session.workspace_kind.clone();
        let mut active_capabilities = session.active_capabilities.capability_ids.clone();
        for inferred in inferred_capabilities(request_kind, &intent_tags, workspace_kind.as_deref())
        {
            if !active_capabilities.iter().any(|id| id == &inferred) {
                active_capabilities.push(inferred);
            }
        }

        Self {
            request_kind,
            intent_tags,
            workspace_kind,
            active_capabilities,
        }
    }

    /// True when the given intent tag was derived.
    pub fn has_intent(&self, tag: IntentTag) -> bool {
        self.intent_tags.contains(&tag)
    }

    /// True when a capability id is active (session or inferred).
    pub fn has_capability(&self, id: &str) -> bool {
        self.active_capabilities.iter().any(|value| value == id)
    }

    /// True when the active workspace kind matches `id`.
    pub fn workspace_is(&self, id: &str) -> bool {
        self.workspace_kind.as_deref() == Some(id)
    }

    /// True when the workspace looks like a coding surface.
    pub fn coding_workspace(&self) -> bool {
        matches!(
            self.workspace_kind.as_deref(),
            Some("coding") | Some("code") | Some("development")
        )
    }
}

fn inferred_capabilities(
    kind: RequestKind,
    tags: &[IntentTag],
    workspace: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    match kind {
        RequestKind::Search | RequestKind::Discover => out.push("search".into()),
        RequestKind::FileRead => out.push("read_documents".into()),
        RequestKind::FileWrite => out.push("file_management".into()),
        RequestKind::Terminal => out.push("execute_terminal_commands".into()),
        RequestKind::Git | RequestKind::Lsp => out.push("code".into()),
        RequestKind::Index => out.push("index".into()),
        RequestKind::ProjectSession => out.push("code".into()),
        RequestKind::Chat => out.push("chat".into()),
    }
    if tags.contains(&IntentTag::Code) || matches!(workspace, Some("coding") | Some("code")) {
        if !out.iter().any(|id| id == "code") {
            out.push("code".into());
        }
    }
    if tags.contains(&IntentTag::Search) && !out.iter().any(|id| id == "search") {
        out.push("search".into());
    }
    out
}

fn push_unique(tags: &mut Vec<IntentTag>, tag: IntentTag) {
    if !tags.contains(&tag) {
        tags.push(tag);
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextSessionInputs;
    use jaymi_core::SearchRequest;

    #[test]
    fn derives_search_kind_from_structured_search() {
        let signals = RelevanceSignals::derive(
            &UserRequest::search(SearchRequest::free_text("fungi")),
            &ContextSessionInputs::default(),
        );
        assert_eq!(signals.request_kind, RequestKind::Search);
        assert!(signals.has_intent(IntentTag::Search));
        assert!(signals.has_capability("search"));
    }

    #[test]
    fn derives_chat_for_plain_text() {
        let signals = RelevanceSignals::derive(
            &UserRequest::new("hello there"),
            &ContextSessionInputs::default(),
        );
        assert_eq!(signals.request_kind, RequestKind::Chat);
        assert!(signals.has_intent(IntentTag::Chat));
    }

    #[test]
    fn coding_workspace_marks_coding_surface() {
        let mut session = ContextSessionInputs::default();
        session.workspace_kind = Some("coding".into());
        let signals = RelevanceSignals::derive(&UserRequest::new("hi"), &session);
        assert!(signals.coding_workspace());
        assert!(signals.has_capability("code"));
    }
}
