//! Stable Knowledge API for Jaymi's indexed inventory.
//!
//! The Planner and tools never query SQLite directly. They use
//! [`KnowledgeStore`]. Discovery indexing and future providers publish through
//! the same API.

#![forbid(unsafe_code)]

mod collections;
mod path;
mod sqlite;
mod stats;
mod store;
mod types;

pub use collections::{Collection, CollectionId};
pub use path::normalize_path;
pub use sqlite::SqliteKnowledgeStore;
pub use stats::{CollectionStats, InventoryStats};
pub use store::KnowledgeStore;
pub use types::{
    KnowledgeItem, KnowledgeQuery, KnowledgeSort, PublishOutcome, RecentKind, ScanSummary,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use jaymi_core::Lifecycle;
    use jaymi_database::Database;

    fn boot_store(data: &std::path::Path) -> Arc<SqliteKnowledgeStore> {
        let mut db = Database::with_data_dir(data);
        db.initialize().unwrap();
        let mut store = SqliteKnowledgeStore::new(Arc::new(db));
        store.initialize().unwrap();
        Arc::new(store)
    }

    fn item(path: PathBuf, name: &str, ext: Option<&str>, size: u64, dir: bool) -> KnowledgeItem {
        let parent = path.parent().map(|p| p.to_path_buf());
        KnowledgeItem {
            path,
            filename: name.to_string(),
            extension: ext.map(|v| v.to_string()),
            size,
            created: Some(100),
            modified: Some(200),
            is_directory: dir,
            hidden: name.starts_with('.'),
            parent,
            first_discovered: None,
            last_indexed: None,
            last_modified: Some(200),
            last_verified: None,
            device_id: None,
            inode: None,
        }
    }

    #[test]
    fn every_knowledge_api_operation() {
        let data = temp_dir("knowledge-api-data");
        let root = temp_dir("knowledge-api-root");
        let downloads = root.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let pdf = downloads.join("report.pdf");
        let txt = downloads.join("notes.txt");
        let hidden = downloads.join(".secret");
        fs::write(&pdf, b"%PDF").unwrap();
        fs::write(&txt, b"hi").unwrap();
        fs::write(&hidden, b"x").unwrap();

        let store = boot_store(&data);
        let now = 1_700_000_000i64;

        // Publish directory + files.
        let root_item = item(normalize_path(&root).unwrap(), "knowledge-api-root", None, 0, true);
        let downloads_item = item(
            normalize_path(&downloads).unwrap(),
            "Downloads",
            None,
            0,
            true,
        );
        let mut pdf_item = item(normalize_path(&pdf).unwrap(), "report.pdf", Some("pdf"), 4, false);
        pdf_item.modified = Some(300);
        let mut txt_item = item(normalize_path(&txt).unwrap(), "notes.txt", Some("txt"), 2, false);
        txt_item.modified = Some(250);
        txt_item.created = Some(150);
        let hidden_item = item(normalize_path(&hidden).unwrap(), ".secret", None, 1, false);

        assert_eq!(store.publish(&root_item, now).unwrap(), PublishOutcome::Inserted);
        assert_eq!(
            store.publish(&downloads_item, now).unwrap(),
            PublishOutcome::Inserted
        );
        assert_eq!(store.publish(&pdf_item, now).unwrap(), PublishOutcome::Inserted);
        assert_eq!(store.publish(&txt_item, now).unwrap(), PublishOutcome::Inserted);
        assert_eq!(
            store.publish(&hidden_item, now).unwrap(),
            PublishOutcome::Inserted
        );

        // find by path
        let found = store.get_by_path(&pdf).unwrap().expect("pdf");
        assert_eq!(found.filename, "report.pdf");
        assert!(store.get_by_path(&root.join("missing.txt")).unwrap().is_none());

        // existence
        assert!(store.exists(&pdf).unwrap());
        assert!(!store.exists(&root.join("missing.txt")).unwrap());

        // find by name
        let by_name = store.find_by_name("report", Some(10)).unwrap();
        assert!(by_name.iter().any(|item| item.filename == "report.pdf"));

        // recent
        let recent = store.recent(RecentKind::Modified, 10).unwrap();
        assert_eq!(recent.first().map(|i| i.filename.as_str()), Some("report.pdf"));
        let recent_created = store.recent(RecentKind::Created, 10).unwrap();
        assert!(!recent_created.is_empty());

        // by extension
        let pdfs = store.by_extension("pdf", Some(10)).unwrap();
        assert_eq!(pdfs.len(), 1);
        assert_eq!(pdfs[0].filename, "report.pdf");

        // collections
        let collections = store.list_collections().unwrap();
        assert!(
            collections
                .iter()
                .any(|c| c.id == CollectionId::Downloads),
            "{collections:?}"
        );
        let downloads_col = store.resolve_collection("downloads").unwrap().unwrap();
        let in_downloads = store
            .items_in_collection("downloads", true, Some(100))
            .unwrap();
        assert!(in_downloads.iter().any(|i| i.filename == "report.pdf"));
        assert_eq!(downloads_col.name, "Downloads");

        // statistics
        let stats = store.stats().unwrap();
        assert!(stats.files >= 3);
        assert!(stats.folders >= 2);
        assert!(stats.query_count >= 1);
        let cstats = store.collection_stats().unwrap();
        assert!(cstats.collection_count >= 1);

        // verify / update / remove / items_under_root / record_scan
        assert_eq!(
            store.publish(&pdf_item, now + 1).unwrap(),
            PublishOutcome::Verified
        );
        pdf_item.size = 99;
        assert_eq!(
            store.publish(&pdf_item, now + 2).unwrap(),
            PublishOutcome::Updated
        );
        store.verify(&txt, now + 3).unwrap();

        let under = store.items_under_root(&root).unwrap();
        assert!(under.len() >= 5);

        let scan_id = store
            .record_scan(&ScanSummary {
                started_at: now,
                finished_at: now + 1,
                duration_ms: 10,
                roots: vec![normalize_path(&root).unwrap().to_string_lossy().into_owned()],
                files_seen: 3,
                folders_seen: 2,
                files_added: 5,
                files_updated: 1,
                files_removed: 0,
                files_unchanged: 0,
                status: "completed".into(),
            })
            .unwrap();
        assert!(scan_id > 0);

        store.remove(&hidden).unwrap();
        assert!(!store.exists(&hidden).unwrap());
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-knowledge-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
