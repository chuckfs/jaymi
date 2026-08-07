//! Build [`ContextSessionInputs`] from live Application / Coding UI state.
//!
//! The Context Engine never discovers editor or UX state itself — the host
//! pushes a full snapshot via [`crate::Application`]'s `prepare_context_session`
//! before **every** Planner assemble path (tool-backed `handle`, conversational
//! `begin_generation` / streaming, workspace expand/close). There is no
//! alternate preparation path for conversation.
//!
//! Sprint **B2.1 / B2.2 / B2.13.2:** [`jaymi_context::WorkspaceSnapshot`] is
//! **ambient-only**. Prepare merges the latest **completed** maintenance
//! observation and never rebuilds a WorkspaceSnapshot or probes the filesystem
//! (`observe_toolchain`) on the conversational path.
//!
//! Sprint **B2.3:** the same prep / ambient path captures a read-only
//! [`jaymi_context::EditorSnapshot`]. Context providers consume it; Planner and
//! Reasoning never call LSP to obtain editor intelligence.
//!
//! Sprint **B2.4:** ambient Application maintenance captures a
//! [`jaymi_context::ProjectSnapshot`]. `ProjectProvider` consumes it; Planner
//! never scans projects and providers never filesystem-scan during assemble.
//!
//! Sprint **B2.6:** ambient Application maintenance captures a
//! [`jaymi_context::RuntimeSnapshot`] from Coding terminal sessions. Terminal
//! Provider owns PTY updates; conversation never waits for runtime.
//!
//! Sprint **B2.9:** prepare also captures a
//! [`jaymi_context::WorkspaceMemorySnapshot`] from CodingState activity rings
//! (recent edits / opens / builds / failures / coding objective). Distinct from
//! Conversation Memory; Context Policy decides inclusion.

use jaymi_capabilities::CodingState;
use jaymi_context::{
    observe_toolchain, observe_workspace_memory, ActiveProjectRef, BundleDiagnostic,
    BundlePermissionEntry, BundleSearchHit, ContextSessionInputs, CurrentFileSection,
    CurrentSelectionSection, CursorPosition, DiagnosticsSection, EditorHover, EditorReference,
    EditorSnapshot, EditorSnapshotObservation, EditorSymbol, OpenFileEntry, OpenFilesSection,
    PermissionsSection, WorkspaceMemoryCommand, WorkspaceMemoryHostFacts, WorkspaceMemoryPath,
    WorkspaceMemorySnapshot, WorkspaceSnapshot, WorkspaceSnapshotObservation,
};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionEngine, PermissionRequest, PermissionScope,
};

use crate::monaco_host::language_for_path;

/// Host-observed project / git facts for [`WorkspaceSnapshot`] (not discovered by Context).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceSnapshotHostFacts {
    /// Open project id from Project Engine.
    pub project_id: Option<String>,
    /// Open project display name.
    pub project_name: Option<String>,
    /// Canonical project root directory.
    pub project_root: Option<String>,
    /// Active Git branch from completed maintenance / CodingState.git.
    pub active_branch: Option<String>,
}

/// Assemble a complete session snapshot for the Context Engine.
///
/// When Coding is inactive, editor / diagnostics / search sections stay empty.
/// Permissions are filled from the Permission Engine.
///
/// Attaches a live [`EditorSnapshot`] and [`WorkspaceMemorySnapshot`] from
/// in-memory CodingState (no filesystem). Does **not** attach a
/// [`WorkspaceSnapshot`] — that observation is ambient-only (Sprint B2.13.2);
/// [`crate::Application::prepare_context_session`] merges the latest completed
/// maintenance snapshot.
///
/// **Ownership:** do not pass the Capability catalog here. Request-selected
/// capability ids are supplied only by the Planner via [`jaymi_context::AssembleHints`].
pub fn build_context_session_inputs(
    workspace_kind: Option<String>,
    coding: Option<&CodingState>,
    permissions: &PermissionEngine,
    project_open: bool,
    project_indexed_documents: Option<u64>,
    _facts: WorkspaceSnapshotHostFacts,
) -> ContextSessionInputs {
    let mut inputs = ContextSessionInputs {
        workspace_kind: workspace_kind.clone(),
        project_open,
        project_indexed_documents,
        permissions: PermissionsSection {
            entries: synthesize_permission_summary(permissions),
        },
        ..ContextSessionInputs::default()
    };

    if let Some(coding) = coding {
        fill_editor_sections(&mut inputs, coding);
        fill_diagnostics(&mut inputs, coding);
        fill_search_hits(&mut inputs, coding);
    }

    // WorkspaceSnapshot is ambient-only — never rebuild or probe FS here.
    inputs.workspace_snapshot = None;
    inputs.editor_snapshot = Some(capture_editor_snapshot_from_coding(
        coding,
        EditorSnapshotEnrichment::default(),
    ));
    inputs.workspace_memory_snapshot = Some(capture_workspace_memory_from_coding(coding));
    inputs
}

/// Capture Workspace Memory from Coding activity (Sprint B2.9).
///
/// Observational only — no Memory Engine writes, no tools, no ContextBundle.
pub fn capture_workspace_memory_from_coding(
    coding: Option<&CodingState>,
) -> WorkspaceMemorySnapshot {
    let Some(coding) = coding else {
        return WorkspaceMemorySnapshot::empty();
    };
    let activity = &coding.workspace_activity;
    observe_workspace_memory(WorkspaceMemoryHostFacts {
        coding_objective: activity.coding_objective.clone(),
        recent_edits: activity
            .recent_edits
            .iter()
            .map(|entry| WorkspaceMemoryPath {
                path: entry.path.clone(),
                timestamp: entry.timestamp,
            })
            .collect(),
        recently_opened: coding.editors.recently_opened.clone(),
        recent_builds: activity
            .recent_builds
            .iter()
            .map(|entry| WorkspaceMemoryCommand {
                command: entry.command.clone(),
                summary: entry.summary.clone(),
                ok: entry.ok,
                timestamp: entry.timestamp,
            })
            .collect(),
        recent_failures: activity
            .recent_failures
            .iter()
            .map(|entry| WorkspaceMemoryCommand {
                command: entry.command.clone(),
                summary: entry.summary.clone(),
                ok: entry.ok,
                timestamp: entry.timestamp,
            })
            .collect(),
    })
}

/// Optional language-intelligence enrichments for [`EditorSnapshot`].
///
/// Filled by Application ambient maintenance via read-only `LspProvider`
/// observation — never by Planner/Reasoning mid-assemble.
#[derive(Debug, Clone, Default)]
pub struct EditorSnapshotEnrichment {
    /// Symbol at cursor.
    pub symbol: Option<EditorSymbol>,
    /// Enclosing function.
    pub enclosing_function: Option<EditorSymbol>,
    /// Enclosing type.
    pub enclosing_type: Option<EditorSymbol>,
    /// Semantic tokens.
    pub semantic_tokens: Vec<jaymi_context::EditorSemanticToken>,
    /// References.
    pub references: Vec<EditorReference>,
    /// Code lenses.
    pub code_lens: Vec<jaymi_context::EditorCodeLens>,
    /// Hover at cursor.
    pub hover: Option<EditorHover>,
}

/// Capture a read-only [`EditorSnapshot`] from Coding (+ optional enrichments).
///
/// Observational only: no tools, no Planner path, no Reasoning, no ContextBundle.
pub fn capture_editor_snapshot_from_coding(
    coding: Option<&CodingState>,
    enrichment: EditorSnapshotEnrichment,
) -> EditorSnapshot {
    let mut editor = ContextSessionInputs::default();
    if let Some(coding) = coding {
        fill_editor_sections(&mut editor, coding);
        fill_diagnostics(&mut editor, coding);
    }

    let cursor = editor.current_selection.path.as_ref().map(|_| CursorPosition {
        line: editor.current_selection.start_line,
        column: editor.current_selection.start_column,
    });

    EditorSnapshot::from_observation(EditorSnapshotObservation {
        active_file: editor.current_file,
        open_editors: editor.open_files,
        cursor,
        selection: editor.current_selection,
        symbol: enrichment.symbol,
        enclosing_function: enrichment.enclosing_function,
        enclosing_type: enrichment.enclosing_type,
        semantic_tokens: enrichment.semantic_tokens,
        references: enrichment.references,
        diagnostics: editor.diagnostics.diagnostics,
        code_lens: enrichment.code_lens,
        hover: enrichment.hover,
        timestamp: None,
    })
}

/// Capture a [`WorkspaceSnapshot`] from Coding + host facts (**ambient only**).
///
/// Called from Application `ContextMaintenance` workers — never from
/// conversational `prepare_context_session`. Marker-file toolchain detection
/// (`observe_toolchain`) runs here in the background.
///
/// Fills editor sections locally — does not need the Permission Engine.
/// Observational only: no tools, reasoning, policy, or ContextBundle assembly.
pub fn capture_workspace_snapshot_from_coding(
    workspace_kind: Option<String>,
    coding: Option<&CodingState>,
    facts: &WorkspaceSnapshotHostFacts,
) -> WorkspaceSnapshot {
    let mut editor = ContextSessionInputs::default();
    if let Some(coding) = coding {
        fill_editor_sections(&mut editor, coding);
    }
    capture_workspace_snapshot(workspace_kind, coding, &editor, facts)
}

/// Capture the canonical Coding [`WorkspaceSnapshot`] (Sprint B2.1).
///
/// **Ambient Context Maintenance only** (Sprint B2.13.2). Observational:
/// reads host-supplied state + marker-file toolchain presence. Does not
/// execute tools, reason, apply policy, or assemble a bundle. Must not run on
/// the conversational prepare path.
pub fn capture_workspace_snapshot(
    workspace_kind: Option<String>,
    coding: Option<&CodingState>,
    editor: &ContextSessionInputs,
    facts: &WorkspaceSnapshotHostFacts,
) -> WorkspaceSnapshot {
    use std::path::Path;

    let workspace_root = facts
        .project_root
        .clone()
        .or_else(|| {
            coding
                .and_then(|state| state.explorer.project_root.clone())
        });

    let active_branch = facts.active_branch.clone().or_else(|| {
        coding
            .and_then(|state| state.git.as_ref())
            .and_then(|git| git.branch.clone())
    });

    let toolchain = workspace_root
        .as_deref()
        .map(Path::new)
        .map(observe_toolchain)
        .unwrap_or_default();

    let cursor = editor.current_selection.path.as_ref().map(|_| CursorPosition {
        line: editor.current_selection.start_line,
        column: editor.current_selection.start_column,
    });

    WorkspaceSnapshot::from_observation(WorkspaceSnapshotObservation {
        active_project: ActiveProjectRef {
            project_id: facts.project_id.clone(),
            name: facts.project_name.clone(),
            root_directory: facts.project_root.clone(),
        },
        workspace_root,
        workspace_kind,
        current_file: editor.current_file.clone(),
        open_files: editor.open_files.clone(),
        active_selection: editor.current_selection.clone(),
        cursor,
        active_branch,
        package_manager: toolchain.package_manager,
        build_system: toolchain.build_system,
        timestamp: None,
    })
}

fn fill_editor_sections(inputs: &mut ContextSessionInputs, coding: &CodingState) {
    let active_path = coding.active_tab_path().map(str::to_string);
    let active_session = coding.editors.active_session();

    inputs.current_file = match &active_session {
        Some(session) => CurrentFileSection {
            path: Some(session.path.clone()),
            dirty: session.dirty,
            language: Some(language_for_path(&session.path).to_string()),
        },
        None => CurrentFileSection::default(),
    };

    // Monaco selection IPC → CodingState.selection → Workspace/EditorSnapshot.
    // Empty range keeps caret coordinates with text: None (no invented span).
    inputs.current_selection = match &active_session {
        Some(session) => {
            let sel = &session.view.selection;
            let cursor = session.view.cursor;
            // Prefer stored selection; fall back to caret when selection is still
            // the default zero-range and the caret has moved (pre-IPC tabs).
            let use_cursor_fallback = sel.is_empty()
                && sel.text.is_none()
                && sel.start_line == 0
                && sel.start_column == 0
                && (cursor.line != 0 || cursor.column != 0);
            if use_cursor_fallback {
                CurrentSelectionSection {
                    path: Some(session.path.clone()),
                    start_line: cursor.line,
                    start_column: cursor.column,
                    end_line: cursor.line,
                    end_column: cursor.column,
                    text: None,
                }
            } else {
                CurrentSelectionSection {
                    path: Some(session.path.clone()),
                    start_line: sel.start_line,
                    start_column: sel.start_column,
                    end_line: sel.end_line,
                    end_column: sel.end_column,
                    text: sel.text.clone(),
                }
            }
        }
        None => CurrentSelectionSection::default(),
    };

    inputs.open_files = OpenFilesSection {
        files: coding
            .editors
            .open_files()
            .into_iter()
            .map(|file| OpenFileEntry {
                active: active_path.as_deref() == Some(file.path.as_str()),
                path: file.path,
                dirty: file.dirty,
            })
            .collect(),
    };
}

fn fill_diagnostics(inputs: &mut ContextSessionInputs, coding: &CodingState) {
    let mut diagnostics = Vec::new();
    if !coding.problems.is_empty() {
        for issue in &coding.problems {
            diagnostics.push(BundleDiagnostic {
                path: issue.path.clone(),
                severity: issue.severity.as_str().to_string(),
                message: issue.message.clone(),
                line: issue.line,
                column: issue.column,
                source: Some(if issue.source_label.is_empty() {
                    issue.source.clone()
                } else {
                    issue.source_label.clone()
                }),
            });
        }
    } else {
        for diagnostic in &coding.diagnostics {
            diagnostics.push(BundleDiagnostic {
                path: diagnostic.path.clone(),
                severity: diagnostic.severity.clone(),
                message: diagnostic.message.clone(),
                line: diagnostic.line,
                column: diagnostic.character,
                source: if diagnostic.source.is_empty() {
                    None
                } else {
                    Some(diagnostic.source.clone())
                },
            });
        }
    }
    inputs.diagnostics = DiagnosticsSection { diagnostics };
}

fn fill_search_hits(inputs: &mut ContextSessionInputs, coding: &CodingState) {
    inputs.search_hits = coding
        .search
        .results
        .iter()
        .enumerate()
        .map(|(index, entry)| BundleSearchHit {
            item_id: format!("{}:{}", entry.path, index),
            title: if entry.title.is_empty() {
                entry.path.clone()
            } else {
                entry.title.clone()
            },
            path: Some(entry.path.clone()),
            score: None,
            match_reason: if entry.why_matched.is_empty() {
                None
            } else {
                Some(entry.why_matched.clone())
            },
            preview: if entry.preview.is_empty() {
                None
            } else {
                Some(entry.preview.clone())
            },
            line: entry.line,
            column: entry.column,
        })
        .collect();
}

/// Policy snapshot for the ContextBundle — mirrors PermissionEngine rules.
///
/// Not a grant store; entries describe the current auto-allow / deny /
/// approval matrix so providers can expose a permission summary.
fn synthesize_permission_summary(permissions: &PermissionEngine) -> Vec<BundlePermissionEntry> {
    const MATRIX: &[(PermissionCategory, PermissionAction, &str)] = &[
        (
            PermissionCategory::Filesystem,
            PermissionAction::Read,
            "Local filesystem read",
        ),
        (
            PermissionCategory::Filesystem,
            PermissionAction::Write,
            "Local filesystem write",
        ),
        (
            PermissionCategory::Filesystem,
            PermissionAction::Delete,
            "Local filesystem delete",
        ),
        (
            PermissionCategory::Terminal,
            PermissionAction::Execute,
            "Local terminal execute",
        ),
        (
            PermissionCategory::Internet,
            PermissionAction::Network,
            "Outbound network",
        ),
        (
            PermissionCategory::Communication,
            PermissionAction::Network,
            "External communication",
        ),
        (
            PermissionCategory::System,
            PermissionAction::Execute,
            "System-level actions",
        ),
        (
            PermissionCategory::AiProviders,
            PermissionAction::Network,
            "Cloud AI providers",
        ),
    ];

    let mut entries = Vec::new();
    for &(category, action, explanation) in MATRIX {
        let request = PermissionRequest {
            category,
            action,
            scope: PermissionScope::Conversation,
            explanation: explanation.to_string(),
            resource: None,
        };
        let Ok(result) = permissions.check(&request) else {
            continue;
        };
        entries.push(BundlePermissionEntry {
            category: permission_category_label(category).to_string(),
            action: permission_action_label(action).to_string(),
            decision: result.decision.as_str().to_string(),
            resource: None,
            explanation: Some(explanation.to_string()),
        });
    }
    entries
}

fn permission_category_label(category: PermissionCategory) -> &'static str {
    match category {
        PermissionCategory::Filesystem => "filesystem",
        PermissionCategory::Internet => "internet",
        PermissionCategory::Terminal => "terminal",
        PermissionCategory::Communication => "communication",
        PermissionCategory::System => "system",
        PermissionCategory::AiProviders => "ai_providers",
    }
}

fn permission_action_label(action: PermissionAction) -> &'static str {
    match action {
        PermissionAction::Read => "read",
        PermissionAction::Write => "write",
        PermissionAction::Execute => "execute",
        PermissionAction::Delete => "delete",
        PermissionAction::Network => "network",
        PermissionAction::Import => "import",
        PermissionAction::Export => "export",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::{DiagnosticState, SearchResultEntry};
    use jaymi_core::Lifecycle;
    use jaymi_permissions::PermissionEngine;

    fn permissions_engine() -> PermissionEngine {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        engine
    }

    #[test]
    fn empty_coding_still_fills_permissions() {
        let permissions = permissions_engine();
        let inputs = build_context_session_inputs(
            Some("coding".into()),
            None,
            &permissions,
            false,
            None,
            WorkspaceSnapshotHostFacts::default(),
        );
        assert_eq!(inputs.workspace_kind.as_deref(), Some("coding"));
        assert!(inputs.current_file.path.is_none());
        assert!(inputs.open_files.files.is_empty());
        assert!(!inputs.permissions.entries.is_empty());
        assert!(inputs
            .permissions
            .entries
            .iter()
            .any(|entry| entry.category == "filesystem" && entry.decision == "allowed"));
        assert!(
            inputs.active_capabilities.capability_ids.is_empty(),
            "session must not carry capability catalog"
        );
        assert!(
            inputs.workspace_snapshot.is_none(),
            "WorkspaceSnapshot is ambient-only — prepare builder must not attach one"
        );
        assert!(inputs.editor_snapshot.is_some());
    }

    #[test]
    fn coding_state_maps_editor_diagnostics_and_search() {
        let permissions = permissions_engine();
        let mut coding = CodingState::default();
        coding.open_permanent("/proj/main.rs", "fn main() {}\n".into());
        coding.diagnostics.push(DiagnosticState {
            message: "unused".into(),
            path: Some("/proj/main.rs".into()),
            severity: "warning".into(),
            source: "rustc".into(),
            line: Some(0),
            character: Some(0),
            end_line: None,
            end_character: None,
        });
        coding.search.results.push(SearchResultEntry {
            path: "/proj/main.rs".into(),
            title: "main.rs".into(),
            line: Some(0),
            column: Some(3),
            end_line: None,
            end_column: None,
            preview: "fn main".into(),
            why_matched: "text".into(),
        });

        let inputs = build_context_session_inputs(
            Some("coding".into()),
            Some(&coding),
            &permissions,
            true,
            Some(12),
            WorkspaceSnapshotHostFacts {
                project_id: Some("proj-1".into()),
                project_name: Some("demo".into()),
                project_root: Some("/proj".into()),
                active_branch: Some("main".into()),
            },
        );
        assert!(inputs.project_open);
        assert_eq!(inputs.project_indexed_documents, Some(12));
        assert_eq!(inputs.current_file.path.as_deref(), Some("/proj/main.rs"));
        assert_eq!(inputs.current_file.language.as_deref(), Some("rust"));
        assert_eq!(
            inputs.current_selection.path.as_deref(),
            Some("/proj/main.rs")
        );
        assert_eq!(inputs.open_files.files.len(), 1);
        assert!(inputs.open_files.files[0].active);
        assert_eq!(inputs.diagnostics.diagnostics.len(), 1);
        assert_eq!(inputs.search_hits.len(), 1);
        assert_eq!(inputs.search_hits[0].title, "main.rs");

        assert!(
            inputs.workspace_snapshot.is_none(),
            "builder must not rebuild WorkspaceSnapshot (ambient-only)"
        );

        let editor = inputs.editor_snapshot.expect("editor snapshot");
        assert_eq!(editor.active_file.path.as_deref(), Some("/proj/main.rs"));
        assert_eq!(editor.open_editors.files.len(), 1);
        assert_eq!(editor.cursor.map(|c| (c.line, c.column)), Some((0, 0)));
        assert_eq!(editor.diagnostics.len(), 1);
        assert!(editor.has_editor_state());
    }

    #[test]
    fn prepare_builder_does_not_probe_toolchain_markers() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let permissions = permissions_engine();
        let dir = std::env::temp_dir().join(format!(
            "jaymi-b2132-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let root = dir.display().to_string();

        let inputs = build_context_session_inputs(
            Some("coding".into()),
            None,
            &permissions,
            true,
            None,
            WorkspaceSnapshotHostFacts {
                project_root: Some(root.clone()),
                ..WorkspaceSnapshotHostFacts::default()
            },
        );
        assert!(
            inputs.workspace_snapshot.is_none(),
            "must not attach WorkspaceSnapshot (would probe Cargo.toml on prepare)"
        );

        // Ambient capture still observes toolchain in the background path.
        let ambient = capture_workspace_snapshot_from_coding(
            Some("coding".into()),
            None,
            &WorkspaceSnapshotHostFacts {
                project_root: Some(root),
                ..WorkspaceSnapshotHostFacts::default()
            },
        );
        assert!(matches!(
            ambient.package_manager,
            Some(jaymi_context::PackageManagerKind::Cargo)
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
