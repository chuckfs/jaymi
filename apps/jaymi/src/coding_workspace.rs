//! Coding Workspace shell — thin egui chrome for the five Coding panels.
//!
//! Conversation stays in the central panel. This module only renders the
//! right-side Coding expansion. Real editor / terminal / git tools remain
//! Layer 7 Target; the shell binds placeholders to [`CodingState`].

use eframe::egui;
use jaymi_capabilities::{CodingState, WorkspaceExpansion, WorkspacePanel};

/// Pure text summary of the Coding shell for tests and headless checks.
pub fn coding_shell_summary(state: &CodingState) -> String {
    let mut lines = Vec::new();
    lines.push("Coding Workspace Shell".to_string());
    for panel in [
        WorkspacePanel::ProjectExplorer,
        WorkspacePanel::Editor,
        WorkspacePanel::Terminal,
        WorkspacePanel::Git,
        WorkspacePanel::Diagnostics,
    ] {
        lines.push(format!("## {}", panel.label()));
        for line in coding_panel_lines(panel, Some(state)) {
            lines.push(format!("- {line}"));
        }
    }
    lines.join("\n")
}

/// Lines shown inside one Coding panel, driven by optional [`CodingState`].
pub fn coding_panel_lines(panel: WorkspacePanel, state: Option<&CodingState>) -> Vec<String> {
    let Some(state) = state else {
        return vec![placeholder_for(panel).to_string()];
    };
    match panel {
        WorkspacePanel::ProjectExplorer => {
            let mut lines = Vec::new();
            if let Some(selected) = &state.selected_path {
                lines.push(format!("selected: {selected}"));
            }
            if state.explorer_entries.is_empty() {
                lines.push("No project tree yet — stub explorer.".to_string());
            } else {
                for entry in &state.explorer_entries {
                    let marker = if state.selected_path.as_deref() == Some(entry.as_str()) {
                        "▸ "
                    } else {
                        "  "
                    };
                    lines.push(format!("{marker}{entry}"));
                }
            }
            lines
        }
        WorkspacePanel::Editor => {
            if state.open_files.is_empty() {
                vec!["No open files — source editing surface (stub).".to_string()]
            } else {
                state
                    .open_files
                    .iter()
                    .map(|file| {
                        format!(
                            "{}{}",
                            file.path,
                            if file.dirty { " · dirty" } else { "" }
                        )
                    })
                    .collect()
            }
        }
        WorkspacePanel::Terminal => {
            if state.terminal_sessions.is_empty() {
                vec!["No terminal sessions — command surface (stub).".to_string()]
            } else {
                state
                    .terminal_sessions
                    .iter()
                    .map(|session| {
                        format!(
                            "{} · cwd={} · last={}",
                            session.id,
                            session.cwd.as_deref().unwrap_or("-"),
                            session.last_command.as_deref().unwrap_or("-")
                        )
                    })
                    .collect()
            }
        }
        WorkspacePanel::Git => match &state.git {
            Some(git) => vec![format!(
                "branch={} · {}",
                git.branch.as_deref().unwrap_or("-"),
                git.summary
            )],
            None => vec!["Git not connected — status placeholder.".to_string()],
        },
        WorkspacePanel::Diagnostics => {
            if state.diagnostics.is_empty() {
                vec!["No workspace diagnostics.".to_string()]
            } else {
                state
                    .diagnostics
                    .iter()
                    .map(|item| {
                        format!(
                            "[{}] {}{}",
                            item.severity,
                            item.message,
                            item.path
                                .as_ref()
                                .map(|path| format!(" · {path}"))
                                .unwrap_or_default()
                        )
                    })
                    .collect()
            }
        }
        _ => vec![panel.id().to_string()],
    }
}

fn placeholder_for(panel: WorkspacePanel) -> &'static str {
    match panel {
        WorkspacePanel::ProjectExplorer => "No project tree yet — stub explorer.",
        WorkspacePanel::Editor => "No open files — source editing surface (stub).",
        WorkspacePanel::Terminal => "No terminal sessions — command surface (stub).",
        WorkspacePanel::Git => "Git not connected — status placeholder.",
        WorkspacePanel::Diagnostics => "No workspace diagnostics.",
        _ => "panel",
    }
}

/// Render the Coding Workspace shell into the right-side expansion panel.
pub fn render_coding_shell(
    ui: &mut egui::Ui,
    expansion: &WorkspaceExpansion,
    state: Option<&CodingState>,
) {
    ui.heading(expansion.kind.title());
    ui.label(format!(
        "Requested by capability · {}",
        expansion.capability.id()
    ));
    ui.label(format!("Expands from: {}", expansion.expands_from.as_str()));
    ui.weak("Shell panels only — editor / terminal / git tools are not wired yet.");
    ui.add_space(8.0);
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("coding_shell_scroll")
        .show(ui, |ui| {
            for panel in &expansion.panels {
                render_coding_panel(ui, *panel, state);
                ui.add_space(8.0);
            }
        });
}

fn render_coding_panel(ui: &mut egui::Ui, panel: WorkspacePanel, state: Option<&CodingState>) {
    ui.group(|ui| {
        ui.strong(panel.label());
        ui.add_space(4.0);
        for line in coding_panel_lines(panel, state) {
            if line.starts_with("No ") || line.contains("placeholder") || line.contains("(stub)") {
                ui.weak(line);
            } else {
                ui.label(line);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::{
        DiagnosticState, GitStatusState, OpenFileState, TerminalSessionState,
    };

    #[test]
    fn summary_includes_all_five_coding_panels() {
        let state = CodingState {
            selected_path: Some("src/main.rs".into()),
            explorer_entries: vec!["src/".into(), "src/main.rs".into()],
            open_files: vec![OpenFileState {
                path: "src/main.rs".into(),
                dirty: true,
            }],
            terminal_sessions: vec![TerminalSessionState {
                id: "term-1".into(),
                cwd: Some("/tmp/app".into()),
                last_command: Some("cargo check".into()),
            }],
            git: Some(GitStatusState {
                branch: Some("main".into()),
                summary: "clean".into(),
            }),
            diagnostics: vec![DiagnosticState {
                message: "unused import".into(),
                path: Some("src/main.rs".into()),
                severity: "warning".into(),
            }],
        };
        let summary = coding_shell_summary(&state);
        assert!(summary.contains("Project Explorer"));
        assert!(summary.contains("Editor"));
        assert!(summary.contains("Terminal"));
        assert!(summary.contains("Git"));
        assert!(summary.contains("Diagnostics"));
        assert!(summary.contains("src/main.rs"));
        assert!(summary.contains("cargo check"));
        assert!(summary.contains("branch=main"));
        assert!(summary.contains("unused import"));
    }
}
