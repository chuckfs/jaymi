//! Code Relationship Tracing (Sprint C2.3).
//!
//! Planner-owned read-only investigation that traces how functionality moves
//! through a codebase by combining Workspace Intelligence, Search, LSP symbols,
//! Project Intelligence, and Git/context — without a second context assembler
//! and without letting the LLM navigate the filesystem or LSP.
//!
//! Examples:
//! - "What happens when I click Save?"
//! - "Trace this request through the application."
//! - "How does this value get here?"
//! - "Where does this event eventually end up?"
//! - "What calls this provider?"

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jaymi_context::{ContextBundle, ContextSessionInputs};
use jaymi_core::{Citation, LspLocation, UserRequest};

/// Max investigation depth (symbol hops).
pub const MAX_DEPTH: usize = 4;
/// Max distinct files inspected / cited.
pub const MAX_FILES: usize = 12;
/// Max distinct symbols followed.
pub const MAX_SYMBOLS: usize = 8;
/// Max Planner investigation steps (actions in the chain).
pub const MAX_STEPS: usize = 10;
/// Soft wall-clock budget for the investigation loop (ms).
pub const MAX_ELAPSED_MS: u64 = 2_500;
/// Conversation evidence cap (context-budget friendly).
pub const CONVERSATION_EVIDENCE_CAP: usize = 16;
/// Max Search Knowledge hits when Planner mediates a lexical step.
pub const SEARCH_HIT_LIMIT: usize = 8;

/// Kind of relationship-tracing ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    /// UI / click / action flow.
    EventFlow,
    /// Request / pipeline through the application.
    RequestTrace,
    /// Value / data provenance (“how does this get here”).
    ValueProvenance,
    /// Event sink / eventual destination.
    EventSink,
    /// Provider / dependency callers.
    ProviderCallers,
    /// Generic multi-hop trace.
    General,
}

impl TraceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EventFlow => "event_flow",
            Self::RequestTrace => "request_trace",
            Self::ValueProvenance => "value_provenance",
            Self::EventSink => "event_sink",
            Self::ProviderCallers => "provider_callers",
            Self::General => "general",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::EventFlow => "Event / action flow",
            Self::RequestTrace => "Request trace",
            Self::ValueProvenance => "Value provenance",
            Self::EventSink => "Event destination",
            Self::ProviderCallers => "Provider callers",
            Self::General => "Relationship trace",
        }
    }
}

/// One step in the investigation chain (diagnostics + conversation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TraceStep {
    pub index: u32,
    /// Stable action id (`identify_symbol`, `find_definition`, …).
    pub action: String,
    /// Human detail for this step.
    pub detail: String,
    /// Evidence rows produced by this step.
    pub evidence_added: usize,
}

/// One piece of investigation evidence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TraceEvidence {
    pub path: PathBuf,
    pub title: String,
    pub matched: String,
    pub relevance: u32,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub preview: String,
    pub why: String,
    /// Step index that produced this row.
    pub step: u32,
}

/// Why the investigation stopped.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStopReason {
    Complete,
    MaxDepth,
    MaxFiles,
    MaxSymbols,
    MaxSteps,
    ElapsedTime,
    NoSeed,
    NoFurtherTargets,
}

impl TraceStopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::MaxDepth => "max_depth",
            Self::MaxFiles => "max_files",
            Self::MaxSymbols => "max_symbols",
            Self::MaxSteps => "max_steps",
            Self::ElapsedTime => "elapsed_time",
            Self::NoSeed => "no_seed",
            Self::NoFurtherTargets => "no_further_targets",
        }
    }
}

/// Planner-authored relationship-trace report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct RelationshipTraceReport {
    pub goal: String,
    pub kind: Option<TraceKind>,
    pub seed_symbol: String,
    pub steps: Vec<TraceStep>,
    pub evidence: Vec<TraceEvidence>,
    pub total_hits: usize,
    pub truncated: bool,
    pub stop_reason: Option<TraceStopReason>,
    pub depth_reached: usize,
    pub files_inspected: usize,
    pub symbols_followed: usize,
    pub elapsed_ms: u64,
    pub diagnostics: Vec<String>,
}

impl RelationshipTraceReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Relationship Trace\n");
        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Kind\n- {}\n", kind.label()));
        }
        out.push_str("\n### Seed\n- ");
        out.push_str(if self.seed_symbol.is_empty() {
            "(unknown — focus an editor symbol or name the subject)"
        } else {
            &self.seed_symbol
        });
        out.push('\n');

        out.push_str("\n### Investigation Chain\n");
        if self.steps.is_empty() {
            out.push_str("- _(no steps executed)_\n");
        } else {
            for step in &self.steps {
                out.push_str(&format!(
                    "{}. `{}` — {} (+{} evidence)\n",
                    step.index, step.action, step.detail, step.evidence_added
                ));
            }
        }

        out.push_str("\n### Evidence\n");
        if self.evidence.is_empty() {
            out.push_str(
                "- _(no relationship evidence — ensure Coding focus, indexed search, or clearer subject)_\n",
            );
        } else {
            for (i, hit) in self.evidence.iter().enumerate() {
                let loc = match (hit.line, hit.end_line) {
                    (Some(a), Some(b)) if a != b => {
                        format!(":{}-{}", a.saturating_add(1), b.saturating_add(1))
                    }
                    (Some(a), _) => format!(":{}", a.saturating_add(1)),
                    _ => String::new(),
                };
                out.push_str(&format!(
                    "{}. `{}{}` — {} ({}%) · step {}\n",
                    i + 1,
                    hit.title,
                    loc,
                    hit.matched,
                    hit.relevance,
                    hit.step
                ));
                if !hit.preview.trim().is_empty() {
                    out.push_str(&format!("   {}\n", truncate(&hit.preview, 140)));
                }
            }
        }

        out.push_str("\n### Answer\n");
        out.push_str(&self.synthesize_answer());
        out.push('\n');

        if self.truncated {
            out.push_str(&format!(
                "\n_Showing {} of {} evidence rows (context budget)._\n",
                self.evidence.len(),
                self.total_hits
            ));
        }

        if let Some(stop) = &self.stop_reason {
            out.push_str(&format!(
                "\n### Limits\n- stopped={} · depth={} · files={} · symbols={} · steps={} · {}ms\n",
                stop.as_str(),
                self.depth_reached,
                self.files_inspected,
                self.symbols_followed,
                self.steps.len(),
                self.elapsed_ms
            ));
        }

        out.push_str("\n### Notes\n");
        out.push_str("- Read-only investigation — no file modifications or destructive tools.\n");
        out.push_str("- Every retrieval was Planner-mediated (WI / Search / LSP routes).\n");
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "relationship_trace kind={} seed={:?} steps={} hits={} shown={} stop={} depth={} files={} symbols={} ms={}",
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.seed_symbol,
            self.steps.len(),
            self.total_hits,
            self.evidence.len(),
            self.stop_reason
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("—"),
            self.depth_reached,
            self.files_inspected,
            self.symbols_followed,
            self.elapsed_ms
        )
    }

    /// Chain-only diagnostics lines (Developer Diagnostics).
    pub fn chain_diagnostics_lines(&self) -> Vec<String> {
        let mut lines = vec![self.to_diagnostics_summary()];
        for step in &self.steps {
            lines.push(format!(
                "  {}. {} — {}",
                step.index, step.action, step.detail
            ));
        }
        lines.extend(self.diagnostics.iter().cloned());
        lines
    }

    fn synthesize_answer(&self) -> String {
        if self.seed_symbol.is_empty() && self.evidence.is_empty() {
            return "- Could not start a relationship trace — no seed symbol or ambient WI evidence."
                .into();
        }
        let mut parts = Vec::new();
        if !self.seed_symbol.is_empty() {
            parts.push(format!(
                "- Seed subject **`{}`** ({})",
                self.seed_symbol,
                self.kind.map(|k| k.label()).unwrap_or("trace")
            ));
        }
        let def = self
            .evidence
            .iter()
            .find(|e| e.why.contains("definition") || e.why.contains("seed_location"));
        if let Some(hit) = def {
            parts.push(format!(
                "- Definition / location observed at `{}`{}",
                hit.title,
                hit.line
                    .map(|l| format!(":{}", l.saturating_add(1)))
                    .unwrap_or_default()
            ));
        }
        let caller_files: HashSet<_> = self
            .evidence
            .iter()
            .filter(|e| {
                e.why.contains("caller")
                    || e.why.contains("reference")
                    || e.why.contains("provider")
                    || e.why.contains("search")
            })
            .map(|e| e.title.as_str())
            .collect();
        if !caller_files.is_empty() {
            let mut names: Vec<_> = caller_files.into_iter().collect();
            names.sort_unstable();
            parts.push(format!(
                "- Related call sites / usages across: {}",
                names.join(", ")
            ));
        }
        if parts.len() <= 1 {
            parts.push(
                "- Investigation gathered limited ambient evidence; refine the symbol or ensure LSP/WI enrichment."
                    .into(),
            );
        } else {
            parts.push(
                "- Follow the investigation chain above for hop-by-hop provenance (read-only)."
                    .into(),
            );
        }
        parts.join("\n")
    }
}

/// Detected tracing assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceAssessment {
    pub kind: TraceKind,
    pub goal: String,
    pub seed_hint: Option<String>,
    pub matched_rules: Vec<&'static str>,
}

/// Detect relationship-tracing phrasing (Planner ownership).
pub fn detect_relationship_trace_request(request: &UserRequest) -> Option<TraceAssessment> {
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
    if crate::coding_review::detect_review_request(request).is_some()
        || crate::coding_plan::detect_coding_plan_request(request).is_some()
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
    {
        return None;
    }

    let (kind, rules, hint) = classify_phrasing(&lower, content)?;
    Some(TraceAssessment {
        kind,
        goal: content.to_string(),
        seed_hint: hint,
        matched_rules: rules,
    })
}

fn classify_phrasing(
    lower: &str,
    original: &str,
) -> Option<(TraceKind, Vec<&'static str>, Option<String>)> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    if trimmed.starts_with("what happens when i click ")
        || trimmed.starts_with("what happens when i press ")
        || trimmed.starts_with("what happens when clicking ")
        || trimmed.contains("when i click ")
    {
        let hint = extract_after(original, &["click ", "press ", "clicking "]);
        return Some((TraceKind::EventFlow, vec!["event_flow"], hint));
    }

    if trimmed.starts_with("trace this request")
        || trimmed.starts_with("trace the request")
        || trimmed.contains("through the application")
        || trimmed.contains("through the app")
        || trimmed == "trace this request through the application"
    {
        return Some((TraceKind::RequestTrace, vec!["request_trace"], None));
    }

    if trimmed.starts_with("how does this value get here")
        || trimmed.starts_with("how does this get here")
        || trimmed.contains("how does this value")
        || trimmed.contains("where does this value come from")
        || trimmed.contains("how did this value get")
    {
        return Some((TraceKind::ValueProvenance, vec!["value_provenance"], None));
    }

    if trimmed.starts_with("where does this event eventually end up")
        || trimmed.contains("eventually end up")
        || trimmed.contains("where does this event")
    {
        return Some((TraceKind::EventSink, vec!["event_sink"], None));
    }

    if trimmed.starts_with("what calls this provider")
        || trimmed.contains("calls this provider")
        || trimmed.contains("who calls this provider")
        || trimmed.contains("callers of this provider")
    {
        return Some((
            TraceKind::ProviderCallers,
            vec!["provider_callers"],
            Some("provider".into()),
        ));
    }

    if trimmed.starts_with("trace ")
        || trimmed.starts_with("follow this ")
        || trimmed.starts_with("follow the ")
        || trimmed.contains("call chain")
        || trimmed.contains("data flow")
        || trimmed.contains("relationship between")
    {
        let hint = extract_trace_subject(original);
        return Some((TraceKind::General, vec!["general_trace"], hint));
    }

    None
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
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn extract_trace_subject(original: &str) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    for marker in ["trace ", "follow this ", "follow the "] {
        if let Some(idx) = lower.find(marker) {
            let after = original[idx + marker.len()..]
                .trim()
                .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!');
            let token = after
                .split_whitespace()
                .next()?
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if token.is_empty() || token.eq_ignore_ascii_case("this") {
                continue;
            }
            return Some(token.to_string());
        }
    }
    None
}

/// Mutable investigation budget (prevents unbounded loops).
#[derive(Debug)]
struct Budget {
    started: Instant,
    steps: usize,
    depth: usize,
    files: HashSet<String>,
    symbols: HashSet<String>,
    stop: Option<TraceStopReason>,
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
            self.stop = Some(TraceStopReason::MaxSteps);
            return false;
        }
        if self.depth > MAX_DEPTH {
            self.stop = Some(TraceStopReason::MaxDepth);
            return false;
        }
        if self.files.len() >= MAX_FILES {
            self.stop = Some(TraceStopReason::MaxFiles);
            return false;
        }
        if self.symbols.len() >= MAX_SYMBOLS {
            self.stop = Some(TraceStopReason::MaxSymbols);
            return false;
        }
        if self.elapsed_ms() >= MAX_ELAPSED_MS {
            self.stop = Some(TraceStopReason::ElapsedTime);
            return false;
        }
        true
    }

    fn note_file(&mut self, path: &str) {
        if !path.is_empty() {
            self.files.insert(path.to_string());
        }
    }

    fn note_symbol(&mut self, name: &str) {
        let n = name.trim();
        if !n.is_empty() {
            self.symbols.insert(n.to_string());
        }
    }
}

/// Run a Planner-owned read-only investigation against assembled Context.
///
/// Does not call tools itself — optional Search/LSP results are merged via
/// [`merge_search_citations`] / [`merge_lsp_locations`] after Planner mediation.
pub fn investigate(
    assessment: &TraceAssessment,
    bundle: &ContextBundle,
    session: &ContextSessionInputs,
) -> RelationshipTraceReport {
    let mut budget = Budget::new();
    let mut steps = Vec::new();
    let mut evidence = Vec::new();
    let mut diagnostics = vec![
        format!("matched_rules={}", assessment.matched_rules.join(",")),
        format!("kind={}", assessment.kind.as_str()),
        format!(
            "limits depth={} files={} symbols={} steps={} ms={}",
            MAX_DEPTH, MAX_FILES, MAX_SYMBOLS, MAX_STEPS, MAX_ELAPSED_MS
        ),
    ];

    // 1. Identify seed symbol.
    let seed = resolve_seed(assessment, bundle, session);
    push_step(
        &mut steps,
        &mut budget,
        "identify_symbol",
        if seed.is_empty() {
            "no seed symbol from WI / hint".into()
        } else {
            format!("seed=`{seed}`")
        },
        0,
    );
    if seed.is_empty() {
        budget.stop = Some(TraceStopReason::NoSeed);
        return finalize(
            assessment,
            seed,
            steps,
            evidence,
            budget,
            diagnostics,
        );
    }
    budget.note_symbol(&seed);

    let editor = bundle.editor_intelligence();

    // 2. Definition / location.
    if budget.check() {
        let before = evidence.len();
        if let Some(sym) = &editor.symbol {
            if !sym.name.trim().is_empty() {
                evidence.push(evidence_from_symbol(
                    bundle,
                    &sym.name,
                    sym,
                    "seed_location",
                    95,
                    steps.len() as u32,
                ));
                if let Some(path) = bundle.current_file().path.as_deref() {
                    budget.note_file(path);
                }
            }
        } else if let Some(func) = &editor.enclosing_function {
            evidence.push(evidence_from_symbol(
                bundle,
                &func.name,
                func,
                "definition_enclosing_function",
                88,
                steps.len() as u32,
            ));
            if let Some(path) = bundle.current_file().path.as_deref() {
                budget.note_file(path);
            }
        }
        let added = evidence.len().saturating_sub(before);
        push_step(
            &mut steps,
            &mut budget,
            "find_definition",
            format!("ambient definition/location rows={added}"),
            added,
        );
        budget.depth = budget.depth.saturating_add(1);
    }

    // 3. Callers / references (hop).
    if budget.check() {
        let before = evidence.len();
        for reference in editor.references.iter().take(MAX_FILES) {
            if !budget.check() {
                break;
            }
            budget.note_file(&reference.path);
            evidence.push(evidence_from_reference(
                &seed,
                reference,
                match assessment.kind {
                    TraceKind::ProviderCallers => "provider_caller",
                    TraceKind::EventSink | TraceKind::EventFlow => "event_handler_ref",
                    TraceKind::ValueProvenance => "value_use_site",
                    _ => "caller_or_reference",
                },
                steps.len() as u32,
            ));
        }
        let added = evidence.len().saturating_sub(before);
        push_step(
            &mut steps,
            &mut budget,
            "find_callers",
            format!("WI references added={added}"),
            added,
        );
        if added > 0 {
            budget.depth = budget.depth.saturating_add(1);
        }
    }

    // 4. Related symbols (enclosing type / function).
    if budget.check() {
        let before = evidence.len();
        if let Some(ty) = &editor.enclosing_type {
            budget.note_symbol(&ty.name);
            evidence.push(evidence_from_symbol(
                bundle,
                &ty.name,
                ty,
                "related_enclosing_type",
                80,
                steps.len() as u32,
            ));
        }
        if let Some(func) = &editor.enclosing_function {
            if func.name != seed {
                budget.note_symbol(&func.name);
                evidence.push(evidence_from_symbol(
                    bundle,
                    &func.name,
                    func,
                    "related_enclosing_function",
                    78,
                    steps.len() as u32,
                ));
            }
        }
        let added = evidence.len().saturating_sub(before);
        push_step(
            &mut steps,
            &mut budget,
            "inspect_related",
            format!("related symbols added={added}"),
            added,
        );
        if added > 0 {
            budget.depth = budget.depth.saturating_add(1);
        }
    }

    // 5. Project intelligence (observational only — already in ContextBundle).
    if budget.check() {
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
            0
        } else {
            evidence.push(TraceEvidence {
                path: PathBuf::from("(project)"),
                title: "project_intelligence".into(),
                matched: seed.clone(),
                relevance: 60,
                line: None,
                end_line: None,
                preview: detail.join(" · "),
                why: "project_context".into(),
                step: steps.len() as u32,
            });
            1
        };
        push_step(
            &mut steps,
            &mut budget,
            "project_context",
            if detail.is_empty() {
                "no project intelligence summary".into()
            } else {
                detail.join(" · ")
            },
            added,
        );
    }

    // 6. Git overlap (when relevant).
    if budget.check() {
        let git = bundle.git_status();
        let evidence_paths: HashSet<String> = evidence
            .iter()
            .filter_map(|e| {
                let p = e.path.to_string_lossy();
                if p == "(project)" || p == "(git)" {
                    None
                } else {
                    Some(p.into_owned())
                }
            })
            .collect();
        let mut overlap = Vec::new();
        for path in git
            .dirty_paths
            .iter()
            .chain(git.staged_paths.iter())
            .chain(git.untracked_paths.iter())
            .chain(git.sample_paths.iter())
        {
            if evidence_paths.iter().any(|e| {
                e.contains(path.as_str())
                    || path.contains(
                        Path::new(e)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(""),
                    )
            }) {
                overlap.push(path.clone());
            }
        }
        if let Some(snap) = &session.git_snapshot {
            for entry in snap
                .dirty
                .iter()
                .chain(snap.staged.iter())
                .chain(snap.untracked.iter())
            {
                let p = entry.path.clone();
                if evidence_paths.iter().any(|e| {
                    e.contains(&p)
                        || p.contains(
                            Path::new(e)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or(""),
                        )
                }) && !overlap.contains(&p)
                {
                    overlap.push(p);
                }
            }
        }
        let added = if overlap.is_empty() {
            0
        } else {
            evidence.push(TraceEvidence {
                path: PathBuf::from("(git)"),
                title: "git_overlap".into(),
                matched: seed.clone(),
                relevance: 55,
                line: None,
                end_line: None,
                preview: format!(
                    "dirty paths overlapping trace: {}",
                    overlap.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
                ),
                why: "git_context".into(),
                step: steps.len() as u32,
            });
            1
        };
        push_step(
            &mut steps,
            &mut budget,
            "git_context",
            if overlap.is_empty() {
                "no overlapping dirty paths".into()
            } else {
                format!("overlap={}", overlap.len())
            },
            added,
        );
    }

    if budget.stop.is_none() {
        if evidence
            .iter()
            .all(|e| e.why == "project_context" || e.why == "git_context" || e.why.contains("seed"))
            && editor.references.is_empty()
        {
            budget.stop = Some(TraceStopReason::NoFurtherTargets);
            diagnostics.push("frontier exhausted after seed/project/git".into());
        } else {
            budget.stop = Some(TraceStopReason::Complete);
        }
    }

    finalize(assessment, seed, steps, evidence, budget, diagnostics)
}

/// Merge Planner-mediated Search Knowledge citations into an existing report.
pub fn merge_search_citations(
    report: &mut RelationshipTraceReport,
    citations: &[Citation],
) {
    if citations.is_empty() {
        return;
    }
    if report.steps.len() >= MAX_STEPS {
        report.diagnostics.push("search skipped: max_steps".into());
        return;
    }
    let step_idx = (report.steps.len() as u32).saturating_add(1);
    let mut added = 0usize;
    let mut files = report.files_inspected;
    for citation in citations.iter().take(SEARCH_HIT_LIMIT) {
        if report.evidence.len() >= CONVERSATION_EVIDENCE_CAP {
            report.truncated = true;
            break;
        }
        if files >= MAX_FILES {
            report.stop_reason = Some(TraceStopReason::MaxFiles);
            break;
        }
        let path = citation.location.display().to_string();
        files = files.saturating_add(1);
        report.evidence.push(TraceEvidence {
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
                .unwrap_or_else(|| report.seed_symbol.clone()),
            relevance: citation.confidence_percent(),
            line: citation.line,
            end_line: citation.end_line.or(citation.line),
            preview: truncate(&citation.preview, 140),
            why: "search_knowledge".into(),
            step: step_idx,
        });
        added += 1;
    }
    report.files_inspected = files;
    report.total_hits = report.total_hits.saturating_add(added);
    report.steps.push(TraceStep {
        index: step_idx,
        action: "search_knowledge".into(),
        detail: format!("Planner-mediated search hits={added}"),
        evidence_added: added,
    });
    report
        .diagnostics
        .push(format!("search_knowledge_merged={added}"));
}

/// Merge Planner-mediated LSP locations into an existing report.
pub fn merge_lsp_locations(
    report: &mut RelationshipTraceReport,
    locations: &[LspLocation],
    action: &str,
) {
    if locations.is_empty() {
        return;
    }
    if report.steps.len() >= MAX_STEPS {
        report.diagnostics.push(format!("{action} skipped: max_steps"));
        return;
    }
    let step_idx = (report.steps.len() as u32).saturating_add(1);
    let mut added = 0usize;
    let mut files = report.files_inspected;
    for loc in locations.iter().take(MAX_FILES) {
        if report.evidence.len() >= CONVERSATION_EVIDENCE_CAP {
            report.truncated = true;
            break;
        }
        if files >= MAX_FILES {
            report.stop_reason = Some(TraceStopReason::MaxFiles);
            break;
        }
        files = files.saturating_add(1);
        let title = Path::new(&loc.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| loc.path.clone());
        report.evidence.push(TraceEvidence {
            path: PathBuf::from(&loc.path),
            title,
            matched: report.seed_symbol.clone(),
            relevance: 92,
            line: Some(loc.range.start.line),
            end_line: Some(loc.range.end.line),
            preview: format!(
                "L{}:{}",
                loc.range.start.line.saturating_add(1),
                loc.range.start.character.saturating_add(1)
            ),
            why: format!("lsp_{action}"),
            step: step_idx,
        });
        added += 1;
    }
    report.files_inspected = files;
    report.total_hits = report.total_hits.saturating_add(added);
    report.steps.push(TraceStep {
        index: step_idx,
        action: action.into(),
        detail: format!("Planner-mediated LSP {action} hits={added}"),
        evidence_added: added,
    });
}

/// Whether the WI investigation is thin enough to warrant a Search step.
pub fn needs_search_extension(report: &RelationshipTraceReport) -> bool {
    let code_hits = report
        .evidence
        .iter()
        .filter(|e| e.why != "project_context" && e.why != "git_context")
        .count();
    code_hits < 2 && !report.seed_symbol.is_empty()
}

/// Whether callers are missing and LSP refs may help.
pub fn needs_lsp_references_extension(report: &RelationshipTraceReport) -> bool {
    !report.evidence.iter().any(|e| {
        e.why.contains("caller")
            || e.why.contains("reference")
            || e.why.contains("provider")
            || e.why.starts_with("lsp_")
    }) && !report.seed_symbol.is_empty()
}

fn resolve_seed(
    assessment: &TraceAssessment,
    bundle: &ContextBundle,
    session: &ContextSessionInputs,
) -> String {
    if let Some(hint) = &assessment.seed_hint {
        let h = hint.trim();
        if !h.is_empty()
            && !h.eq_ignore_ascii_case("this")
            && !h.eq_ignore_ascii_case("provider")
        {
            return h.to_string();
        }
        // "provider" hint — prefer cursor symbol if present.
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
    if let Some(sym) = session
        .editor_snapshot
        .as_ref()
        .and_then(|s| s.symbol.as_ref())
    {
        if !sym.name.trim().is_empty() {
            return sym.name.clone();
        }
    }
    if let Some(hint) = &assessment.seed_hint {
        return hint.trim().to_string();
    }
    String::new()
}

fn push_step(
    steps: &mut Vec<TraceStep>,
    budget: &mut Budget,
    action: &str,
    detail: String,
    evidence_added: usize,
) {
    if budget.stop.is_some() && budget.steps >= MAX_STEPS {
        return;
    }
    budget.steps = budget.steps.saturating_add(1);
    steps.push(TraceStep {
        index: budget.steps as u32,
        action: action.into(),
        detail,
        evidence_added,
    });
}

fn finalize(
    assessment: &TraceAssessment,
    seed: String,
    steps: Vec<TraceStep>,
    mut evidence: Vec<TraceEvidence>,
    budget: Budget,
    diagnostics: Vec<String>,
) -> RelationshipTraceReport {
    let total = evidence.len();
    let truncated = total > CONVERSATION_EVIDENCE_CAP;
    if truncated {
        evidence.truncate(CONVERSATION_EVIDENCE_CAP);
    }
    RelationshipTraceReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        seed_symbol: seed,
        steps,
        evidence,
        total_hits: total,
        truncated,
        stop_reason: budget.stop.clone().or(Some(TraceStopReason::Complete)),
        depth_reached: budget.depth.min(MAX_DEPTH),
        files_inspected: budget.files.len(),
        symbols_followed: budget.symbols.len(),
        elapsed_ms: budget.elapsed_ms(),
        diagnostics,
    }
}

fn evidence_from_symbol(
    bundle: &ContextBundle,
    matched: &str,
    symbol: &jaymi_context::EditorSymbol,
    why: &str,
    relevance: u32,
    step: u32,
) -> TraceEvidence {
    let path = bundle
        .current_file()
        .path
        .clone()
        .unwrap_or_else(|| "(active editor)".into());
    let title = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let (line, end_line) = symbol
        .range
        .as_ref()
        .map(|r| (Some(r.start_line), Some(r.end_line)))
        .unwrap_or((None, None));
    TraceEvidence {
        path: PathBuf::from(path),
        title,
        matched: matched.to_string(),
        relevance,
        line,
        end_line,
        preview: symbol.detail.clone().unwrap_or_else(|| {
            format!(
                "{} ({})",
                symbol.name,
                symbol.kind.as_deref().unwrap_or("symbol")
            )
        }),
        why: why.into(),
        step,
    }
}

fn evidence_from_reference(
    symbol: &str,
    reference: &jaymi_context::EditorReference,
    why: &str,
    step: u32,
) -> TraceEvidence {
    let title = Path::new(&reference.path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| reference.path.clone());
    TraceEvidence {
        path: PathBuf::from(&reference.path),
        title,
        matched: if symbol.is_empty() {
            "reference".into()
        } else {
            symbol.to_string()
        },
        relevance: 90,
        line: Some(reference.range.start_line),
        end_line: Some(reference.range.end_line),
        preview: format!(
            "L{}:{}",
            reference.range.start_line.saturating_add(1),
            reference.range.start_column.saturating_add(1)
        ),
        why: why.into(),
        step,
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

    fn coding_bundle() -> ContextBundle {
        ContextBundle::builder()
            .current_file(jaymi_context::CurrentFileSection {
                path: Some("/proj/src/ui.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            })
            .editor_intelligence(EditorIntelligenceSection {
                symbol: Some(EditorSymbol {
                    name: "on_save".into(),
                    kind: Some("function".into()),
                    range: Some(EditorRange {
                        start_line: 40,
                        start_column: 3,
                        end_line: 40,
                        end_column: 10,
                    }),
                    ..Default::default()
                }),
                enclosing_function: Some(EditorSymbol {
                    name: "on_save".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                enclosing_type: Some(EditorSymbol {
                    name: "EditorChrome".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![
                    EditorReference {
                        path: "/proj/src/ui.rs".into(),
                        range: EditorRange {
                            start_line: 12,
                            start_column: 0,
                            end_line: 12,
                            end_column: 7,
                        },
                    },
                    EditorReference {
                        path: "/proj/src/actions.rs".into(),
                        range: EditorRange {
                            start_line: 8,
                            start_column: 2,
                            end_line: 8,
                            end_column: 9,
                        },
                    },
                ],
                ..Default::default()
            })
            .project_intelligence(ProjectIntelligenceSection {
                languages: vec!["rust".into()],
                frameworks: vec!["egui".into()],
                layout_shape: Some("workspace".into()),
                top_level_dirs: vec!["crates".into(), "apps".into()],
                ..Default::default()
            })
            .build()
    }

    #[test]
    fn detects_sprint_examples() {
        let click = detect_relationship_trace_request(&UserRequest::new(
            "What happens when I click Save?",
        ))
        .unwrap();
        assert_eq!(click.kind, TraceKind::EventFlow);
        assert_eq!(click.seed_hint.as_deref(), Some("Save"));

        assert_eq!(
            detect_relationship_trace_request(&UserRequest::new(
                "Trace this request through the application.",
            ))
            .unwrap()
            .kind,
            TraceKind::RequestTrace
        );
        assert_eq!(
            detect_relationship_trace_request(&UserRequest::new(
                "How does this value get here?",
            ))
            .unwrap()
            .kind,
            TraceKind::ValueProvenance
        );
        assert_eq!(
            detect_relationship_trace_request(&UserRequest::new(
                "Where does this event eventually end up?",
            ))
            .unwrap()
            .kind,
            TraceKind::EventSink
        );
        assert_eq!(
            detect_relationship_trace_request(&UserRequest::new(
                "What calls this provider?",
            ))
            .unwrap()
            .kind,
            TraceKind::ProviderCallers
        );
    }

    #[test]
    fn investigation_builds_chain_with_limits_metadata() {
        let assessment = detect_relationship_trace_request(&UserRequest::new(
            "What happens when I click Save?",
        ))
        .unwrap();
        let report = investigate(
            &assessment,
            &coding_bundle(),
            &ContextSessionInputs::default(),
        );
        assert!(!report.steps.is_empty());
        assert!(report.steps.iter().any(|s| s.action == "identify_symbol"));
        assert!(report.steps.iter().any(|s| s.action == "find_callers"));
        assert!(report.steps.iter().any(|s| s.action == "project_context"));
        assert!(report.evidence.len() >= 2);
        assert!(report.to_markdown().contains("### Investigation Chain"));
        assert!(report.to_diagnostics_summary().contains("relationship_trace"));
        assert!(report.files_inspected <= MAX_FILES);
        assert!(report.symbols_followed <= MAX_SYMBOLS);
        assert!(report.depth_reached <= MAX_DEPTH);
        assert!(report.steps.len() <= MAX_STEPS);
    }

    #[test]
    fn missing_seed_stops_honestly() {
        let assessment = detect_relationship_trace_request(&UserRequest::new(
            "Trace this request through the application.",
        ))
        .unwrap();
        let report = investigate(
            &assessment,
            &ContextBundle::builder().build(),
            &ContextSessionInputs::default(),
        );
        assert_eq!(report.stop_reason, Some(TraceStopReason::NoSeed));
        assert!(report.evidence.is_empty() || report.seed_symbol.is_empty());
    }

    #[test]
    fn merge_search_respects_caps() {
        let assessment = detect_relationship_trace_request(&UserRequest::new(
            "What calls this provider?",
        ))
        .unwrap();
        let mut report = investigate(
            &assessment,
            &coding_bundle(),
            &ContextSessionInputs::default(),
        );
        let citations: Vec<_> = (0..20)
            .map(|i| Citation {
                title: format!("f{i}.rs"),
                location: PathBuf::from(format!("/proj/f{i}.rs")),
                preview: "provider".into(),
                why_matched: "test".into(),
                confidence: 7_000,
                matching_section: Some("provider".into()),
                line: Some(i),
                column: Some(0),
                end_line: Some(i),
                end_column: Some(3),
            })
            .collect();
        merge_search_citations(&mut report, &citations);
        assert!(report.steps.iter().any(|s| s.action == "search_knowledge"));
        assert!(report.evidence.len() <= CONVERSATION_EVIDENCE_CAP);
    }

    #[test]
    fn does_not_steal_simple_navigation() {
        assert!(detect_relationship_trace_request(&UserRequest::new(
            "Where is this function defined?",
        ))
        .is_none());
        assert!(detect_relationship_trace_request(&UserRequest::new(
            "Who calls this?",
        ))
        .is_none());
        assert!(detect_relationship_trace_request(&UserRequest::new(
            "Where is authentication handled?",
        ))
        .is_none());
    }
}
