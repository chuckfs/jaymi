//! Codebase Q&A (Sprint C2.5).
//!
//! Integrates Intelligent Code Search, Symbol Navigation, Relationship Tracing,
//! and Deep Investigation into natural conversational codebase questions.
//!
//! The Planner decides when investigation is necessary:
//! - Simple questions use ambient Workspace Intelligence / light search.
//! - Complex questions gather evidence before Reasoning.
//!
//! Constraints: Conversation First, read-only, no special Q&A workspace,
//! no modification, no autonomous execution. Claims are grounded in retrieved
//! evidence; conflicts are surfaced; missing evidence is stated explicitly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use jaymi_context::{ContextBundle, ContextSessionInputs};
use jaymi_core::{Citation, Document, LspLocation, UserRequest};

use crate::deep_investigation::{
    self, DeepInvestigationReport, InvestigationAssessment, InvestigationEvidence,
    InvestigationKind,
};
use crate::relationship_tracing::{
    RelationshipTraceReport, TraceAssessment, TraceEvidence, TraceKind,
};

/// Conversation evidence cap (not a repo dump).
pub const CONVERSATION_EVIDENCE_CAP: usize = 12;
/// Search hit limit for light / focused Q&A search.
pub const SEARCH_HIT_LIMIT: usize = 8;
/// Max file reads for focused Q&A.
pub const MAX_FILE_READS: usize = 2;
/// Preview chars kept on evidence rows.
pub const PREVIEW_CHARS: usize = 200;

/// How much retrieval the Planner should run before answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaComplexity {
    /// Ambient WI / project only — do not deep-investigate.
    Simple,
    /// One focused Search Knowledge pass.
    Search,
    /// Relationship / event-flow style hops.
    Flow,
    /// Multi-source deep investigation before answer.
    Investigate,
}

impl QaComplexity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Search => "search",
            Self::Flow => "flow",
            Self::Investigate => "investigate",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "Simple (ambient evidence)",
            Self::Search => "Focused search",
            Self::Flow => "Flow / relationship",
            Self::Investigate => "Deep investigation",
        }
    }
}

/// Conversational question shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaKind {
    HowHandles,
    WhereGets,
    Performance,
    EventFlow,
    FeaturePlacement,
    ChangeImpact,
    General,
}

impl QaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HowHandles => "how_handles",
            Self::WhereGets => "where_gets",
            Self::Performance => "performance",
            Self::EventFlow => "event_flow",
            Self::FeaturePlacement => "feature_placement",
            Self::ChangeImpact => "change_impact",
            Self::General => "general",
        }
    }
}

/// Which C2 capability the Planner routes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaRoute {
    Ambient,
    CodeSearch,
    Relationship,
    DeepInvestigation,
}

impl QaRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ambient => "ambient",
            Self::CodeSearch => "code_search",
            Self::Relationship => "relationship",
            Self::DeepInvestigation => "deep_investigation",
        }
    }
}

/// Detected Codebase Q&A assessment (Planner-owned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodebaseQaAssessment {
    pub kind: QaKind,
    pub complexity: QaComplexity,
    pub route: QaRoute,
    pub goal: String,
    pub subject: Option<String>,
    pub search_query: Option<String>,
    pub matched_rules: Vec<&'static str>,
}

/// One grounded evidence row for conversation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QaEvidence {
    pub path: PathBuf,
    pub title: String,
    pub matched: String,
    pub relevance: u32,
    pub line: Option<u32>,
    pub preview: String,
    pub source: String,
}

/// Conflicting evidence sides — never silently pick one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceConflict {
    pub topic: String,
    pub note: String,
    pub sides: Vec<QaEvidence>,
}

/// Planner-authored Codebase Q&A report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodebaseQaReport {
    pub goal: String,
    pub kind: Option<QaKind>,
    pub complexity: Option<QaComplexity>,
    pub route: Option<QaRoute>,
    pub subject: String,
    pub searches: Vec<String>,
    pub files_inspected: Vec<String>,
    pub symbols_followed: Vec<String>,
    pub evidence: Vec<QaEvidence>,
    pub conflicts: Vec<EvidenceConflict>,
    pub gaps: Vec<String>,
    pub investigation_needed: bool,
    pub diagnostics: Vec<String>,
}

impl CodebaseQaReport {
    /// Conversation body — grounded answer, conflicts, gaps. No raw diagnostics dump.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Codebase Q&A\n");
        if let Some(complexity) = self.complexity {
            out.push_str(&format!("\n### Approach\n- {}\n", complexity.label()));
            if let Some(route) = self.route {
                out.push_str(&format!("- Routed via `{}`\n", route.as_str()));
            }
        }
        if !self.subject.is_empty() {
            out.push_str(&format!("\n### Subject\n- {}\n", self.subject));
        }

        out.push_str("\n### Answer\n");
        out.push_str(&self.synthesize_answer());
        out.push('\n');

        out.push_str("\n### Evidence\n");
        if self.evidence.is_empty() {
            out.push_str("- _(no project evidence retrieved)_\n");
        } else {
            for (i, hit) in self
                .evidence
                .iter()
                .take(CONVERSATION_EVIDENCE_CAP)
                .enumerate()
            {
                let loc = hit
                    .line
                    .map(|l| format!(":{}", l.saturating_add(1)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{}. `{}{}` — {} ({})\n",
                    i + 1,
                    hit.title,
                    loc,
                    if hit.matched.is_empty() {
                        "match"
                    } else {
                        &hit.matched
                    },
                    hit.source
                ));
                if !hit.preview.trim().is_empty() {
                    out.push_str(&format!("   {}\n", truncate(&hit.preview, 140)));
                }
            }
        }

        if !self.conflicts.is_empty() {
            out.push_str("\n### Evidence conflicts\n");
            out.push_str(
                "- Multiple sources disagree — Jaymi is **not** choosing one silently.\n",
            );
            for conflict in &self.conflicts {
                out.push_str(&format!("- **{}** — {}\n", conflict.topic, conflict.note));
                for side in &conflict.sides {
                    out.push_str(&format!(
                        "  - `{}` ({}) {}\n",
                        side.title,
                        side.path.display(),
                        truncate(&side.preview, 100)
                    ));
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

        out.push_str("\n### Notes\n");
        out.push_str("- Conversation First — no special Codebase Q&A workspace.\n");
        out.push_str("- Read-only — no modification or autonomous execution.\n");
        out.push_str("- Substantive claims are limited to retrieved project evidence.\n");
        out
    }

    /// Compact brief for Reasoning (Planner-authored; model must not invent retrieval).
    pub fn evidence_brief_for_reasoning(&self) -> String {
        let mut lines = vec![
            format!(
                "codebase_qa complexity={} route={} subject={}",
                self.complexity.map(|c| c.as_str()).unwrap_or("?"),
                self.route.map(|r| r.as_str()).unwrap_or("?"),
                self.subject
            ),
            "Use only the evidence below. If something is missing, say so. Do not invent project behavior. If conflicts are listed, surface them — do not pick one silently.".into(),
        ];
        for hit in self.evidence.iter().take(CONVERSATION_EVIDENCE_CAP) {
            lines.push(format!(
                "- {} ({}) matched={} source={} preview={}",
                hit.title,
                hit.path.display(),
                hit.matched,
                hit.source,
                truncate(&hit.preview, 100)
            ));
        }
        if !self.conflicts.is_empty() {
            lines.push("conflicts:".into());
            for c in &self.conflicts {
                lines.push(format!("- {}: {}", c.topic, c.note));
            }
        }
        if !self.gaps.is_empty() {
            lines.push("gaps:".into());
            for g in &self.gaps {
                lines.push(format!("- {g}"));
            }
        }
        lines.join("\n")
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "codebase_qa kind={} complexity={} route={} subject={:?} evidence={} conflicts={} gaps={} investigation_needed={} searches={} files={} symbols={}",
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.complexity.map(|c| c.as_str()).unwrap_or("?"),
            self.route.map(|r| r.as_str()).unwrap_or("?"),
            self.subject,
            self.evidence.len(),
            self.conflicts.len(),
            self.gaps.len(),
            self.investigation_needed,
            self.searches.len(),
            self.files_inspected.len(),
            self.symbols_followed.len()
        )
    }

    fn synthesize_answer(&self) -> String {
        let mut parts = Vec::new();
        if self.evidence.is_empty() {
            parts.push(
                "- Could not establish project behavior from retrieved evidence — no grounded claim is offered."
                    .into(),
            );
        } else if !self.conflicts.is_empty() {
            parts.push(format!(
                "- Retrieved evidence for **`{}`** conflicts across {} site(s). See conflicts below; do not treat any single side as authoritative without review.",
                if self.subject.is_empty() {
                    "this question"
                } else {
                    &self.subject
                },
                self.conflicts.len()
            ));
        } else {
            match self.kind {
                Some(QaKind::HowHandles) => {
                    parts.push(format!(
                        "- From retrieved evidence, **`{}`** appears related to {} file(s)/symbol site(s). This is observational, not a full architecture claim.",
                        if self.subject.is_empty() {
                            "subject"
                        } else {
                            &self.subject
                        },
                        self.evidence.len()
                    ));
                }
                Some(QaKind::WhereGets) => {
                    parts.push(
                        "- Candidate sources for where the value/model is obtained are listed in Evidence (project hits only)."
                            .into(),
                    );
                }
                Some(QaKind::FeaturePlacement) => {
                    parts.push(
                        "- Candidate homes are suggested only from observed project layout / search hits — confirm ownership before editing."
                            .into(),
                    );
                }
                Some(QaKind::ChangeImpact) => {
                    parts.push(
                        "- Potential impact sites from references / search are listed below. Absence of a hit does not prove safety."
                            .into(),
                    );
                }
                Some(QaKind::EventFlow) => {
                    parts.push(
                        "- Observed flow-related symbols / call sites are listed; the full runtime path may be incomplete."
                            .into(),
                    );
                }
                Some(QaKind::Performance) => {
                    parts.push(
                        "- Performance notes below are observational (call sites / related code). Jaymi did not measure wall-clock latency."
                            .into(),
                    );
                }
                _ => {
                    parts.push(format!(
                        "- Grounded in {} retrieved evidence row(s).",
                        self.evidence.len()
                    ));
                }
            }
        }
        if self.investigation_needed {
            parts.push(
                "- Complex question — evidence was gathered before answering (Planner-orchestrated)."
                    .into(),
            );
        } else {
            parts.push(
                "- Simple / focused question — deep investigation was not required."
                    .into(),
            );
        }
        parts.join("\n")
    }
}

/// Detect natural conversational codebase Q&A (Planner ownership).
///
/// Yields when a narrower C2.1–C2.4 detector already owns the phrasing, so those
/// soft paths keep priority and simple/search/flow/deep stay specialized.
pub fn detect_codebase_qa_request(request: &UserRequest) -> Option<CodebaseQaAssessment> {
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
        || crate::build_awareness::detect_build_awareness_request(request).is_some()
        || crate::test_execution::detect_test_execution_request(request).is_some()
        || crate::fix_from_verification::steals_from_other_detect(request)
    {
        return None;
    }
    // Narrower C2 modes win when they already match.
    // Use code_search phrasing only (not full detect) to avoid
    // code_search → understanding → codebase_qa recursion.
    if crate::relationship_tracing::detect_relationship_trace_request(request).is_some()
        || crate::deep_investigation::detect_deep_investigation_request(request).is_some()
        || crate::symbol_navigation::detect_symbol_navigation_request(request).is_some()
        || crate::code_search::looks_like_code_search_goal(&request.content)
    {
        return None;
    }

    let (kind, complexity, route, rules, subject, search_query) =
        classify_phrasing(&lower, content)?;
    Some(CodebaseQaAssessment {
        kind,
        complexity,
        route,
        goal: content.to_string(),
        subject,
        search_query,
        matched_rules: rules,
    })
}

fn classify_phrasing(
    lower: &str,
    original: &str,
) -> Option<(
    QaKind,
    QaComplexity,
    QaRoute,
    Vec<&'static str>,
    Option<String>,
    Option<String>,
)> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    // "How does Jaymi handle permissions?" / "How does X handle Y?"
    if (trimmed.starts_with("how does ") || trimmed.starts_with("how do "))
        && (trimmed.contains(" handle ")
            || trimmed.contains(" handles ")
            || trimmed.contains(" manage ")
            || trimmed.contains(" manages "))
    {
        let subject = extract_how_handles_subject(original);
        let query = subject.clone().or_else(|| Some("permissions".into()));
        return Some((
            QaKind::HowHandles,
            QaComplexity::Investigate,
            QaRoute::DeepInvestigation,
            vec!["how_handles"],
            subject,
            query,
        ));
    }

    // "Where does Settings get its models?"
    if trimmed.starts_with("where does ")
        && (trimmed.contains(" get ")
            || trimmed.contains(" gets ")
            || trimmed.contains(" load ")
            || trimmed.contains(" loads ")
            || trimmed.contains(" obtain "))
    {
        let subject = extract_where_does_subject(original);
        let query = subject
            .clone()
            .map(|s| format!("{s} models"))
            .or_else(|| Some("settings models".into()));
        return Some((
            QaKind::WhereGets,
            QaComplexity::Search,
            QaRoute::CodeSearch,
            vec!["where_gets"],
            subject,
            query,
        ));
    }

    // Performance variants not already claimed by C2.4
    if (trimmed.contains("slow") || trimmed.contains("latency") || trimmed.contains("performance"))
        && (trimmed.starts_with("why ")
            || trimmed.contains("why is")
            || trimmed.contains("what\'s making")
            || trimmed.contains("what is making"))
    {
        return Some((
            QaKind::Performance,
            QaComplexity::Investigate,
            QaRoute::DeepInvestigation,
            vec!["performance_qa"],
            None,
            Some("slow request performance".into()),
        ));
    }

    // "What happens after I press Enter?"
    if trimmed.starts_with("what happens after i press ")
        || trimmed.starts_with("what happens after i hit ")
        || trimmed.starts_with("what happens after pressing ")
        || (trimmed.starts_with("what happens after ") && trimmed.contains(" press "))
    {
        let hint = extract_after(original, &["press ", "hit ", "pressing "]);
        return Some((
            QaKind::EventFlow,
            QaComplexity::Flow,
            QaRoute::Relationship,
            vec!["event_flow_after"],
            hint,
            None,
        ));
    }

    // "Where would image generation belong?"
    // Keep C1.2 "Where should this feature live: …" with Understanding.
    if trimmed.starts_with("where would ")
        && (trimmed.contains(" belong")
            || trimmed.contains(" live")
            || trimmed.contains(" go")
            || trimmed.contains(" fit"))
    {
        let subject = extract_feature_subject(original);
        let query = subject.clone();
        return Some((
            QaKind::FeaturePlacement,
            QaComplexity::Search,
            QaRoute::CodeSearch,
            vec!["feature_belong"],
            subject,
            query,
        ));
    }
    if trimmed.starts_with("where should ")
        && trimmed.contains(" belong")
        && !trimmed.contains("this feature")
    {
        let subject = extract_feature_subject(original);
        let query = subject.clone();
        return Some((
            QaKind::FeaturePlacement,
            QaComplexity::Search,
            QaRoute::CodeSearch,
            vec!["feature_belong_should"],
            subject,
            query,
        ));
    }

    // "What would break if I removed this provider?"
    if trimmed.contains("what would break if")
        || trimmed.contains("what breaks if i remove")
        || trimmed.contains("what would break if i removed")
        || (trimmed.contains("if i removed") && trimmed.contains("break"))
        || (trimmed.contains("if i remove")
            && (trimmed.contains("break") || trimmed.contains("affected")))
    {
        return Some((
            QaKind::ChangeImpact,
            QaComplexity::Investigate,
            QaRoute::DeepInvestigation,
            vec!["remove_impact"],
            Some("provider".into()),
            Some("provider".into()),
        ));
    }

    // Gentle general codebase questions — keep search-focused when possible.
    if trimmed.starts_with("tell me how ")
        || (trimmed.starts_with("explain how ")
            && (trimmed.contains(" in this project")
                || trimmed.contains(" in the codebase")
                || trimmed.contains(" in jaymi")))
    {
        let subject = extract_after(original, &["tell me how ", "explain how "]);
        return Some((
            QaKind::General,
            QaComplexity::Search,
            QaRoute::CodeSearch,
            vec!["explain_how_project"],
            subject.clone(),
            subject,
        ));
    }

    None
}

fn extract_how_handles_subject(original: &str) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    let start = lower.find("how does ").or_else(|| lower.find("how do "))?;
    let marker_len = if lower[start..].starts_with("how does ") {
        "how does ".len()
    } else {
        "how do ".len()
    };
    let after = original[start + marker_len..].trim();
    let lower_after = after.to_ascii_lowercase();
    for verb in [" handle ", " handles ", " manage ", " manages "] {
        if let Some(idx) = lower_after.find(verb) {
            let object = after[idx + verb.len()..]
                .trim()
                .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!');
            if !object.is_empty() {
                return Some(object.to_string());
            }
        }
    }
    None
}

fn extract_where_does_subject(original: &str) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    let Some(idx) = lower.find("where does ") else {
        return None;
    };
    let after = original[idx + "where does ".len()..].trim();
    let lower_after = after.to_ascii_lowercase();
    for verb in [" get ", " gets ", " load ", " loads ", " obtain "] {
        if let Some(v) = lower_after.find(verb) {
            let subject = after[..v].trim();
            if !subject.is_empty() {
                return Some(subject.to_string());
            }
        }
    }
    None
}

fn extract_feature_subject(original: &str) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    for marker in ["where would ", "where should "] {
        if let Some(idx) = lower.find(marker) {
            let after = original[idx + marker.len()..]
                .trim()
                .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!');
            let mut s = after.to_string();
            let lower_s = s.to_ascii_lowercase();
            for suffix in [" belong", " live", " go", " fit"] {
                if let Some(stripped) = lower_s.strip_suffix(suffix.trim_start()) {
                    // Use byte length carefully — suffixes are ASCII.
                    let keep = stripped.len();
                    s.truncate(keep);
                    break;
                }
            }
            let s = s
                .trim()
                .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!')
                .to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
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
                .next()
                .unwrap_or(after)
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Begin an ambient (simple) report from Workspace Intelligence only.
pub fn begin_ambient(
    assessment: &CodebaseQaAssessment,
    bundle: &ContextBundle,
    _session: &ContextSessionInputs,
) -> CodebaseQaReport {
    let mut report = empty_report(assessment);
    let editor = bundle.editor_intelligence();
    if let Some(sym) = &editor.symbol {
        report.symbols_followed.push(sym.name.clone());
        report.evidence.push(QaEvidence {
            path: PathBuf::from(
                bundle
                    .current_file()
                    .path
                    .clone()
                    .unwrap_or_else(|| "(cursor)".into()),
            ),
            title: sym.name.clone(),
            matched: sym.name.clone(),
            relevance: 90,
            line: sym.range.as_ref().map(|r| r.start_line),
            preview: format!("kind={}", sym.kind.clone().unwrap_or_default()),
            source: "cursor_symbol".into(),
        });
    }
    for reference in editor.references.iter().take(6) {
        note_file(&mut report.files_inspected, &reference.path);
        report.evidence.push(QaEvidence {
            path: PathBuf::from(&reference.path),
            title: Path::new(&reference.path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&reference.path)
                .to_string(),
            matched: report.subject.clone(),
            relevance: 70,
            line: Some(reference.range.start_line),
            preview: String::new(),
            source: "editor_reference".into(),
        });
    }
    let project = bundle.project_intelligence();
    if !project.languages.is_empty() || project.layout_shape.is_some() {
        report.evidence.push(QaEvidence {
            path: PathBuf::from("(project)"),
            title: "project_intelligence".into(),
            matched: project.languages.first().cloned().unwrap_or_default(),
            relevance: 50,
            line: None,
            preview: format!(
                "languages={} layout={:?}",
                project.languages.join(","),
                project.layout_shape
            ),
            source: "project_context".into(),
        });
    }
    if report.evidence.is_empty() {
        report
            .gaps
            .push("No ambient Workspace Intelligence evidence available for this question.".into());
    }
    finalize_conflicts_and_gaps(report)
}

/// Merge search citations into a Q&A report.
pub fn merge_search_citations(report: &mut CodebaseQaReport, citations: &[Citation], query: &str) {
    report.searches.push(query.to_string());
    if citations.is_empty() {
        report
            .gaps
            .push(format!("Search for {query:?} returned no project hits."));
        return;
    }
    for c in citations.iter().take(SEARCH_HIT_LIMIT) {
        note_file(
            &mut report.files_inspected,
            &c.location.display().to_string(),
        );
        if let Some(section) = &c.matching_section {
            if !section.is_empty() && !report.symbols_followed.iter().any(|s| s == section) {
                report.symbols_followed.push(section.clone());
            }
        }
        report.evidence.push(QaEvidence {
            path: c.location.clone(),
            title: c.title.clone(),
            matched: c
                .matching_section
                .clone()
                .unwrap_or_else(|| c.why_matched.clone()),
            relevance: c.confidence_percent(),
            line: c.line,
            preview: truncate(&c.preview, PREVIEW_CHARS),
            source: "search_knowledge".into(),
        });
    }
}

pub fn merge_read_document(
    report: &mut CodebaseQaReport,
    path: &Path,
    document: Option<&Document>,
) {
    note_file(&mut report.files_inspected, &path.display().to_string());
    match document {
        Some(doc) => {
            report.evidence.push(QaEvidence {
                path: path.to_path_buf(),
                title: doc
                    .title
                    .clone()
                    .unwrap_or_else(|| path.display().to_string()),
                matched: report.subject.clone(),
                relevance: 80,
                line: None,
                preview: truncate(&doc.text, PREVIEW_CHARS),
                source: "read_file".into(),
            });
        }
        None => {
            report
                .gaps
                .push(format!("Read of `{}` returned no document.", path.display()));
        }
    }
}

pub fn merge_lsp_locations(
    report: &mut CodebaseQaReport,
    locations: &[LspLocation],
    label: &str,
) {
    if locations.is_empty() {
        report
            .gaps
            .push(format!("No LSP {label} locations returned."));
        return;
    }
    for loc in locations.iter().take(8) {
        let path = loc.path.clone();
        note_file(&mut report.files_inspected, &path);
        report.evidence.push(QaEvidence {
            path: PathBuf::from(&path),
            title: Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&path)
                .to_string(),
            matched: report.subject.clone(),
            relevance: 75,
            line: Some(loc.range.start.line),
            preview: String::new(),
            source: format!("lsp_{label}"),
        });
    }
}

/// Lift a deep-investigation report into Codebase Q&A (route = deep).
pub fn from_deep_investigation(
    assessment: &CodebaseQaAssessment,
    deep: &DeepInvestigationReport,
) -> CodebaseQaReport {
    let mut report = empty_report(assessment);
    report.route = Some(QaRoute::DeepInvestigation);
    report.complexity = Some(QaComplexity::Investigate);
    report.investigation_needed = true;
    report.searches = deep.searches.clone();
    report.files_inspected = deep.files_inspected.clone();
    report.symbols_followed = deep.symbols_followed.clone();
    report.gaps.extend(deep.gaps.iter().cloned());
    report.diagnostics.push(deep.to_diagnostics_summary());
    for e in &deep.evidence {
        report.evidence.push(from_investigation_evidence(e));
    }
    if report.subject.is_empty() {
        report.subject = deep.subject.clone();
    }
    finalize_conflicts_and_gaps(report)
}

/// Lift a relationship-trace report into Codebase Q&A (route = relationship).
pub fn from_relationship_trace(
    assessment: &CodebaseQaAssessment,
    trace: &RelationshipTraceReport,
) -> CodebaseQaReport {
    let mut report = empty_report(assessment);
    report.route = Some(QaRoute::Relationship);
    report.complexity = Some(QaComplexity::Flow);
    report.investigation_needed = true;
    report.diagnostics.push(trace.to_diagnostics_summary());
    report.diagnostics.push(format!(
        "relationship_files={} relationship_symbols={}",
        trace.files_inspected, trace.symbols_followed
    ));
    if trace.evidence.is_empty() {
        report
            .gaps
            .push("Relationship trace found no chain evidence.".into());
    }
    for e in &trace.evidence {
        note_file(
            &mut report.files_inspected,
            &e.path.display().to_string(),
        );
        if !e.matched.is_empty()
            && !report.symbols_followed.iter().any(|s| s == &e.matched)
        {
            report.symbols_followed.push(e.matched.clone());
        }
        report.evidence.push(from_trace_evidence(e));
    }
    if report.subject.is_empty() {
        report.subject = trace.seed_symbol.clone();
    }
    finalize_conflicts_and_gaps(report)
}

/// Build a deep-investigation assessment from a Q&A assessment (for reuse).
pub fn to_deep_assessment(assessment: &CodebaseQaAssessment) -> InvestigationAssessment {
    let kind = match assessment.kind {
        QaKind::Performance => InvestigationKind::Performance,
        QaKind::HowHandles => InvestigationKind::SubsystemHow,
        QaKind::FeaturePlacement => InvestigationKind::FeaturePlacement,
        QaKind::ChangeImpact => InvestigationKind::ChangeImpact,
        _ => InvestigationKind::General,
    };
    InvestigationAssessment {
        kind,
        goal: assessment.goal.clone(),
        subject: assessment.subject.clone(),
        matched_rules: vec!["codebase_qa"],
    }
}

/// Build a relationship-trace assessment from a Q&A assessment (for reuse).
pub fn to_trace_assessment(assessment: &CodebaseQaAssessment) -> TraceAssessment {
    TraceAssessment {
        kind: TraceKind::EventFlow,
        goal: assessment.goal.clone(),
        seed_hint: assessment.subject.clone(),
        matched_rules: vec!["codebase_qa"],
    }
}

/// Primary search query for focused / investigate routes.
pub fn primary_search_query(assessment: &CodebaseQaAssessment) -> Option<String> {
    assessment
        .search_query
        .clone()
        .filter(|q| !q.trim().is_empty())
        .or_else(|| assessment.subject.clone())
}

/// Candidate paths to read (bounded).
pub fn candidate_read_paths(report: &CodebaseQaReport) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for hit in &report.evidence {
        if hit.source == "project_context" {
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

pub fn citations_to_search_hits(
    citations: &[Citation],
) -> Vec<jaymi_context::BundleSearchHit> {
    deep_investigation::citations_to_search_hits(citations)
}

/// Detect conflicts: same topic/matched token spanning multiple files with divergent previews.
pub fn detect_conflicts(evidence: &[QaEvidence]) -> Vec<EvidenceConflict> {
    let mut by_token: HashMap<String, Vec<&QaEvidence>> = HashMap::new();
    for e in evidence {
        let token = normalize_token(&e.matched);
        if token.is_empty() || token.len() < 3 {
            continue;
        }
        by_token.entry(token).or_default().push(e);
    }
    let mut conflicts = Vec::new();
    for (token, rows) in by_token {
        if rows.len() < 2 {
            continue;
        }
        let distinct_paths: HashSet<String> = rows
            .iter()
            .map(|r| r.path.display().to_string())
            .collect();
        if distinct_paths.len() < 2 {
            continue;
        }
        let joined = rows
            .iter()
            .map(|r| r.preview.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let has_pos = CONFLICT_POSITIVE.iter().any(|w| joined.contains(w));
        let has_neg = CONFLICT_NEGATIVE.iter().any(|w| joined.contains(w));
        let divergent_previews = rows.iter().any(|a| {
            rows.iter().any(|b| {
                !a.preview.is_empty()
                    && !b.preview.is_empty()
                    && a.path != b.path
                    && preview_fingerprint(&a.preview) != preview_fingerprint(&b.preview)
            })
        });
        if (has_pos && has_neg) || divergent_previews {
            let note = if has_pos && has_neg {
                "Sources mention both affirming and rejecting language for this topic.".into()
            } else {
                "Same symbol/topic appears in multiple files with different local context.".into()
            };
            conflicts.push(EvidenceConflict {
                topic: token,
                note,
                sides: rows.iter().take(4).map(|r| (*r).clone()).collect(),
            });
        }
    }
    conflicts
}

const CONFLICT_POSITIVE: &[&str] = &[
    "allow", "permit", "grant", "enable", "true", "ok", "approve",
];
const CONFLICT_NEGATIVE: &[&str] = &[
    "deny", "forbid", "reject", "disable", "false", "block", "refuse",
];

fn finalize_conflicts_and_gaps(mut report: CodebaseQaReport) -> CodebaseQaReport {
    report.conflicts = detect_conflicts(&report.evidence);
    if report.evidence.is_empty()
        && !report
            .gaps
            .iter()
            .any(|g| g.contains("Could not") || g.contains("No "))
    {
        report.gaps.push(
            "Insufficient project evidence — Jaymi will not invent codebase behavior.".into(),
        );
    }
    if !report.conflicts.is_empty() {
        report.diagnostics.push(format!(
            "conflicts_detected={}",
            report.conflicts.len()
        ));
    }
    report
}

fn empty_report(assessment: &CodebaseQaAssessment) -> CodebaseQaReport {
    CodebaseQaReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        complexity: Some(assessment.complexity),
        route: Some(assessment.route),
        subject: assessment.subject.clone().unwrap_or_default(),
        searches: Vec::new(),
        files_inspected: Vec::new(),
        symbols_followed: Vec::new(),
        evidence: Vec::new(),
        conflicts: Vec::new(),
        gaps: Vec::new(),
        investigation_needed: matches!(
            assessment.complexity,
            QaComplexity::Flow | QaComplexity::Investigate
        ),
        diagnostics: vec![
            format!("matched_rules={}", assessment.matched_rules.join(",")),
            format!("complexity={}", assessment.complexity.as_str()),
            format!("route={}", assessment.route.as_str()),
        ],
    }
}

fn from_investigation_evidence(e: &InvestigationEvidence) -> QaEvidence {
    QaEvidence {
        path: e.path.clone(),
        title: e.title.clone(),
        matched: e.matched.clone(),
        relevance: e.relevance,
        line: e.line,
        preview: e.preview.clone(),
        source: e.why.clone(),
    }
}

fn from_trace_evidence(e: &TraceEvidence) -> QaEvidence {
    QaEvidence {
        path: e.path.clone(),
        title: e.title.clone(),
        matched: e.matched.clone(),
        relevance: e.relevance,
        line: e.line,
        preview: e.preview.clone(),
        source: e.why.clone(),
    }
}

fn note_file(files: &mut Vec<String>, path: &str) {
    if path.is_empty() {
        return;
    }
    if !files.iter().any(|f| f == path) {
        files.push(path.to_string());
    }
}

fn normalize_token(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn preview_fingerprint(preview: &str) -> String {
    preview
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn truncate(text: &str, max_chars: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max_chars {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{EditorReference, EditorSymbol, ProjectIntelligenceSection};
    use jaymi_core::UserRequest;

    #[test]
    fn detects_sprint_examples() {
        let perms = detect_codebase_qa_request(&UserRequest::new(
            "How does Jaymi handle permissions?",
        ))
        .expect("permissions");
        assert_eq!(perms.kind, QaKind::HowHandles);
        assert_eq!(perms.complexity, QaComplexity::Investigate);
        assert_eq!(perms.route, QaRoute::DeepInvestigation);

        let settings = detect_codebase_qa_request(&UserRequest::new(
            "Where does Settings get its models?",
        ))
        .expect("settings");
        assert_eq!(settings.kind, QaKind::WhereGets);
        assert_eq!(settings.complexity, QaComplexity::Search);
        assert_eq!(settings.route, QaRoute::CodeSearch);

        let after = detect_codebase_qa_request(&UserRequest::new(
            "What happens after I press Enter?",
        ))
        .expect("enter");
        assert_eq!(after.kind, QaKind::EventFlow);
        assert_eq!(after.complexity, QaComplexity::Flow);

        let belong = detect_codebase_qa_request(&UserRequest::new(
            "Where would image generation belong?",
        ))
        .expect("belong");
        assert_eq!(belong.kind, QaKind::FeaturePlacement);
        assert_eq!(belong.complexity, QaComplexity::Search);
        assert!(
            belong
                .subject
                .as_deref()
                .is_some_and(|s| s.to_ascii_lowercase().contains("image")),
            "{:?}",
            belong.subject
        );

        let remove = detect_codebase_qa_request(&UserRequest::new(
            "What would break if I removed this provider?",
        ))
        .expect("remove");
        assert_eq!(remove.kind, QaKind::ChangeImpact);
        assert_eq!(remove.complexity, QaComplexity::Investigate);
    }

    #[test]
    fn yields_to_narrower_c2_modes() {
        assert!(detect_codebase_qa_request(&UserRequest::new("Why is this slow?")).is_none());
        assert!(detect_codebase_qa_request(&UserRequest::new(
            "What happens when I click Save?",
        ))
        .is_none());
        assert!(detect_codebase_qa_request(&UserRequest::new("Who calls this?")).is_none());
        assert!(detect_codebase_qa_request(&UserRequest::new(
            "Where is authentication handled?",
        ))
        .is_none());
    }

    #[test]
    fn simple_questions_do_not_force_investigate() {
        let settings = detect_codebase_qa_request(&UserRequest::new(
            "Where does Settings get its models?",
        ))
        .unwrap();
        assert_ne!(settings.complexity, QaComplexity::Investigate);
        let belong = detect_codebase_qa_request(&UserRequest::new(
            "Where would image generation belong?",
        ))
        .unwrap();
        assert_ne!(belong.complexity, QaComplexity::Investigate);
    }

    #[test]
    fn surfaces_conflicts_and_keeps_diagnostics_out_of_markdown() {
        let assessment = detect_codebase_qa_request(&UserRequest::new(
            "How does Jaymi handle permissions?",
        ))
        .unwrap();
        let mut report = empty_report(&assessment);
        report.evidence = vec![
            QaEvidence {
                path: PathBuf::from("/proj/a.rs"),
                title: "a.rs".into(),
                matched: "permissions".into(),
                relevance: 80,
                line: Some(1),
                preview: "allow all network grants".into(),
                source: "search_knowledge".into(),
            },
            QaEvidence {
                path: PathBuf::from("/proj/b.rs"),
                title: "b.rs".into(),
                matched: "permissions".into(),
                relevance: 80,
                line: Some(2),
                preview: "deny network and forbid grants".into(),
                source: "search_knowledge".into(),
            },
        ];
        report = finalize_conflicts_and_gaps(report);
        assert!(!report.conflicts.is_empty());
        let md = report.to_markdown();
        assert!(md.contains("Evidence conflicts"));
        assert!(md.contains("choosing one silently"));
        assert!(!md.contains("codebase_qa kind="));
        assert!(report.to_diagnostics_summary().contains("codebase_qa"));
    }

    #[test]
    fn ambient_begin_records_gaps_when_empty() {
        let assessment = detect_codebase_qa_request(&UserRequest::new(
            "Where does Settings get its models?",
        ))
        .unwrap();
        let mut simple = assessment.clone();
        simple.complexity = QaComplexity::Simple;
        simple.route = QaRoute::Ambient;
        let report = begin_ambient(
            &simple,
            &ContextBundle::builder().build(),
            &ContextSessionInputs::default(),
        );
        assert!(!report.gaps.is_empty());
        assert!(report.to_markdown().contains("What could not be established"));
    }

    #[test]
    fn ambient_uses_editor_without_deep_route() {
        let assessment = detect_codebase_qa_request(&UserRequest::new(
            "Where does Settings get its models?",
        ))
        .unwrap();
        let mut simple = assessment.clone();
        simple.complexity = QaComplexity::Simple;
        simple.route = QaRoute::Ambient;
        let bundle = ContextBundle::builder()
            .editor_intelligence(jaymi_context::EditorIntelligenceSection {
                symbol: Some(EditorSymbol {
                    name: "Settings".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                references: vec![EditorReference {
                    path: "/proj/settings.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .project_intelligence(ProjectIntelligenceSection {
                languages: vec!["rust".into()],
                ..Default::default()
            })
            .build();
        let report = begin_ambient(&simple, &bundle, &ContextSessionInputs::default());
        assert!(!report.evidence.is_empty());
        assert!(!report.investigation_needed);
        assert!(
            report
                .to_markdown()
                .contains("deep investigation was not required")
                || report.to_markdown().contains("Simple")
        );
    }
}
