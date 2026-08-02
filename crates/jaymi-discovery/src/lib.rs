//! Filesystem discovery engine — Layer 1 indexing and watching.
//!
//! Walks configured roots, collects metadata only, and publishes into the
//! Knowledge API. Does not query SQLite directly.

#![forbid(unsafe_code)]

mod walk;
mod watch;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
use jaymi_knowledge::{
    KnowledgeItem, KnowledgeStore, PublishOutcome, ScanSummary, SqliteKnowledgeStore,
};

pub use jaymi_knowledge::{
    normalize_path, Collection, CollectionId, CollectionStats, InventoryStats,
};
pub use walk::{is_hidden_name, DiscoveredItem};
pub use watch::{FilesystemWatcher, WatcherDiagnostics, WatcherStatus};

const NAME: &str = "discovery_engine";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database", "knowledge"];

/// Result of a recursive discovery scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// Roots that were scanned.
    pub roots: Vec<PathBuf>,
    /// Filesystem entries visited during the scan.
    pub files_seen: u64,
    /// Folders visited during the scan.
    pub folders_seen: u64,
    /// Newly inserted inventory rows.
    pub added: u64,
    /// Existing rows whose metadata changed (including renames).
    pub updated: u64,
    /// Paths removed because they no longer exist under scanned roots.
    pub removed: u64,
    /// Rows confirmed unchanged (verified only).
    pub unchanged: u64,
    /// Wall-clock scan duration.
    pub duration: Duration,
    /// Unix seconds when the scan finished.
    pub finished_at: i64,
}

/// Discovery engine lifecycle and indexing operations.
pub struct DiscoveryEngine {
    initialized: bool,
    knowledge: Arc<SqliteKnowledgeStore>,
    configured_roots: Vec<PathBuf>,
    indexing_enabled: bool,
}

impl DiscoveryEngine {
    /// Create an uninitialized discovery engine bound to the Knowledge API.
    pub fn new(
        knowledge: Arc<SqliteKnowledgeStore>,
        configured_roots: Vec<PathBuf>,
        indexing_enabled: bool,
    ) -> Self {
        Self {
            initialized: false,
            knowledge,
            configured_roots,
            indexing_enabled,
        }
    }

    /// Whether indexing/scans are permitted by configuration.
    pub fn indexing_enabled(&self) -> bool {
        self.indexing_enabled
    }

    /// Configured discovery roots from settings.
    pub fn configured_roots(&self) -> &[PathBuf] {
        &self.configured_roots
    }

    /// Returns true after initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Shared knowledge store used for publishing scan results.
    pub fn knowledge(&self) -> &Arc<SqliteKnowledgeStore> {
        &self.knowledge
    }

    /// Incrementally scan roots and publish only changed metadata.
    ///
    /// Never reads file contents. Unchanged paths only bump verification.
    pub fn scan(&self, roots: &[PathBuf]) -> JaymiResult<ScanReport> {
        self.ensure_initialized()?;
        if !self.indexing_enabled {
            return Err(JaymiError::new(
                "indexing is disabled in configuration; enable indexing_enabled to scan",
            ));
        }
        if roots.is_empty() {
            return Err(JaymiError::new(
                "no discovery roots provided; set discovery_roots or pass an explicit path",
            ));
        }

        let started = Instant::now();
        let started_at = unix_now();
        let now = started_at;
        let mut files_seen = 0u64;
        let mut folders_seen = 0u64;
        let mut added = 0u64;
        let mut updated = 0u64;
        let mut removed = 0u64;
        let mut unchanged = 0u64;
        let mut normalized_roots = Vec::new();

        for root in roots {
            let normalized = normalize_path(root)?;
            if !normalized.exists() {
                return Err(JaymiError::new(format!(
                    "discovery root does not exist: {}",
                    normalized.display()
                )));
            }
            normalized_roots.push(normalized.clone());

            let existing = self.knowledge.items_under_root(&normalized)?;
            let mut by_path: HashMap<String, KnowledgeItem> = existing
                .into_iter()
                .map(|item| (item.path.to_string_lossy().into_owned(), item))
                .collect();
            let mut by_inode: HashMap<(u64, u64), String> = by_path
                .values()
                .filter_map(|item| match (item.device_id, item.inode) {
                    (Some(device), Some(inode)) => {
                        Some(((device, inode), item.path.to_string_lossy().into_owned()))
                    }
                    _ => None,
                })
                .collect();

            let walked = walk::walk_recursive(&normalized)?;
            let mut seen_paths: HashSet<String> = HashSet::new();

            for item in walked {
                let path_key = item.path.to_string_lossy().into_owned();
                seen_paths.insert(path_key.clone());
                if item.is_directory {
                    folders_seen += 1;
                } else {
                    files_seen += 1;
                }

                let knowledge_item = discovered_to_knowledge(&item);

                if let Some(existing) = by_path.get(&path_key).cloned() {
                    if metadata_changed(&existing, &item) {
                        let mut published = knowledge_item;
                        published.first_discovered = existing.first_discovered.or(Some(now));
                        match self.knowledge.publish(&published, now)? {
                            PublishOutcome::Updated | PublishOutcome::Inserted => updated += 1,
                            PublishOutcome::Verified => unchanged += 1,
                        }
                        by_path.insert(path_key.clone(), published.clone());
                        if let (Some(device), Some(inode)) = (published.device_id, published.inode)
                        {
                            by_inode.insert((device, inode), path_key);
                        }
                    } else {
                        self.knowledge.verify(&item.path, now)?;
                        unchanged += 1;
                    }
                    continue;
                }

                // Rename detection: same device/inode previously at another path.
                if let (Some(device), Some(inode)) = (item.device_id, item.inode) {
                    if let Some(old_path) = by_inode.get(&(device, inode)).cloned() {
                        if old_path != path_key {
                            if let Some(existing) = by_path.remove(&old_path) {
                                let mut published = knowledge_item.clone();
                                published.first_discovered =
                                    existing.first_discovered.or(Some(now));
                                self.knowledge.rename(
                                    PathBuf::from(&old_path).as_path(),
                                    &published,
                                    now,
                                )?;
                                updated += 1;
                                by_inode.insert((device, inode), path_key.clone());
                                by_path.insert(path_key.clone(), published);
                                seen_paths.insert(old_path);
                                continue;
                            }
                        }
                    }
                }

                match self.knowledge.publish(&knowledge_item, now)? {
                    PublishOutcome::Inserted => added += 1,
                    PublishOutcome::Updated => updated += 1,
                    PublishOutcome::Verified => unchanged += 1,
                }
                if let (Some(device), Some(inode)) =
                    (knowledge_item.device_id, knowledge_item.inode)
                {
                    by_inode.insert((device, inode), path_key.clone());
                }
                by_path.insert(path_key, knowledge_item);
            }

            let stale: Vec<PathBuf> = by_path
                .keys()
                .filter(|path| !seen_paths.contains(*path))
                .map(PathBuf::from)
                .collect();
            for path in stale {
                self.knowledge.remove(&path)?;
                removed += 1;
            }
        }

        let duration = started.elapsed();
        let finished_at = unix_now();
        let roots_strings: Vec<String> = normalized_roots
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();

        self.knowledge.record_scan(&ScanSummary {
            started_at,
            finished_at,
            duration_ms: duration.as_millis() as u64,
            roots: roots_strings,
            files_seen,
            folders_seen,
            files_added: added,
            files_updated: updated,
            files_removed: removed,
            files_unchanged: unchanged,
            status: "completed".to_string(),
        })?;

        jaymi_logging::info(
            "discovery",
            format!(
                "incremental scan completed seen_files={files_seen} seen_folders={folders_seen} \
                 added={added} updated={updated} removed={removed} unchanged={unchanged} duration_ms={}",
                duration.as_millis()
            ),
        );

        Ok(ScanReport {
            roots: normalized_roots,
            files_seen,
            folders_seen,
            added,
            updated,
            removed,
            unchanged,
            duration,
            finished_at,
        })
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("discovery engine is not initialized"))
        }
    }
}

impl Lifecycle for DiscoveryEngine {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        jaymi_logging::info(
            "discovery",
            format!(
                "discovery engine initialized indexing_enabled={} roots={}",
                self.indexing_enabled,
                self.configured_roots.len()
            ),
        );
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let stats = self
            .initialized
            .then(|| self.knowledge.stats().ok())
            .flatten()
            .unwrap_or_default();
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            (
                "indexing_enabled".to_string(),
                self.indexing_enabled.to_string(),
            ),
            ("files".to_string(), stats.files.to_string()),
            ("folders".to_string(), stats.folders.to_string()),
            (
                "configured_roots".to_string(),
                self.configured_roots.len().to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

fn metadata_changed(existing: &KnowledgeItem, item: &DiscoveredItem) -> bool {
    existing.size != item.size
        || existing.modified != item.modified
        || existing.created != item.created
        || existing.is_directory != item.is_directory
        || existing.hidden != item.hidden
        || existing.filename != item.filename
        || existing.extension != item.extension
}

fn discovered_to_knowledge(item: &DiscoveredItem) -> KnowledgeItem {
    KnowledgeItem {
        path: item.path.clone(),
        filename: item.filename.clone(),
        extension: item.extension.clone(),
        size: item.size,
        created: item.created,
        modified: item.modified,
        is_directory: item.is_directory,
        hidden: item.hidden,
        parent: item.parent.clone(),
        first_discovered: None,
        last_indexed: None,
        last_modified: item.modified,
        last_verified: None,
        device_id: item.device_id,
        inode: item.inode,
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::thread;
    use std::time::Duration as StdDuration;

    use jaymi_database::Database;
    use jaymi_knowledge::{KnowledgeQuery, KnowledgeStore};

    fn boot_engine(data: &std::path::Path, root: PathBuf) -> DiscoveryEngine {
        let mut db = Database::with_data_dir(data);
        db.initialize().unwrap();
        let mut knowledge = SqliteKnowledgeStore::new(Arc::new(db));
        knowledge.initialize().unwrap();
        let mut engine = DiscoveryEngine::new(Arc::new(knowledge), vec![root], true);
        engine.initialize().unwrap();
        engine
    }

    #[test]
    fn scan_persists_recursive_metadata() {
        let data = temp_dir("discover-data");
        let root = temp_dir("discover-root");
        fs::create_dir_all(root.join("nested")).unwrap();
        let mut file = fs::File::create(root.join("nested").join("note.txt")).unwrap();
        write!(file, "hello").unwrap();
        fs::File::create(root.join(".hidden")).unwrap();

        let engine = boot_engine(&data, root.clone());
        let report = engine.scan(&[root.clone()]).unwrap();
        assert!(report.files_seen >= 2);
        assert!(report.folders_seen >= 2);
        assert!(report.added >= 2);

        let items = engine
            .knowledge()
            .query(KnowledgeQuery {
                path_prefix: Some(
                    root.canonicalize()
                        .unwrap_or_else(|_| root.clone())
                        .to_string_lossy()
                        .into_owned(),
                ),
                ..KnowledgeQuery::default()
            })
            .unwrap();
        assert!(items.iter().any(|item| item.filename == "note.txt"));
        assert!(items
            .iter()
            .any(|item| item.filename == ".hidden" && item.hidden));
        let note = items
            .iter()
            .find(|item| item.filename == "note.txt")
            .unwrap();
        assert_eq!(note.extension.as_deref(), Some("txt"));
        assert!(!note.is_directory);
        assert_eq!(note.size, 5);
    }

    #[test]
    fn incremental_scan_skips_unchanged_and_detects_changes() {
        let data = temp_dir("incr-data");
        let root = temp_dir("incr-root");
        fs::write(root.join("a.txt"), "one").unwrap();

        let engine = boot_engine(&data, root.clone());

        let first = engine.scan(&[root.clone()]).unwrap();
        assert!(first.added >= 1);
        assert_eq!(first.updated, 0);
        assert_eq!(first.removed, 0);

        let second = engine.scan(&[root.clone()]).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.updated, 0);
        assert_eq!(second.removed, 0);
        assert!(second.unchanged >= 1);

        thread::sleep(StdDuration::from_millis(20));
        fs::write(root.join("a.txt"), "changed").unwrap();
        fs::write(root.join("b.txt"), "new").unwrap();
        let third = engine.scan(&[root.clone()]).unwrap();
        assert!(third.added >= 1);
        assert!(third.updated >= 1);

        fs::remove_file(root.join("b.txt")).unwrap();
        let fourth = engine.scan(&[root.clone()]).unwrap();
        assert!(fourth.removed >= 1);
    }

    #[test]
    fn rename_preserves_first_discovered_when_inode_matches() {
        let data = temp_dir("rename-data");
        let root = temp_dir("rename-root");
        let original = root.join("old-name.txt");
        fs::write(&original, "same-inode").unwrap();

        let engine = boot_engine(&data, root.clone());
        engine.scan(&[root.clone()]).unwrap();
        let before = engine
            .knowledge()
            .get_by_path(&original)
            .unwrap()
            .expect("original");
        let first = before.first_discovered;

        let renamed = root.join("new-name.txt");
        fs::rename(&original, &renamed).unwrap();
        engine.scan(&[root.clone()]).unwrap();

        assert!(!engine.knowledge().exists(&original).unwrap());
        let after = engine
            .knowledge()
            .get_by_path(&renamed)
            .unwrap()
            .expect("renamed");
        assert_eq!(after.first_discovered, first);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-discovery-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
