//! Environmental Resolution (Sprint B2.10).
//!
//! The Planner resolves ambiguous workspace deixis ("this", "it", "why?",
//! "rename this", "fix it", "clean this up") from Workspace Intelligence
//! **before** Reasoning. LLMs never invent workspace paths or referents.
//!
//! Deterministic heuristics only — no AI scoring.
//!
//! ```text
//! UserRequest
//!     │
//!     ▼
//! Intent → Capability → Complexity
//!     │
//!     ▼
//! Environmental Resolution  ← session Workspace Intelligence
//!     │
//!     ▼
//! AssembleHints.environmental → Context assemble → Prompt
//! ```

use jaymi_context::{ContextSessionInputs, EnvironmentalHints};
use jaymi_core::UserRequest;

/// One bound workspace referent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReference {
    /// Deictic span or cue from the request (`this`, `it`, `why`, …).
    pub cue: String,
    /// What was bound.
    pub kind: ResolvedKind,
    /// Path when known.
    pub path: Option<String>,
    /// Symbol name when known.
    pub symbol: Option<String>,
    /// Short evidence label (`selection`, `active_file`, …).
    pub evidence: String,
    /// Confidence of this binding.
    pub confidence: ResolutionConfidence,
}

/// Kind of resolved workspace referent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedKind {
    /// Editor selection.
    Selection,
    /// Active / current file.
    File,
    /// Symbol at cursor / editor intelligence.
    Symbol,
    /// Diagnostic / problem.
    Diagnostic,
    /// Recent edit from Workspace Memory.
    RecentEdit,
    /// Open editor tab.
    OpenTab,
    /// Unresolved after heuristics.
    Unresolved,
}

impl ResolvedKind {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::File => "file",
            Self::Symbol => "symbol",
            Self::Diagnostic => "diagnostic",
            Self::RecentEdit => "recent_edit",
            Self::OpenTab => "open_tab",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Confidence of a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionConfidence {
    /// Clear single referent.
    High,
    /// Best-effort among several candidates.
    Medium,
    /// Weak / fallback.
    Low,
    /// Could not bind.
    Unresolved,
}

impl ResolutionConfidence {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Full Planner-owned environmental resolution for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentalResolution {
    /// True when the request contained workspace deixis / incomplete refs.
    pub needed: bool,
    /// True when multiple plausible referents competed or none bound.
    pub ambiguous: bool,
    /// Bound references (empty when not needed or all unresolved).
    pub references: Vec<ResolvedReference>,
    /// Rule ids that fired (explainability).
    pub rules_matched: Vec<&'static str>,
}

impl EnvironmentalResolution {
    /// Empty — no deixis detected.
    pub fn none() -> Self {
        Self {
            needed: false,
            ambiguous: false,
            references: Vec::new(),
            rules_matched: Vec::new(),
        }
    }

    /// Convert to Context [`EnvironmentalHints`] for AssembleHints / prompt.
    pub fn to_hints(&self) -> EnvironmentalHints {
        if !self.needed {
            return EnvironmentalHints::default();
        }
        let primary = self
            .references
            .iter()
            .find(|r| r.path.is_some() && r.kind != ResolvedKind::Unresolved)
            .or_else(|| self.references.first());
        let bindings: Vec<String> = self
            .references
            .iter()
            .map(|r| {
                let path = r.path.as_deref().unwrap_or("(none)");
                let symbol = r
                    .symbol
                    .as_ref()
                    .map(|s| format!(" symbol={s}"))
                    .unwrap_or_default();
                format!(
                    "{} → {} ({}, {}, conf={}){}",
                    r.cue,
                    path,
                    r.kind.as_str(),
                    r.evidence,
                    r.confidence.as_str(),
                    symbol
                )
            })
            .collect();
        EnvironmentalHints {
            needed: true,
            ambiguous: self.ambiguous,
            primary_path: primary.and_then(|r| r.path.clone()),
            selection_preview: self.references.iter().find_map(|r| {
                if r.kind == ResolvedKind::Selection {
                    r.symbol
                        .as_ref()
                        .map(|s| s.trim_start_matches("selection:").to_string())
                        .or_else(|| Some("selection".into()))
                } else {
                    None
                }
            }),
            symbol: self
                .references
                .iter()
                .find_map(|r| {
                    r.symbol
                        .as_ref()
                        .filter(|s| !s.starts_with("selection:"))
                        .cloned()
                })
                .or_else(|| primary.and_then(|r| r.symbol.clone())),
            diagnostic: self
                .references
                .iter()
                .find(|r| r.kind == ResolvedKind::Diagnostic)
                .map(|r| {
                    format!(
                        "{} ({})",
                        r.path.as_deref().unwrap_or("diagnostic"),
                        r.evidence
                    )
                }),
            bindings,
            rules: self
                .rules_matched
                .iter()
                .map(|r| (*r).to_string())
                .collect(),
        }
    }

    /// Human summary for logs / planner notes.
    pub fn summary_note(&self) -> Option<String> {
        if !self.needed {
            return None;
        }
        let bindings = if self.bindings_summary().is_empty() {
            "unresolved".to_string()
        } else {
            self.bindings_summary()
        };
        Some(format!(
            "environmental_resolution needed=true ambiguous={} rules=[{}] bindings=[{}]",
            self.ambiguous,
            self.rules_matched.join(","),
            bindings
        ))
    }

    fn bindings_summary(&self) -> String {
        self.references
            .iter()
            .map(|r| {
                format!(
                    "{}=>{}",
                    r.cue,
                    r.path.as_deref().unwrap_or(r.kind.as_str())
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Resolve workspace deixis from session intelligence (deterministic, no AI).
pub fn resolve_environment(
    request: &UserRequest,
    session: &ContextSessionInputs,
) -> EnvironmentalResolution {
    // Structured requests already carry explicit paths — no deixis resolution.
    if request.file.is_some()
        || request.write_file.is_some()
        || request.directory.is_some()
        || request.terminal.is_some()
        || request.git.is_some()
        || request.lsp.is_some()
    {
        return EnvironmentalResolution::none();
    }

    let text = normalize(request.content.as_str());
    if text.is_empty() {
        return EnvironmentalResolution::none();
    }

    let cues = detect_deixis(&text);
    if cues.is_empty() {
        return EnvironmentalResolution::none();
    }

    let mut rules = vec!["deixis_detected"];
    let evidence = gather_evidence(session);
    let mut references = Vec::new();
    let mut ambiguous = false;

    for cue in &cues {
        let binding = bind_cue(cue, &text, &evidence, &mut rules);
        if binding.confidence == ResolutionConfidence::Unresolved {
            ambiguous = true;
        }
        if binding.confidence == ResolutionConfidence::Medium {
            ambiguous = true;
        }
        references.push(binding);
    }

    if evidence.competing_paths > 1 && references.iter().any(|r| r.path.is_some()) {
        ambiguous = true;
        rules.push("competing_paths");
    }

    // Enrich selection preview on hints via a synthetic field in first selection ref.
    let mut resolution = EnvironmentalResolution {
        needed: true,
        ambiguous,
        references,
        rules_matched: rules,
    };

    // Attach selection preview into a dedicated binding note when selection used.
    if let Some(preview) = evidence.selection_preview.as_ref() {
        if let Some(sel) = resolution
            .references
            .iter_mut()
            .find(|r| r.kind == ResolvedKind::Selection)
        {
            if sel.symbol.is_none() {
                let capped: String = preview.chars().take(80).collect();
                sel.symbol = Some(format!("selection:{capped}"));
            }
        }
    }

    resolution
}

#[derive(Default)]
struct Evidence {
    selection_path: Option<String>,
    selection_preview: Option<String>,
    active_file: Option<String>,
    symbol: Option<String>,
    diagnostic_path: Option<String>,
    diagnostic_message: Option<String>,
    recent_edit: Option<String>,
    open_tab: Option<String>,
    competing_paths: usize,
}

fn gather_evidence(session: &ContextSessionInputs) -> Evidence {
    let mut evidence = Evidence::default();
    let mut paths = Vec::new();

    let selection_text = session
        .current_selection
        .text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            session.editor_snapshot.as_ref().and_then(|snap| {
                snap.selection
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
        });
    if let Some(text) = selection_text {
        evidence.selection_path = session
            .current_selection
            .path
            .clone()
            .or_else(|| {
                session
                    .editor_snapshot
                    .as_ref()
                    .and_then(|snap| snap.selection.path.clone())
            });
        evidence.selection_preview = Some(text.chars().take(120).collect());
        if let Some(p) = &evidence.selection_path {
            paths.push(p.clone());
        }
    }

    if let Some(path) = session.current_file.path.as_ref() {
        if !path.trim().is_empty() {
            evidence.active_file = Some(path.clone());
            paths.push(path.clone());
        }
    }

    if let Some(snap) = session.editor_snapshot.as_ref() {
        if evidence.active_file.is_none() {
            if let Some(path) = snap.active_file.path.as_ref() {
                evidence.active_file = Some(path.clone());
                paths.push(path.clone());
            }
        }
        if let Some(symbol) = snap.symbol.as_ref() {
            evidence.symbol = Some(symbol.name.clone());
        } else if let Some(symbol) = snap.enclosing_function.as_ref() {
            evidence.symbol = Some(symbol.name.clone());
        }
    }

    let diag = session
        .diagnostics
        .diagnostics
        .iter()
        .find(|d| {
            let sev = d.severity.to_ascii_lowercase();
            sev.contains("error") || sev.contains("err")
        })
        .or_else(|| session.diagnostics.diagnostics.first());
    if let Some(d) = diag {
        evidence.diagnostic_path = d.path.clone();
        evidence.diagnostic_message = Some(d.message.chars().take(120).collect());
        if let Some(p) = &evidence.diagnostic_path {
            paths.push(p.clone());
        }
    }

    if let Some(mem) = session.workspace_memory_snapshot.as_ref() {
        if let Some(edit) = mem.recent_edits.first() {
            evidence.recent_edit = Some(edit.path.clone());
            paths.push(edit.path.clone());
        }
    }

    if let Some(tab) = session.open_files.files.iter().find(|f| f.active) {
        evidence.open_tab = Some(tab.path.clone());
        paths.push(tab.path.clone());
    } else if let Some(tab) = session.open_files.files.first() {
        evidence.open_tab = Some(tab.path.clone());
        paths.push(tab.path.clone());
    }

    paths.sort();
    paths.dedup();
    evidence.competing_paths = paths.len();
    evidence
}

fn bind_cue(
    cue: &str,
    text: &str,
    evidence: &Evidence,
    rules: &mut Vec<&'static str>,
) -> ResolvedReference {
    // "why?" / why → prefer diagnostic, else active file.
    if cue == "why" || text == "why" || text == "why?" {
        if let Some(path) = evidence.diagnostic_path.clone() {
            rules.push("why_diagnostic");
            return ResolvedReference {
                cue: cue.to_string(),
                kind: ResolvedKind::Diagnostic,
                path: Some(path),
                symbol: evidence.diagnostic_message.clone(),
                evidence: "primary_diagnostic".into(),
                confidence: ResolutionConfidence::High,
            };
        }
        if let Some(path) = evidence.active_file.clone() {
            rules.push("why_active_file");
            return ResolvedReference {
                cue: cue.to_string(),
                kind: ResolvedKind::File,
                path: Some(path),
                symbol: evidence.symbol.clone(),
                evidence: "active_file".into(),
                confidence: ResolutionConfidence::Medium,
            };
        }
    }

    // Selection beats file for "this" / "it" when present.
    if matches!(cue, "this" | "that" | "it" | "here" | "these" | "those") {
        if evidence.selection_preview.is_some() {
            rules.push("deixis_selection");
            return ResolvedReference {
                cue: cue.to_string(),
                kind: ResolvedKind::Selection,
                path: evidence.selection_path.clone().or_else(|| evidence.active_file.clone()),
                symbol: evidence.symbol.clone(),
                evidence: "current_selection".into(),
                confidence: ResolutionConfidence::High,
            };
        }
    }

    // "fix it" / error-shaped → diagnostic then selection then file.
    if text.contains("fix") || text.contains("error") || text.contains("broken") {
        if let Some(path) = evidence.diagnostic_path.clone() {
            rules.push("fix_diagnostic");
            return ResolvedReference {
                cue: cue.to_string(),
                kind: ResolvedKind::Diagnostic,
                path: Some(path),
                symbol: evidence.diagnostic_message.clone(),
                evidence: "primary_diagnostic".into(),
                confidence: ResolutionConfidence::High,
            };
        }
    }

    // "rename this" / "clean this up" → selection or active file.
    if text.contains("rename") || text.contains("clean") {
        if evidence.selection_preview.is_some() {
            rules.push("action_selection");
            return ResolvedReference {
                cue: cue.to_string(),
                kind: ResolvedKind::Selection,
                path: evidence.selection_path.clone().or_else(|| evidence.active_file.clone()),
                symbol: evidence.symbol.clone(),
                evidence: "current_selection".into(),
                confidence: ResolutionConfidence::High,
            };
        }
        if let Some(path) = evidence.active_file.clone() {
            rules.push("action_active_file");
            return ResolvedReference {
                cue: cue.to_string(),
                kind: ResolvedKind::File,
                path: Some(path),
                symbol: evidence.symbol.clone(),
                evidence: "active_file".into(),
                confidence: ResolutionConfidence::High,
            };
        }
    }

    // Default deixis chain.
    if let Some(path) = evidence.active_file.clone() {
        rules.push("fallback_active_file");
        let conf = if evidence.competing_paths > 1 {
            ResolutionConfidence::Medium
        } else {
            ResolutionConfidence::High
        };
        return ResolvedReference {
            cue: cue.to_string(),
            kind: if evidence.symbol.is_some() {
                ResolvedKind::Symbol
            } else {
                ResolvedKind::File
            },
            path: Some(path),
            symbol: evidence.symbol.clone(),
            evidence: "active_file".into(),
            confidence: conf,
        };
    }

    if let Some(path) = evidence.recent_edit.clone() {
        rules.push("fallback_recent_edit");
        return ResolvedReference {
            cue: cue.to_string(),
            kind: ResolvedKind::RecentEdit,
            path: Some(path),
            symbol: None,
            evidence: "workspace_memory_recent_edit".into(),
            confidence: ResolutionConfidence::Medium,
        };
    }

    if let Some(path) = evidence.open_tab.clone() {
        rules.push("fallback_open_tab");
        return ResolvedReference {
            cue: cue.to_string(),
            kind: ResolvedKind::OpenTab,
            path: Some(path),
            symbol: None,
            evidence: "open_tab".into(),
            confidence: ResolutionConfidence::Low,
        };
    }

    rules.push("unresolved");
    ResolvedReference {
        cue: cue.to_string(),
        kind: ResolvedKind::Unresolved,
        path: None,
        symbol: None,
        evidence: "none".into(),
        confidence: ResolutionConfidence::Unresolved,
    }
}

fn normalize(raw: &str) -> String {
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

fn detect_deixis(lower: &str) -> Vec<String> {
    let mut cues = Vec::new();

    // Bare why / why?
    if lower == "why" || lower == "why?" {
        cues.push("why".into());
        return cues;
    }

    const PHRASES: &[&str] = &[
        "rename this",
        "fix it",
        "fix this",
        "clean this up",
        "clean it up",
        "clean this",
        "what about this",
        "look at this",
        "check this",
    ];
    for phrase in PHRASES {
        if lower == *phrase || lower.starts_with(&format!("{phrase} ")) || lower.contains(&format!(" {phrase}")) {
            if phrase.contains("this") {
                cues.push("this".into());
            } else if phrase.contains("it") {
                cues.push("it".into());
            }
        }
    }

    // Token-level deixis for short requests.
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.len() <= 12 {
        for word in ["this", "that", "it", "here", "these", "those"] {
            if words.iter().any(|w| *w == word || w.trim_matches('?') == word) {
                if !cues.iter().any(|c| c == word) {
                    cues.push(word.to_string());
                }
            }
        }
        if words.iter().any(|w| *w == "why" || *w == "why?") && !cues.iter().any(|c| c == "why") {
            cues.push("why".into());
        }
    }

    // "the file" / "the error" as soft deixis.
    if lower.contains("the file") && cues.is_empty() {
        cues.push("the file".into());
    }
    if lower.contains("the error") || lower.contains("the bug") {
        if !cues.iter().any(|c| c == "it" || c == "this") {
            cues.push("it".into());
        }
    }

    cues.sort();
    cues.dedup();
    cues
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        BundleDiagnostic, CurrentFileSection, CurrentSelectionSection, DiagnosticsSection,
        EditorSnapshot, OpenFileEntry, OpenFilesSection, WorkspaceMemoryPath,
        WorkspaceMemorySnapshot,
    };

    fn session_with_file(path: &str) -> ContextSessionInputs {
        ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            current_file: CurrentFileSection {
                path: Some(path.into()),
                dirty: false,
                language: Some("rust".into()),
            },
            ..ContextSessionInputs::default()
        }
    }

    #[test]
    fn rename_this_binds_active_file() {
        let session = session_with_file("/proj/main.rs");
        let resolution = resolve_environment(&UserRequest::new("rename this"), &session);
        assert!(resolution.needed);
        assert_eq!(
            resolution.references[0].path.as_deref(),
            Some("/proj/main.rs")
        );
        assert!(matches!(
            resolution.references[0].kind,
            ResolvedKind::File | ResolvedKind::Selection
        ));
    }

    #[test]
    fn fix_it_prefers_diagnostic() {
        let mut session = session_with_file("/proj/main.rs");
        session.diagnostics = DiagnosticsSection {
            diagnostics: vec![BundleDiagnostic {
                path: Some("/proj/lib.rs".into()),
                severity: "error".into(),
                message: "cannot find value".into(),
                line: Some(10),
                column: Some(0),
                source: None,
            }],
        };
        let resolution = resolve_environment(&UserRequest::new("fix it"), &session);
        assert!(resolution.needed);
        assert_eq!(
            resolution.references[0].path.as_deref(),
            Some("/proj/lib.rs")
        );
        assert_eq!(resolution.references[0].kind, ResolvedKind::Diagnostic);
    }

    #[test]
    fn why_binds_diagnostic() {
        let mut session = session_with_file("/proj/main.rs");
        session.diagnostics = DiagnosticsSection {
            diagnostics: vec![BundleDiagnostic {
                path: Some("/proj/main.rs".into()),
                severity: "error".into(),
                message: "borrow checker".into(),
                line: Some(1),
                column: Some(0),
                source: None,
            }],
        };
        let resolution = resolve_environment(&UserRequest::new("why?"), &session);
        assert!(resolution.needed);
        assert_eq!(resolution.references[0].kind, ResolvedKind::Diagnostic);
    }

    #[test]
    fn selection_beats_file_for_this() {
        let mut session = session_with_file("/proj/main.rs");
        session.current_selection = CurrentSelectionSection {
            path: Some("/proj/main.rs".into()),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 8,
            text: Some("fn hello".into()),
        };
        let resolution = resolve_environment(&UserRequest::new("clean this up"), &session);
        assert_eq!(resolution.references[0].kind, ResolvedKind::Selection);
    }

    #[test]
    fn explain_this_binds_selection() {
        let mut session = session_with_file("/proj/main.rs");
        session.current_selection = CurrentSelectionSection {
            path: Some("/proj/main.rs".into()),
            start_line: 2,
            start_column: 0,
            end_line: 4,
            end_column: 1,
            text: Some("let x = 1;".into()),
        };
        let resolution = resolve_environment(&UserRequest::new("Explain this."), &session);
        assert!(resolution.needed);
        assert_eq!(resolution.references[0].kind, ResolvedKind::Selection);
        assert_eq!(
            resolution.references[0].path.as_deref(),
            Some("/proj/main.rs")
        );
    }

    #[test]
    fn rename_this_binds_selection() {
        let mut session = session_with_file("/proj/main.rs");
        session.current_selection = CurrentSelectionSection {
            path: Some("/proj/main.rs".into()),
            start_line: 0,
            start_column: 3,
            end_line: 0,
            end_column: 8,
            text: Some("hello".into()),
        };
        let resolution = resolve_environment(&UserRequest::new("Rename this."), &session);
        assert_eq!(resolution.references[0].kind, ResolvedKind::Selection);
    }

    #[test]
    fn editor_snapshot_selection_feeds_evidence() {
        let mut session = session_with_file("/proj/main.rs");
        session.current_selection = CurrentSelectionSection::default();
        let mut snap = EditorSnapshot::default();
        snap.selection = CurrentSelectionSection {
            path: Some("/proj/main.rs".into()),
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 4,
            text: Some("main".into()),
        };
        session.editor_snapshot = Some(snap);
        let resolution = resolve_environment(&UserRequest::new("look at this"), &session);
        assert_eq!(resolution.references[0].kind, ResolvedKind::Selection);
    }

    #[test]
    fn greeting_needs_no_resolution() {
        let session = session_with_file("/proj/main.rs");
        let resolution = resolve_environment(&UserRequest::new("hello"), &session);
        assert!(!resolution.needed);
        assert!(resolution.references.is_empty());
    }

    #[test]
    fn unresolved_without_workspace_evidence() {
        let session = ContextSessionInputs::default();
        let resolution = resolve_environment(&UserRequest::new("fix it"), &session);
        assert!(resolution.needed);
        assert!(resolution.ambiguous);
        assert_eq!(
            resolution.references[0].confidence,
            ResolutionConfidence::Unresolved
        );
    }

    #[test]
    fn falls_back_to_recent_edit() {
        let mut session = ContextSessionInputs::default();
        session.workspace_memory_snapshot = Some(WorkspaceMemorySnapshot {
            recent_edits: vec![WorkspaceMemoryPath {
                path: "/proj/recent.rs".into(),
                timestamp: 1,
            }],
            ..WorkspaceMemorySnapshot::empty()
        });
        let resolution = resolve_environment(&UserRequest::new("look at this"), &session);
        assert_eq!(
            resolution.references[0].path.as_deref(),
            Some("/proj/recent.rs")
        );
    }

    #[test]
    fn structured_file_request_skips_resolution() {
        let session = session_with_file("/proj/main.rs");
        let resolution = resolve_environment(&UserRequest::read_file("/proj/other.rs"), &session);
        assert!(!resolution.needed);
    }

    #[test]
    fn to_hints_carries_bindings() {
        let session = session_with_file("/proj/main.rs");
        let hints = resolve_environment(&UserRequest::new("rename this"), &session).to_hints();
        assert!(hints.needed);
        assert_eq!(hints.primary_path.as_deref(), Some("/proj/main.rs"));
        assert!(!hints.bindings.is_empty());
    }

    #[test]
    fn open_tab_fallback() {
        let session = ContextSessionInputs {
            open_files: OpenFilesSection {
                files: vec![OpenFileEntry {
                    path: "/proj/tab.rs".into(),
                    dirty: false,
                    active: true,
                }],
            },
            ..ContextSessionInputs::default()
        };
        let resolution = resolve_environment(&UserRequest::new("check this"), &session);
        assert_eq!(
            resolution.references[0].path.as_deref(),
            Some("/proj/tab.rs")
        );
    }
}
