//! Deep Codebase Investigation (Sprint C2.4).
//!
//! General Planner-owned read-only investigation for questions that need
//! evidence from multiple parts of a codebase.
//!
//! Lifecycle:
//! Question → Planner determines required evidence → Search / symbols /
//! project context → Read relevant files → Follow relationships →
//! Context Policy selects evidence → ContextEngine assembles → Reasoning.
//!
//! Examples:
//! - "Why is this slow?"
//! - "How does authentication work?"
//! - "Where should this feature be implemented?"
//! - "Why does this state sometimes disappear?"
//! - "What would be affected if I changed this?"

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jaymi_context::{BundleSearchHit, ContextBundle, ContextSessionInputs};
use jaymi_core::{Citation, Document, LspLocation, UserRequest};

/// Max Planner investigation steps.
pub const MAX_STEPS: usize = 12;
/// Max distinct files inspected / read.
pub const MAX_FILES: usize = 10;
/// Max symbols followed.
pub const MAX_SYMBOLS: usize = 8;
/// Max relationship hops.
pub const MAX_DEPTH: usize = 3;
/// Soft wall-clock budget (ms).
pub const MAX_ELAPSED_MS: u64 = 3_500;
/// Conversation evidence rows (not diagnostics).
pub const CONVERSATION_EVIDENCE_CAP: usize = 14;
/// Search Knowledge hit limit per Planner-mediated search.
pub const SEARCH_HIT_LIMIT: usize = 8;
/// Max files to read via read_file tool.
pub const MAX_FILE_READS: usize = 3;
/// Max preview chars kept from a read document in evidence.
pub const READ_PREVIEW_CHARS: usize = 240;

/// Kind of deep investigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvestigationKind {
    Performance,
    SubsystemHow,
    FeaturePlacement,
    StateBug,
    ChangeImpact,
    General,
}

impl InvestigationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::SubsystemHow => "subsystem_how",
            Self::FeaturePlacement => "feature_placement",
            Self::StateBug => "state_bug",
            Self::ChangeImpact => "change_impact",
            Self::General => "general",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance investigation",
            Self::SubsystemHow => "Subsystem / how-it-works",
            Self::FeaturePlacement => "Feature placement",
            Self::StateBug => "State / intermittent bug",
            Self::ChangeImpact => "Change impact",
            Self::General => "Deep investigation",
        }
    }
}

/// Planner-determined evidence need (model does not invent these).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceNeed {
    Search { query: String },
    CursorSymbol,
    ProjectContext,
    GitContext,
    FollowReferences,
    ReadTopFiles,
}

impl EvidenceNeed {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Search { .. } => "search",
            Self::CursorSymbol => "cursor_symbol",
            Self::ProjectContext => "project_context",
            Self::GitContext => "git_context",
            Self::FollowReferences => "follow_references",
            Self::ReadTopFiles => "read_top_files",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Search { query } => format!("search query={query:?}"),
            other => other.as_str().to_string(),
        }
    }
}

/// One observable investigation step (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvestigationStep {
    pub index: u32,
    pub action: String,
    pub detail: String,
    pub evidence_added: usize,
    pub duration_ms: u64,
}

/// Conversation-safe evidence row (not a diagnostics dump).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvestigationEvidence {
    pub path: PathBuf,
    pub title: String,
    pub matched: String,
    pub relevance: u32,
    pub line: Option<u32>,
    pub preview: String,
    pub why: String,
}

/// Why investigation stopped.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvestigationStopReason {
    Complete,
    MaxSteps,
    MaxFiles,
    MaxSymbols,
    MaxDepth,
    ElapsedTime,
    InsufficientEvidence,
}

impl InvestigationStopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::MaxSteps => "max_steps",
            Self::MaxFiles => "max_files",
            Self::MaxSymbols => "max_symbols",
            Self::MaxDepth => "max_depth",
            Self::ElapsedTime => "elapsed_time",
            Self::InsufficientEvidence => "insufficient_evidence",
        }
    }
}

/// Planner-authored deep investigation report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct DeepInvestigationReport {
    pub goal: String,
    pub kind: Option<InvestigationKind>,
    pub subject: String,
    /// Evidence needs the Planner decided up front.
    pub required_evidence: Vec<EvidenceNeed>,
    pub steps: Vec<InvestigationStep>,
    pub searches: Vec<String>,
    pub files_inspected: Vec<String>,
    pub symbols_followed: Vec<String>,
    /// ContextBundle section ids observed after assemble / re-assemble.
    pub selected_context: Vec<String>,
    pub evidence: Vec<InvestigationEvidence>,
    /// Explicit gaps — what could not be established.
    pub gaps: Vec<String>,
    pub total_hits: usize,
    pub truncated: bool,
    pub stop_reason: Option<InvestigationStopReason>,
    pub depth_reached: usize,
    pub elapsed_ms: u64,
    pub budget: InvestigationBudgetSnapshot,
    pub diagnostics: Vec<String>,
}

/// Budget snapshot for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct InvestigationBudgetSnapshot {
    pub max_steps: usize,
    pub max_files: usize,
    pub max_symbols: usize,
    pub max_depth: usize,
    pub max_elapsed_ms: u64,
    pub steps_used: usize,
    pub files_used: usize,
    pub symbols_used: usize,
    pub depth_used: usize,
    pub elapsed_ms: u64,
}

impl DeepInvestigationReport {
    /// Conversation body — findings + gaps; no raw diagnostics dump.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Deep Investigation\n");
        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Focus\n- {}\n", kind.label()));
        }
        if !self.subject.is_empty() {
            out.push_str(&format!("\n### Subject\n- {}\n", self.subject));
        }

        out.push_str("\n### Findings\n");
        if self.evidence.is_empty() {
            out.push_str("- _(no solid evidence gathered yet)_\n");
        } else {
            for (i, hit) in self.evidence.iter().enumerate() {
                let loc = hit
                    .line
                    .map(|l| format!(":{}", l.saturating_add(1)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{}. `{}{}` — {} ({})\n",
                    i + 1,
                    hit.title,
                    loc,
                    hit.matched,
                    hit.why
                ));
                if !hit.preview.trim().is_empty() {
                    out.push_str(&format!("   {}\n", truncate(&hit.preview, 160)));
                }
            }
        }

        out.push_str("\n### What could not be established\n");
        if self.gaps.is_empty() {
            out.push_str("- _(no explicit gaps recorded)_\n");
        } else {
            for gap in &self.gaps {
                out.push_str(&format!("- {gap}\n"));
            }
        }

        out.push_str("\n### Answer\n");
        out.push_str(&self.synthesize_answer());
        out.push('\n');

        out.push_str("\n### Notes\n");
        out.push_str("- Read-only investigation — no file modifications.\n");
        out.push_str("- Retrieval was Planner-orchestrated; ContextEngine assembled the bundle.\n");
        out.push_str("- Unsupported architectural claims are avoided when evidence is thin.\n");
        out
    }

    /// Compact evidence block for Reasoning (still Planner-authored).
    pub fn evidence_brief_for_reasoning(&self) -> String {
        let mut lines = vec![
            format!("investigation_kind={}", self.kind.map(|k| k.as_str()).unwrap_or("general")),
            format!("subject={}", self.subject),
        ];
        for hit in self.evidence.iter().take(CONVERSATION_EVIDENCE_CAP) {
            lines.push(format!(
                "- {} ({}) why={} preview={}",
                hit.title,
                hit.path.display(),
                hit.why,
                truncate(&hit.preview, 100)
            ));
        }
        if !self.gaps.is_empty() {
            lines.push("gaps:".into());
            for gap in &self.gaps {
                lines.push(format!("- {gap}"));
            }
        }
        lines.join("\n")
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "deep_investigation kind={} subject={:?} steps={} searches={} files={} symbols={} context={} hits={} gaps={} stop={} ms={}",
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.subject,
            self.steps.len(),
            self.searches.len(),
            self.files_inspected.len(),
            self.symbols_followed.len(),
            self.selected_context.len(),
            self.total_hits,
            self.gaps.len(),
            self.stop_reason
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("—"),
            self.elapsed_ms
        )
    }

    pub fn chain_diagnostics_lines(&self) -> Vec<String> {
        let mut lines = vec![self.to_diagnostics_summary()];
        lines.push(format!(
            "budget steps={}/{} files={}/{} symbols={}/{} depth={}/{} ms={}/{}",
            self.budget.steps_used,
            self.budget.max_steps,
            self.budget.files_used,
            self.budget.max_files,
            self.budget.symbols_used,
            self.budget.max_symbols,
            self.budget.depth_used,
            self.budget.max_depth,
            self.budget.elapsed_ms,
            self.budget.max_elapsed_ms
        ));
        for need in &self.required_evidence {
            lines.push(format!("need: {}", need.detail()));
        }
        for step in &self.steps {
            lines.push(format!(
                "  {}. {} — {} (+{}, {}ms)",
                step.index, step.action, step.detail, step.evidence_added, step.duration_ms
            ));
        }
        for search in &self.searches {
            lines.push(format!("search: {search}"));
        }
        for file in &self.files_inspected {
            lines.push(format!("file: {file}"));
        }
        for sym in &self.symbols_followed {
            lines.push(format!("symbol: {sym}"));
        }
        for section in &self.selected_context {
            lines.push(format!("context: {section}"));
        }
        lines.extend(self.diagnostics.iter().cloned());
        lines
    }

    fn synthesize_answer(&self) -> String {
        let mut parts = Vec::new();
        match self.kind {
            Some(InvestigationKind::Performance) => {
                parts.push(
                    "- Performance: evidence below is observational (hot paths / call sites). Timing root-cause was not measured by Jaymi."
                        .into(),
                );
            }
            Some(InvestigationKind::SubsystemHow) => {
                if self.evidence.is_empty() {
                    parts.push(
                        "- Could not establish how the subsystem works from available evidence."
                            .into(),
                    );
                } else {
                    parts.push(format!(
                        "- Subsystem **`{}`** appears to touch {} evidence site(s); treat as provisional, not a full architecture claim.",
                        if self.subject.is_empty() {
                            "subject"
                        } else {
                            &self.subject
                        },
                        self.evidence.len()
                    ));
                }
            }
            Some(InvestigationKind::FeaturePlacement) => {
                parts.push(
                    "- Feature placement: prefer modules already related in findings; do not invent new layers without evidence."
                        .into(),
                );
            }
            Some(InvestigationKind::StateBug) => {
                parts.push(
                    "- Intermittent state: only ambient/read evidence was available — race/root-cause is unproven without runtime traces."
                        .into(),
                );
            }
            Some(InvestigationKind::ChangeImpact) => {
                parts.push(
                    "- Change impact: listed references/usages are candidates only; full blast radius may be larger than observed."
                        .into(),
                );
            }
            _ => {
                parts.push("- Investigation summary follows the Planner-gathered findings.".into());
            }
        }
        if !self.gaps.is_empty() {
            parts.push(format!(
                "- Explicitly unresolved: {}.",
                self.gaps
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        parts.join("\n")
    }
}

/// Detected assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationAssessment {
    pub kind: InvestigationKind,
    pub goal: String,
    pub subject: Option<String>,
    pub matched_rules: Vec<&'static str>,
}

/// Detect deep-investigation phrasing (Planner ownership).
pub fn detect_deep_investigation_request(request: &UserRequest) -> Option<InvestigationAssessment> {
    if request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.lsp.is_some()
        || request.search.is_some()
        || request.project_knowledge.is_some()
    {
        return None;
    }
    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    if crate::pair_programming::looks_like_dead_code_request(&lower) {
        return None;
    }
    // Narrower C2.x modes win when they match first at Planner soft-path order;
    // still avoid stealing reviews / plans / multi-file / generation.
    if crate::coding_review::detect_review_request(request).is_some()
        || crate::coding_plan::detect_coding_plan_request(request).is_some()
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
        || crate::relationship_tracing::detect_relationship_trace_request(request).is_some()
    {
        return None;
    }

    let (kind, rules, subject) = classify_phrasing(&lower, content)?;
    Some(InvestigationAssessment {
        kind,
        goal: content.to_string(),
        subject,
        matched_rules: rules,
    })
}

fn classify_phrasing(
    lower: &str,
    original: &str,
) -> Option<(InvestigationKind, Vec<&'static str>, Option<String>)> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    if trimmed == "why is this slow"
        || trimmed.starts_with("why is this slow")
        || trimmed.contains("why is this so slow")
        || (trimmed.starts_with("why is ") && trimmed.contains("slow"))
        || trimmed.contains("performance problem")
        || trimmed.contains("what's making this slow")
    {
        return Some((InvestigationKind::Performance, vec!["performance"], None));
    }

    if trimmed.starts_with("how does authentication work")
        || trimmed.starts_with("how does auth work")
        || (trimmed.starts_with("how does ")
            && trimmed.ends_with(" work")
            && !trimmed.contains("project")
            && !trimmed.contains("this file")
            && !trimmed.contains("this function")
            && !trimmed.contains("this value"))
    {
        let subject = extract_how_does_subject(original);
        return Some((
            InvestigationKind::SubsystemHow,
            vec!["subsystem_how"],
            subject,
        ));
    }

    if trimmed == "where should this feature be implemented"
        || trimmed.starts_with("where should this feature be implemented")
        || trimmed.contains("where should this feature be implemented")
        || (trimmed.contains("where should this feature") && trimmed.contains("implement"))
    {
        return Some((
            InvestigationKind::FeaturePlacement,
            vec!["feature_placement"],
            extract_feature_subject(original),
        ));
    }

    if trimmed.contains("state sometimes disappear")
        || trimmed.contains("state disappears")
        || trimmed.contains("why does this state")
        || (trimmed.contains("sometimes")
            && (trimmed.contains("disappear") || trimmed.contains("missing") || trimmed.contains("reset")))
        || trimmed.contains("intermittent state")
    {
        return Some((InvestigationKind::StateBug, vec!["state_bug"], None));
    }

    if trimmed.starts_with("what would be affected if i changed")
        || trimmed.contains("what would be affected if i change")
        || trimmed.contains("what breaks if i change")
        || trimmed.contains("impact of changing this")
        || trimmed.contains("blast radius if i change")
    {
        return Some((InvestigationKind::ChangeImpact, vec!["change_impact"], None));
    }

    if trimmed.starts_with("investigate ")
        || trimmed.starts_with("dig into ")
        || trimmed.starts_with("deep dive ")
    {
        return Some((
            InvestigationKind::General,
            vec!["general"],
            extract_after(original, &["investigate ", "dig into ", "deep dive "]),
        ));
    }

    None
}

fn extract_how_does_subject(original: &str) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    let Some(idx) = lower.find("how does ") else {
        return None;
    };
    let after = original[idx + "how does ".len()..]
        .trim()
        .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!');
    let cleaned = after
        .strip_suffix(" work")
        .or_else(|| after.strip_suffix(" works"))
        .unwrap_or(after)
        .trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("this") {
        return None;
    }
    Some(cleaned.to_string())
}

fn extract_feature_subject(original: &str) -> Option<String> {
    extract_after(original, &["feature ", "implement "])
}

fn extract_after(original: &str, markers: &[&str]) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    for marker in markers {
        if let Some(idx) = lower.find(marker) {
            let after = original[idx + marker.len()..]
                .trim()
                .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!');
            let token = after
                .split_whitespace()
                .next()?
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
            if !token.is_empty() && !token.eq_ignore_ascii_case("this") {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Planner determines required evidence up front (model does not invent actions).
pub fn plan_required_evidence(assessment: &InvestigationAssessment) -> Vec<EvidenceNeed> {
    let mut needs = Vec::new();
    match assessment.kind {
        InvestigationKind::Performance => {
            needs.push(EvidenceNeed::CursorSymbol);
            needs.push(EvidenceNeed::FollowReferences);
            needs.push(EvidenceNeed::Search {
                query: assessment
                    .subject
                    .clone()
                    .unwrap_or_else(|| "slow performance hot path".into()),
            });
            needs.push(EvidenceNeed::GitContext);
            needs.push(EvidenceNeed::ReadTopFiles);
        }
        InvestigationKind::SubsystemHow => {
            let q = assessment
                .subject
                .clone()
                .unwrap_or_else(|| "subsystem".into());
            needs.push(EvidenceNeed::Search { query: q });
            needs.push(EvidenceNeed::ProjectContext);
            needs.push(EvidenceNeed::FollowReferences);
            needs.push(EvidenceNeed::ReadTopFiles);
        }
        InvestigationKind::FeaturePlacement => {
            needs.push(EvidenceNeed::ProjectContext);
            needs.push(EvidenceNeed::Search {
                query: assessment
                    .subject
                    .clone()
                    .unwrap_or_else(|| "feature".into()),
            });
            needs.push(EvidenceNeed::CursorSymbol);
            needs.push(EvidenceNeed::ReadTopFiles);
        }
        InvestigationKind::StateBug => {
            needs.push(EvidenceNeed::CursorSymbol);
            needs.push(EvidenceNeed::FollowReferences);
            needs.push(EvidenceNeed::Search {
                query: "state".into(),
            });
            needs.push(EvidenceNeed::GitContext);
            needs.push(EvidenceNeed::ReadTopFiles);
        }
        InvestigationKind::ChangeImpact => {
            needs.push(EvidenceNeed::CursorSymbol);
            needs.push(EvidenceNeed::FollowReferences);
            needs.push(EvidenceNeed::Search {
                query: assessment
                    .subject
                    .clone()
                    .unwrap_or_else(|| "symbol".into()),
            });
            needs.push(EvidenceNeed::ReadTopFiles);
        }
        InvestigationKind::General => {
            needs.push(EvidenceNeed::CursorSymbol);
            needs.push(EvidenceNeed::ProjectContext);
            needs.push(EvidenceNeed::Search {
                query: assessment
                    .subject
                    .clone()
                    .unwrap_or_else(|| assessment.goal.clone()),
            });
            needs.push(EvidenceNeed::FollowReferences);
            needs.push(EvidenceNeed::ReadTopFiles);
        }
    }
    needs
}

#[derive(Debug)]
struct Budget {
    started: Instant,
    steps: usize,
    depth: usize,
    files: HashSet<String>,
    symbols: HashSet<String>,
    stop: Option<InvestigationStopReason>,
}

impl Budget {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            steps: 0,
            depth: 0,
            files: HashSet::new(),
            symbols: HashSet::new(),
            stop: None,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn check(&mut self) -> bool {
        if self.stop.is_some() {
            return false;
        }
        if self.steps >= MAX_STEPS {
            self.stop = Some(InvestigationStopReason::MaxSteps);
            return false;
        }
        if self.files.len() >= MAX_FILES {
            self.stop = Some(InvestigationStopReason::MaxFiles);
            return false;
        }
        if self.symbols.len() >= MAX_SYMBOLS {
            self.stop = Some(InvestigationStopReason::MaxSymbols);
            return false;
        }
        if self.depth > MAX_DEPTH {
            self.stop = Some(InvestigationStopReason::MaxDepth);
            return false;
        }
        if self.elapsed_ms() >= MAX_ELAPSED_MS {
            self.stop = Some(InvestigationStopReason::ElapsedTime);
            return false;
        }
        true
    }
}

/// Begin report from WI / project / git using the Planner evidence plan.
pub fn begin_investigation(
    assessment: &InvestigationAssessment,
    bundle: &ContextBundle,
    _session: &ContextSessionInputs,
) -> DeepInvestigationReport {
    let started = Instant::now();
    let needs = plan_required_evidence(assessment);
    let mut budget = Budget::new();
    let mut steps = Vec::new();
    let mut evidence = Vec::new();
    let mut files_inspected = Vec::new();
    let mut symbols_followed = Vec::new();
    let mut gaps = Vec::new();
    let mut diagnostics = vec![
        format!("matched_rules={}", assessment.matched_rules.join(",")),
        format!("kind={}", assessment.kind.as_str()),
    ];

    let subject = resolve_subject(assessment, bundle);
    let plan_step = Instant::now();
    push_step(
        &mut steps,
        &mut budget,
        "plan_evidence",
        format!(
            "needs={}",
            needs
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
        0,
        plan_step.elapsed().as_millis() as u64,
    );

    let editor = bundle.editor_intelligence();

    for need in &needs {
        if !budget.check() {
            break;
        }
        let step_start = Instant::now();
        match need {
            EvidenceNeed::CursorSymbol => {
                let before = evidence.len();
                if let Some(sym) = &editor.symbol {
                    note_symbol(&mut budget, &mut symbols_followed, &sym.name);
                    if let Some(path) = bundle.current_file().path.as_deref() {
                        note_file(&mut budget, &mut files_inspected, path);
                    }
                    evidence.push(from_symbol(bundle, sym, "cursor_symbol", 92));
                } else if let Some(func) = &editor.enclosing_function {
                    note_symbol(&mut budget, &mut symbols_followed, &func.name);
                    evidence.push(from_symbol(bundle, func, "enclosing_function", 85));
                } else {
                    gaps.push("No cursor/enclosing symbol available in Workspace Intelligence.".into());
                }
                let added = evidence.len() - before;
                push_step(
                    &mut steps,
                    &mut budget,
                    "cursor_symbol",
                    format!("added={added}"),
                    added,
                    step_start.elapsed().as_millis() as u64,
                );
            }
            EvidenceNeed::FollowReferences => {
                let before = evidence.len();
                if editor.references.is_empty() {
                    gaps.push("No ambient references to follow (LSP enrichment may be missing).".into());
                } else {
                    for reference in editor.references.iter().take(MAX_FILES) {
                        if !budget.check() {
                            break;
                        }
                        note_file(&mut budget, &mut files_inspected, &reference.path);
                        evidence.push(from_reference(&subject, reference, "follow_reference"));
                    }
                    budget.depth = budget.depth.saturating_add(1);
                }
                let added = evidence.len() - before;
                push_step(
                    &mut steps,
                    &mut budget,
                    "follow_references",
                    format!("added={added}"),
                    added,
                    step_start.elapsed().as_millis() as u64,
                );
            }
            EvidenceNeed::ProjectContext => {
                let project = bundle.project_intelligence();
                let mut detail = Vec::new();
                if !project.languages.is_empty() {
                    detail.push(format!("languages={}", project.languages.join(",")));
                }
                if !project.frameworks.is_empty() {
                    detail.push(format!("frameworks={}", project.frameworks.join(",")));
                }
                if let Some(layout) = &project.layout_shape {
                    detail.push(format!("layout={layout}"));
                }
                if !project.top_level_dirs.is_empty() {
                    detail.push(format!(
                        "dirs={}",
                        project
                            .top_level_dirs
                            .iter()
                            .take(6)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
                }
                let added = if detail.is_empty() {
                    gaps.push("Project intelligence summary was empty.".into());
                    0
                } else {
                    evidence.push(InvestigationEvidence {
                        path: PathBuf::from("(project)"),
                        title: "project_intelligence".into(),
                        matched: subject.clone(),
                        relevance: 60,
                        line: None,
                        preview: detail.join(" · "),
                        why: "project_context".into(),
                    });
                    1
                };
                push_step(
                    &mut steps,
                    &mut budget,
                    "project_context",
                    if detail.is_empty() {
                        "empty".into()
                    } else {
                        detail.join(" · ")
                    },
                    added,
                    step_start.elapsed().as_millis() as u64,
                );
            }
            EvidenceNeed::GitContext => {
                let git = bundle.git_status();
                let preview = if git.summary.is_empty() {
                    format!(
                        "repo={} modified={} staged={}",
                        git.is_repository, git.modified_count, git.staged_count
                    )
                } else {
                    git.summary.clone()
                };
                let added = if !git.is_repository && git.summary.is_empty() {
                    gaps.push("No Git context available for this investigation.".into());
                    0
                } else {
                    evidence.push(InvestigationEvidence {
                        path: PathBuf::from("(git)"),
                        title: "git_status".into(),
                        matched: subject.clone(),
                        relevance: 50,
                        line: None,
                        preview,
                        why: "git_context".into(),
                    });
                    1
                };
                push_step(
                    &mut steps,
                    &mut budget,
                    "git_context",
                    format!("added={added}"),
                    added,
                    step_start.elapsed().as_millis() as u64,
                );
            }
            EvidenceNeed::Search { query } => {
                // Planned — executed by Planner via search_knowledge tool.
                push_step(
                    &mut steps,
                    &mut budget,
                    "plan_search",
                    format!("queued query={query:?}"),
                    0,
                    step_start.elapsed().as_millis() as u64,
                );
                diagnostics.push(format!("search_queued={query}"));
            }
            EvidenceNeed::ReadTopFiles => {
                push_step(
                    &mut steps,
                    &mut budget,
                    "plan_reads",
                    format!("queued up to {MAX_FILE_READS} reads"),
                    0,
                    step_start.elapsed().as_millis() as u64,
                );
            }
        }
    }

    let selected_context = observe_selected_context(bundle);
    let code_hits = evidence
        .iter()
        .filter(|e| e.why != "project_context" && e.why != "git_context")
        .count();
    if code_hits == 0 {
        gaps.push("No code-location evidence yet — search/read extensions may be required.".into());
        if budget.stop.is_none() {
            budget.stop = Some(InvestigationStopReason::InsufficientEvidence);
        }
    } else if budget.stop.is_none() {
        budget.stop = Some(InvestigationStopReason::Complete);
    }

    finalize(
        assessment,
        subject,
        needs,
        steps,
        Vec::new(),
        files_inspected,
        symbols_followed,
        selected_context,
        evidence,
        gaps,
        budget,
        started.elapsed().as_millis() as u64,
        diagnostics,
    )
}

/// Record ContextBundle sections present (Context Policy / assemble observability).
pub fn observe_selected_context(bundle: &ContextBundle) -> Vec<String> {
    let mut sections = Vec::new();
    if bundle.current_file().path.is_some() {
        sections.push("current_file".into());
    }
    if bundle.editor_intelligence().symbol.is_some()
        || !bundle.editor_intelligence().references.is_empty()
    {
        sections.push("editor_intelligence".into());
    }
    if !bundle.project_intelligence().languages.is_empty()
        || bundle.project_intelligence().layout_shape.is_some()
    {
        sections.push("project_intelligence".into());
    }
    if bundle.git_status().is_repository || !bundle.git_status().summary.is_empty() {
        sections.push("git_status".into());
    }
    if bundle.project().is_some() {
        sections.push("project".into());
    }
    if !bundle.search_results().hits.is_empty() {
        sections.push("search_results".into());
    }
    sections
}

/// Merge Planner-mediated search citations.
pub fn merge_search_citations(report: &mut DeepInvestigationReport, citations: &[Citation], query: &str) {
    report.searches.push(query.to_string());
    if citations.is_empty() {
        report
            .gaps
            .push(format!("Search for {query:?} returned no citations."));
        report.steps.push(InvestigationStep {
            index: (report.steps.len() as u32).saturating_add(1),
            action: "search_knowledge".into(),
            detail: format!("query={query:?} hits=0"),
            evidence_added: 0,
            duration_ms: 0,
        });
        return;
    }
    let mut added = 0usize;
    for citation in citations.iter().take(SEARCH_HIT_LIMIT) {
        if report.evidence.len() >= CONVERSATION_EVIDENCE_CAP {
            report.truncated = true;
            break;
        }
        if report.files_inspected.len() >= MAX_FILES {
            break;
        }
        let path = citation.location.display().to_string();
        if !report.files_inspected.contains(&path) {
            report.files_inspected.push(path.clone());
        }
        report.evidence.push(InvestigationEvidence {
            path: citation.location.clone(),
            title: if citation.title.is_empty() {
                Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or(path)
            } else {
                citation.title.clone()
            },
            matched: citation
                .matching_section
                .clone()
                .unwrap_or_else(|| report.subject.clone()),
            relevance: citation.confidence_percent(),
            line: citation.line,
            preview: truncate(&citation.preview, READ_PREVIEW_CHARS),
            why: "search_knowledge".into(),
        });
        added += 1;
    }
    report.total_hits = report.total_hits.saturating_add(added);
    report.steps.push(InvestigationStep {
        index: (report.steps.len() as u32).saturating_add(1),
        action: "search_knowledge".into(),
        detail: format!("query={query:?} hits={added}"),
        evidence_added: added,
        duration_ms: 0,
    });
    report.budget.files_used = report.files_inspected.len();
    report.budget.steps_used = report.steps.len();
}

/// Merge Planner-mediated read_file document (truncated preview only).
pub fn merge_read_document(
    report: &mut DeepInvestigationReport,
    path: &Path,
    document: Option<&Document>,
) {
    if report.files_inspected.len() >= MAX_FILES {
        return;
    }
    let path_str = path.display().to_string();
    if !report.files_inspected.contains(&path_str) {
        report.files_inspected.push(path_str.clone());
    }
    let title = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_str.clone());
    let preview = document
        .map(|doc| truncate(&doc.text, READ_PREVIEW_CHARS))
        .unwrap_or_else(|| "(empty read)".into());
    if document.is_none() {
        report
            .gaps
            .push(format!("Could not read file `{title}` through Planner."));
    }
    report.evidence.push(InvestigationEvidence {
        path: path.to_path_buf(),
        title,
        matched: report.subject.clone(),
        relevance: 80,
        line: None,
        preview,
        why: "read_file".into(),
    });
    report.total_hits = report.total_hits.saturating_add(1);
    report.steps.push(InvestigationStep {
        index: (report.steps.len() as u32).saturating_add(1),
        action: "read_file".into(),
        detail: path.display().to_string(),
        evidence_added: 1,
        duration_ms: 0,
    });
    report.budget.files_used = report.files_inspected.len();
    report.budget.steps_used = report.steps.len();
}

/// Merge LSP locations (Planner-mediated).
pub fn merge_lsp_locations(
    report: &mut DeepInvestigationReport,
    locations: &[LspLocation],
    action: &str,
) {
    if locations.is_empty() {
        report
            .gaps
            .push(format!("LSP {action} returned no locations."));
        return;
    }
    let mut added = 0usize;
    for loc in locations.iter().take(MAX_FILES) {
        if report.evidence.len() >= CONVERSATION_EVIDENCE_CAP {
            report.truncated = true;
            break;
        }
        if !report.files_inspected.contains(&loc.path) {
            report.files_inspected.push(loc.path.clone());
        }
        let title = Path::new(&loc.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| loc.path.clone());
        report.evidence.push(InvestigationEvidence {
            path: PathBuf::from(&loc.path),
            title,
            matched: report.subject.clone(),
            relevance: 90,
            line: Some(loc.range.start.line),
            preview: format!(
                "L{}:{}",
                loc.range.start.line.saturating_add(1),
                loc.range.start.character.saturating_add(1)
            ),
            why: format!("lsp_{action}"),
        });
        added += 1;
    }
    report.depth_reached = report.depth_reached.saturating_add(1);
    report.total_hits = report.total_hits.saturating_add(added);
    report.steps.push(InvestigationStep {
        index: (report.steps.len() as u32).saturating_add(1),
        action: format!("lsp_{action}"),
        detail: format!("hits={added}"),
        evidence_added: added,
        duration_ms: 0,
    });
}

/// Paths suggested for Planner-mediated read_file (bounded).
pub fn candidate_read_paths(report: &DeepInvestigationReport) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for hit in &report.evidence {
        if hit.why == "project_context" || hit.why == "git_context" {
            continue;
        }
        let p = hit.path.to_string_lossy();
        if p.starts_with('(') {
            continue;
        }
        if !hit.path.as_os_str().is_empty() && !out.contains(&hit.path) {
            out.push(hit.path.clone());
        }
        if out.len() >= MAX_FILE_READS {
            break;
        }
    }
    out
}

/// Convert citations to session search hits for ContextEngine re-assemble.
pub fn citations_to_search_hits(citations: &[Citation]) -> Vec<BundleSearchHit> {
    citations
        .iter()
        .take(SEARCH_HIT_LIMIT)
        .enumerate()
        .map(|(i, c)| BundleSearchHit {
            item_id: format!("investigation-{i}"),
            title: c.title.clone(),
            path: Some(c.location.display().to_string()),
            score: Some(c.confidence_percent()),
            match_reason: Some(c.why_matched.clone()),
            preview: Some(truncate(&c.preview, READ_PREVIEW_CHARS)),
            line: c.line,
            column: c.column,
        })
        .collect()
}

/// Primary search query from the evidence plan.
pub fn primary_search_query(assessment: &InvestigationAssessment) -> Option<String> {
    plan_required_evidence(assessment).into_iter().find_map(|need| match need {
        EvidenceNeed::Search { query } => Some(query),
        _ => None,
    })
}

/// Whether the Planner evidence plan includes reading top files.
pub fn plan_includes_read(assessment: &InvestigationAssessment) -> bool {
    plan_required_evidence(assessment)
        .iter()
        .any(|n| matches!(n, EvidenceNeed::ReadTopFiles))
}

/// Whether the Planner evidence plan includes following references.
pub fn plan_includes_follow_refs(assessment: &InvestigationAssessment) -> bool {
    plan_required_evidence(assessment)
        .iter()
        .any(|n| matches!(n, EvidenceNeed::FollowReferences))
}

fn resolve_subject(assessment: &InvestigationAssessment, bundle: &ContextBundle) -> String {
    if let Some(s) = &assessment.subject {
        if !s.trim().is_empty() {
            return s.trim().to_string();
        }
    }
    let editor = bundle.editor_intelligence();
    if let Some(sym) = &editor.symbol {
        if !sym.name.trim().is_empty() {
            return sym.name.clone();
        }
    }
    if let Some(func) = &editor.enclosing_function {
        if !func.name.trim().is_empty() {
            return func.name.clone();
        }
    }
    String::new()
}

fn note_file(budget: &mut Budget, files: &mut Vec<String>, path: &str) {
    if path.is_empty() {
        return;
    }
    budget.files.insert(path.to_string());
    if !files.iter().any(|f| f == path) {
        files.push(path.to_string());
    }
}

fn note_symbol(budget: &mut Budget, symbols: &mut Vec<String>, name: &str) {
    let n = name.trim();
    if n.is_empty() {
        return;
    }
    budget.symbols.insert(n.to_string());
    if !symbols.iter().any(|s| s == n) {
        symbols.push(n.to_string());
    }
}

fn push_step(
    steps: &mut Vec<InvestigationStep>,
    budget: &mut Budget,
    action: &str,
    detail: String,
    evidence_added: usize,
    duration_ms: u64,
) {
    budget.steps = budget.steps.saturating_add(1);
    steps.push(InvestigationStep {
        index: budget.steps as u32,
        action: action.into(),
        detail,
        evidence_added,
        duration_ms,
    });
}

fn finalize(
    assessment: &InvestigationAssessment,
    subject: String,
    needs: Vec<EvidenceNeed>,
    steps: Vec<InvestigationStep>,
    searches: Vec<String>,
    files_inspected: Vec<String>,
    symbols_followed: Vec<String>,
    selected_context: Vec<String>,
    mut evidence: Vec<InvestigationEvidence>,
    gaps: Vec<String>,
    budget: Budget,
    elapsed_ms: u64,
    diagnostics: Vec<String>,
) -> DeepInvestigationReport {
    let total = evidence.len();
    let truncated = total > CONVERSATION_EVIDENCE_CAP;
    if truncated {
        evidence.truncate(CONVERSATION_EVIDENCE_CAP);
    }
    let budget_snap = InvestigationBudgetSnapshot {
        max_steps: MAX_STEPS,
        max_files: MAX_FILES,
        max_symbols: MAX_SYMBOLS,
        max_depth: MAX_DEPTH,
        max_elapsed_ms: MAX_ELAPSED_MS,
        steps_used: steps.len(),
        files_used: files_inspected.len(),
        symbols_used: symbols_followed.len(),
        depth_used: budget.depth.min(MAX_DEPTH),
        elapsed_ms,
    };
    DeepInvestigationReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        subject,
        required_evidence: needs,
        steps,
        searches,
        files_inspected,
        symbols_followed,
        selected_context,
        evidence,
        gaps,
        total_hits: total,
        truncated,
        stop_reason: budget.stop.clone().or(Some(InvestigationStopReason::Complete)),
        depth_reached: budget.depth.min(MAX_DEPTH),
        elapsed_ms,
        budget: budget_snap,
        diagnostics,
    }
}

fn from_symbol(
    bundle: &ContextBundle,
    symbol: &jaymi_context::EditorSymbol,
    why: &str,
    relevance: u32,
) -> InvestigationEvidence {
    let path = bundle
        .current_file()
        .path
        .clone()
        .unwrap_or_else(|| "(active editor)".into());
    let title = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    InvestigationEvidence {
        path: PathBuf::from(path),
        title,
        matched: symbol.name.clone(),
        relevance,
        line: symbol.range.as_ref().map(|r| r.start_line),
        preview: symbol.detail.clone().unwrap_or_else(|| {
            format!(
                "{} ({})",
                symbol.name,
                symbol.kind.as_deref().unwrap_or("symbol")
            )
        }),
        why: why.into(),
    }
}

fn from_reference(
    subject: &str,
    reference: &jaymi_context::EditorReference,
    why: &str,
) -> InvestigationEvidence {
    let title = Path::new(&reference.path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| reference.path.clone());
    InvestigationEvidence {
        path: PathBuf::from(&reference.path),
        title,
        matched: if subject.is_empty() {
            "reference".into()
        } else {
            subject.to_string()
        },
        relevance: 88,
        line: Some(reference.range.start_line),
        preview: format!(
            "L{}:{}",
            reference.range.start_line.saturating_add(1),
            reference.range.start_column.saturating_add(1)
        ),
        why: why.into(),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let short: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{short}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ContextBundle, EditorIntelligenceSection, EditorRange, EditorReference, EditorSymbol,
        ProjectIntelligenceSection,
    };

    #[test]
    fn detects_sprint_examples() {
        assert_eq!(
            detect_deep_investigation_request(&UserRequest::new("Why is this slow?"))
                .unwrap()
                .kind,
            InvestigationKind::Performance
        );
        let auth = detect_deep_investigation_request(&UserRequest::new(
            "How does authentication work?",
        ))
        .unwrap();
        assert_eq!(auth.kind, InvestigationKind::SubsystemHow);
        assert_eq!(auth.subject.as_deref(), Some("authentication"));

        assert_eq!(
            detect_deep_investigation_request(&UserRequest::new(
                "Where should this feature be implemented?",
            ))
            .unwrap()
            .kind,
            InvestigationKind::FeaturePlacement
        );
        assert_eq!(
            detect_deep_investigation_request(&UserRequest::new(
                "Why does this state sometimes disappear?",
            ))
            .unwrap()
            .kind,
            InvestigationKind::StateBug
        );
        assert_eq!(
            detect_deep_investigation_request(&UserRequest::new(
                "What would be affected if I changed this?",
            ))
            .unwrap()
            .kind,
            InvestigationKind::ChangeImpact
        );
    }

    #[test]
    fn plans_evidence_without_model_invention() {
        let assessment = detect_deep_investigation_request(&UserRequest::new(
            "How does authentication work?",
        ))
        .unwrap();
        let needs = plan_required_evidence(&assessment);
        assert!(needs.iter().any(|n| matches!(n, EvidenceNeed::Search { .. })));
        assert!(needs.iter().any(|n| matches!(n, EvidenceNeed::ProjectContext)));
        assert!(needs.iter().any(|n| matches!(n, EvidenceNeed::ReadTopFiles)));
    }

    #[test]
    fn begin_investigation_records_gaps_and_context() {
        let assessment = detect_deep_investigation_request(&UserRequest::new(
            "Why is this slow?",
        ))
        .unwrap();
        let bundle = ContextBundle::builder()
            .current_file(jaymi_context::CurrentFileSection {
                path: Some("/proj/src/hot.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            })
            .editor_intelligence(EditorIntelligenceSection {
                symbol: Some(EditorSymbol {
                    name: "render".into(),
                    kind: Some("function".into()),
                    range: Some(EditorRange {
                        start_line: 10,
                        start_column: 0,
                        end_line: 10,
                        end_column: 6,
                    }),
                    ..Default::default()
                }),
                references: vec![EditorReference {
                    path: "/proj/src/ui.rs".into(),
                    range: EditorRange {
                        start_line: 2,
                        start_column: 0,
                        end_line: 2,
                        end_column: 6,
                    },
                }],
                ..Default::default()
            })
            .project_intelligence(ProjectIntelligenceSection {
                languages: vec!["rust".into()],
                ..Default::default()
            })
            .build();
        let report = begin_investigation(
            &assessment,
            &bundle,
            &ContextSessionInputs::default(),
        );
        assert!(!report.steps.is_empty());
        assert!(report.selected_context.contains(&"editor_intelligence".into()));
        assert!(report.to_markdown().contains("### What could not be established"));
        assert!(!report.to_markdown().contains("budget steps=")); // diagnostics not in conversation
        assert!(report.to_diagnostics_summary().contains("deep_investigation"));
        assert!(report.chain_diagnostics_lines().iter().any(|l| l.contains("budget")));
    }

    #[test]
    fn does_not_steal_relationship_or_simple_nav() {
        assert!(detect_deep_investigation_request(&UserRequest::new(
            "What happens when I click Save?",
        ))
        .is_none());
        assert!(detect_deep_investigation_request(&UserRequest::new(
            "Who calls this?",
        ))
        .is_none());
        assert!(detect_deep_investigation_request(&UserRequest::new(
            "Where is authentication handled?",
        ))
        .is_none());
    }

    #[test]
    fn merge_search_updates_searches_and_files() {
        let assessment = detect_deep_investigation_request(&UserRequest::new(
            "How does authentication work?",
        ))
        .unwrap();
        let mut report = begin_investigation(
            &assessment,
            &ContextBundle::builder().build(),
            &ContextSessionInputs::default(),
        );
        let citations = vec![Citation {
            title: "auth.rs".into(),
            location: PathBuf::from("/proj/src/auth.rs"),
            preview: "fn authenticate".into(),
            why_matched: "free_text".into(),
            confidence: 8_000,
            matching_section: Some("authenticate".into()),
            line: Some(1),
            column: Some(0),
            end_line: Some(1),
            end_column: Some(4),
        }];
        merge_search_citations(&mut report, &citations, "authentication");
        assert!(report.searches.iter().any(|s| s == "authentication"));
        assert!(report.files_inspected.iter().any(|f| f.contains("auth.rs")));
        assert!(report.evidence.iter().any(|e| e.why == "search_knowledge"));
    }
}
