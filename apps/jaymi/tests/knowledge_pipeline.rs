//! Integration tests for Layer 1 Slice 6 — Knowledge API.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_knowledge::{
    KnowledgeQuery, KnowledgeStore, RecentKind, SqliteKnowledgeStore,
};

#[test]
fn knowledge_api_is_single_interface_after_boot() {
    let data_dir = temp_dir("knowledge-boot-data");
    let root = temp_dir("knowledge-boot-root");
    let downloads = root.join("Downloads");
    fs::create_dir_all(&downloads).unwrap();
    fs::write(downloads.join("a.pdf"), b"%PDF").unwrap();
    fs::write(downloads.join("b.txt"), b"text").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");

    let pdf = downloads.join("a.pdf");
    assert!(knowledge.exists(&pdf).unwrap());
    let by_path = knowledge.get_by_path(&pdf).unwrap().expect("pdf item");
    assert_eq!(by_path.filename, "a.pdf");

    let by_name = knowledge.find_by_name("a.pdf", Some(10)).unwrap();
    assert!(by_name.iter().any(|item| item.filename == "a.pdf"));

    let recent = knowledge.recent(RecentKind::Modified, 10).unwrap();
    assert!(!recent.is_empty());

    let pdfs = knowledge.by_extension("pdf", Some(10)).unwrap();
    assert_eq!(pdfs.len(), 1);

    let collections = knowledge.list_collections().unwrap();
    assert!(collections.iter().any(|c| c.name == "Downloads"));
    let items = knowledge
        .items_in_collection("downloads", true, Some(100))
        .unwrap();
    assert!(items.iter().any(|item| item.filename == "a.pdf"));

    let stats = knowledge.stats().unwrap();
    assert!(stats.files >= 2);
    let cstats = knowledge.collection_stats().unwrap();
    assert!(cstats.collection_count >= 1);

    // General query still works without exposing SQL types to the app.
    let under = knowledge
        .query(KnowledgeQuery {
            path_prefix: Some(
                root.canonicalize()
                    .unwrap_or(root.clone())
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(under.len() >= 3);

    // Planner discover path still goes through Knowledge via query_inventory.
    let response = app.discover_inventory().expect("discover");
    assert_eq!(response.tool_id.as_deref(), Some("query_inventory"));
    assert!(response.entries.iter().any(|entry| entry.name == "a.pdf"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-knowledge-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
