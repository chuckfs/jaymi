//! Symbol Navigation (Sprint C2.2).
//!
//! Planner-owned deep semantic navigation through Workspace Intelligence and
//! the existing language_server Tool → LSP Provider path.
//!
//! Examples:
//! - "Where is this function defined?"
//! - "Who calls this?"
//! - "What implements this trait?"
//! - "Where is this type used?"
//! - "Show me everything that depends on this."
//!
//! Read-only only. No rename, no edits, no mutation. Reasoning never calls LSP.

use std::path::{Path, PathBuf};

use jaymi_context::ContextBundle;
use jaymi_core::{LspLocation, LspOperation, LspRequest, UserRequest};

/// Max evidence rows in conversation.
pub const CONVERSATION_EVIDENCE_CAP: usize = 16;

/// Kind of symbol-navigation ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolNavKind {
    Definition,
    References,
    Implementations,
    Callers,
    Callees,
    Symbols,
    Imports,
    EnclosingSymbol,
    SymbolLocation,
    Dependents,
}

impl SymbolNavKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::References => "references",
            Self::Implementations => "implementations",
            Self::Callers => "callers",
            Self::Callees => "callees",
            Self::Symbols => "symbols",
            Self::Imports => "imports",
            Self::EnclosingSymbol => "enclosing_symbol",
            Self::SymbolLocation => "symbol_location",
            Self::Dependents => "dependents",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Definition => "Go to definition",
            Self::References => "Find references",
            Self::Implementations => "Find implementations",
            Self::Callers => "Find callers",
            Self::Callees => "Find callees",
            Self::Symbols => "Symbol overview",
            Self::Imports => "Imports / dependents overview",
            Self::EnclosingSymbol => "Enclosing symbol",
            Self::SymbolLocation => "Symbol location",
            Self::Dependents => "Dependents",
        }
    }

    /// Preferred LSP operation when live tool execution is needed.
    pub fn lsp_operation(self) -> Option<LspOperation> {
        match self {
            Self::Definition | Self::SymbolLocation => Some(LspOperation::Definition),
            Self::References | Self::Callers | Self::Dependents => Some(LspOperation::References),
            Self::Implementations => Some(LspOperation::Implementation),
            // Soft / WI-only (no mutating ops).
            Self::Callees | Self::Symbols | Self::Imports | Self::EnclosingSymbol => None,
        }
    }

    /// Prefer answering from Workspace Intelligence before calling the LSP tool.
    pub fn prefers_workspace_intelligence(self) -> bool {
        matches!(
            self,
            Self::EnclosingSymbol
                | Self::Symbols
                | Self::Imports
                | Self::Callees
                | Self::Callers
                | Self::References
                | Self::Dependents
                | Self::SymbolLocation
                | Self::Definition
        )
    }
}

/// One navigation evidence row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SymbolNavEvidence {
    pub path: PathBuf,
    pub title: String,
    pub matched: String,
    pub relevance: u32,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub column: Option<u32>,
    pub preview: String,
    pub why: String,
}

/// Planner-authored navigation report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SymbolNavigationReport {
    pub goal: String,
    pub kind: Option<SymbolNavKind>,
    pub symbol: String,
    pub strategy: String,
    pub evidence: Vec<SymbolNavEvidence>,
    pub total_hits: usize,
    pub truncated: bool,
    pub diagnostics: Vec<String>,
}

impl SymbolNavigationReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Symbol Navigation\n");
        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Kind\n- {}\n", kind.label()));
        }
        out.push_str("\n### Symbol\n- ");
        out.push_str(if self.symbol.is_empty() {
            "(unknown — place the cursor on a symbol)"
        } else {
            &self.symbol
        });
        out.push('\n');

        out.push_str("\n### Evidence\n");
        if self.evidence.is_empty() {
            out.push_str(
                "- _(no navigation targets — ensure Coding editor focus, language server readiness, or a clearer symbol)_\n",
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
                "\n_Showing {} of {} locations (context budget)._\n",
                self.evidence.len(),
                self.total_hits
            ));
        }

        out.push_str("\n### Notes\n");
        out.push_str("- Read-only — no rename, edits, or mutation.\n");
        out.push_str("- Results flow through Planner → LSP tool / Workspace Intelligence → Context.\n");
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "symbol_nav kind={} symbol={:?} strategy={} hits={} shown={} truncated={}",
            self.kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.symbol,
            self.strategy,
            self.total_hits,
            self.evidence.len(),
            self.truncated
        )
    }
}

/// Detected navigation assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolNavAssessment {
    pub kind: SymbolNavKind,
    pub goal: String,
    pub symbol_hint: Option<String>,
    pub deixis: bool,
    pub matched_rules: Vec<&'static str>,
}

/// Detect symbol-navigation phrasing (Planner ownership).
pub fn detect_symbol_navigation_request(request: &UserRequest) -> Option<SymbolNavAssessment> {
    if request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.search.is_some()
        || request.project_knowledge.is_some()
    {
        return None;
    }
    // Structured LSP already set — Application coding chrome; still annotate.
    if let Some(lsp) = &request.lsp {
        if !lsp.workspace_root.as_os_str().is_empty() {
            let kind = match lsp.operation {
                LspOperation::Definition => SymbolNavKind::Definition,
                LspOperation::References => SymbolNavKind::References,
                LspOperation::Implementation => SymbolNavKind::Implementations,
                LspOperation::Rename => return None, // mutation — not C2.2
                _ => return None,
            };
            return Some(SymbolNavAssessment {
                kind,
                goal: request.content.trim().to_string(),
                symbol_hint: None,
                deixis: true,
                matched_rules: vec!["structured_lsp"],
            });
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
    if crate::coding_review::detect_review_request(request).is_some()
        || crate::coding_plan::detect_coding_plan_request(request).is_some()
        || crate::multi_file::detect_multi_file_request(request).is_some()
        || crate::code_generation::detect_generation_request(request)
        || crate::relationship_tracing::detect_relationship_trace_request(request).is_some()
    {
        return None;
    }

    let (kind, rules, hint) = classify_phrasing(&lower, content)?;
    let deixis = lower.contains("this")
        || lower.contains("here")
        || lower.contains("current")
        || hint
            .as_ref()
            .map(|h| h.eq_ignore_ascii_case("this") || h.to_ascii_lowercase().contains("this "))
            .unwrap_or(false);

    Some(SymbolNavAssessment {
        kind,
        goal: content.to_string(),
        symbol_hint: hint,
        deixis,
        matched_rules: rules,
    })
}

fn classify_phrasing(
    lower: &str,
    original: &str,
) -> Option<(SymbolNavKind, Vec<&'static str>, Option<String>)> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');

    if looks_like(trimmed, &[
        "where is this function defined",
        "where is this defined",
        "where is this method defined",
        "go to definition",
        "goto definition",
        "show definition",
        "find definition",
        "jump to definition",
    ]) || trimmed.starts_with("where is this ") && trimmed.contains("defined")
        || trimmed.contains("defined?") && (trimmed.contains("this function") || trimmed.contains("this method"))
    {
        return Some((
            SymbolNavKind::Definition,
            vec!["definition"],
            extract_symbol_hint(original, &["defined", "definition"]),
        ));
    }

    if looks_like(trimmed, &[
        "who calls this",
        "who calls this function",
        "who calls this method",
        "show callers",
        "find callers",
        "callers of this",
        "show me all the places that call this function",
        "show me the places that call this function",
        "places that call this function",
        "places that call this",
    ]) || trimmed.starts_with("who calls ")
        || trimmed.contains("call sites for this")
    {
        return Some((
            SymbolNavKind::Callers,
            vec!["callers"],
            extract_symbol_hint(original, &["calls", "callers", "call"]),
        ));
    }

    if looks_like(trimmed, &[
        "what does this call",
        "what does this function call",
        "show callees",
        "find callees",
        "callees of this",
        "who does this call",
    ]) || trimmed.starts_with("what does this ") && trimmed.contains("call")
    {
        return Some((SymbolNavKind::Callees, vec!["callees"], None));
    }

    if looks_like(trimmed, &[
        "what implements this",
        "what implements this trait",
        "who implements this",
        "find implementations",
        "show implementations",
        "implementations of this",
    ]) || trimmed.contains("implements this trait")
        || trimmed.starts_with("what implements ")
    {
        return Some((
            SymbolNavKind::Implementations,
            vec!["implementations"],
            extract_symbol_hint(original, &["implements", "implementations", "trait"]),
        ));
    }

    if looks_like(trimmed, &[
        "where is this type used",
        "where is this used",
        "where is this symbol used",
        "find usages",
        "find references",
        "show references",
        "show usages",
        "references to this",
    ]) || (trimmed.contains("used")
        && (trimmed.contains("this type")
            || trimmed.contains("this symbol")
            || trimmed.contains("this struct")
            || trimmed.contains("this enum")
            || trimmed.starts_with("where is this")))
    {
        return Some((
            SymbolNavKind::References,
            vec!["references"],
            extract_symbol_hint(original, &["used", "usages", "references"]),
        ));
    }

    if looks_like(trimmed, &[
        "show me everything that depends on this",
        "what depends on this",
        "everything that depends on this",
        "show dependents",
        "find dependents",
    ]) || trimmed.contains("depends on this")
    {
        return Some((SymbolNavKind::Dependents, vec!["dependents"], None));
    }

    if looks_like(trimmed, &[
        "what is the enclosing symbol",
        "enclosing symbol",
        "enclosing function",
        "what function am i in",
        "what method am i in",
        "current enclosing symbol",
    ]) || trimmed.contains("enclosing")
    {
        return Some((SymbolNavKind::EnclosingSymbol, vec!["enclosing"], None));
    }

    if looks_like(trimmed, &[
        "where is this symbol",
        "symbol location",
        "where is the cursor symbol",
        "locate this symbol",
    ]) {
        return Some((SymbolNavKind::SymbolLocation, vec!["symbol_location"], None));
    }

    if looks_like(trimmed, &[
        "show symbols",
        "list symbols",
        "what symbols are here",
        "symbols at cursor",
        "show me the symbols",
    ]) {
        return Some((SymbolNavKind::Symbols, vec!["symbols"], None));
    }

    if looks_like(trimmed, &[
        "show imports",
        "what imports this",
        "who imports this",
        "list imports",
    ]) || trimmed.contains("imports this")
    {
        return Some((SymbolNavKind::Imports, vec!["imports"], None));
    }

    None
}

fn looks_like(trimmed: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|p| trimmed == *p || trimmed.starts_with(&format!("{p} ")))
}

fn extract_symbol_hint(original: &str, _stop: &[&str]) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    for marker in [
        "function ",
        "method ",
        "type ",
        "trait ",
        "symbol ",
        "struct ",
        "enum ",
    ] {
        if let Some(idx) = lower.find(marker) {
            let after = original[idx + marker.len()..]
                .split_whitespace()
                .next()?
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if !after.is_empty()
                && !after.eq_ignore_ascii_case("this")
                && !after.eq_ignore_ascii_case("defined")
                && !after.eq_ignore_ascii_case("used")
            {
                return Some(after.to_string());
            }
        }
    }
    None
}

/// Resolve the active symbol name from WI / hint.
pub fn resolve_symbol_name(assessment: &SymbolNavAssessment, bundle: &ContextBundle) -> String {
    if let Some(hint) = &assessment.symbol_hint {
        let h = hint.trim();
        if !h.is_empty() && !h.eq_ignore_ascii_case("this") {
            return h.to_string();
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
    if let Some(ty) = &editor.enclosing_type {
        if !ty.name.trim().is_empty() {
            return ty.name.clone();
        }
    }
    String::new()
}

/// Soft-path report from Workspace Intelligence (no tools).
pub fn report_from_workspace_intelligence(
    assessment: &SymbolNavAssessment,
    bundle: &ContextBundle,
) -> Option<SymbolNavigationReport> {
    let symbol = resolve_symbol_name(assessment, bundle);
    let editor = bundle.editor_intelligence();
    let mut evidence = Vec::new();
    let mut diagnostics = vec![
        format!("matched_rules={}", assessment.matched_rules.join(",")),
        format!("kind={}", assessment.kind.as_str()),
        "source=workspace_intelligence".into(),
        "tools=none (read-only soft path)".into(),
    ];

    match assessment.kind {
        SymbolNavKind::EnclosingSymbol | SymbolNavKind::Symbols => {
            if let Some(func) = &editor.enclosing_function {
                evidence.push(evidence_from_editor_symbol(
                    bundle,
                    func,
                    "enclosing_function",
                    95,
                ));
            }
            if let Some(ty) = &editor.enclosing_type {
                evidence.push(evidence_from_editor_symbol(
                    bundle,
                    ty,
                    "enclosing_type",
                    90,
                ));
            }
            if let Some(sym) = &editor.symbol {
                evidence.push(evidence_from_editor_symbol(bundle, sym, "cursor_symbol", 88));
            }
        }
        SymbolNavKind::SymbolLocation | SymbolNavKind::Definition => {
            if let Some(sym) = &editor.symbol {
                evidence.push(evidence_from_editor_symbol(
                    bundle,
                    sym,
                    "cursor_symbol_location",
                    92,
                ));
            } else if let Some(func) = &editor.enclosing_function {
                evidence.push(evidence_from_editor_symbol(
                    bundle,
                    func,
                    "enclosing_function_location",
                    85,
                ));
            }
        }
        SymbolNavKind::References
        | SymbolNavKind::Callers
        | SymbolNavKind::Dependents
        | SymbolNavKind::Imports => {
            for reference in &editor.references {
                evidence.push(evidence_from_editor_reference(
                    &symbol,
                    reference,
                    assessment.kind.as_str(),
                ));
            }
            if evidence.is_empty() && assessment.kind == SymbolNavKind::Imports {
                diagnostics.push(
                    "imports: no ambient import graph; showing WI references when present".into(),
                );
            }
        }
        SymbolNavKind::Implementations => {
            // WI has no dedicated implementations list — leave empty for LSP fallback.
            diagnostics.push("wi has no implementations list; prefer LSP Implementation".into());
        }
        SymbolNavKind::Callees => {
            diagnostics.push(
                "callees: call hierarchy not exposed by current LSP tool surface; soft path only"
                    .into(),
            );
        }
    }

    // Soft path is useful when we have evidence, or when the kind is WI-only
    // (even if empty — honest empty report).
    let wi_only = matches!(
        assessment.kind,
        SymbolNavKind::EnclosingSymbol
            | SymbolNavKind::Symbols
            | SymbolNavKind::Callees
            | SymbolNavKind::Imports
    );
    if evidence.is_empty() && !wi_only {
        return None;
    }

    let total = evidence.len();
    let truncated = total > CONVERSATION_EVIDENCE_CAP;
    evidence.truncate(CONVERSATION_EVIDENCE_CAP);
    Some(SymbolNavigationReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        symbol,
        strategy: "workspace_intelligence".into(),
        evidence,
        total_hits: total,
        truncated,
        diagnostics,
    })
}

/// Build a report from LSP location vectors (existing tool output).
pub fn report_from_lsp_locations(
    assessment: &SymbolNavAssessment,
    symbol: &str,
    locations: &[LspLocation],
    strategy: impl Into<String>,
) -> SymbolNavigationReport {
    let total = locations.len();
    let truncated = total > CONVERSATION_EVIDENCE_CAP;
    let evidence: Vec<_> = locations
        .iter()
        .take(CONVERSATION_EVIDENCE_CAP)
        .map(|loc| evidence_from_lsp_location(symbol, loc, assessment.kind.as_str()))
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
    SymbolNavigationReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        symbol: symbol.to_string(),
        strategy: strategy.into(),
        evidence,
        total_hits: total,
        truncated,
        diagnostics,
    }
}

/// Empty honest report.
pub fn empty_report(
    assessment: &SymbolNavAssessment,
    symbol: impl Into<String>,
    strategy: impl Into<String>,
) -> SymbolNavigationReport {
    SymbolNavigationReport {
        goal: assessment.goal.clone(),
        kind: Some(assessment.kind),
        symbol: symbol.into(),
        strategy: strategy.into(),
        evidence: Vec::new(),
        total_hits: 0,
        truncated: false,
        diagnostics: vec!["no_targets".into()],
    }
}

/// Build an LSP request from session / bundle when position is known.
pub fn try_build_lsp_request(
    assessment: &SymbolNavAssessment,
    bundle: &ContextBundle,
    workspace_root: Option<PathBuf>,
    session: &jaymi_context::ContextSessionInputs,
) -> Option<LspRequest> {
    let operation = assessment.kind.lsp_operation()?;
    let path = session
        .editor_snapshot
        .as_ref()
        .and_then(|s| s.active_file.path.clone())
        .or_else(|| session.current_file.path.clone())
        .or_else(|| bundle.current_file().path.clone())?;
    let (line, character) = session
        .editor_snapshot
        .as_ref()
        .and_then(|s| s.cursor.map(|c| (c.line, c.column)))
        .or_else(|| {
            session
                .editor_snapshot
                .as_ref()
                .and_then(|s| s.symbol.as_ref())
                .and_then(|sym| sym.range.as_ref())
                .map(|r| (r.start_line, r.start_column))
        })
        .or_else(|| {
            bundle
                .editor_intelligence()
                .symbol
                .as_ref()
                .and_then(|sym| sym.range.as_ref())
                .map(|r| (r.start_line, r.start_column))
        })?;

    let root = workspace_root
        .or_else(|| Path::new(&path).parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let language = session
        .current_file
        .language
        .clone()
        .or_else(|| {
            session
                .editor_snapshot
                .as_ref()
                .and_then(|s| s.active_file.language.clone())
        });

    Some(LspRequest {
        workspace_root: root,
        operation,
        path: Some(PathBuf::from(path)),
        content: None,
        language,
        version: None,
        line: Some(line),
        character: Some(character),
        new_name: None,
    })
}

fn evidence_from_editor_symbol(
    bundle: &ContextBundle,
    symbol: &jaymi_context::EditorSymbol,
    why: &str,
    relevance: u32,
) -> SymbolNavEvidence {
    let path = bundle
        .current_file()
        .path
        .clone()
        .unwrap_or_else(|| "(active editor)".into());
    let title = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let (line, end_line, column) = symbol
        .range
        .as_ref()
        .map(|r| (Some(r.start_line), Some(r.end_line), Some(r.start_column)))
        .unwrap_or((None, None, None));
    SymbolNavEvidence {
        path: PathBuf::from(path),
        title,
        matched: symbol.name.clone(),
        relevance,
        line,
        end_line,
        column,
        preview: symbol
            .detail
            .clone()
            .unwrap_or_else(|| format!("{} ({})", symbol.name, symbol.kind.as_deref().unwrap_or("symbol"))),
        why: why.into(),
    }
}

fn evidence_from_editor_reference(
    symbol: &str,
    reference: &jaymi_context::EditorReference,
    why: &str,
) -> SymbolNavEvidence {
    let title = Path::new(&reference.path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| reference.path.clone());
    SymbolNavEvidence {
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
        column: Some(reference.range.start_column),
        preview: format!(
            "L{}:{}",
            reference.range.start_line.saturating_add(1),
            reference.range.start_column.saturating_add(1)
        ),
        why: why.into(),
    }
}

fn evidence_from_lsp_location(symbol: &str, loc: &LspLocation, why: &str) -> SymbolNavEvidence {
    let title = Path::new(&loc.path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| loc.path.clone());
    SymbolNavEvidence {
        path: PathBuf::from(&loc.path),
        title,
        matched: if symbol.is_empty() {
            why.to_string()
        } else {
            symbol.to_string()
        },
        relevance: 95,
        line: Some(loc.range.start.line),
        end_line: Some(loc.range.end.line),
        column: Some(loc.range.start.character),
        preview: format!(
            "L{}:{}",
            loc.range.start.line.saturating_add(1),
            loc.range.start.character.saturating_add(1)
        ),
        why: format!("lsp_{why}"),
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
        let def = detect_symbol_navigation_request(&UserRequest::new(
            "Where is this function defined?",
        ))
        .unwrap();
        assert_eq!(def.kind, SymbolNavKind::Definition);
        assert!(def.deixis);

        let callers =
            detect_symbol_navigation_request(&UserRequest::new("Who calls this?")).unwrap();
        assert_eq!(callers.kind, SymbolNavKind::Callers);

        let impls = detect_symbol_navigation_request(&UserRequest::new(
            "What implements this trait?",
        ))
        .unwrap();
        assert_eq!(impls.kind, SymbolNavKind::Implementations);

        let used = detect_symbol_navigation_request(&UserRequest::new(
            "Where is this type used?",
        ))
        .unwrap();
        assert_eq!(used.kind, SymbolNavKind::References);

        let deps = detect_symbol_navigation_request(&UserRequest::new(
            "Show me everything that depends on this.",
        ))
        .unwrap();
        assert_eq!(deps.kind, SymbolNavKind::Dependents);
    }

    #[test]
    fn does_not_steal_mutation_or_review() {
        assert!(detect_symbol_navigation_request(&UserRequest::new("Rename symbol")).is_none());
        assert!(detect_symbol_navigation_request(&UserRequest::new("Review my changes.")).is_none());
        let mut rename = UserRequest::new("rename");
        rename.lsp = Some(LspRequest {
            workspace_root: PathBuf::from("/proj"),
            operation: LspOperation::Rename,
            path: Some(PathBuf::from("/proj/a.rs")),
            content: None,
            language: None,
            version: None,
            line: Some(1),
            character: Some(0),
            new_name: Some("other".into()),
        });
        assert!(detect_symbol_navigation_request(&rename).is_none());
    }

    #[test]
    fn wi_enclosing_and_multi_file_refs() {
        let assessment = detect_symbol_navigation_request(&UserRequest::new(
            "Who calls this?",
        ))
        .unwrap();
        let bundle = ContextBundle::builder()
            .current_file(jaymi_context::CurrentFileSection {
                path: Some("/proj/src/game.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            })
            .editor_intelligence(EditorIntelligenceSection {
                symbol: Some(EditorSymbol {
                    name: "update_score".into(),
                    kind: Some("function".into()),
                    range: Some(EditorRange {
                        start_line: 10,
                        start_column: 3,
                        end_line: 10,
                        end_column: 15,
                    }),
                    ..Default::default()
                }),
                enclosing_function: Some(EditorSymbol {
                    name: "tick".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![
                    EditorReference {
                        path: "/proj/src/game.rs".into(),
                        range: EditorRange {
                            start_line: 20,
                            start_column: 4,
                            end_line: 20,
                            end_column: 16,
                        },
                    },
                    EditorReference {
                        path: "/proj/src/ui.rs".into(),
                        range: EditorRange {
                            start_line: 5,
                            start_column: 0,
                            end_line: 5,
                            end_column: 12,
                        },
                    },
                ],
                ..Default::default()
            })
            .build();
        let report = report_from_workspace_intelligence(&assessment, &bundle).unwrap();
        assert_eq!(report.evidence.len(), 2);
        assert!(report.symbol.contains("update_score"));
        assert!(report.to_markdown().contains("## Symbol Navigation"));

        let enc = detect_symbol_navigation_request(&UserRequest::new(
            "What is the enclosing symbol?",
        ))
        .unwrap();
        let enc_report = report_from_workspace_intelligence(&enc, &bundle).unwrap();
        assert!(!enc_report.evidence.is_empty());
    }

    #[test]
    fn caps_large_lsp_result_sets() {
        let assessment = detect_symbol_navigation_request(&UserRequest::new(
            "Where is this type used?",
        ))
        .unwrap();
        let locations: Vec<_> = (0..40)
            .map(|i| LspLocation {
                path: format!("/proj/src/f{i}.rs"),
                range: jaymi_core::LspRange {
                    start: jaymi_core::LspPosition {
                        line: i,
                        character: 0,
                    },
                    end: jaymi_core::LspPosition {
                        line: i,
                        character: 4,
                    },
                },
            })
            .collect();
        let report = report_from_lsp_locations(&assessment, "Foo", &locations, "language_server");
        assert_eq!(report.evidence.len(), CONVERSATION_EVIDENCE_CAP);
        assert!(report.truncated);
    }

    #[test]
    fn missing_symbol_empty_report() {
        let assessment = detect_symbol_navigation_request(&UserRequest::new(
            "Where is this function defined?",
        ))
        .unwrap();
        let report = empty_report(&assessment, "", "missing_symbol");
        assert!(report.evidence.is_empty());
        assert!(report.to_markdown().contains("no navigation targets"));
    }
}
