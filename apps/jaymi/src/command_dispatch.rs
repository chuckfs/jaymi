//! Dispatch registered command ids to Application / UI actions.
//!
//! The palette never hardcodes commands — it resolves ids through
//! [`CommandRegistry`] and this host-side dispatcher.

use jaymi_capabilities::CodingBottomTab;
use jaymi_commands::ids;
use jaymi_core::{JaymiError, JaymiResult, UserRequest};
use jaymi_memory::MemoryQuery;

use crate::boot::Application;

/// Execute a registered command id with an optional argument.
pub fn dispatch_command(
    app: &Application,
    id: &str,
    argument: Option<&str>,
) -> JaymiResult<CommandDispatchEffect> {
    match id {
        ids::OPEN_FILE => Ok(CommandDispatchEffect::PickAndOpenFile),
        ids::OPEN_FOLDER => Ok(CommandDispatchEffect::PickAndOpenFolder),
        ids::SEARCH_FILES => {
            let query = required_argument(argument, "Search Files")?;
            search_files(app, query)
        }
        ids::QUICK_OPEN => Ok(CommandDispatchEffect::OpenQuickOpen),
        ids::FIND_IN_FILES => {
            ensure_coding(app)?;
            if let Some(query) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                app.with_coding_state(|coding| {
                    coding.search.query = query.to_string();
                })?;
            }
            app.with_coding_state(|coding| {
                coding.bottom_tab = CodingBottomTab::Search;
            })?;
            Ok(CommandDispatchEffect::None)
        }
        ids::TOGGLE_EXPLORER => {
            ensure_coding(app)?;
            app.with_coding_state(|coding| {
                coding.explorer_visible = !coding.explorer_visible;
            })?;
            Ok(CommandDispatchEffect::None)
        }
        ids::TOGGLE_TERMINAL => toggle_bottom_tab(app, CodingBottomTab::Terminal),
        ids::NEW_TERMINAL => {
            ensure_coding(app)?;
            let title = argument
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            app.create_coding_terminal(title)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::KILL_TERMINAL => {
            ensure_coding(app)?;
            let session_id = active_terminal_id(app)?
                .ok_or_else(|| JaymiError::new("no active terminal to close"))?;
            app.kill_coding_terminal(&session_id)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::RENAME_TERMINAL => {
            ensure_coding(app)?;
            let title = required_argument(argument, "Rename Terminal")?;
            let session_id = active_terminal_id(app)?
                .ok_or_else(|| JaymiError::new("no active terminal to rename"))?;
            app.rename_coding_terminal(&session_id, title)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::TOGGLE_GIT => toggle_bottom_tab(app, CodingBottomTab::Git),
        ids::TOGGLE_PROBLEMS => toggle_bottom_tab(app, CodingBottomTab::Diagnostics),
        ids::CREATE_FILE => {
            ensure_coding(app)?;
            let parent = explorer_parent(app)?;
            app.begin_coding_new_file(&parent)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::CREATE_FOLDER => {
            ensure_coding(app)?;
            let parent = explorer_parent(app)?;
            app.begin_coding_new_folder(&parent)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::SAVE => {
            app.save_active_coding_file()?;
            Ok(CommandDispatchEffect::None)
        }
        ids::CLOSE_EDITOR => {
            let path = app.with_coding_state(|coding| {
                coding.active_tab_path().map(str::to_string)
            })?;
            let Some(path) = path else {
                return Err(JaymiError::new("no active editor tab"));
            };
            app.close_coding_tab(&path)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::CLOSE_WORKSPACE => Ok(CommandDispatchEffect::CloseWorkspace),
        ids::OPEN_CODING => {
            app.start_coding_project()?;
            Ok(CommandDispatchEffect::RefreshExperience)
        }
        ids::OPEN_RESEARCH => {
            app.start_research_workspace()?;
            Ok(CommandDispatchEffect::RefreshExperience)
        }
        ids::OPEN_CREATIVE => {
            app.start_creation_workspace()?;
            Ok(CommandDispatchEffect::RefreshExperience)
        }
        ids::INDEX_PROJECT => {
            let root = app
                .active_project_root_path()
                .ok_or_else(|| JaymiError::new("no active project to index"))?;
            let _ = app.index_root(root)?;
            Ok(CommandDispatchEffect::None)
        }
        ids::SEARCH_KNOWLEDGE => {
            let query = required_argument(argument, "Search Knowledge")?;
            search_knowledge(app, query)
        }
        ids::SEARCH_MEMORY => {
            let query = required_argument(argument, "Search Memory")?;
            search_memory(app, query)
        }
        ids::RUN_PLANNER => {
            let prompt = required_argument(argument, "Run Planner")?;
            let _ = app.handle_with_workspace(UserRequest::new(prompt))?;
            Ok(CommandDispatchEffect::RefreshExperience)
        }
        other => Err(JaymiError::new(format!("unknown command: {other}"))),
    }
}

/// Side effects the UI layer must apply after dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandDispatchEffect {
    /// Nothing further.
    None,
    /// Reload experience session from Application.
    RefreshExperience,
    /// Tear down Monaco and close the workspace.
    CloseWorkspace,
    /// Show native file picker then open the chosen file.
    PickAndOpenFile,
    /// Show native folder picker then open as project.
    PickAndOpenFolder,
    /// Surface a search summary into the UI error/status line.
    Status(String),
    /// Open the Quick Open filename modal.
    OpenQuickOpen,
}

fn required_argument<'a>(argument: Option<&'a str>, label: &str) -> JaymiResult<&'a str> {
    argument
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| JaymiError::new(format!("{label} requires an argument")))
}

fn ensure_coding(app: &Application) -> JaymiResult<()> {
    let kind = app.experience()?.active_workspace_kind();
    if kind != Some(jaymi_capabilities::WorkspaceKind::Coding) {
        app.start_coding_project()?;
    }
    Ok(())
}

fn toggle_bottom_tab(app: &Application, tab: CodingBottomTab) -> JaymiResult<CommandDispatchEffect> {
    ensure_coding(app)?;
    app.with_coding_state(|coding| {
        coding.bottom_tab = if coding.bottom_tab == tab {
            CodingBottomTab::Hidden
        } else {
            tab
        };
    })?;
    Ok(CommandDispatchEffect::None)
}

fn active_terminal_id(app: &Application) -> JaymiResult<Option<String>> {
    app.with_coding_state(|coding| {
        coding
            .active_terminal_id
            .clone()
            .or_else(|| coding.terminal_sessions.first().map(|session| session.id.clone()))
    })
}

fn explorer_parent(app: &Application) -> JaymiResult<String> {
    app.with_coding_state(|coding| {
        if let Some(selected) = &coding.explorer.selected_path {
            let path = std::path::Path::new(selected);
            if path.is_dir() {
                return selected.clone();
            }
            if let Some(parent) = path.parent() {
                let parent = parent.to_string_lossy().into_owned();
                if !parent.is_empty() {
                    return parent;
                }
            }
        }
        coding
            .explorer
            .project_root
            .clone()
            .unwrap_or_else(|| ".".into())
    })
}

/// Seed the Find in Files panel with a filename query and switch to it.
///
/// Replaces the previous behavior of opening the first hit directly — the
/// Search panel gives a reviewable results list before the user opens
/// anything.
fn search_files(app: &Application, query: &str) -> JaymiResult<CommandDispatchEffect> {
    ensure_coding(app)?;
    app.with_coding_state(|coding| {
        coding.search.query = query.to_string();
        coding.search.filename_only = true;
    })?;
    app.run_coding_search_from_panel()?;
    let count = app
        .with_coding_state(|coding| coding.search.results.len())
        .unwrap_or(0);
    Ok(CommandDispatchEffect::Status(format!(
        "Search Files: {count} hit(s) for “{query}”"
    )))
}

fn search_knowledge(app: &Application, query: &str) -> JaymiResult<CommandDispatchEffect> {
    let project_id = app
        .active_project_id()
        .ok_or_else(|| JaymiError::new("no active project for knowledge search"))?;
    let hits = app.search_project_knowledge(&project_id, query, Some(10))?;
    Ok(CommandDispatchEffect::Status(format!(
        "Search Knowledge: {} hits for “{query}”",
        hits.len()
    )))
}

fn search_memory(app: &Application, query: &str) -> JaymiResult<CommandDispatchEffect> {
    let records = app.retrieve_memory(&MemoryQuery {
        text: Some(query.to_string()),
        project_id: app.active_project_id(),
        limit: Some(10),
        ..MemoryQuery::default()
    })?;
    Ok(CommandDispatchEffect::Status(format!(
        "Search Memory: {} hits for “{query}”",
        records.len()
    )))
}
