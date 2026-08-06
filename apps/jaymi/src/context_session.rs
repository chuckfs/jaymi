//! Build [`ContextSessionInputs`] from live Application / Coding UI state.
//!
//! The Context Engine never discovers editor or UX state itself — the host
//! pushes a full snapshot via [`crate::Application`]'s `prepare_context_session`
//! before **every** Planner assemble path (tool-backed `handle`, conversational
//! `begin_generation` / streaming, workspace expand/close). There is no
//! alternate preparation path for conversation.

use jaymi_capabilities::CodingState;
use jaymi_context::{
    BundleDiagnostic, BundlePermissionEntry, BundleSearchHit, ContextSessionInputs,
    CurrentFileSection, CurrentSelectionSection, DiagnosticsSection, OpenFileEntry,
    OpenFilesSection, PermissionsSection,
};
use jaymi_permissions::{
    PermissionAction, PermissionCategory, PermissionEngine, PermissionRequest, PermissionScope,
};

use crate::monaco_host::language_for_path;

/// Assemble a complete session snapshot for the Context Engine.
///
/// When Coding is inactive, editor / diagnostics / search sections stay empty.
/// Permissions are filled from the Permission Engine.
///
/// **Ownership:** do not pass the Capability catalog here. Request-selected
/// capability ids are supplied only by the Planner via [`jaymi_context::AssembleHints`].
pub fn build_context_session_inputs(
    workspace_kind: Option<String>,
    coding: Option<&CodingState>,
    permissions: &PermissionEngine,
    project_open: bool,
    project_indexed_documents: Option<u64>,
) -> ContextSessionInputs {
    let mut inputs = ContextSessionInputs {
        workspace_kind,
        project_open,
        project_indexed_documents,
        permissions: PermissionsSection {
            entries: synthesize_permission_summary(permissions),
        },
        ..ContextSessionInputs::default()
    };

    let Some(coding) = coding else {
        return inputs;
    };

    fill_editor_sections(&mut inputs, coding);
    fill_diagnostics(&mut inputs, coding);
    fill_search_hits(&mut inputs, coding);
    inputs
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

    // Honest cursor mapping until Monaco selection IPC exists: zero-width
    // selection at the caret (path + line/column), no invented selected text.
    inputs.current_selection = match &active_session {
        Some(session) => {
            let cursor = session.view.cursor;
            CurrentSelectionSection {
                path: Some(session.path.clone()),
                start_line: cursor.line,
                start_column: cursor.column,
                end_line: cursor.line,
                end_column: cursor.column,
                text: None,
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
    }
}
