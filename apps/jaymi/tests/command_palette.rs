//! Command Palette + CommandRegistry integration tests.

use jaymi::command_dispatch::{dispatch_command, CommandDispatchEffect};
use jaymi::Application;
use jaymi_commands::{builtin_descriptors, ids, CommandCategory, CommandDescriptor, CommandRegistry};
use jaymi_core::Lifecycle;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn boot_registers_builtin_commands_in_registry() {
    let data_dir = temp_dir("cmd-boot");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let registry = app.command_registry().expect("registry");
    assert!(registry.len() >= 19);
    for descriptor in builtin_descriptors() {
        assert!(
            registry.contains(&descriptor.id).unwrap(),
            "missing builtin {}",
            descriptor.id
        );
    }
    let hits = registry.search("save").expect("search");
    assert_eq!(hits[0].id, ids::SAVE);
}

#[test]
fn plugin_commands_can_register_after_boot() {
    let data_dir = temp_dir("cmd-plugin");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let registry = app.command_registry().expect("registry");
    registry
        .register(
            CommandDescriptor::plugin(
                "ext.test.echo",
                "Echo from Test Plugin",
                CommandCategory::Extension,
            )
            .with_keywords(["echo", "plugin"]),
        )
        .expect("register plugin command");
    assert!(registry.contains("ext.test.echo").unwrap());
    let hits = registry.search("echo").unwrap();
    assert!(hits.iter().any(|cmd| cmd.id == "ext.test.echo"));
}

#[test]
fn dispatch_toggle_explorer_and_save_paths() {
    use std::fs;

    let data_dir = temp_dir("cmd-dispatch");
    let root = temp_dir("cmd-dispatch-root");
    fs::create_dir_all(root.join("src")).unwrap();
    let main = root.join("src/main.rs");
    fs::write(&main, "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:cmd-dispatch".into()),
            name: "Cmd".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    let _ = app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    dispatch_command(&app, ids::TOGGLE_EXPLORER, None).expect("toggle");
    let visible = app
        .with_coding_state(|coding| coding.explorer_visible)
        .expect("state");
    assert!(!visible);

    let main_str = main.to_string_lossy().into_owned();
    app.open_coding_file(&main_str).expect("open file");
    app.set_coding_tab_content(&main_str, "fn main() { /* edited */ }\n".into())
        .expect("edit");
    dispatch_command(&app, ids::SAVE, None).expect("save");
    let dirty = app
        .with_coding_state(|coding| {
            coding
                .editors
                .buffer_by_path(&main_str)
                .map(|buffer| buffer.dirty)
        })
        .expect("state");
    assert_eq!(dirty, Some(false));

    let effect = dispatch_command(&app, ids::CLOSE_WORKSPACE, None).expect("close");
    assert_eq!(effect, CommandDispatchEffect::CloseWorkspace);
}

#[test]
fn quick_open_and_find_in_files_commands_are_registered() {
    let data_dir = temp_dir("cmd-search-commands");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let registry = app.command_registry().expect("registry");
    assert!(registry.contains(ids::QUICK_OPEN).unwrap());
    assert!(registry.contains(ids::FIND_IN_FILES).unwrap());
    let hits = registry.search("quick open").expect("search");
    assert!(hits.iter().any(|cmd| cmd.id == ids::QUICK_OPEN));
}

#[test]
fn find_in_files_seeds_search_panel_and_shows_it() {
    let data_dir = temp_dir("cmd-find-in-files");
    let root = temp_dir("cmd-find-in-files-root");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:find-in-files".into()),
            name: "FindInFiles".into(),
            description: None,
            root_directory: Some(root),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    dispatch_command(&app, ids::FIND_IN_FILES, Some("needle")).expect("find in files");
    let (query, bottom_tab) = app
        .with_coding_state(|coding| (coding.search.query.clone(), coding.bottom_tab))
        .expect("state");
    assert_eq!(query, "needle");
    assert_eq!(bottom_tab, jaymi_capabilities::CodingBottomTab::Search);
}

#[test]
fn search_files_populates_search_panel_results() {
    use std::fs;

    let data_dir = temp_dir("cmd-search-files");
    let root = temp_dir("cmd-search-files-root");
    let target = root.join("widget_alpha.rs");
    fs::write(&target, "pub struct WidgetAlpha;\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:search-files".into()),
            name: "SearchFiles".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");
    app.index_root(&root).expect("index");

    dispatch_command(&app, ids::SEARCH_FILES, Some("widget_alpha")).expect("search files");
    let (results, bottom_tab) = app
        .with_coding_state(|coding| (coding.search.results.clone(), coding.bottom_tab))
        .expect("state");
    assert!(!results.is_empty(), "expected Search Files to populate results");
    assert_eq!(bottom_tab, jaymi_capabilities::CodingBottomTab::Search);
}

#[test]
fn standalone_registry_rejects_duplicates_like_plugins_must() {
    let mut registry = CommandRegistry::new();
    registry.initialize().unwrap();
    registry.register_all(builtin_descriptors()).unwrap();
    let err = registry
        .register(CommandDescriptor::builtin(
            ids::OPEN_FILE,
            "Open File Dup",
            CommandCategory::File,
        ))
        .unwrap_err();
    assert!(err.message().contains("already registered"));
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-{label}-{nanos}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}
