//! Integration tests for Layer 1 — discovery and incremental indexing.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::Lifecycle;
use jaymi_database::Database;
use jaymi_knowledge::{KnowledgeQuery, KnowledgeStore, SqliteKnowledgeStore};

#[test]
fn recursive_discovery_and_metadata_extraction() {
    let data_dir = temp_dir("discovery-meta-data");
    let root = temp_dir("discovery-meta-root");
    fs::create_dir_all(root.join("docs").join("nested")).unwrap();
    fs::write(root.join("docs").join("nested").join("notes.txt"), "hello").unwrap();
    fs::write(root.join("docs").join(".secret"), "x").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let response = app.index_root(&root).expect("index");
    assert!(!response.blocked);
    assert!(response.content.contains("added="));
    assert_eq!(response.tool_id.as_deref(), Some("scan_filesystem"));

    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");
    let items = knowledge
        .query(KnowledgeQuery {
            path_prefix: Some(canonical_string(&root)),
            ..Default::default()
        })
        .expect("query");

    assert!(items.iter().any(|item| item.filename == "notes.txt"));
    assert!(items
        .iter()
        .any(|item| item.filename == ".secret" && item.hidden));
    let notes = items
        .iter()
        .find(|item| item.filename == "notes.txt")
        .unwrap();
    assert_eq!(notes.extension.as_deref(), Some("txt"));
    assert!(!notes.is_directory);
    assert_eq!(notes.size, 5);
    assert!(notes.parent.is_some());
    assert!(items
        .iter()
        .any(|item| item.is_directory && item.filename == "nested"));
}

#[test]
fn persistence_survives_restart() {
    let data_dir = temp_dir("discovery-persist-data");
    let root = temp_dir("discovery-persist-root");
    fs::create_dir_all(root.join("a")).unwrap();
    fs::write(root.join("a").join("file.md"), "# hi").unwrap();

    {
        let app = Application::boot_with_data_dir(&data_dir).expect("boot");
        app.index_root(&root).expect("index");
        let mut app = app;
        app.shutdown().expect("shutdown");
    }

    let app = Application::boot_with_data_dir(&data_dir).expect("reboot");
    let response = app.discover_inventory().expect("discover");
    assert!(!response.blocked);
    assert!(
        response
            .entries
            .iter()
            .any(|entry| entry.name == "file.md"),
        "inventory should survive restart: {:?}",
        response.entries
    );

    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");
    let stats = knowledge.stats().expect("stats");
    assert!(stats.files >= 1);
    assert!(stats.folders >= 1);

    let db = app
        .container()
        .resolve::<Arc<Database>>()
        .expect("database");
    assert!(db.is_connected());
}

#[test]
fn discover_answers_from_database_without_live_scan() {
    let data_dir = temp_dir("discovery-dbonly-data");
    let root = temp_dir("discovery-dbonly-root");
    fs::write(root.join("tracked.txt"), "persist me").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    fs::remove_dir_all(&root).unwrap();
    assert!(!root.exists());

    let response = app.discover_inventory().expect("discover");
    assert!(!response.blocked);
    assert!(
        response
            .entries
            .iter()
            .any(|entry| entry.name == "tracked.txt"),
        "query must use DB, not filesystem: {}",
        response.content
    );
    assert_eq!(response.tool_id.as_deref(), Some("query_inventory"));

    let snapshot = app
        .diagnostics_from_response(Some(response))
        .expect("diagnostics");
    let index = snapshot.subsystem("Index Status").unwrap();
    assert!(index.detail.contains("files="));
    assert!(index.detail.contains("db_bytes="));
    assert!(index.detail.contains("last_scan="));
    assert!(index.detail.contains("added="));
    assert!(index.detail.contains("updated="));
    assert!(index.detail.contains("removed="));

    let queries = snapshot.subsystem("Discovery Queries").unwrap();
    assert!(queries.detail.contains("query_count="));
    assert!(queries.detail.contains("last_query="));
    assert!(queries.detail.contains("last_rows="));
    assert!(queries.detail.contains("last_duration_ms="));
    let query_count = parse_counter(&queries.detail, "query_count=");
    assert!(query_count >= 1, "expected query stats: {}", queries.detail);
}

#[test]
fn indexing_disabled_blocks_scan() {
    let data_dir = temp_dir("discovery-disabled-data");
    let root = temp_dir("discovery-disabled-root");
    fs::write(root.join("a.txt"), "x").unwrap();

    let mut config = jaymi_config::Config::with_data_dir(&data_dir);
    config.initialize().unwrap();
    config.settings_mut().indexing_enabled = false;
    config.save().unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let err = app
        .index_root(&root)
        .expect_err("scan should fail when indexing disabled");
    assert!(err.message().contains("indexing is disabled"));
}

#[test]
fn incremental_indexing_detects_add_update_delete_and_noop() {
    let data_dir = temp_dir("incr-it-data");
    let root = temp_dir("incr-it-root");
    fs::write(root.join("keep.txt"), "v1").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let first = app.index_root(&root).expect("first scan");
    assert!(first.content.contains("added="));
    assert!(first.content.contains("updated=0"));
    assert!(first.content.contains("removed=0"));

    let second = app.index_root(&root).expect("noop scan");
    assert!(second.content.contains("added=0"), "{}", second.content);
    assert!(second.content.contains("updated=0"), "{}", second.content);
    assert!(second.content.contains("removed=0"), "{}", second.content);
    let unchanged = parse_counter(&second.content, "unchanged=");
    assert!(unchanged >= 1, "noop scan should verify unchanged rows: {}", second.content);

    thread::sleep(Duration::from_millis(25));
    fs::write(root.join("keep.txt"), "v2-changed").unwrap();
    fs::write(root.join("new.txt"), "fresh").unwrap();
    let third = app.index_root(&root).expect("change scan");
    assert!(
        parse_counter(&third.content, "added=") >= 1,
        "expected additions: {}",
        third.content
    );
    assert!(
        parse_counter(&third.content, "updated=") >= 1,
        "expected updates: {}",
        third.content
    );

    fs::remove_file(root.join("new.txt")).unwrap();
    let fourth = app.index_root(&root).expect("delete scan");
    assert!(
        parse_counter(&fourth.content, "removed=") >= 1,
        "expected removals: {}",
        fourth.content
    );

    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");
    let keep = knowledge
        .get_by_path(&root.join("keep.txt"))
        .unwrap()
        .expect("keep.txt");
    assert!(keep.first_discovered.is_some());
    assert!(keep.last_indexed.is_some());
    assert!(keep.last_verified.is_some());
    assert!(keep.last_modified.is_some());
    assert_eq!(keep.size, 10);

    let stats = knowledge.stats().unwrap();
    assert_eq!(stats.last_removed, Some(1));

    let snapshot = app.diagnostics().expect("diagnostics");
    let detail = &snapshot.subsystem("Index Status").unwrap().detail;
    assert!(detail.contains("added="));
    assert!(detail.contains("updated="));
    assert!(detail.contains("removed="));
    assert!(detail.contains("unchanged="));
}

#[test]
fn knowledge_queries_answer_from_database_only() {
    use jaymi_core::{DiscoveryQueryKind, UserRequest};
    use jaymi_planner::Planner;

    let data_dir = temp_dir("knowledge-q-data");
    let root = temp_dir("knowledge-q-root");
    let docs = root.join("docs");
    let empty = root.join("empty_dir");
    fs::create_dir_all(docs.join("nested")).unwrap();
    fs::create_dir_all(&empty).unwrap();
    fs::write(docs.join("report.pdf"), b"%PDF-fake").unwrap();
    fs::write(docs.join("notes.txt"), "small").unwrap();
    fs::write(docs.join(".hidden"), "secret").unwrap();
    fs::write(root.join("big.bin"), vec![0u8; 4096]).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    // Prove queries are DB-backed: delete the live tree after indexing.
    fs::remove_dir_all(&root).unwrap();
    assert!(!root.exists());

    let planner = app.container().resolve::<Planner>().expect("planner");

    let all = app.discover_inventory().expect("all");
    assert!(all.content.contains("database only"));
    assert!(all.entries.iter().any(|e| e.name == "report.pdf"));

    let pdfs = app
        .discover_query(DiscoveryQueryKind::ByExtension {
            extension: "pdf".into(),
        })
        .expect("pdf");
    assert!(pdfs.entries.iter().all(|e| e.name.ends_with(".pdf")));
    assert!(pdfs.entries.iter().any(|e| e.name == "report.pdf"));
    assert!(!pdfs.entries.iter().any(|e| e.name == "notes.txt"));

    let folder = app
        .discover_query(DiscoveryQueryKind::ByFolder {
            path: docs.clone(),
            immediate: true,
        })
        .expect("folder");
    assert!(folder.entries.iter().any(|e| e.name == "report.pdf"));
    assert!(!folder.entries.iter().any(|e| e.name == "big.bin"));

    let under = planner
        .handle(UserRequest::new(format!(
            "files under {}",
            docs.display()
        )))
        .expect("under nl");
    assert_eq!(under.tool_id.as_deref(), Some("query_inventory"));
    assert!(under.entries.iter().any(|e| e.name == "report.pdf"));

    let recent_mod = planner
        .handle(UserRequest::new("recently modified files"))
        .expect("recent mod");
    assert!(recent_mod.entries.iter().any(|e| e.name == "big.bin"));

    let recent_created = app
        .discover_query(DiscoveryQueryKind::RecentlyCreated)
        .expect("recent created");
    assert!(!recent_created.entries.is_empty());

    let largest = app
        .discover_query(DiscoveryQueryKind::Largest)
        .expect("largest");
    assert_eq!(largest.entries.first().map(|e| e.name.as_str()), Some("big.bin"));

    let hidden = app
        .discover_query(DiscoveryQueryKind::Hidden)
        .expect("hidden");
    assert!(hidden.entries.iter().any(|e| e.name == ".hidden"));

    let empties = app
        .discover_query(DiscoveryQueryKind::EmptyFolders)
        .expect("empty");
    assert!(
        empties.entries.iter().any(|e| e.name == "empty_dir"),
        "expected empty_dir in {:?}",
        empties.entries
    );

    let by_ext_nl = planner
        .handle(UserRequest::new("*.pdf"))
        .expect("star pdf");
    assert!(by_ext_nl.entries.iter().any(|e| e.name == "report.pdf"));

    let snapshot = app.diagnostics().expect("diagnostics");
    let queries = snapshot.subsystem("Discovery Queries").unwrap();
    assert!(parse_counter(&queries.detail, "query_count=") >= 8);
    assert!(queries.detail.contains("last_query="));
    assert_ne!(
        queries
            .detail
            .split_whitespace()
            .find_map(|token| token.strip_prefix("last_query="))
            .unwrap_or("-"),
        "-"
    );
}

#[test]
fn collections_generate_and_answer_without_manual_paths() {
    use jaymi_core::{DiscoveryQueryKind, UserRequest};
    use jaymi_planner::Planner;

    let data_dir = temp_dir("collections-it-data");
    let root = temp_dir("collections-it-root");
    let downloads = root.join("Downloads");
    let projects = root.join("Projects");
    fs::create_dir_all(downloads.join("inbox")).unwrap();
    fs::create_dir_all(projects.join("alpha")).unwrap();
    fs::write(downloads.join("setup.dmg"), b"bin").unwrap();
    fs::write(projects.join("alpha").join("main.rs"), b"fn main() {}").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let listed = app.list_collections().expect("list collections");
    assert!(!listed.blocked);
    assert!(
        listed.entries.iter().any(|entry| entry.name == "Downloads"),
        "expected Downloads in {:?}",
        listed.entries
    );
    assert!(
        listed.entries.iter().any(|entry| entry.name == "Projects"),
        "expected Projects in {:?}",
        listed.entries
    );

    // DB-only: delete the live tree after indexing.
    fs::remove_dir_all(&root).unwrap();

    let planner = app.container().resolve::<Planner>().expect("planner");
    let downloads_response = planner
        .handle(UserRequest::new("What's in Downloads?"))
        .expect("downloads nl");
    assert_eq!(
        downloads_response.tool_id.as_deref(),
        Some("query_inventory")
    );
    assert!(downloads_response.content.contains("database only"));
    assert!(downloads_response
        .entries
        .iter()
        .any(|entry| entry.name == "setup.dmg"));

    let projects_response = planner
        .handle(UserRequest::new("What projects do I have?"))
        .expect("projects nl");
    assert!(projects_response
        .entries
        .iter()
        .any(|entry| entry.name == "alpha"));

    let structured = app
        .discover_query(DiscoveryQueryKind::ByCollection {
            name: "downloads".into(),
            immediate: true,
        })
        .expect("structured downloads");
    assert!(structured
        .entries
        .iter()
        .any(|entry| entry.name == "setup.dmg" || entry.name == "inbox"));

    let snapshot = app.diagnostics().expect("diagnostics");
    let collections = snapshot.subsystem("Collections").unwrap();
    assert!(parse_counter(&collections.detail, "collections=") >= 2);
    assert!(collections.detail.contains("names="));
    assert!(collections.detail.contains("Downloads"));
}

fn canonical_string(path: &std::path::Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn parse_counter(message: &str, key: &str) -> u64 {
    message
        .split_whitespace()
        .find_map(|token| token.strip_prefix(key)?.parse().ok())
        .unwrap_or(0)
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-discovery-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
