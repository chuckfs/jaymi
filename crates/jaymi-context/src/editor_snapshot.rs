//! Canonical read-only editor intelligence observation (Sprint B2.3).
//!
//! [`EditorSnapshot`] is the immutable representation of language-aware editor
//! state for the active Coding buffer. It is observational only:
//!
//! * executes no tools
//! * performs no reasoning
//! * owns no policy
//! * never builds a [`crate::ContextBundle`]
//! * never talks to an LLM
//!
//! ## Ownership
//!
//! | Role | Owner |
//! |------|--------|
//! | Orchestration (when to assemble) | Planner (via Application host prep) |
//! | Ambient refresh | Application `ContextMaintenance` |
//! | Observation contract | [`EditorSnapshot`] |
//! | Consumption | Context providers (`EditorProvider`, `DiagnosticsProvider`, …) |
//! | Interactive LSP (rename / goto UI) | Application `coding_lsp_*` → Planner → `language_server` |
//! | Reasoning | Assembled [`crate::ContextBundle`] / [`crate::LlmContext`] only — never LSP |
//!
//! Distinct from [`crate::WorkspaceSnapshot`] (environment chrome / project /
//! toolchain) and from capability-engine `EditorWorkspaceSnapshot` (chrome
//! persistence).

use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::bundle::{
    BundleDiagnostic, CurrentFileSection, CurrentSelectionSection, OpenFilesSection,
};
use crate::workspace_snapshot::CursorPosition;

/// Inclusive start / exclusive-or-inclusive end text range (zero-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EditorRange {
    /// Start line.
    pub start_line: u32,
    /// Start column.
    pub start_column: u32,
    /// End line.
    pub end_line: u32,
    /// End column.
    pub end_column: u32,
}

/// A named symbol observation (document / cursor / enclosing).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct EditorSymbol {
    /// Symbol name.
    pub name: String,
    /// Kind label (`function`, `struct`, `method`, …) when known.
    pub kind: Option<String>,
    /// Optional detail / signature.
    pub detail: Option<String>,
    /// Symbol range when known.
    pub range: Option<EditorRange>,
}

/// One semantic-token span.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct EditorSemanticToken {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based start column.
    pub start_column: u32,
    /// Token length in characters.
    pub length: u32,
    /// Token type label (`function`, `variable`, …).
    pub token_type: String,
    /// Optional modifier labels.
    pub modifiers: Vec<String>,
}

/// One reference location.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct EditorReference {
    /// Absolute file path.
    pub path: String,
    /// Reference range.
    pub range: EditorRange,
}

/// Hover observation at the cursor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct EditorHover {
    /// Markdown / plaintext hover body.
    pub contents: String,
    /// Optional range the hover applies to.
    pub range: Option<EditorRange>,
}

/// One code-lens observation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct EditorCodeLens {
    /// Lens title / label.
    pub title: String,
    /// Lens range.
    pub range: EditorRange,
    /// Optional command id.
    pub command: Option<String>,
}

/// Read-only editor intelligence snapshot.
///
/// Built by the Application host (live prepare + ambient maintenance). Context
/// providers consume it; the Planner and Reasoning engine never call LSP to
/// obtain these fields.
#[derive(Debug, Clone, Eq)]
pub struct EditorSnapshot {
    /// Focused editor file.
    pub active_file: CurrentFileSection,
    /// Open editor tabs.
    pub open_editors: OpenFilesSection,
    /// Explicit caret position.
    pub cursor: Option<CursorPosition>,
    /// Active selection (range + text when Monaco selection IPC captured a span).
    pub selection: CurrentSelectionSection,
    /// Symbol under / at the cursor when known.
    pub symbol: Option<EditorSymbol>,
    /// Enclosing function / method when known.
    pub enclosing_function: Option<EditorSymbol>,
    /// Enclosing type (struct / class / enum / …) when known.
    pub enclosing_type: Option<EditorSymbol>,
    /// Semantic tokens for the active buffer (capped by host).
    pub semantic_tokens: Vec<EditorSemanticToken>,
    /// References related to the cursor symbol (capped by host).
    pub references: Vec<EditorReference>,
    /// Diagnostics observed for the editor (usually current-file focused).
    pub diagnostics: Vec<BundleDiagnostic>,
    /// Code lenses for the active buffer (capped by host).
    pub code_lens: Vec<EditorCodeLens>,
    /// Hover at the cursor when known.
    pub hover: Option<EditorHover>,
    /// Unix seconds when this observation was captured.
    ///
    /// Ignored by [`PartialEq`] / [`Hash`] so repeated captures do not churn
    /// Context session fingerprints.
    pub timestamp: i64,
}

impl Default for EditorSnapshot {
    fn default() -> Self {
        Self {
            active_file: CurrentFileSection::default(),
            open_editors: OpenFilesSection::default(),
            cursor: None,
            selection: CurrentSelectionSection::default(),
            symbol: None,
            enclosing_function: None,
            enclosing_type: None,
            semantic_tokens: Vec::new(),
            references: Vec::new(),
            diagnostics: Vec::new(),
            code_lens: Vec::new(),
            hover: None,
            timestamp: 0,
        }
    }
}

impl PartialEq for EditorSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.active_file == other.active_file
            && self.open_editors == other.open_editors
            && self.cursor == other.cursor
            && self.selection == other.selection
            && self.symbol == other.symbol
            && self.enclosing_function == other.enclosing_function
            && self.enclosing_type == other.enclosing_type
            && self.semantic_tokens == other.semantic_tokens
            && self.references == other.references
            && self.diagnostics == other.diagnostics
            && self.code_lens == other.code_lens
            && self.hover == other.hover
        // timestamp intentionally excluded
    }
}

impl Hash for EditorSnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.active_file.path.hash(state);
        self.active_file.dirty.hash(state);
        self.active_file.language.hash(state);
        for file in &self.open_editors.files {
            file.path.hash(state);
            file.dirty.hash(state);
            file.active.hash(state);
        }
        self.cursor.hash(state);
        self.selection.path.hash(state);
        self.selection.start_line.hash(state);
        self.selection.start_column.hash(state);
        self.selection.end_line.hash(state);
        self.selection.end_column.hash(state);
        self.selection.text.hash(state);
        self.symbol.hash(state);
        self.enclosing_function.hash(state);
        self.enclosing_type.hash(state);
        self.semantic_tokens.hash(state);
        self.references.hash(state);
        for diag in &self.diagnostics {
            diag.path.hash(state);
            diag.severity.hash(state);
            diag.message.hash(state);
            diag.line.hash(state);
            diag.column.hash(state);
            diag.source.hash(state);
        }
        self.code_lens.hash(state);
        self.hover.hash(state);
    }
}

/// Host-supplied parts for building an [`EditorSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditorSnapshotObservation {
    /// Active file.
    pub active_file: CurrentFileSection,
    /// Open editors.
    pub open_editors: OpenFilesSection,
    /// Cursor.
    pub cursor: Option<CursorPosition>,
    /// Selection.
    pub selection: CurrentSelectionSection,
    /// Symbol at cursor.
    pub symbol: Option<EditorSymbol>,
    /// Enclosing function.
    pub enclosing_function: Option<EditorSymbol>,
    /// Enclosing type.
    pub enclosing_type: Option<EditorSymbol>,
    /// Semantic tokens.
    pub semantic_tokens: Vec<EditorSemanticToken>,
    /// References.
    pub references: Vec<EditorReference>,
    /// Diagnostics.
    pub diagnostics: Vec<BundleDiagnostic>,
    /// Code lenses.
    pub code_lens: Vec<EditorCodeLens>,
    /// Hover.
    pub hover: Option<EditorHover>,
    /// Optional capture time; defaults to now.
    pub timestamp: Option<i64>,
}

/// Intelligence-only subset contributed into a [`crate::ContextBundle`].
///
/// Derived from [`EditorSnapshot`] by Context providers — never by Planner or
/// Reasoning calling LSP.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditorIntelligenceSection {
    /// Symbol under / at the cursor.
    pub symbol: Option<EditorSymbol>,
    /// Enclosing function / method.
    pub enclosing_function: Option<EditorSymbol>,
    /// Enclosing type.
    pub enclosing_type: Option<EditorSymbol>,
    /// Semantic tokens (capped).
    pub semantic_tokens: Vec<EditorSemanticToken>,
    /// References (capped).
    pub references: Vec<EditorReference>,
    /// Code lenses (capped).
    pub code_lens: Vec<EditorCodeLens>,
    /// Hover at the cursor.
    pub hover: Option<EditorHover>,
}

impl EditorSnapshot {
    /// Empty observational snapshot (no Coding editor open).
    pub fn empty() -> Self {
        Self {
            timestamp: now_unix_secs(),
            ..Self::default()
        }
    }

    /// Build an immutable snapshot from host-observed parts.
    ///
    /// Does not execute tools, reason, or assemble a ContextBundle.
    pub fn from_observation(parts: EditorSnapshotObservation) -> Self {
        Self {
            active_file: parts.active_file,
            open_editors: parts.open_editors,
            cursor: parts.cursor,
            selection: parts.selection,
            symbol: parts.symbol,
            enclosing_function: parts.enclosing_function,
            enclosing_type: parts.enclosing_type,
            semantic_tokens: parts.semantic_tokens,
            references: parts.references,
            diagnostics: parts.diagnostics,
            code_lens: parts.code_lens,
            hover: parts.hover,
            timestamp: parts.timestamp.unwrap_or_else(now_unix_secs),
        }
    }

    /// True when any editor identity is present.
    pub fn has_editor_state(&self) -> bool {
        self.active_file.path.is_some() || !self.open_editors.files.is_empty()
    }

    /// True when any language-intelligence field is populated.
    pub fn has_intelligence(&self) -> bool {
        self.symbol.is_some()
            || self.enclosing_function.is_some()
            || self.enclosing_type.is_some()
            || !self.semantic_tokens.is_empty()
            || !self.references.is_empty()
            || !self.code_lens.is_empty()
            || self.hover.is_some()
    }

    /// Intelligence subset for ContextBundle contribution.
    pub fn intelligence_section(&self) -> EditorIntelligenceSection {
        EditorIntelligenceSection {
            symbol: self.symbol.clone(),
            enclosing_function: self.enclosing_function.clone(),
            enclosing_type: self.enclosing_type.clone(),
            semantic_tokens: self.semantic_tokens.clone(),
            references: self.references.clone(),
            code_lens: self.code_lens.clone(),
            hover: self.hover.clone(),
        }
    }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::OpenFileEntry;

    #[test]
    fn empty_snapshot_has_no_editor_state() {
        let snap = EditorSnapshot::empty();
        assert!(!snap.has_editor_state());
        assert!(!snap.has_intelligence());
    }

    #[test]
    fn from_observation_preserves_fields() {
        let snap = EditorSnapshot::from_observation(EditorSnapshotObservation {
            active_file: CurrentFileSection {
                path: Some("src/lib.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            },
            open_editors: OpenFilesSection {
                files: vec![OpenFileEntry {
                    path: "src/lib.rs".into(),
                    dirty: true,
                    active: true,
                }],
            },
            cursor: Some(CursorPosition {
                line: 10,
                column: 4,
            }),
            selection: CurrentSelectionSection {
                path: Some("src/lib.rs".into()),
                start_line: 10,
                start_column: 4,
                end_line: 10,
                end_column: 4,
                text: None,
            },
            hover: Some(EditorHover {
                contents: "fn main()".into(),
                range: None,
            }),
            diagnostics: vec![BundleDiagnostic {
                path: Some("src/lib.rs".into()),
                severity: "warning".into(),
                message: "unused".into(),
                line: Some(1),
                column: Some(0),
                source: Some("rust-analyzer".into()),
            }],
            ..EditorSnapshotObservation::default()
        });
        assert!(snap.has_editor_state());
        assert!(snap.has_intelligence());
        assert_eq!(snap.active_file.path.as_deref(), Some("src/lib.rs"));
        assert_eq!(snap.hover.as_ref().map(|h| h.contents.as_str()), Some("fn main()"));
        assert_eq!(snap.diagnostics.len(), 1);
    }

    #[test]
    fn snapshot_ignores_timestamp_for_equality() {
        let mut a = EditorSnapshot::from_observation(EditorSnapshotObservation {
            active_file: CurrentFileSection {
                path: Some("a.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            timestamp: Some(1),
            ..EditorSnapshotObservation::default()
        });
        let mut b = a.clone();
        b.timestamp = 999;
        assert_eq!(a, b);
        a.active_file.dirty = true;
        assert_ne!(a, b);
    }
}
