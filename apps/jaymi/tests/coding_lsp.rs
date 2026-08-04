//! Coding Language Server — hover / completion / diagnostics through
//! Planner → language_server → LSP Provider (Rust Analyzer / mock).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_project_engine::{CreateProjectRequest, ProjectType};
use jaymi_providers::LSP_PROVIDER_ID;
use jaymi_tools::LANGUAGE_SERVER_TOOL_ID;

#[test]
fn lsp_hover_completion_and_diagnostics_through_planner() {
    let data_dir = temp_dir("lsp-data");
    let root = temp_dir("lsp-root");
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("lib.rs");
    let content = "fn helper() {}\nfn main() {\n    let x = BAD_IDENT;\n    helper();\n}\n";
    fs::write(&file, content).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:lsp".into()),
            name: "LSP Demo".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");
    app.open_coding_file(file.to_str().unwrap())
        .expect("open file");

    let opened = app
        .coding_lsp_did_open(file.to_str().unwrap(), content)
        .expect("did_open");
    assert!(
        opened
            .lsp_diagnostics
            .iter()
            .any(|diag| diag.message.contains("BAD_IDENT")),
        "did_open should publish BAD_IDENT diagnostic, got {:?}",
        opened.lsp_diagnostics
    );

    let hover = app
        .coding_lsp_hover(file.to_str().unwrap(), 3, 6)
        .expect("hover");
    assert!(!hover.blocked);
    assert_eq!(hover.tool_id.as_deref(), Some(LANGUAGE_SERVER_TOOL_ID));
    assert_eq!(hover.provider_id.as_deref(), Some(LSP_PROVIDER_ID));
    assert_eq!(
        hover.capability.map(|capability| capability.id()),
        Some("code")
    );
    assert!(
        hover
            .lsp_hover
            .as_ref()
            .map(|item| item.contents.contains("helper"))
            .unwrap_or(false),
        "expected hover on helper, got {:?}",
        hover.lsp_hover
    );

    let completion = app
        .coding_lsp_completion(file.to_str().unwrap(), 2, 8)
        .expect("completion");
    assert!(!completion.blocked);
    assert_eq!(completion.tool_id.as_deref(), Some(LANGUAGE_SERVER_TOOL_ID));
    assert!(
        !completion.lsp_completions.is_empty(),
        "expected completion items"
    );

    let diagnostics = app
        .coding_lsp_diagnostics(Some(file.to_str().unwrap()))
        .expect("diagnostics");
    assert!(!diagnostics.blocked);
    assert!(
        diagnostics
            .lsp_diagnostics
            .iter()
            .any(|diag| diag.message.contains("BAD_IDENT")),
        "expected BAD_IDENT diagnostic, got {:?}",
        diagnostics.lsp_diagnostics
    );

    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    assert!(
        coding
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("BAD_IDENT")),
        "CodingState should mirror LSP diagnostics"
    );
}

#[test]
fn lsp_definition_rename_and_references() {
    let data_dir = temp_dir("lsp-nav-data");
    let root = temp_dir("lsp-nav-root");
    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/main.rs");
    let content = "fn greet() {}\nfn main() { greet(); }\n";
    fs::write(&file, content).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:lsp-nav".into()),
            name: "LSP Nav".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");
    app.open_coding_file(file.to_str().unwrap()).expect("open");

    let path = file.to_str().unwrap();
    let definition = app.coding_lsp_definition(path, 1, 13).expect("definition");
    assert!(!definition.lsp_definitions.is_empty());

    let references = app.coding_lsp_references(path, 1, 13).expect("references");
    assert!(references.lsp_references.len() >= 2);

    let rename = app
        .coding_lsp_rename(path, 1, 13, "say_hello")
        .expect("rename");
    assert!(
        rename
            .lsp_edits
            .iter()
            .any(|edit| edit.new_text == "say_hello"),
        "expected rename edits, got {:?}",
        rename.lsp_edits
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
