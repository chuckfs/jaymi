//! Unified codebase evidence for C2.1–C2.5 Reasoning handoff (Sprint C2.7.2).
//!
//! Converts search / symbol-navigation / relationship / deep / Q&A evidence into
//! one Planner-owned representation that can:
//! 1. Inject as session `search_hits` for ContextEngine re-assemble (ContextPolicy
//!    selects what enters the ContextBundle), and
//! 2. Supply a compact evidence brief for the conversational Prompt → Reasoning
//!    path when Reasoning is available and the question is non-trivial.
//!
//! C2 modules never call ReasoningProvider. Deterministic markdown remains the
//! fallback when Reasoning is unavailable or the question is trivial.

use std::path::PathBuf;

use jaymi_context::BundleSearchHit;

use crate::code_search::{CodeSearchEvidence, CodeSearchKind, CodeSearchReport};
use crate::codebase_qa::{CodebaseQaReport, QaEvidence};
use crate::deep_investigation::{DeepInvestigationReport, InvestigationEvidence};
use crate::relationship_tracing::{RelationshipTraceReport, TraceEvidence};
use crate::symbol_navigation::{SymbolNavEvidence, SymbolNavKind, SymbolNavigationReport};

/// Max hits injected into session / brief (bounded).
pub const EVIDENCE_HIT_CAP: usize = 16;
/// Preview chars in brief / BundleSearchHit.
pub const PREVIEW_CHARS: usize = 180;

/// Shared evidence row (same shape as deep investigation / codebase Q&A).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodebaseEvidenceHit {
    pub path: PathBuf,
    pub title: String,
    pub matched: String,
    pub relevance: u32,
    pub line: Option<u32>,
    pub preview: String,
    /// Source label (`search_knowledge`, `editor_reference`, `lsp_references`, …).
    pub source: String,
}

/// Which C2 path produced the evidence (handoff policy + brief header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodebaseEvidenceOrigin {
    CodeSearch,
    SymbolNavigation,
    RelationshipTrace,
    DeepInvestigation,
    CodebaseQa,
}

impl CodebaseEvidenceOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeSearch => "code_search",
            Self::SymbolNavigation => "symbol_navigation",
            Self::RelationshipTrace => "relationship_trace",
            Self::DeepInvestigation => "deep_investigation",
            Self::CodebaseQa => "codebase_qa",
        }
    }

    pub fn brief_banner(self) -> &'static str {
        match self {
            Self::CodeSearch => {
                "[Planner code search — use only this evidence; if something is missing, say so. Do not invent retrieval.]"
            }
            Self::SymbolNavigation => {
                "[Planner symbol navigation — use only this evidence; if something is missing, say so. Do not invent retrieval.]"
            }
            Self::RelationshipTrace => {
                "[Planner relationship trace — use only this evidence; if something is missing, say so. Do not invent retrieval.]"
            }
            Self::DeepInvestigation => {
                "[Planner investigation — use only this evidence; if something is missing, say so explicitly. Do not invent retrieval.]"
            }
            Self::CodebaseQa => {
                "[Planner Codebase Q&A — use only this evidence; surface conflicts; if something is missing, say so. Do not invent retrieval or project behavior.]"
            }
        }
    }
}

/// Hints that refine whether Reasoning should run (vs fast deterministic report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandoffHints {
    pub code_search_kind: Option<CodeSearchKind>,
    pub symbol_nav_kind: Option<SymbolNavKind>,
    pub investigation_needed: bool,
}

/// Whether Planner should hand off to Context → Prompt → Reasoning.
///
/// Trivial lookups stay deterministic even when Reasoning is implemented
/// (Conversation First + Offline First cost control).
pub fn prefers_reasoning_handoff(
    origin: CodebaseEvidenceOrigin,
    evidence_len: usize,
    hints: HandoffHints,
) -> bool {
    match origin {
        CodebaseEvidenceOrigin::DeepInvestigation => true,
        CodebaseEvidenceOrigin::CodebaseQa => hints.investigation_needed || evidence_len > 0,
        CodebaseEvidenceOrigin::RelationshipTrace => evidence_len > 0 || hints.investigation_needed,
        CodebaseEvidenceOrigin::CodeSearch => match hints.code_search_kind {
            // Exact / reference lists are already the answer — no LLM.
            Some(CodeSearchKind::ExactSymbol) => false,
            Some(CodeSearchKind::FindReferences) => false,
            // Empty honest report — no LLM.
            _ if evidence_len == 0 => false,
            // Natural-language / responsibility: gather then reason.
            Some(CodeSearchKind::NaturalLanguage) | Some(CodeSearchKind::Responsibility) => true,
            None => evidence_len > 3,
        },
        CodebaseEvidenceOrigin::SymbolNavigation => match hints.symbol_nav_kind {
            // Simple location lists stay deterministic.
            Some(
                SymbolNavKind::Definition
                | SymbolNavKind::SymbolLocation
                | SymbolNavKind::References
                | SymbolNavKind::Callers
                | SymbolNavKind::EnclosingSymbol
                | SymbolNavKind::Symbols
                | SymbolNavKind::Callees
                | SymbolNavKind::Imports,
            ) => false,
            Some(SymbolNavKind::Implementations | SymbolNavKind::Dependents) => evidence_len > 0,
            None => false,
        },
    }
}

pub fn from_code_search_evidence(hit: &CodeSearchEvidence) -> CodebaseEvidenceHit {
    CodebaseEvidenceHit {
        path: hit.path.clone(),
        title: hit.title.clone(),
        matched: hit.matched.clone(),
        relevance: hit.relevance,
        line: hit.line,
        preview: truncate(&hit.preview, PREVIEW_CHARS),
        source: if hit.why.is_empty() {
            "code_search".into()
        } else {
            hit.why.clone()
        },
    }
}

pub fn from_symbol_nav_evidence(hit: &SymbolNavEvidence) -> CodebaseEvidenceHit {
    CodebaseEvidenceHit {
        path: hit.path.clone(),
        title: hit.title.clone(),
        matched: hit.matched.clone(),
        relevance: hit.relevance,
        line: hit.line,
        preview: truncate(&hit.preview, PREVIEW_CHARS),
        source: if hit.why.is_empty() {
            "symbol_navigation".into()
        } else {
            hit.why.clone()
        },
    }
}

pub fn from_trace_evidence(hit: &TraceEvidence) -> CodebaseEvidenceHit {
    CodebaseEvidenceHit {
        path: hit.path.clone(),
        title: hit.title.clone(),
        matched: hit.matched.clone(),
        relevance: hit.relevance,
        line: hit.line,
        preview: truncate(&hit.preview, PREVIEW_CHARS),
        source: if hit.why.is_empty() {
            "relationship_trace".into()
        } else {
            hit.why.clone()
        },
    }
}

pub fn from_investigation_evidence(hit: &InvestigationEvidence) -> CodebaseEvidenceHit {
    CodebaseEvidenceHit {
        path: hit.path.clone(),
        title: hit.title.clone(),
        matched: hit.matched.clone(),
        relevance: hit.relevance,
        line: hit.line,
        preview: truncate(&hit.preview, PREVIEW_CHARS),
        source: hit.why.clone(),
    }
}

pub fn from_qa_evidence(hit: &QaEvidence) -> CodebaseEvidenceHit {
    CodebaseEvidenceHit {
        path: hit.path.clone(),
        title: hit.title.clone(),
        matched: hit.matched.clone(),
        relevance: hit.relevance,
        line: hit.line,
        preview: truncate(&hit.preview, PREVIEW_CHARS),
        source: hit.source.clone(),
    }
}

pub fn hits_from_code_search(report: &CodeSearchReport) -> Vec<CodebaseEvidenceHit> {
    report
        .evidence
        .iter()
        .take(EVIDENCE_HIT_CAP)
        .map(from_code_search_evidence)
        .collect()
}

pub fn hits_from_symbol_navigation(report: &SymbolNavigationReport) -> Vec<CodebaseEvidenceHit> {
    report
        .evidence
        .iter()
        .take(EVIDENCE_HIT_CAP)
        .map(from_symbol_nav_evidence)
        .collect()
}

pub fn hits_from_relationship_trace(report: &RelationshipTraceReport) -> Vec<CodebaseEvidenceHit> {
    report
        .evidence
        .iter()
        .take(EVIDENCE_HIT_CAP)
        .map(from_trace_evidence)
        .collect()
}

pub fn hits_from_deep_investigation(report: &DeepInvestigationReport) -> Vec<CodebaseEvidenceHit> {
    report
        .evidence
        .iter()
        .take(EVIDENCE_HIT_CAP)
        .map(from_investigation_evidence)
        .collect()
}

pub fn hits_from_codebase_qa(report: &CodebaseQaReport) -> Vec<CodebaseEvidenceHit> {
    report
        .evidence
        .iter()
        .take(EVIDENCE_HIT_CAP)
        .map(from_qa_evidence)
        .collect()
}

/// Session search hits for ContextEngine re-assemble (ContextPolicy selects).
pub fn to_bundle_search_hits(hits: &[CodebaseEvidenceHit]) -> Vec<BundleSearchHit> {
    hits.iter()
        .take(EVIDENCE_HIT_CAP)
        .enumerate()
        .map(|(i, h)| BundleSearchHit {
            item_id: format!("c2-evidence-{i}"),
            title: h.title.clone(),
            path: Some(h.path.display().to_string()),
            score: Some(h.relevance),
            match_reason: Some(h.source.clone()),
            preview: Some(truncate(&h.preview, PREVIEW_CHARS)),
            line: h.line,
            column: None,
        })
        .collect()
}

/// Compact Planner-authored brief for conversational Reasoning.
pub fn evidence_brief_for_reasoning(
    origin: CodebaseEvidenceOrigin,
    goal: &str,
    subject: &str,
    hits: &[CodebaseEvidenceHit],
    gaps: &[String],
) -> String {
    let mut lines = vec![
        format!("origin={}", origin.as_str()),
        format!("goal={goal}"),
        format!("subject={subject}"),
        "Use only the evidence below. If something is missing, say so. Do not invent project behavior or retrieval.".into(),
    ];
    for hit in hits.iter().take(EVIDENCE_HIT_CAP) {
        lines.push(format!(
            "- {} ({}) matched={} source={} preview={}",
            hit.title,
            hit.path.display(),
            hit.matched,
            hit.source,
            truncate(&hit.preview, 100)
        ));
    }
    if hits.is_empty() {
        lines.push("- _(no evidence rows)_".into());
    }
    if !gaps.is_empty() {
        lines.push("gaps:".into());
        for g in gaps {
            lines.push(format!("- {g}"));
        }
    }
    lines.join("\n")
}

pub fn enrich_request_with_brief(
    request_content: &str,
    origin: CodebaseEvidenceOrigin,
    brief: &str,
) -> String {
    format!(
        "{}\n\n{}\n{}",
        request_content.trim(),
        origin.brief_banner(),
        brief
    )
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
    use crate::code_search::CodeSearchKind;
    use crate::symbol_navigation::SymbolNavKind;

    #[test]
    fn trivial_search_and_symbol_stay_deterministic() {
        assert!(!prefers_reasoning_handoff(
            CodebaseEvidenceOrigin::CodeSearch,
            2,
            HandoffHints {
                code_search_kind: Some(CodeSearchKind::ExactSymbol),
                ..Default::default()
            }
        ));
        assert!(!prefers_reasoning_handoff(
            CodebaseEvidenceOrigin::CodeSearch,
            0,
            HandoffHints {
                code_search_kind: Some(CodeSearchKind::NaturalLanguage),
                ..Default::default()
            }
        ));
        assert!(!prefers_reasoning_handoff(
            CodebaseEvidenceOrigin::SymbolNavigation,
            4,
            HandoffHints {
                symbol_nav_kind: Some(SymbolNavKind::Callers),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn nl_search_and_relationship_prefer_reasoning() {
        assert!(prefers_reasoning_handoff(
            CodebaseEvidenceOrigin::CodeSearch,
            3,
            HandoffHints {
                code_search_kind: Some(CodeSearchKind::NaturalLanguage),
                ..Default::default()
            }
        ));
        assert!(prefers_reasoning_handoff(
            CodebaseEvidenceOrigin::RelationshipTrace,
            2,
            HandoffHints {
                investigation_needed: true,
                ..Default::default()
            }
        ));
        assert!(prefers_reasoning_handoff(
            CodebaseEvidenceOrigin::DeepInvestigation,
            0,
            HandoffHints::default()
        ));
    }

    #[test]
    fn converts_and_builds_bundle_hits() {
        let hits = vec![CodebaseEvidenceHit {
            path: PathBuf::from("/proj/a.rs"),
            title: "a.rs".into(),
            matched: "auth".into(),
            relevance: 80,
            line: Some(1),
            preview: "fn authenticate".into(),
            source: "search_knowledge".into(),
        }];
        let bundle = to_bundle_search_hits(&hits);
        assert_eq!(bundle.len(), 1);
        assert_eq!(bundle[0].title, "a.rs");
        let brief = evidence_brief_for_reasoning(
            CodebaseEvidenceOrigin::CodeSearch,
            "Where is auth?",
            "auth",
            &hits,
            &[],
        );
        assert!(brief.contains("origin=code_search"));
        assert!(brief.contains("a.rs"));
    }
}
