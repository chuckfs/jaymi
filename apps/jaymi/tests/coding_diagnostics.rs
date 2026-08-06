//! Coding Diagnostics panel — read-only operational status for development.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{coding_shell_summary, Application};
use jaymi_project_engine::{CreateProjectRequest, ProjectType};

#[test]
fn coding_diagnostics_view_covers_required_sections() {
    let data_dir = temp_dir("coding-diagnostics-data");
    let root = temp_dir("coding-diagnostics-root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let project = app
        .create_project(&CreateProjectRequest {
            project_id: Some("project:coding-diagnostics".into()),
            name: "Diagnostics Demo".into(),
            description: None,
            root_directory: Some(root.clone()),
            project_type: Some(ProjectType::Code),
        })
        .expect("create");
    app.open_project(project.id.as_str()).expect("open");
    app.start_coding_project().expect("coding");

    // Exercise Planner so activity / timing / permissions populate.
    let _ = app
        .list_project_tree(&root)
        .expect("list tree through planner");

    let view = app.coding_diagnostics_view().expect("diagnostics view");
    let titles: Vec<_> = view
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();

    for expected in [
        "Active project",
        "Workspace state",
        "Current Execution Plan",
        "Review state",
        "Risk",
        "Permissions",
        "Planner pause state",
        "Pending approvals",
        "Completed approvals",
        "Execution summaries",
        "Approval history",
        "Planner activity",
        "Tool execution",
        "Provider status",
        "Indexing status",
        "Memory context",
        "Permission engine",
        "Current capability",
        "Timing metrics",
    ] {
        assert!(
            titles.contains(&expected),
            "missing section {expected}; got {titles:?}"
        );
    }

    let flat = view.summary_lines().join("\n");
    assert!(
        flat.contains("Diagnostics Demo"),
        "active project name missing: {flat}"
    );
    assert!(flat.contains("code"), "current capability missing: {flat}");
    assert!(
        flat.to_lowercase().contains("planner") || flat.contains("Operational"),
        "planner activity missing: {flat}"
    );
    assert!(
        flat.to_lowercase().contains("ms"),
        "timing metrics missing: {flat}"
    );
    assert!(
        flat.contains("providers") || flat.contains("lsp") || flat.contains("filesystem"),
        "provider status missing: {flat}"
    );
    assert!(
        flat.contains("indexing_enabled") || flat.contains("Index"),
        "indexing status missing: {flat}"
    );
    assert!(
        flat.contains("Memory") || flat.contains("memory"),
        "memory context missing: {flat}"
    );
    assert!(
        flat.contains("Current Execution Plan") || flat.contains("Planner pause state"),
        "execution inspection missing: {flat}"
    );
    assert!(
        flat.contains("mode=") || flat.contains("Permission engine") || flat.contains("Permissions"),
        "permissions missing: {flat}"
    );

    // The Diagnostics/Problems tab prioritizes the aggregated Problems list;
    // full operational sections remain available on `CodingDiagnosticsView`
    // (asserted above), with only a one-line footer surfaced in the summary.
    let coding = app
        .capability_state()
        .unwrap()
        .unwrap()
        .coding()
        .unwrap()
        .clone();
    let summary = coding_shell_summary(&coding, Some(&view));
    assert!(summary.contains("## Diagnostics"));
    assert!(
        summary.contains("problem(s)") || summary.contains("No problems"),
        "problems summary missing: {summary}"
    );
    assert!(
        summary.contains("Active project:"),
        "operational footer missing: {summary}"
    );
}

#[test]
fn coding_diagnostics_are_read_only_snapshot() {
    let data_dir = temp_dir("coding-diagnostics-ro");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.start_coding_project().expect("coding");

    let first = app.coding_diagnostics_view().expect("first");
    let second = app.coding_diagnostics_view().expect("second");
    assert_eq!(
        first.sections.len(),
        second.sections.len(),
        "diagnostics view should be a stable read-only snapshot shape"
    );
    assert!(first
        .sections
        .iter()
        .all(|section| !section.title.is_empty()));
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
