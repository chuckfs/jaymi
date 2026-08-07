//! Context Candidate Graph (Sprint B2.7).
//!
//! Workspace Intelligence and other Context Providers expose
//! [`ContextCandidate`] nodes instead of writing a finished
//! [`crate::ContextBundle`]. The Context Engine + Context Policy select
//! candidates by **relevance**, **recency**, **importance**, **privacy**, and
//! **budget**; only selected candidates are materialized into bundle sections.
//!
//! ## Ownership
//!
//! | Role | Owns |
//! |------|------|
//! | Context Providers | Propose candidates (never assemble bundles) |
//! | Context Policy | Score / filter candidates |
//! | Context Engine | Select under budget; materialize → `ContextBundle` |
//! | Planner | Orchestration only — ownership unchanged |
//!
//! Providers must not construct [`crate::ContextBundle`] or
//! [`crate::ContextBundleBuilder`].

use serde::Serialize;

use crate::budget::ProviderPriority;
use crate::bundle::{
    ActiveCapabilitiesSection, ActiveProjectSection, ActiveWorkspaceSection, BundleDiagnostic,
    ContextSource, ConversationSection, CurrentFileSection, CurrentSelectionSection,
    DiagnosticsSection, FileSummariesSection, FileSummaryEntry, GitStatusSection,
    MemoryResultsSection, OpenFileEntry, OpenFilesSection, PermissionsSection,
    SearchResultsSection, WorkspaceInventorySection,
};
use crate::policy::Sensitivity;
use crate::provider::ContextContribution;
use crate::EditorIntelligenceSection;
use crate::ProjectIntelligenceSection;
use crate::RuntimeIntelligenceSection;
use crate::WorkspaceMemorySection;

/// Stable candidate identity within one assemble.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ContextCandidateId(pub String);

impl ContextCandidateId {
    /// Build `provider:kind:key`.
    pub fn new(provider_id: &str, kind: ContextCandidateKind, key: impl AsRef<str>) -> Self {
        Self(format!(
            "{provider_id}:{}:{}",
            kind.as_str(),
            key.as_ref()
        ))
    }
}

/// What kind of workspace / context fact this candidate carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCandidateKind {
    /// Conversation summary / identity.
    Conversation,
    /// Active project identity.
    ProjectIdentity,
    /// Project intelligence facts.
    ProjectIntelligence,
    /// Active UX workspace kind.
    WorkspaceKind,
    /// Request-selected capability ids.
    Capabilities,
    /// Current / active editor file.
    CurrentFile,
    /// Current selection / caret.
    Selection,
    /// One open editor tab.
    OpenFile,
    /// Open editors set (legacy coarse).
    OpenFiles,
    /// Editor intelligence (symbol / hover / refs…).
    EditorIntelligence,
    /// One diagnostic finding.
    Diagnostic,
    /// Diagnostics set (legacy coarse).
    Diagnostics,
    /// Git status summary.
    GitStatus,
    /// Runtime intelligence summary.
    RuntimeIntelligence,
    /// Workspace activity memory.
    WorkspaceMemory,
    /// Workspace inventory summary.
    WorkspaceInventory,
    /// One file summary entry.
    FileSummary,
    /// File summaries set (legacy coarse).
    FileSummaries,
    /// Search hits / coordination.
    SearchResults,
    /// Memory results.
    MemoryResults,
    /// Permission grants.
    Permissions,
    /// Opaque multi-section legacy contribution.
    LegacyContribution,
}

impl ContextCandidateKind {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::ProjectIdentity => "project_identity",
            Self::ProjectIntelligence => "project_intelligence",
            Self::WorkspaceKind => "workspace_kind",
            Self::Capabilities => "capabilities",
            Self::CurrentFile => "current_file",
            Self::Selection => "selection",
            Self::OpenFile => "open_file",
            Self::OpenFiles => "open_files",
            Self::EditorIntelligence => "editor_intelligence",
            Self::Diagnostic => "diagnostic",
            Self::Diagnostics => "diagnostics",
            Self::GitStatus => "git_status",
            Self::RuntimeIntelligence => "runtime_intelligence",
            Self::WorkspaceMemory => "workspace_memory",
            Self::WorkspaceInventory => "workspace_inventory",
            Self::FileSummary => "file_summary",
            Self::FileSummaries => "file_summaries",
            Self::SearchResults => "search_results",
            Self::MemoryResults => "memory_results",
            Self::Permissions => "permissions",
            Self::LegacyContribution => "legacy_contribution",
        }
    }
}

/// Payload the engine knows how to fold into a [`ContextContribution`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidatePayload {
    Conversation(ConversationSection),
    ActiveProject(ActiveProjectSection),
    ProjectIntelligence(ProjectIntelligenceSection),
    ActiveWorkspace(ActiveWorkspaceSection),
    ActiveCapabilities(ActiveCapabilitiesSection),
    CurrentFile(CurrentFileSection),
    CurrentSelection(CurrentSelectionSection),
    OpenFiles(OpenFilesSection),
    OpenFile(OpenFileEntry),
    EditorIntelligence(EditorIntelligenceSection),
    Diagnostics(DiagnosticsSection),
    Diagnostic(BundleDiagnostic),
    GitStatus(GitStatusSection),
    RuntimeIntelligence(RuntimeIntelligenceSection),
    WorkspaceMemory(WorkspaceMemorySection),
    WorkspaceInventory(WorkspaceInventorySection),
    FileSummaries(FileSummariesSection),
    FileSummary(FileSummaryEntry),
    SearchResults(SearchResultsSection),
    MemoryResults(MemoryResultsSection),
    Permissions(PermissionsSection),
    Legacy(Box<ContextContribution>),
}

impl CandidatePayload {
    /// Rough character estimate for budgeting.
    pub fn estimated_chars(&self) -> usize {
        match self {
            Self::Conversation(section) => {
                section.id.as_ref().map(|s| s.len()).unwrap_or(0)
                    + section.title.as_ref().map(|s| s.len()).unwrap_or(0)
                    + 48
            }
            Self::ActiveProject(section) => {
                section.project_id.as_ref().map(|s| s.len()).unwrap_or(0)
                    + section.name.as_ref().map(|s| s.len()).unwrap_or(0)
                    + section
                        .root_directory
                        .as_ref()
                        .map(|s| s.len())
                        .unwrap_or(0)
                    + if section.detail.is_some() { 2_048 } else { 64 }
            }
            Self::ProjectIntelligence(section) => {
                128 + section.languages.iter().map(|s| s.len() + 2).sum::<usize>()
                    + section.frameworks.iter().map(|s| s.len() + 2).sum::<usize>()
                    + section
                        .dependency_summary
                        .top_level
                        .iter()
                        .map(|s| s.len() + 2)
                        .sum::<usize>()
            }
            Self::ActiveWorkspace(section) => {
                section.kind_id.as_ref().map(|s| s.len()).unwrap_or(0) + 16
            }
            Self::ActiveCapabilities(section) => {
                section
                    .capability_ids
                    .iter()
                    .map(|s| s.len() + 1)
                    .sum::<usize>()
                    + 16
            }
            Self::CurrentFile(section) => {
                section.path.as_ref().map(|s| s.len()).unwrap_or(0) + 16
            }
            Self::CurrentSelection(section) => {
                section.path.as_ref().map(|s| s.len()).unwrap_or(0)
                    + section.text.as_ref().map(|s| s.len()).unwrap_or(0)
                    + 32
            }
            Self::OpenFiles(section) => section
                .files
                .iter()
                .map(|f| f.path.len() + 8)
                .sum::<usize>()
                .max(16),
            Self::OpenFile(entry) => entry.path.len() + 8,
            Self::EditorIntelligence(section) => {
                let mut n = 64usize;
                if let Some(hover) = &section.hover {
                    n += hover.contents.len().min(2_048);
                }
                if let Some(symbol) = &section.symbol {
                    n += symbol.name.len();
                }
                n += section.references.len().saturating_mul(48);
                n
            }
            Self::Diagnostics(section) => section
                .diagnostics
                .iter()
                .map(|d| d.message.len() + d.severity.len() + 16)
                .sum::<usize>()
                .max(16),
            Self::Diagnostic(diag) => diag.message.len() + diag.severity.len() + 16,
            Self::GitStatus(section) => {
                section.summary.len()
                    + section.sample_paths.iter().map(|p| p.len() + 1).sum::<usize>()
                    + section
                        .recent_commits
                        .iter()
                        .map(|c| c.subject.len() + 8)
                        .sum::<usize>()
                    + 64
            }
            Self::RuntimeIntelligence(section) => {
                let mut n = 64usize;
                n += section
                    .latest_cargo_check
                    .as_ref()
                    .map(|s| s.len())
                    .unwrap_or(0);
                n += section.latest_build.as_ref().map(|s| s.len()).unwrap_or(0);
                n += section.latest_tests.as_ref().map(|s| s.len()).unwrap_or(0);
                n += section.output_tail.len().min(640);
                n += section.running.iter().map(|s| s.len() + 2).sum::<usize>();
                n += section
                    .recent_failures
                    .iter()
                    .map(|s| s.len() + 2)
                    .sum::<usize>();
                n
            }
            Self::WorkspaceMemory(section) => {
                let mut n = 48usize;
                n += section
                    .coding_objective
                    .as_ref()
                    .map(|s| s.len())
                    .unwrap_or(0);
                n += section.recent_edits.iter().map(|s| s.len() + 2).sum::<usize>();
                n += section
                    .recently_opened
                    .iter()
                    .map(|s| s.len() + 2)
                    .sum::<usize>();
                n += section.recent_builds.iter().map(|s| s.len() + 2).sum::<usize>();
                n += section
                    .recent_failures
                    .iter()
                    .map(|s| s.len() + 2)
                    .sum::<usize>();
                n
            }
            Self::WorkspaceInventory(section) => {
                section.root.as_ref().map(|s| s.len()).unwrap_or(0)
                    + section.status.len()
                    + 48
            }
            Self::FileSummaries(section) => section
                .entries
                .iter()
                .map(|e| e.path.len() + e.summary.len() + 8)
                .sum::<usize>()
                .max(16),
            Self::FileSummary(entry) => entry.path.len() + entry.summary.len() + 8,
            Self::SearchResults(section) => {
                section
                    .hint
                    .as_ref()
                    .map(|h| h.query_preview.as_ref().map(|q| q.len()).unwrap_or(0) + 32)
                    .unwrap_or(0)
                    + section
                        .hits
                        .iter()
                        .map(|h| {
                            h.path.as_ref().map(|p| p.len()).unwrap_or(0)
                                + h.preview.as_ref().map(|p| p.len()).unwrap_or(0)
                                + h.title.len()
                                + 8
                        })
                        .sum::<usize>()
                    + 32
            }
            Self::MemoryResults(section) => {
                section
                    .memory
                    .memories
                    .iter()
                    .map(|m| m.record.content.len() + 16)
                    .sum::<usize>()
                    + 32
            }
            Self::Permissions(section) => section.entries.len().saturating_mul(24).max(16),
            Self::Legacy(contribution) => {
                crate::budget::measure_contribution(contribution, 4).characters
            }
        }
    }
}

/// One proposed context fact for policy selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCandidate {
    pub id: ContextCandidateId,
    pub provider_id: &'static str,
    pub source: ContextSource,
    pub kind: ContextCandidateKind,
    pub payload: CandidatePayload,
    pub sensitivity: Sensitivity,
    pub importance: u8,
    pub recency: Option<i64>,
    pub provider_priority: ProviderPriority,
    pub required: bool,
}

impl ContextCandidate {
    /// Construct a candidate node (providers propose; engine materializes).
    pub fn new(
        provider_id: &'static str,
        kind: ContextCandidateKind,
        source: ContextSource,
        key: impl AsRef<str>,
        payload: CandidatePayload,
        sensitivity: Sensitivity,
        importance: u8,
        provider_priority: ProviderPriority,
        required: bool,
    ) -> Self {
        Self {
            id: ContextCandidateId::new(provider_id, kind, key),
            provider_id,
            source,
            kind,
            payload,
            sensitivity,
            importance,
            recency: None,
            provider_priority,
            required,
        }
    }

    pub fn estimated_chars(&self) -> usize {
        self.payload.estimated_chars().max(8)
    }
}

/// Directed relationship between candidates (optional graph edges).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateEdge {
    pub from: ContextCandidateId,
    pub to: ContextCandidateId,
    pub relation: String,
}

/// Graph of proposed candidates for one assemble.
#[derive(Debug, Clone, Default)]
pub struct CandidateGraph {
    pub nodes: Vec<ContextCandidate>,
    pub edges: Vec<CandidateEdge>,
}

impl CandidateGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, candidate: ContextCandidate) {
        self.nodes.push(candidate);
    }

    pub fn extend(&mut self, candidates: impl IntoIterator<Item = ContextCandidate>) {
        self.nodes.extend(candidates);
    }

    pub fn link(
        &mut self,
        from: ContextCandidateId,
        to: ContextCandidateId,
        relation: impl Into<String>,
    ) {
        self.edges.push(CandidateEdge {
            from,
            to,
            relation: relation.into(),
        });
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Deterministic scores used by Context Policy for one candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct CandidateScores {
    pub relevance: u8,
    pub recency: u8,
    pub importance: u8,
    pub privacy_ok: bool,
    pub combined: u32,
}

impl CandidateScores {
    pub fn combine(relevance: u8, recency: u8, importance: u8, privacy_ok: bool) -> Self {
        let combined = if privacy_ok {
            (u32::from(importance) * 10_000)
                + (u32::from(relevance) * 100)
                + u32::from(recency)
        } else {
            0
        };
        Self {
            relevance,
            recency,
            importance,
            privacy_ok,
            combined,
        }
    }
}

/// Policy decision for one candidate item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateItemDecision {
    pub select: bool,
    pub reason: String,
    pub scores: CandidateScores,
}

impl CandidateItemDecision {
    pub fn allow(reason: impl Into<String>, scores: CandidateScores) -> Self {
        Self {
            select: true,
            reason: reason.into(),
            scores,
        }
    }

    pub fn deny(reason: impl Into<String>, scores: CandidateScores) -> Self {
        Self {
            select: false,
            reason: reason.into(),
            scores,
        }
    }
}

/// Per-candidate explainability row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateDecisionSummary {
    pub candidate_id: String,
    pub provider_id: String,
    pub kind: String,
    pub selected: bool,
    pub reason: String,
    pub relevance: u8,
    pub recency: u8,
    pub importance: u8,
    pub estimated_chars: usize,
}

/// Candidate selection explainability for PolicyReport / Inspector.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct CandidateSelectionReport {
    pub proposed: usize,
    pub selected: usize,
    pub rejected_policy: usize,
    pub rejected_budget: usize,
    pub decisions: Vec<CandidateDecisionSummary>,
}

/// Result of selecting candidates under budget.
#[derive(Debug, Clone)]
pub struct CandidateSelection {
    pub selected: Vec<ContextCandidate>,
    pub report: CandidateSelectionReport,
}

pub fn score_recency(recency: Option<i64>, now: i64) -> u8 {
    let Some(ts) = recency else {
        return 50;
    };
    let age = now.saturating_sub(ts).max(0);
    match age {
        0..=60 => 100,
        61..=300 => 90,
        301..=1_800 => 75,
        1_801..=7_200 => 55,
        7_201..=86_400 => 35,
        _ => 15,
    }
}

pub fn score_candidate(
    candidate: &ContextCandidate,
    provider_relevance: u8,
    now: i64,
    max_sensitivity: Sensitivity,
) -> CandidateScores {
    let privacy_ok = candidate.sensitivity <= max_sensitivity;
    let relevance = provider_relevance
        .saturating_add(candidate.importance / 5)
        .min(100);
    let recency = score_recency(candidate.recency, now);
    let importance = candidate.importance;
    CandidateScores::combine(relevance, recency, importance, privacy_ok)
}

pub fn select_candidates_for_budget(
    scored: &[(ContextCandidate, CandidateScores, String)],
    max_characters: usize,
) -> CandidateSelection {
    let mut ordered: Vec<(usize, &ContextCandidate, &CandidateScores, &String)> = scored
        .iter()
        .enumerate()
        .map(|(idx, (cand, scores, reason))| (idx, cand, scores, reason))
        .collect();
    ordered.sort_by(|a, b| {
        b.1.required
            .cmp(&a.1.required)
            .then_with(|| b.2.combined.cmp(&a.2.combined))
            .then_with(|| b.1.provider_priority.cmp(&a.1.provider_priority))
            .then_with(|| a.1.id.0.cmp(&b.1.id.0))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut used = 0usize;
    let mut selected = Vec::new();
    let mut decisions = Vec::new();
    let mut rejected_budget = 0usize;
    let mut selected_count = 0usize;

    for (_, cand, scores, reason) in ordered {
        let cost = cand.estimated_chars();
        if used.saturating_add(cost) <= max_characters
            || (cand.required && selected.is_empty() && cost <= max_characters.saturating_mul(2))
        {
            if used.saturating_add(cost) > max_characters
                && !(cand.required && selected.is_empty())
            {
                rejected_budget += 1;
                decisions.push(CandidateDecisionSummary {
                    candidate_id: cand.id.0.clone(),
                    provider_id: cand.provider_id.to_string(),
                    kind: cand.kind.as_str().to_string(),
                    selected: false,
                    reason: "budget_exhausted".into(),
                    relevance: scores.relevance,
                    recency: scores.recency,
                    importance: scores.importance,
                    estimated_chars: cost,
                });
                continue;
            }
            used = used.saturating_add(cost);
            selected_count += 1;
            decisions.push(CandidateDecisionSummary {
                candidate_id: cand.id.0.clone(),
                provider_id: cand.provider_id.to_string(),
                kind: cand.kind.as_str().to_string(),
                selected: true,
                reason: reason.clone(),
                relevance: scores.relevance,
                recency: scores.recency,
                importance: scores.importance,
                estimated_chars: cost,
            });
            selected.push(cand.clone());
        } else {
            rejected_budget += 1;
            decisions.push(CandidateDecisionSummary {
                candidate_id: cand.id.0.clone(),
                provider_id: cand.provider_id.to_string(),
                kind: cand.kind.as_str().to_string(),
                selected: false,
                reason: "budget_exhausted".into(),
                relevance: scores.relevance,
                recency: scores.recency,
                importance: scores.importance,
                estimated_chars: cost,
            });
        }
    }

    if decisions.len() > 128 {
        decisions.truncate(128);
    }

    CandidateSelection {
        selected,
        report: CandidateSelectionReport {
            proposed: scored.len(),
            selected: selected_count,
            rejected_policy: 0,
            rejected_budget,
            decisions,
        },
    }
}

pub fn materialize_candidates(selected: &[ContextCandidate]) -> ContextContribution {
    let mut contribution = ContextContribution::default();
    let mut open_files: Vec<OpenFileEntry> = Vec::new();
    let mut diagnostics: Vec<BundleDiagnostic> = Vec::new();
    let mut file_summaries: Vec<FileSummaryEntry> = Vec::new();

    for candidate in selected {
        if !contribution.sources.contains(&candidate.source) {
            contribution.sources.push(candidate.source);
        }
        match &candidate.payload {
            CandidatePayload::Conversation(section) => {
                contribution.conversation = Some(section.clone());
            }
            CandidatePayload::ActiveProject(section) => {
                contribution.active_project = Some(section.clone());
            }
            CandidatePayload::ProjectIntelligence(section) => {
                contribution.project_intelligence = Some(section.clone());
            }
            CandidatePayload::ActiveWorkspace(section) => {
                contribution.active_workspace = Some(section.clone());
            }
            CandidatePayload::ActiveCapabilities(section) => {
                contribution.active_capabilities = Some(section.clone());
            }
            CandidatePayload::CurrentFile(section) => {
                contribution.current_file = Some(section.clone());
            }
            CandidatePayload::CurrentSelection(section) => {
                contribution.current_selection = Some(section.clone());
            }
            CandidatePayload::OpenFiles(section) => {
                contribution.open_files = Some(section.clone());
            }
            CandidatePayload::OpenFile(entry) => {
                if !open_files.iter().any(|existing| existing.path == entry.path) {
                    open_files.push(entry.clone());
                }
            }
            CandidatePayload::EditorIntelligence(section) => {
                contribution.editor_intelligence = Some(section.clone());
            }
            CandidatePayload::Diagnostics(section) => {
                contribution.diagnostics = Some(section.clone());
            }
            CandidatePayload::Diagnostic(diag) => {
                diagnostics.push(diag.clone());
            }
            CandidatePayload::GitStatus(section) => {
                contribution.git_status = Some(section.clone());
            }
            CandidatePayload::RuntimeIntelligence(section) => {
                contribution.runtime_intelligence = Some(section.clone());
            }
            CandidatePayload::WorkspaceMemory(section) => {
                contribution.workspace_memory = Some(section.clone());
            }
            CandidatePayload::WorkspaceInventory(section) => {
                contribution.workspace_inventory = Some(section.clone());
            }
            CandidatePayload::FileSummaries(section) => {
                contribution.file_summaries = Some(section.clone());
            }
            CandidatePayload::FileSummary(entry) => {
                if !file_summaries
                    .iter()
                    .any(|existing| existing.path == entry.path)
                {
                    file_summaries.push(entry.clone());
                }
            }
            CandidatePayload::SearchResults(section) => {
                contribution.search_results = Some(section.clone());
            }
            CandidatePayload::MemoryResults(section) => {
                if !section.promotion_suggestions.is_empty()
                    && !contribution
                        .sources
                        .contains(&ContextSource::PromotionSuggestions)
                {
                    contribution
                        .sources
                        .push(ContextSource::PromotionSuggestions);
                }
                contribution.memory_results = Some(section.clone());
            }
            CandidatePayload::Permissions(section) => {
                contribution.permissions = Some(section.clone());
            }
            CandidatePayload::Legacy(legacy) => {
                merge_contribution(&mut contribution, legacy.as_ref());
            }
        }
    }

    if !open_files.is_empty() {
        let mut section = contribution.open_files.take().unwrap_or_default();
        for entry in open_files {
            if !section.files.iter().any(|existing| existing.path == entry.path) {
                section.files.push(entry);
            }
        }
        contribution.open_files = Some(section);
    }
    if !diagnostics.is_empty() {
        let mut section = contribution.diagnostics.take().unwrap_or_default();
        section.diagnostics.extend(diagnostics);
        contribution.diagnostics = Some(section);
    }
    if !file_summaries.is_empty() {
        let mut section = contribution.file_summaries.take().unwrap_or_default();
        for entry in file_summaries {
            if !section
                .entries
                .iter()
                .any(|existing| existing.path == entry.path)
            {
                section.entries.push(entry);
            }
        }
        contribution.file_summaries = Some(section);
    }

    contribution
}

fn merge_contribution(into: &mut ContextContribution, from: &ContextContribution) {
    for source in &from.sources {
        if !into.sources.contains(source) {
            into.sources.push(*source);
        }
    }
    if from.conversation.is_some() {
        into.conversation = from.conversation.clone();
    }
    if from.active_project.is_some() {
        into.active_project = from.active_project.clone();
    }
    if from.active_workspace.is_some() {
        into.active_workspace = from.active_workspace.clone();
    }
    if from.current_file.is_some() {
        into.current_file = from.current_file.clone();
    }
    if from.current_selection.is_some() {
        into.current_selection = from.current_selection.clone();
    }
    if from.open_files.is_some() {
        into.open_files = from.open_files.clone();
    }
    if from.search_results.is_some() {
        into.search_results = from.search_results.clone();
    }
    if from.memory_results.is_some() {
        into.memory_results = from.memory_results.clone();
    }
    if from.diagnostics.is_some() {
        into.diagnostics = from.diagnostics.clone();
    }
    if from.git_status.is_some() {
        into.git_status = from.git_status.clone();
    }
    if from.workspace_inventory.is_some() {
        into.workspace_inventory = from.workspace_inventory.clone();
    }
    if from.file_summaries.is_some() {
        into.file_summaries = from.file_summaries.clone();
    }
    if from.permissions.is_some() {
        into.permissions = from.permissions.clone();
    }
    if from.active_capabilities.is_some() {
        into.active_capabilities = from.active_capabilities.clone();
    }
    if from.editor_intelligence.is_some() {
        into.editor_intelligence = from.editor_intelligence.clone();
    }
    if from.project_intelligence.is_some() {
        into.project_intelligence = from.project_intelligence.clone();
    }
    if from.runtime_intelligence.is_some() {
        into.runtime_intelligence = from.runtime_intelligence.clone();
    }
    if from.workspace_memory.is_some() {
        into.workspace_memory = from.workspace_memory.clone();
    }
}

/// Convert a legacy [`ContextContribution`] into candidate nodes (one per section).
pub fn candidates_from_contribution(
    provider_id: &'static str,
    contribution: ContextContribution,
    sensitivity: Sensitivity,
    provider_priority: ProviderPriority,
    provider_relevance: u8,
) -> Vec<ContextCandidate> {
    let importance = provider_relevance.saturating_add(10).min(100);
    let mut out = Vec::new();
    let push = |out: &mut Vec<ContextCandidate>,
                kind: ContextCandidateKind,
                source: ContextSource,
                key: String,
                payload: CandidatePayload,
                importance: u8,
                required: bool| {
        out.push(ContextCandidate {
            id: ContextCandidateId::new(provider_id, kind, &key),
            provider_id,
            source,
            kind,
            payload,
            sensitivity,
            importance,
            recency: None,
            provider_priority,
            required,
        });
    };

    if let Some(section) = contribution.conversation {
        push(
            &mut out,
            ContextCandidateKind::Conversation,
            ContextSource::PreviousConversation,
            "main".into(),
            CandidatePayload::Conversation(section),
            95,
            true,
        );
    }
    if let Some(section) = contribution.active_project {
        push(
            &mut out,
            ContextCandidateKind::ProjectIdentity,
            ContextSource::ActiveProject,
            section
                .project_id
                .clone()
                .unwrap_or_else(|| "active".into()),
            CandidatePayload::ActiveProject(section),
            90,
            true,
        );
    }
    if let Some(section) = contribution.project_intelligence {
        push(
            &mut out,
            ContextCandidateKind::ProjectIntelligence,
            ContextSource::ProjectIntelligence,
            "intel".into(),
            CandidatePayload::ProjectIntelligence(section),
            70,
            false,
        );
    }
    if let Some(section) = contribution.active_workspace {
        push(
            &mut out,
            ContextCandidateKind::WorkspaceKind,
            ContextSource::ActiveWorkspace,
            section.kind_id.clone().unwrap_or_else(|| "kind".into()),
            CandidatePayload::ActiveWorkspace(section),
            92,
            true,
        );
    }
    if let Some(section) = contribution.active_capabilities {
        push(
            &mut out,
            ContextCandidateKind::Capabilities,
            ContextSource::ActiveCapabilities,
            "caps".into(),
            CandidatePayload::ActiveCapabilities(section),
            88,
            true,
        );
    }
    if let Some(section) = contribution.current_file {
        let key = section.path.clone().unwrap_or_else(|| "file".into());
        push(
            &mut out,
            ContextCandidateKind::CurrentFile,
            ContextSource::EditorState,
            key,
            CandidatePayload::CurrentFile(section),
            85,
            true,
        );
    }
    if let Some(section) = contribution.current_selection {
        push(
            &mut out,
            ContextCandidateKind::Selection,
            ContextSource::EditorState,
            "selection".into(),
            CandidatePayload::CurrentSelection(section),
            80,
            false,
        );
    }
    if let Some(section) = contribution.open_files {
        push(
            &mut out,
            ContextCandidateKind::OpenFiles,
            ContextSource::EditorState,
            "open".into(),
            CandidatePayload::OpenFiles(section),
            60,
            false,
        );
    }
    if let Some(section) = contribution.editor_intelligence {
        push(
            &mut out,
            ContextCandidateKind::EditorIntelligence,
            ContextSource::EditorIntelligence,
            "intel".into(),
            CandidatePayload::EditorIntelligence(section),
            75,
            false,
        );
    }
    if let Some(section) = contribution.diagnostics {
        push(
            &mut out,
            ContextCandidateKind::Diagnostics,
            ContextSource::Diagnostics,
            "all".into(),
            CandidatePayload::Diagnostics(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.git_status {
        push(
            &mut out,
            ContextCandidateKind::GitStatus,
            ContextSource::GitStatus,
            "status".into(),
            CandidatePayload::GitStatus(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.runtime_intelligence {
        push(
            &mut out,
            ContextCandidateKind::RuntimeIntelligence,
            ContextSource::RuntimeIntelligence,
            "runtime".into(),
            CandidatePayload::RuntimeIntelligence(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.workspace_memory {
        push(
            &mut out,
            ContextCandidateKind::WorkspaceMemory,
            ContextSource::WorkspaceMemory,
            "activity".into(),
            CandidatePayload::WorkspaceMemory(section),
            importance.saturating_add(5).min(100),
            false,
        );
    }
    if let Some(section) = contribution.workspace_inventory {
        push(
            &mut out,
            ContextCandidateKind::WorkspaceInventory,
            ContextSource::WorkspaceInventory,
            "inventory".into(),
            CandidatePayload::WorkspaceInventory(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.file_summaries {
        push(
            &mut out,
            ContextCandidateKind::FileSummaries,
            ContextSource::FileSummaries,
            "summaries".into(),
            CandidatePayload::FileSummaries(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.search_results {
        push(
            &mut out,
            ContextCandidateKind::SearchResults,
            ContextSource::SearchResults,
            "search".into(),
            CandidatePayload::SearchResults(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.memory_results {
        push(
            &mut out,
            ContextCandidateKind::MemoryResults,
            ContextSource::RetrievedMemories,
            "memory".into(),
            CandidatePayload::MemoryResults(section),
            importance,
            false,
        );
    }
    if let Some(section) = contribution.permissions {
        push(
            &mut out,
            ContextCandidateKind::Permissions,
            ContextSource::Permissions,
            "permissions".into(),
            CandidatePayload::Permissions(section),
            70,
            true,
        );
    }

    let _ = contribution;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_prefers_required_and_respects_budget() {
        let a = ContextCandidate {
            id: ContextCandidateId::new("workspace", ContextCandidateKind::WorkspaceKind, "coding"),
            provider_id: "workspace",
            source: ContextSource::ActiveWorkspace,
            kind: ContextCandidateKind::WorkspaceKind,
            payload: CandidatePayload::ActiveWorkspace(ActiveWorkspaceSection {
                kind_id: Some("coding".into()),
            }),
            sensitivity: Sensitivity::Workspace,
            importance: 90,
            recency: None,
            provider_priority: ProviderPriority::CRITICAL,
            required: true,
        };
        let b = ContextCandidate {
            id: ContextCandidateId::new("runtime", ContextCandidateKind::RuntimeIntelligence, "r"),
            provider_id: "runtime",
            source: ContextSource::RuntimeIntelligence,
            kind: ContextCandidateKind::RuntimeIntelligence,
            payload: CandidatePayload::RuntimeIntelligence(RuntimeIntelligenceSection {
                output_tail: "x".repeat(500),
                ..RuntimeIntelligenceSection::default()
            }),
            sensitivity: Sensitivity::Project,
            importance: 40,
            recency: None,
            provider_priority: ProviderPriority::RUNTIME,
            required: false,
        };
        let scored = vec![
            (
                b,
                CandidateScores::combine(50, 50, 40, true),
                "ok".into(),
            ),
            (
                a,
                CandidateScores::combine(90, 50, 90, true),
                "required".into(),
            ),
        ];
        let selection = select_candidates_for_budget(&scored, 200);
        assert!(!selection.selected.is_empty());
        assert_eq!(selection.selected[0].kind, ContextCandidateKind::WorkspaceKind);
    }

    #[test]
    fn materialize_merges_open_files_and_diagnostics() {
        let selected = vec![
            ContextCandidate {
                id: ContextCandidateId::new("editor", ContextCandidateKind::OpenFile, "a.rs"),
                provider_id: "editor",
                source: ContextSource::EditorState,
                kind: ContextCandidateKind::OpenFile,
                payload: CandidatePayload::OpenFile(OpenFileEntry {
                    path: "a.rs".into(),
                    dirty: false,
                    active: true,
                }),
                sensitivity: Sensitivity::Private,
                importance: 60,
                recency: None,
                provider_priority: ProviderPriority::EDITOR,
                required: false,
            },
            ContextCandidate {
                id: ContextCandidateId::new("diagnostics", ContextCandidateKind::Diagnostic, "1"),
                provider_id: "diagnostics",
                source: ContextSource::Diagnostics,
                kind: ContextCandidateKind::Diagnostic,
                payload: CandidatePayload::Diagnostic(BundleDiagnostic {
                    path: Some("a.rs".into()),
                    severity: "error".into(),
                    message: "boom".into(),
                    line: Some(1),
                    column: Some(1),
                    source: None,
                }),
                sensitivity: Sensitivity::Project,
                importance: 70,
                recency: None,
                provider_priority: ProviderPriority::DIAGNOSTICS,
                required: false,
            },
        ];
        let contribution = materialize_candidates(&selected);
        assert_eq!(contribution.open_files.as_ref().unwrap().files.len(), 1);
        assert_eq!(
            contribution.diagnostics.as_ref().unwrap().diagnostics.len(),
            1
        );
    }

    #[test]
    fn privacy_gate_zeros_combined_score() {
        let scores = CandidateScores::combine(90, 90, 90, false);
        assert_eq!(scores.combined, 0);
    }
}
