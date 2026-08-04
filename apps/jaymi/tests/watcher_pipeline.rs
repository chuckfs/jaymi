//! Integration tests for Layer 1 Slice 3 — filesystem watching.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_config::Config;
use jaymi_core::Lifecycle;
use jaymi_discovery::{FilesystemWatcher, WatcherStatus};
use jaymi_knowledge::{KnowledgeQuery, KnowledgeStore, SqliteKnowledgeStore};

#[test]
fn watcher_applies_create_modify_delete_without_manual_rescan() {
    let data_dir = temp_dir("watch-data");
    let root = temp_dir("watch-root");

    let mut config = Config::with_data_dir(&data_dir);
    config.initialize().unwrap();
    config.settings_mut().indexing_enabled = true;
    config.settings_mut().discovery_roots = vec![root.display().to_string()];
    config.save().unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let watcher = app
        .container()
        .resolve::<Arc<FilesystemWatcher>>()
        .expect("watcher");
    assert_eq!(watcher.diagnostics().status, WatcherStatus::Watching);
    let watched = watcher.diagnostics().watched_directories;
    assert!(
        watched.iter().any(|path| {
            path == &root || path.canonicalize().ok().as_ref() == root.canonicalize().ok().as_ref()
        }),
        "expected watched root {root:?}, got {watched:?}"
    );

    // Seed inventory so deletes/updates have a baseline under the root.
    app.index_root(&root).expect("initial index");

    let created = root.join("auto.txt");
    fs::write(&created, "hello").unwrap();
    wait_for_inventory(&app, &root, "auto.txt", true);

    fs::write(&created, "updated-content").unwrap();
    wait_for_size(&app, &created, 15);

    fs::remove_file(&created).unwrap();
    wait_for_inventory(&app, &root, "auto.txt", false);

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot.subsystem("Watcher Status").expect("watcher row");
    assert!(row.detail.contains("status=watching"));
    assert!(row.detail.contains("watched="));
    assert!(row.detail.contains("queued="));
    assert!(row.detail.contains("last_event="));
}

#[test]
fn watcher_idle_without_configured_roots() {
    let data_dir = temp_dir("watch-idle-data");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let watcher = app
        .container()
        .resolve::<Arc<FilesystemWatcher>>()
        .expect("watcher");
    assert_eq!(watcher.diagnostics().status, WatcherStatus::Idle);

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot.subsystem("Watcher Status").expect("watcher row");
    assert!(row.detail.contains("status=idle"));
}

fn wait_for_inventory(app: &Application, root: &std::path::Path, name: &str, present: bool) {
    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");
    let watcher = app
        .container()
        .resolve::<Arc<FilesystemWatcher>>()
        .expect("watcher");
    let prefix = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned();

    for _ in 0..40 {
        let _ = watcher.process_pending();
        let items = knowledge
            .query(KnowledgeQuery {
                path_prefix: Some(prefix.clone()),
                ..Default::default()
            })
            .unwrap();
        let found = items.iter().any(|item| item.filename == name);
        if found == present {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let watcher_diag = watcher.diagnostics();
    let items = knowledge
        .query(KnowledgeQuery {
            path_prefix: Some(prefix),
            ..Default::default()
        })
        .unwrap();
    panic!(
        "timed out waiting for inventory present={present} name={name}; last_event={:?}; queued={}; items={:?}",
        watcher_diag.last_event,
        watcher_diag.queued_updates,
        items.iter().map(|item| item.filename.clone()).collect::<Vec<_>>()
    );
}

fn wait_for_size(app: &Application, path: &std::path::Path, expected_size: u64) {
    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");
    let watcher = app
        .container()
        .resolve::<Arc<FilesystemWatcher>>()
        .expect("watcher");
    let key = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();

    for _ in 0..40 {
        let _ = watcher.process_pending();
        let items = knowledge
            .query(KnowledgeQuery {
                path_prefix: Some(key.clone()),
                ..Default::default()
            })
            .unwrap();
        if items
            .iter()
            .any(|item| item.path.to_string_lossy() == key && item.size == expected_size)
        {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "timed out waiting for size={expected_size} path={}",
        path.display()
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-watch-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
