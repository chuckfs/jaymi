//! Deterministic request-aware relevance for [`crate::ContextProvider`]s.
//!
//! No AI / model scoring. Intent semantics come **only** from canonical
//! [`jaymi_core::IntentId`] (Planner-supplied via [`AssembleHints`], or the
//! shared structured classifier when hints are absent). Optional
//! [`AssembleHints::complexity`] is Planner-authored conversational class —
//! Context never invents complexity or Intent from free text.

use jaymi_core::{IntentId, UserRequest};

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

/// Coarse request classification — **derived from [`IntentId`]**, not re-parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestKind {
    /// Free-text / chat with no structured tool payload (`IntentId::Unknown`).
    Chat,
    /// Structured or implied file read.
    FileRead,
    /// Structured write / path management.
    FileWrite,
    /// Search / project-knowledge search / directory listing.
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

    /// Map from canonical Intent identity.
    pub fn from_intent(intent: IntentId) -> Self {
        match intent.request_kind() {
            "file_read" => Self::FileRead,
            "file_write" => Self::FileWrite,
            "search" => Self::Search,
            "project_session" => Self::ProjectSession,
            "terminal" => Self::Terminal,
            "git" => Self::Git,
            "lsp" => Self::Lsp,
            "discover" => Self::Discover,
            "index" => Self::Index,
            _ => Self::Chat,
        }
    }
}

/// Relevance facet tags — **derived from [`IntentId`]**, not a parallel intent taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentTag {
    /// Conversational / free-text facet (`Unknown`).
    Chat,
    /// Search / find facet.
    Search,
    /// Read document / file facet.
    Read,
    /// Write / mutate filesystem facet.
    Write,
    /// Project open / switch / close facet.
    Project,
    /// Terminal facet.
    Terminal,
    /// Git facet.
    Git,
    /// Language server / coding assistance facet.
    Lsp,
    /// Discovery inventory facet.
    Discover,
    /// Indexing facet.
    Index,
    /// Software development / coding facet.
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

    fn from_label(label: &str) -> Option<Self> {
        match label {
            "chat" => Some(Self::Chat),
            "search" => Some(Self::Search),
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "project" => Some(Self::Project),
            "terminal" => Some(Self::Terminal),
            "git" => Some(Self::Git),
            "lsp" => Some(Self::Lsp),
            "discover" => Some(Self::Discover),
            "index" => Some(Self::Index),
            "code" => Some(Self::Code),
            _ => None,
        }
    }

    /// Tags for a canonical Intent.
    pub fn from_intent(intent: IntentId) -> Vec<Self> {
        intent
            .relevance_tags()
            .iter()
            .filter_map(|label| Self::from_label(label))
            .collect()
    }
}

/// Planner-authored environmental resolution summary (Sprint B2.10).
///
/// Produced by the Planner from Workspace Intelligence before assemble.
/// Context fingerprints it; Reasoning renders it. LLMs must not invent
/// workspace referents — they consume these bindings only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentalHints {
    /// True when the request contained workspace deixis.
    pub needed: bool,
    /// True when binding was ambiguous or unresolved.
    pub ambiguous: bool,
    /// Primary resolved path, when any.
    pub primary_path: Option<String>,
    /// Selection preview note, when selection was bound.
    pub selection_preview: Option<String>,
    /// Symbol name when known.
    pub symbol: Option<String>,
    /// Diagnostic summary when bound.
    pub diagnostic: Option<String>,
    /// Human-readable binding lines.
    pub bindings: Vec<String>,
    /// Rule ids that matched.
    pub rules: Vec<String>,
}

impl EnvironmentalHints {
    /// True when there is anything to surface to Reasoning.
    pub fn has_content(&self) -> bool {
        self.needed
            && (self.primary_path.is_some()
                || !self.bindings.is_empty()
                || self.selection_preview.is_some()
                || self.symbol.is_some()
                || self.diagnostic.is_some())
    }

    /// Fingerprint for cache keys.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.needed.hash(&mut hasher);
        self.ambiguous.hash(&mut hasher);
        self.primary_path.hash(&mut hasher);
        self.selection_preview.hash(&mut hasher);
        self.symbol.hash(&mut hasher);
        self.diagnostic.hash(&mut hasher);
        for binding in &self.bindings {
            binding.hash(&mut hasher);
        }
        for rule in &self.rules {
            rule.hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// Planner-resolved Intent + Capability passed into Context assemble.
///
/// Lives in `jaymi-context` (not planner) so there is no crate cycle.
/// Intent is the canonical [`IntentId`] — never a free-form parallel label.
/// Optional [`Self::complexity`] is a Planner-authored conversational class
/// id (e.g. `greeting`, `coding_question`) — Context never invents it.
/// Optional [`Self::environmental`] carries deixis bindings (B2.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembleHints {
    /// Canonical Intent resolved by the Planner (or shared structured classifier).
    pub intent: IntentId,
    /// Capability ids selected by the Decision Engine for this request.
    pub capability_ids: Vec<String>,
    /// Planner conversational complexity class id, when assessed.
    pub complexity: Option<String>,
    /// Environmental resolution bindings (Sprint B2.10), when deixis was present.
    pub environmental: Option<EnvironmentalHints>,
    /// Coding Understanding focus id (Sprint C1.1), when understanding mode.
    pub understanding: Option<String>,
    /// Coding Review focus id (Sprint C1.3), when review mode.
    pub review: Option<String>,
    /// Coding Plan kind id (Sprint C1.4), when generation-planning mode.
    pub coding_plan: Option<String>,
}

impl Default for AssembleHints {
    fn default() -> Self {
        Self {
            intent: IntentId::Unknown,
            capability_ids: Vec::new(),
            complexity: None,
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
        }
    }
}

impl AssembleHints {
    /// Build hints from a canonical Intent and capability ids.
    pub fn new(intent: IntentId, capability_ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            intent,
            capability_ids: capability_ids.into_iter().collect(),
            complexity: None,
            environmental: None,
            understanding: None,
            review: None,
            coding_plan: None,
        }
    }

    /// Attach a Planner complexity class id (does not change Intent / capabilities).
    pub fn with_complexity(mut self, complexity: impl Into<String>) -> Self {
        self.complexity = Some(complexity.into());
        self
    }

    /// Attach environmental resolution (does not change Intent / capabilities).
    pub fn with_environmental(mut self, environmental: EnvironmentalHints) -> Self {
        self.environmental = Some(environmental);
        self
    }

    /// Attach Coding Understanding focus (does not change Intent / capabilities).
    pub fn with_understanding(mut self, understanding: impl Into<String>) -> Self {
        self.understanding = Some(understanding.into());
        self
    }

    /// Attach Coding Review focus (does not change Intent / capabilities).
    pub fn with_review(mut self, review: impl Into<String>) -> Self {
        self.review = Some(review.into());
        self
    }

    /// Attach Coding Plan kind (does not change Intent / capabilities).
    pub fn with_coding_plan(mut self, coding_plan: impl Into<String>) -> Self {
        self.coding_plan = Some(coding_plan.into());
        self
    }

    /// Fingerprint for cache keys.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.intent.as_str().hash(&mut hasher);
        for id in &self.capability_ids {
            id.hash(&mut hasher);
        }
        if let Some(complexity) = &self.complexity {
            complexity.hash(&mut hasher);
        }
        if let Some(env) = &self.environmental {
            env.fingerprint().hash(&mut hasher);
        }
        if let Some(understanding) = &self.understanding {
            understanding.hash(&mut hasher);
        }
        if let Some(review) = &self.review {
            review.hash(&mut hasher);
        }
        if let Some(coding_plan) = &self.coding_plan {
            coding_plan.hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// Precomputed, deterministic cues for provider relevance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevanceSignals {
    /// Canonical Intent for this assemble.
    pub intent: IntentId,
    /// Primary request kind derived from [`Self::intent`].
    pub request_kind: RequestKind,
    /// Relevance facets derived from [`Self::intent`].
    pub intent_tags: Vec<IntentTag>,
    /// Active UX workspace kind id, when set.
    pub workspace_kind: Option<String>,
    /// Active capability ids: Planner-selected first, then session, then Intent defaults.
    pub active_capabilities: Vec<String>,
    /// Canonical Intent label (same as `intent.as_str()`).
    pub planner_intent: Option<String>,
    /// Planner conversational complexity class id (`greeting`, …), when set.
    pub complexity: Option<String>,
}

impl RelevanceSignals {
    /// Build signals from the inbound request and session (no AI).
    ///
    /// Without Planner hints, uses [`IntentId::from_structured_request`] only —
    /// free-text content never invents Intent here.
    pub fn derive(request: &UserRequest, session: &ContextSessionInputs) -> Self {
        Self::derive_with(request, session, None)
    }

    /// Build signals from Planner-resolved Intent / Capability when hints are set.
    ///
    /// When `hints` is `Some`, [`AssembleHints::intent`] and
    /// [`AssembleHints::capability_ids`] are authoritative — session capability
    /// ids and workspace heuristics never invent Capabilities.
    /// When `None`, Intent comes from the shared structured classifier and
    /// capability ids fall back to [`IntentId::default_capability_ids`] only.
    pub fn derive_with(
        request: &UserRequest,
        session: &ContextSessionInputs,
        hints: Option<&AssembleHints>,
    ) -> Self {
        let intent = match hints {
            Some(hints) => hints.intent,
            None => IntentId::from_structured_request(request),
        };
        let request_kind = RequestKind::from_intent(intent);
        let intent_tags = IntentTag::from_intent(intent);
        let planner_intent = Some(intent.as_str().to_string());
        let complexity = hints.and_then(|hints| hints.complexity.clone());

        let workspace_kind = session.workspace_kind.clone();
        let mut active_capabilities = Vec::new();

        match hints {
            Some(hints) => {
                // Planner owns capability selection for the request path.
                for id in &hints.capability_ids {
                    push_unique_string(&mut active_capabilities, id.clone());
                }
            }
            None => {
                // Direct ContextEngine tests only — Intent defaults, never session catalog.
                for id in intent.default_capability_ids() {
                    push_unique_string(&mut active_capabilities, (*id).to_string());
                }
            }
        }

        Self {
            intent,
            request_kind,
            intent_tags,
            workspace_kind,
            active_capabilities,
            planner_intent,
            complexity,
        }
    }

    /// True when the given relevance facet was derived from Intent.
    pub fn has_intent(&self, tag: IntentTag) -> bool {
        self.intent_tags.contains(&tag)
    }

    /// True when a capability id is active.
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

    /// Planner complexity class id, when present.
    pub fn complexity_id(&self) -> Option<&str> {
        self.complexity.as_deref()
    }

    /// Participation tier for a provider under the current Planner complexity class.
    ///
    /// Returns `None` when no complexity hint is present (normal assemble).
    pub fn complexity_tier_for(&self, provider_id: &str) -> Option<ComplexityProviderTier> {
        let complexity = self.complexity.as_deref()?;
        Some(complexity_provider_tier(complexity, provider_id))
    }

    /// Effective relevance score after complexity tier rules.
    pub fn score_for_provider(&self, provider_id: &str, base: RelevanceScore) -> RelevanceScore {
        match self.complexity_tier_for(provider_id) {
            Some(ComplexityProviderTier::Required) => RelevanceScore::HIGH,
            Some(ComplexityProviderTier::Excluded) => RelevanceScore::NONE,
            Some(ComplexityProviderTier::Optional) | None => base,
        }
    }
}

/// How a provider participates under a Planner complexity class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityProviderTier {
    /// Always considered for assemble (high relevance floor).
    Required,
    /// Normal relevance / policy / budget path.
    Optional,
    /// Skipped before policy — lightweight assemble.
    Excluded,
}

/// Deterministic provider tier for a Planner complexity class id.
pub fn complexity_provider_tier(complexity: &str, provider_id: &str) -> ComplexityProviderTier {
    match complexity {
        "greeting" | "small_talk" => match provider_id {
            "conversation" | "memory" => ComplexityProviderTier::Required,
            "search" | "workspace" | "diagnostics" | "project" | "editor" | "git_status"
            | "workspace_inventory" | "file_summaries" | "runtime" | "workspace_memory" => {
                ComplexityProviderTier::Excluded
            }
            _ => ComplexityProviderTier::Optional,
        },
        "general_question" => match provider_id {
            "conversation" => ComplexityProviderTier::Required,
            _ => ComplexityProviderTier::Optional,
        },
        "coding_question" => match provider_id {
            "conversation" | "workspace" | "diagnostics" | "project" | "editor" | "git_status"
            | "file_summaries" | "runtime" | "workspace_memory" => {
                ComplexityProviderTier::Required
            }
            _ => ComplexityProviderTier::Optional,
        },
        "project_question" => match provider_id {
            "conversation" | "project" | "workspace" | "workspace_inventory" | "git_status" => {
                ComplexityProviderTier::Required
            }
            _ => ComplexityProviderTier::Optional,
        },
        "research_question" => match provider_id {
            "conversation" | "search" => ComplexityProviderTier::Required,
            _ => ComplexityProviderTier::Optional,
        },
        _ => ComplexityProviderTier::Optional,
    }
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
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
        assert_eq!(signals.intent, IntentId::SearchKnowledge);
        assert_eq!(signals.request_kind, RequestKind::Search);
        assert!(signals.has_intent(IntentTag::Search));
        assert!(signals.has_capability("search"));
    }

    #[test]
    fn free_text_does_not_invent_search_intent() {
        let signals = RelevanceSignals::derive(
            &UserRequest::new("search for fungi please"),
            &ContextSessionInputs::default(),
        );
        assert_eq!(signals.intent, IntentId::Unknown);
        assert_eq!(signals.request_kind, RequestKind::Chat);
        assert!(signals.has_intent(IntentTag::Chat));
        assert!(!signals.has_intent(IntentTag::Search));
    }

    #[test]
    fn derives_chat_for_plain_text() {
        let signals = RelevanceSignals::derive(
            &UserRequest::new("hello there"),
            &ContextSessionInputs::default(),
        );
        assert_eq!(signals.intent, IntentId::Unknown);
        assert_eq!(signals.request_kind, RequestKind::Chat);
        assert!(signals.has_intent(IntentTag::Chat));
    }

    #[test]
    fn coding_workspace_marks_coding_surface_without_inventing_capabilities() {
        let mut session = ContextSessionInputs::default();
        session.workspace_kind = Some("coding".into());
        let signals = RelevanceSignals::derive(&UserRequest::new("hi"), &session);
        assert!(signals.coding_workspace());
        // Without Planner hints, Unknown → chat default only — never invent "code".
        assert!(signals.has_capability("chat"));
        assert!(!signals.has_capability("code"));
    }

    #[test]
    fn planner_hints_are_authoritative_for_intent_and_capabilities() {
        let hints = AssembleHints::new(IntentId::Lsp, ["code".into()]);
        let signals = RelevanceSignals::derive_with(
            &UserRequest::new("hello"),
            &ContextSessionInputs::default(),
            Some(&hints),
        );
        assert_eq!(signals.intent, IntentId::Lsp);
        assert_eq!(signals.planner_intent.as_deref(), Some("lsp"));
        assert_eq!(signals.request_kind, RequestKind::Lsp);
        assert_eq!(
            signals.active_capabilities.first().map(String::as_str),
            Some("code")
        );
        assert!(signals.has_capability("code"));
        assert!(signals.has_intent(IntentTag::Lsp));
        assert!(signals.has_intent(IntentTag::Code));
        // Free-text "hello" must not keep Chat facets when Planner said Lsp.
        assert!(!signals.has_intent(IntentTag::Chat));
        assert!(signals.complexity.is_none());
    }

    #[test]
    fn complexity_tiers_drive_participation() {
        assert_eq!(
            complexity_provider_tier("greeting", "conversation"),
            ComplexityProviderTier::Required
        );
        assert_eq!(
            complexity_provider_tier("greeting", "memory"),
            ComplexityProviderTier::Required
        );
        assert_eq!(
            complexity_provider_tier("general_question", "memory"),
            ComplexityProviderTier::Optional
        );
        assert_eq!(
            complexity_provider_tier("coding_question", "diagnostics"),
            ComplexityProviderTier::Required
        );

        let hints = AssembleHints::new(IntentId::Unknown, Vec::<String>::new())
            .with_complexity("greeting");
        let signals = RelevanceSignals::derive_with(
            &UserRequest::new("hi"),
            &ContextSessionInputs::default(),
            Some(&hints),
        );
        assert_eq!(signals.complexity_id(), Some("greeting"));
        assert_eq!(
            signals.complexity_tier_for("search"),
            Some(ComplexityProviderTier::Excluded)
        );
        let base = RelevanceScore::new(70);
        assert_eq!(
            signals.score_for_provider("project", base).value(),
            0,
            "excluded tier should not use base score when skipped upstream"
        );
        assert_eq!(
            signals.score_for_provider("conversation", base).value(),
            RelevanceScore::HIGH.value()
        );
    }

    #[test]
    fn planner_capabilities_ignore_session_catalog() {
        let hints = AssembleHints::new(IntentId::SearchKnowledge, ["search".into()]);
        let mut session = ContextSessionInputs::default();
        session.workspace_kind = Some("coding".into());
        session.active_capabilities.capability_ids =
            vec!["code".into(), "lsp".into(), "search".into()];
        let signals = RelevanceSignals::derive_with(
            &UserRequest::new("hello"),
            &session,
            Some(&hints),
        );
        assert_eq!(signals.active_capabilities, vec!["search".to_string()]);
        assert!(!signals.has_capability("code"));
        assert!(!signals.has_capability("chat"));
    }

    #[test]
    fn empty_planner_capabilities_stay_empty() {
        let hints = AssembleHints::new(IntentId::Unknown, Vec::<String>::new());
        let mut session = ContextSessionInputs::default();
        session.active_capabilities.capability_ids = vec!["code".into()];
        let signals = RelevanceSignals::derive_with(
            &UserRequest::new("hello"),
            &session,
            Some(&hints),
        );
        assert!(signals.active_capabilities.is_empty());
    }

    #[test]
    fn ownership_search_provider_does_not_need_project_engine() {
        // Compile-time ownership: SearchProvider::new takes only SearchEngineApi.
        // Runtime: session.project_indexed_documents drives the hint.
        let mut session = ContextSessionInputs::default();
        session.project_indexed_documents = Some(7);
        let signals = RelevanceSignals::derive(
            &UserRequest::search(SearchRequest::free_text("x")),
            &session,
        );
        assert_eq!(signals.intent, IntentId::SearchKnowledge);
        assert_eq!(session.project_indexed_documents, Some(7));
    }
}
