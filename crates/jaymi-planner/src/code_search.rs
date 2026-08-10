//! Intelligent Code Search (Sprint C2.1).
//!
//! Planner-owned natural-language code search across the current project.
//! Reuses Search Knowledge / Search Engine / Context / citations — no parallel
//! search or context stack. Read-only only.
//!
//! Examples:
//! - "Where is authentication handled?"
//! - "Find where the score gets updated."
//! - "Where is ContextBundle created?"
//! - "Show me all the places that call this function."
//! - "Find the code responsible for opening Settings."

use std::path::{Path, PathBuf};

use jaymi_context::ContextBundle;
use jaymi_core::{Citation, SearchRequest, UserRequest};

/// Max evidence rows surfaced in conversation (budget / Context Policy friendly).
pub const CONVERSATION_EVIDENCE_CAP: usize = 12;

/// Max hits requested from Search Engine for a code-search turn.
pub const SEARCH_HIT_LIMIT: usize = 24;

/// Kind of intelligent code-search ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeSearchKind {
    /// Natural-language / responsibility question.
    NaturalLanguage,
    /// Exact or near-exact symbol / type name.
    ExactSymbol,
    /// Call sites / references for a symbol (WI refs preferred).
    FindReferences,
    /// “Code responsible for …” phrasing.
    Responsibility,
}

impl CodeSearchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NaturalLanguage => "natural_language",
            Self::ExactSymbol => "exact_symbol",
            Self::FindReferences => "find_references",
            Self::Responsibility => "responsibility",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NaturalLanguage => "Natural-language search",
            Self::ExactSymbol => "Exact symbol search",
            Self::FindReferences => "Find references",
            Self::Responsibility => "Responsibility search",
        }
    }
}

/// One piece of search evidence for conversation / diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeSearchEvidence {
    /// Absolute or workspace path.
    pub path: PathBuf,
    /// Display title (usually filename).
    pub title: String,
    /// Matched symbol or text.
    pub matched: String,
    /// Relevance 0..=100.
    pub relevance: u32,
    /// Start line (1-based for display when known).
    pub line: Option<u32>,
    /// End line (1-based) when known.
    pub end_line: Option<u32>,
    /// Surrounding / preview context (truncated).
    pub preview: String,
    /// Why this hit was selected.
    pub why: String,
}

/// Planner-authored code-search report (Conversation First + diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodeSearchReport {
    /// Original user goal.
    pub goal: String,
    /// Detected kind.
    pub kind: Option<CodeSearchKind>,
    /// Query sent to Search Engine / WI.
    pub query: String,
    /// Strategy / source label (`search_knowledge`, `editor_references`, …).
    pub strategy: String,
    /// Evidence selected for conversation (capped).
    pub evidence: Vec<CodeSearchEvidence>,
    /// Total hits before conversation cap.
    pub total_hits: usize,
    /// True when conversation evidence was truncated for budget.
    pub truncated: bool,
    /// Diagnostics-only notes (not required in conversation body).
    pub diagnostics: Vec<String>,
}

impl CodeSearchReport {
    pub fn is_empty(&self) -> bool {
        self.evidence.is_empty() && self.total_hits == 0
    }

    /// Conversation markdown — evidence only, not a repo dump.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Code Search\n");
        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Kind\n- {}\n", kind.label()));
        }
        out.push_str("\n### Query\n- ");
        out.push_str(if self.query.is_empty() {
            "(none)"
        } else {
            &self.query
        });
        out.push('\n');

        out.push_str("\n### Evidence\n");
        if self.evidence.is_empty() {
            out.push_str("- _(no matches in the indexed project — try a more specific symbol or phrase)_\n");
        } else {
            for (i, hit) in self.evidence.iter().enumerate() {
                let loc = match (hit.line, hit.end_line) {
                    (Some(a), Some(b)) if a != b => format!(":{}-{}", a.saturating_add(1), b.saturating_add(1)),
                    (Some(a), _) => format!(":{}", a.saturating_add(1)),
                    _ => String::new(),
                };
                out.push_str(&format!(
                    "{}. `{}{}` — {} ({}%)\n",
                    i + 1,
                    hit.title,
                    loc,
                    hit.matched,
                    hit.relevance
                ));
                if !hit.preview.trim().is_empty() {
                    out.push_str(&format!("   {}\n", truncate(&hit.preview, 160)));
                }
                if !hit.why.is_empty() {
                    out.push_str(&format!("   why: {}\n", hit.why));
                }
            }
        }

        if self.truncated {
            out.push_str(&format!(
                "\n_Showing {} of {} hits (context budget). Full set stays in diagnostics._\n",
                self.evidence.len(),
                self.total_hits
            ));
        }

        out.push_str("\n### Notes\n");
        out.push_str("- Read-only — no file modifications.\n");
        out.push_str("- Results came through Planner → Search → Context citations.\n");
        out
    }

    /// Compact diagnostics lines (Developer Diagnostics / activity).
    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "code_search kind={} query={:?} strategy={} hits={} shown={} truncated={}",
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.query,
            self.strategy,
            self.total_hits,
            self.evidence.len(),
            self.truncated
        )
    }
}

/// Detected code-search assessment before tool execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSearchAssessment {
    pub kind: CodeSearchKind,
    pub goal: String,
    pub query: String,
    /// When true, prefer editor-intelligence references over Search Engine.
    pub prefer_editor_references: bool,
    pub matched_rules: Vec<&'static str>,
}

impl CodeSearchAssessment {
    pub fn to_search_request(&self) -> SearchRequest {
        let mut request = SearchRequest::free_text(&self.query);
        request.limit = Some(SEARCH_HIT_LIMIT);
        request.metadata.files_only = true;
        request
    }
}

/// Detect intelligent code-search phrasing (Planner ownership).
pub fn detect_code_search_request(request: &UserRequest) -> Option<CodeSearchAssessment> {
    if request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.lsp.is_some()
        || request.project_knowledge.is_some()
    {
        return None;
    }
    // Structured search already set — still annotate as code search when conversational.
    if let Some(search) = &request.search {
        if let Some(query) = search
            .free_text
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            if request.coding_action == Some(jaymi_core::CodingAction::SearchWorkspace)
                || looks_like_code_search_goal(&request.content)
            {
                return Some(CodeSearchAssessment {
                    kind: classify_query_kind(&query),
                    goal: request.content.trim().to_string(),
                    query,
                    prefer_editor_references: false,
                    matched_rules: vec!["structured_or_coding_action_search"],
                });
            }
        }
    }

    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    if crate::pair_programming::looks_like_dead_code_request(&lower) {
        return None;
    }

    // Prefer intelligent code-search phrasing before Understanding steals
    // "this function" as an explain-selection question.
    // Symbol Navigation (C2.2) / Relationship Tracing (C2.3) / Deep Investigation (C2.4)
    // / Codebase Q&A (C2.5).
    if crate::symbol_navigation::detect_symbol_navigation_request(request).is_some()
        || crate::relationship_tracing::detect_relationship_trace_request(request).is_some()
        || crate::deep_investigation::detect_deep_investigation_request(request).is_some()
        || crate::codebase_qa::detect_codebase_qa_request(request).is_some()
    {
        return None;
    }
    if let Some((kind, query, rules, prefer_refs)) = classify_phrasing(&lower, content) {
        let query = query
            .trim()
            .trim_matches(|c: char| c == '.' || c == '?' || c == '!')
            .to_string();
        if !query.is_empty() {
            // Still don't steal review / plan / multi-file / generation.
            if crate::coding_review::detect_review_request(request).is_some()
                || crate::coding_plan::detect_coding_plan_request(request).is_some()
                || crate::multi_file::detect_multi_file_request(request).is_some()
                || crate::code_generation::detect_generation_request(request)
            {
                return None;
            }
            return Some(CodeSearchAssessment {
                kind,
                goal: content.to_string(),
                query,
                prefer_editor_references: prefer_refs,
                matched_rules: rules,
            });
        }
    }

    // Don't steal understanding / review / plan / multi-file for non-search.
    if crate::coding_understanding::detect_understanding_request(request).is_some()
        || crate::coding_review::detect_review_request(request).is_some()
        || crate::coding_plan::detect_coding_plan_request(request).is_some()
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
    {
        return None;
    }

    None
}

/// True when Decision Engine should map this free text onto SearchKnowledge.
pub fn should_route_to_search_knowledge(request: &UserRequest) -> Option<SearchRequest> {
    let assessment = detect_code_search_request(request)?;
    if assessment.prefer_editor_references {
        // Deixis / "this function" — assemble first; soft path may answer from WI.
        return None;
    }
    Some(assessment.to_search_request())
}

/// Parse intelligent code-search phrasing for the Decision Engine.
///
/// Returns `None` for deixis ("this function") so Context can assemble first.
pub fn parse_intelligent_search(lower: &str, original: &str) -> Option<SearchRequest> {
    let (kind, query, _rules, prefer_refs) = classify_phrasing(lower, original)?;
    let _ = kind;
    if prefer_refs {
        return None;
    }
    let query = query
        .trim()
        .trim_matches(|c: char| c == '.' || c == '?' || c == '!')
        .to_string();
    if query.is_empty() {
        return None;
    }
    let mut request = SearchRequest::free_text(query);
    request.limit = Some(SEARCH_HIT_LIMIT);
    request.metadata.files_only = true;
    Some(request)
}

/// Whether this turn should replace the raw tool dump with a Code Search report.
pub fn should_present_code_search_report(request: &UserRequest) -> Option<CodeSearchAssessment> {
    detect_code_search_request(request)
}

pub fn looks_like_code_search_goal(content: &str) -> bool {
    let lower = content.trim().to_ascii_lowercase();
    classify_phrasing(&lower, content).is_some()
}

fn classify_phrasing(
    lower: &str,
    original: &str,
) -> Option<(CodeSearchKind, String, Vec<&'static str>, bool)> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    if looks_like_find_references(trimmed) {
        let query = extract_after_markers(
            original,
            &[
                "places that call ",
                "call sites for ",
                "references to ",
                "callers of ",
            ],
        )
        .unwrap_or_else(|| "this function".into());
        let prefer = query.to_ascii_lowercase().contains("this")
            || query.to_ascii_lowercase().contains("current");
        return Some((
            CodeSearchKind::FindReferences,
            query,
            vec!["find_references"],
            prefer,
        ));
    }

    if let Some(rest) = trimmed.strip_prefix("where is ") {
        let query = restore_casing(original, &strip_trailing_filler(rest));
        let kind = if looks_like_symbol(&query) {
            CodeSearchKind::ExactSymbol
        } else {
            CodeSearchKind::NaturalLanguage
        };
        return Some((kind, query, vec!["where_is"], false));
    }
    if let Some(rest) = trimmed.strip_prefix("where's ") {
        let query = restore_casing(original, &strip_trailing_filler(rest));
        return Some((
            CodeSearchKind::NaturalLanguage,
            query,
            vec!["wheres"],
            false,
        ));
    }
    if let Some(rest) = trimmed
        .strip_prefix("find where ")
        .or_else(|| trimmed.strip_prefix("find the place where "))
        .or_else(|| trimmed.strip_prefix("find the places where "))
    {
        let query = restore_casing(original, &strip_trailing_filler(rest));
        return Some((
            CodeSearchKind::NaturalLanguage,
            query,
            vec!["find_where"],
            false,
        ));
    }
    if let Some(rest) = trimmed
        .strip_prefix("find the code responsible for ")
        .or_else(|| trimmed.strip_prefix("find code responsible for "))
        .or_else(|| trimmed.strip_prefix("code responsible for "))
        .or_else(|| trimmed.strip_prefix("what handles "))
        .or_else(|| trimmed.strip_prefix("what code handles "))
    {
        let query = restore_casing(original, &strip_trailing_filler(rest));
        return Some((
            CodeSearchKind::Responsibility,
            query,
            vec!["responsibility"],
            false,
        ));
    }
    if let Some(rest) = trimmed
        .strip_prefix("find symbol ")
        .or_else(|| trimmed.strip_prefix("find the symbol "))
        .or_else(|| trimmed.strip_prefix("locate symbol "))
    {
        return Some((
            CodeSearchKind::ExactSymbol,
            restore_casing(original, rest.trim()),
            vec!["find_symbol"],
            false,
        ));
    }
    None
}

fn looks_like_find_references(lower: &str) -> bool {
    lower.contains("places that call")
        || lower.contains("call sites")
        || lower.contains("callers of")
        || lower.contains("references to")
        || lower.contains("all the places that call")
        || lower == "show me all the places that call this function"
        || lower.starts_with("show me all the places that call")
}

fn looks_like_symbol(query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() || q.contains(' ') {
        // "ContextBundle created" → still symbol-ish if first token CamelCase
        let first = q.split_whitespace().next().unwrap_or("");
        return is_camel_or_snake(first);
    }
    is_camel_or_snake(q)
}

fn is_camel_or_snake(token: &str) -> bool {
    let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
    if t.len() < 2 {
        return false;
    }
    t.chars().any(|c| c.is_ascii_uppercase()) || t.contains('_')
}

fn strip_trailing_filler(rest: &str) -> String {
    let mut q = rest.trim().to_string();
    for suffix in [
        " handled",
        " handle",
        " created",
        " create",
        " defined",
        " implemented",
        " updated",
        " update",
        " called",
        " opened",
        " opening",
    ] {
        if let Some(stripped) = q
            .to_ascii_lowercase()
            .strip_suffix(suffix)
            .map(|s| s.to_string())
        {
            // preserve length mapping roughly
            q = q.chars().take(stripped.chars().count()).collect();
            break;
        }
    }
    q.trim()
        .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!')
        .to_string()
}

fn extract_after_markers(original: &str, markers: &[&str]) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    for marker in markers {
        if let Some(idx) = lower.find(marker) {
            let after = original[idx + marker.len()..].trim();
            let cleaned = after
                .trim_end_matches(|c: char| c == '?' || c == '.' || c == '!')
                .trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn restore_casing(original: &str, lower_query: &str) -> String {
    let orig_lower = original.to_ascii_lowercase();
    if let Some(idx) = orig_lower.find(&lower_query.to_ascii_lowercase()) {
        let end = idx + lower_query.len();
        if end <= original.len() {
            return original[idx..end].to_string();
        }
    }
    lower_query.to_string()
}

fn classify_query_kind(query: &str) -> CodeSearchKind {
    if looks_like_symbol(query) && !query.contains(' ') {
        CodeSearchKind::ExactSymbol
    } else {
        CodeSearchKind::NaturalLanguage
    }
}

/// Build a report from Search Knowledge citations (existing pipeline).
pub fn report_from_citations(
    assessment: &CodeSearchAssessment,
    citations: &[Citation],
    strategy: impl Into<String>,
) -> CodeSearchReport {
    let total = citations.len();
    let truncated = total > CONVERSATION_EVIDENCE_CAP;
    let evidence: Vec<_> = citations
        .iter()
        .take(CONVERSATION_EVIDENCE_CAP)
        .map(evidence_from_citation)
        .collect();
    let mut diagnostics = vec![
        format!("matched_rules={}", assessment.matched_rules.join(",")),
        format!("kind={}", assessment.kind.as_str()),
    ];
    if truncated {
        diagnostics.push(format!(
            "truncated conversation evidence to {CONVERSATION_EVIDENCE_CAP} of {total}"
        ));
    }
    CodeSearchReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        query: assessment.query.clone(),
        strategy: strategy.into(),
        evidence,
        total_hits: total,
        truncated,
        diagnostics,
    }
}

/// Soft-path report from editor intelligence references (no tools).
pub fn report_from_editor_references(
    assessment: &CodeSearchAssessment,
    bundle: &ContextBundle,
) -> Option<CodeSearchReport> {
    let editor = bundle.editor_intelligence();
    if editor.references.is_empty() {
        return None;
    }
    let symbol = editor
        .symbol
        .as_ref()
        .map(|s| s.name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| assessment.query.clone());
    let mut evidence = Vec::new();
    for reference in editor.references.iter().take(CONVERSATION_EVIDENCE_CAP) {
        let title = Path::new(&reference.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| reference.path.clone());
        evidence.push(CodeSearchEvidence {
            path: PathBuf::from(&reference.path),
            title,
            matched: symbol.clone(),
            relevance: 90,
            line: Some(reference.range.start_line),
            end_line: Some(reference.range.end_line),
            preview: format!(
                "reference at {}:{}",
                reference.range.start_line.saturating_add(1),
                reference.range.start_column.saturating_add(1)
            ),
            why: "Editor intelligence reference observation".into(),
        });
    }
    let total = editor.references.len();
    Some(CodeSearchReport {
        goal: assessment.goal.clone(),
        kind: Some(CodeSearchKind::FindReferences),
        query: symbol,
        strategy: "editor_references".into(),
        evidence,
        total_hits: total,
        truncated: total > CONVERSATION_EVIDENCE_CAP,
        diagnostics: vec![
            "source=workspace_intelligence.editor_references".into(),
            "tools=none (read-only soft path)".into(),
        ],
    })
}

/// Empty-result report (honest).
pub fn empty_report(assessment: &CodeSearchAssessment, strategy: impl Into<String>) -> CodeSearchReport {
    CodeSearchReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        query: assessment.query.clone(),
        strategy: strategy.into(),
        evidence: Vec::new(),
        total_hits: 0,
        truncated: false,
        diagnostics: vec!["no_matches".into()],
    }
}

fn evidence_from_citation(citation: &Citation) -> CodeSearchEvidence {
    let title = if citation.title.trim().is_empty() {
        Path::new(&citation.location)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| citation.location.display().to_string())
    } else {
        citation.title.clone()
    };
    CodeSearchEvidence {
        path: citation.location.clone(),
        title,
        matched: citation
            .matching_section
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| truncate(&citation.preview, 48)),
        relevance: citation.confidence_percent(),
        line: citation.line,
        end_line: citation.end_line.or(citation.line),
        preview: citation.preview.clone(),
        why: citation.why_matched.clone(),
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
    };

    #[test]
    fn detects_sprint_examples() {
        let auth = detect_code_search_request(&UserRequest::new(
            "Where is authentication handled?",
        ))
        .unwrap();
        assert_eq!(auth.kind, CodeSearchKind::NaturalLanguage);
        assert!(auth.query.to_ascii_lowercase().contains("authentication"));

        let score =
            detect_code_search_request(&UserRequest::new("Find where the score gets updated."))
                .unwrap();
        assert!(score.query.to_ascii_lowercase().contains("score"));

        let symbol =
            detect_code_search_request(&UserRequest::new("Where is ContextBundle created?")).unwrap();
        assert!(
            symbol.query.contains("ContextBundle"),
            "query={}",
            symbol.query
        );

        // Call-site deixis is owned by Symbol Navigation (C2.2).
        assert!(detect_code_search_request(&UserRequest::new(
            "Show me all the places that call this function.",
        ))
        .is_none());

        let settings = detect_code_search_request(&UserRequest::new(
            "Find the code responsible for opening Settings.",
        ))
        .unwrap();
        assert_eq!(settings.kind, CodeSearchKind::Responsibility);
        assert!(settings.query.to_ascii_lowercase().contains("settings"));
    }

    #[test]
    fn does_not_steal_understanding_or_dead_code() {
        assert!(detect_code_search_request(&UserRequest::new("Explain this file.")).is_none());
        assert!(detect_code_search_request(&UserRequest::new("Find dead code.")).is_none());
        assert!(detect_code_search_request(&UserRequest::new("Review my changes.")).is_none());
    }

    #[test]
    fn caps_conversation_evidence() {
        let assessment = CodeSearchAssessment {
            kind: CodeSearchKind::ExactSymbol,
            goal: "Where is Foo?".into(),
            query: "Foo".into(),
            prefer_editor_references: false,
            matched_rules: vec!["test"],
        };
        let citations: Vec<_> = (0..30)
            .map(|i| Citation {
                title: format!("f{i}.rs"),
                location: PathBuf::from(format!("/proj/f{i}.rs")),
                preview: format!("fn foo_{i}"),
                why_matched: "symbol".into(),
                confidence: 8_000,
                matching_section: Some("Foo".into()),
                line: Some(i),
                column: Some(0),
                end_line: Some(i),
                end_column: Some(3),
            })
            .collect();
        let report = report_from_citations(&assessment, &citations, "search_knowledge");
        assert_eq!(report.evidence.len(), CONVERSATION_EVIDENCE_CAP);
        assert_eq!(report.total_hits, 30);
        assert!(report.truncated);
        assert!(report.to_markdown().contains("## Code Search"));
        assert!(report.to_diagnostics_summary().contains("truncated=true"));
    }

    #[test]
    fn editor_references_soft_path() {
        let assessment = CodeSearchAssessment {
            kind: CodeSearchKind::FindReferences,
            goal: "Show me all the places that call this function.".into(),
            query: "this function".into(),
            prefer_editor_references: true,
            matched_rules: vec!["find_references"],
        };
        let bundle = ContextBundle::builder()
            .editor_intelligence(EditorIntelligenceSection {
                symbol: Some(EditorSymbol {
                    name: "update_score".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![
                    EditorReference {
                        path: "/proj/src/game.rs".into(),
                        range: EditorRange {
                            start_line: 10,
                            start_column: 0,
                            end_line: 10,
                            end_column: 12,
                        },
                    },
                    EditorReference {
                        path: "/proj/src/ui.rs".into(),
                        range: EditorRange {
                            start_line: 4,
                            start_column: 2,
                            end_line: 4,
                            end_column: 14,
                        },
                    },
                ],
                ..Default::default()
            })
            .build();
        let report = report_from_editor_references(&assessment, &bundle).unwrap();
        assert_eq!(report.evidence.len(), 2);
        assert_eq!(report.strategy, "editor_references");
        assert!(report.query.contains("update_score"));
    }

    #[test]
    fn search_request_is_files_only_and_capped() {
        let assessment = detect_code_search_request(&UserRequest::new(
            "Where is authentication handled?",
        ))
        .unwrap();
        let req = assessment.to_search_request();
        assert!(req.metadata.files_only);
        assert_eq!(req.limit, Some(SEARCH_HIT_LIMIT));
        assert!(req.free_text.as_ref().unwrap().contains("authentication"));
    }
}
