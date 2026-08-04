//! SQLite-backed KnowledgeStore — the only component that talks to inventory SQL.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
use jaymi_database::{
    Database, DiscoveredItemRecord, DiscoveredQuery, DiscoveryScanInput, DiscoverySort,
};

use crate::collections::Collection;
use crate::path::normalize_path;
use crate::stats::{CollectionStats, InventoryStats};
use crate::store::KnowledgeStore;
use crate::types::{
    KnowledgeItem, KnowledgeQuery, KnowledgeSort, PublishOutcome, RecentKind, ScanSummary,
};

const NAME: &str = "knowledge";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database"];

#[derive(Default)]
struct QueryStatsState {
    query_count: u64,
    last_query_label: Option<String>,
    last_query_rows: Option<u64>,
    last_query_duration_ms: Option<u64>,
}

impl QueryStatsState {
    fn clone_stats(&self) -> Self {
        Self {
            query_count: self.query_count,
            last_query_label: self.last_query_label.clone(),
            last_query_rows: self.last_query_rows,
            last_query_duration_ms: self.last_query_duration_ms,
        }
    }
}

/// SQLite implementation of the Knowledge API.
pub struct SqliteKnowledgeStore {
    initialized: bool,
    database: Arc<Database>,
    query_stats: Mutex<QueryStatsState>,
}

impl SqliteKnowledgeStore {
    /// Create an uninitialized knowledge store bound to the shared database.
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            initialized: false,
            database,
            query_stats: Mutex::new(QueryStatsState::default()),
        }
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("knowledge store is not initialized"))
        }
    }

    pub(crate) fn query_untracked(
        &self,
        filter: KnowledgeQuery,
    ) -> JaymiResult<Vec<KnowledgeItem>> {
        let records = self.database.query_discovered(&to_db_query(&filter))?;
        Ok(records.into_iter().map(record_to_item).collect())
    }

    fn record_query(&self, label: String, rows: u64, duration_ms: u64) {
        if let Ok(mut stats) = self.query_stats.lock() {
            stats.query_count = stats.query_count.saturating_add(1);
            stats.last_query_label = Some(label);
            stats.last_query_rows = Some(rows);
            stats.last_query_duration_ms = Some(duration_ms);
        }
    }
}

impl KnowledgeStore for SqliteKnowledgeStore {
    fn get_by_path(&self, path: &Path) -> JaymiResult<Option<KnowledgeItem>> {
        self.ensure_initialized()?;
        let started = Instant::now();
        let key = normalize_path(path)?.to_string_lossy().into_owned();
        let item = self.database.get_discovered_item(&key)?.map(record_to_item);
        self.record_query(
            "get_by_path".to_string(),
            u64::from(item.is_some()),
            started.elapsed().as_millis() as u64,
        );
        Ok(item)
    }

    fn exists(&self, path: &Path) -> JaymiResult<bool> {
        Ok(self.get_by_path(path)?.is_some())
    }

    fn find_by_name(&self, name: &str, limit: Option<usize>) -> JaymiResult<Vec<KnowledgeItem>> {
        self.query(KnowledgeQuery {
            name_contains: Some(name.to_string()),
            limit,
            ..KnowledgeQuery::default()
        })
    }

    fn query(&self, filter: KnowledgeQuery) -> JaymiResult<Vec<KnowledgeItem>> {
        self.ensure_initialized()?;
        let started = Instant::now();
        let label = query_label(&filter);
        let items = self.query_untracked(filter)?;
        let rows = items.len() as u64;
        self.record_query(label, rows, started.elapsed().as_millis() as u64);
        Ok(items)
    }

    fn recent(&self, kind: RecentKind, limit: usize) -> JaymiResult<Vec<KnowledgeItem>> {
        let sort = match kind {
            RecentKind::Modified => KnowledgeSort::RecentlyModified,
            RecentKind::Created => KnowledgeSort::RecentlyCreated,
        };
        self.query(KnowledgeQuery {
            files_only: true,
            sort,
            limit: Some(limit),
            ..KnowledgeQuery::default()
        })
    }

    fn by_extension(
        &self,
        extension: &str,
        limit: Option<usize>,
    ) -> JaymiResult<Vec<KnowledgeItem>> {
        self.query(KnowledgeQuery {
            extension: Some(extension.trim_start_matches('.').to_ascii_lowercase()),
            files_only: true,
            limit,
            ..KnowledgeQuery::default()
        })
    }

    fn list_collections(&self) -> JaymiResult<Vec<Collection>> {
        self.ensure_initialized()?;
        self.list_collections_inner()
    }

    fn resolve_collection(&self, name: &str) -> JaymiResult<Option<Collection>> {
        self.ensure_initialized()?;
        self.resolve_collection_inner(name)
    }

    fn items_in_collection(
        &self,
        name: &str,
        immediate: bool,
        limit: Option<usize>,
    ) -> JaymiResult<Vec<KnowledgeItem>> {
        let Some(collection) = self.resolve_collection(name)? else {
            return Ok(Vec::new());
        };
        let key = collection.root.to_string_lossy().into_owned();
        if immediate {
            self.query(KnowledgeQuery {
                parent: Some(key),
                limit,
                ..KnowledgeQuery::default()
            })
        } else {
            self.query(KnowledgeQuery {
                path_prefix: Some(key),
                limit,
                ..KnowledgeQuery::default()
            })
        }
    }

    fn stats(&self) -> JaymiResult<InventoryStats> {
        self.ensure_initialized()?;
        let counts = self.database.discovered_counts()?;
        let latest = self.database.latest_scan()?;
        let database_size_bytes = self.database.file_size_bytes().unwrap_or(0);
        let query_stats = self
            .query_stats
            .lock()
            .map(|guard| guard.clone_stats())
            .unwrap_or_default();
        Ok(InventoryStats {
            files: counts.files,
            folders: counts.folders,
            last_scan_at: latest.as_ref().map(|scan| scan.finished_at),
            last_scan_duration_ms: latest.as_ref().map(|scan| scan.duration_ms),
            last_added: latest.as_ref().map(|scan| scan.files_added),
            last_updated: latest.as_ref().map(|scan| scan.files_updated),
            last_removed: latest.as_ref().map(|scan| scan.files_removed),
            last_unchanged: latest.as_ref().map(|scan| scan.files_unchanged),
            database_size_bytes,
            query_count: query_stats.query_count,
            last_query_label: query_stats.last_query_label,
            last_query_rows: query_stats.last_query_rows,
            last_query_duration_ms: query_stats.last_query_duration_ms,
        })
    }

    fn collection_stats(&self) -> JaymiResult<CollectionStats> {
        self.ensure_initialized()?;
        self.collection_stats_inner()
    }

    fn publish(&self, item: &KnowledgeItem, now: i64) -> JaymiResult<PublishOutcome> {
        self.ensure_initialized()?;
        let key = normalize_path(&item.path)?.to_string_lossy().into_owned();
        if let Some(existing) = self.database.get_discovered_item(&key)? {
            if metadata_changed(&existing, item) {
                let mut record = item_to_record(item, now, existing.first_discovered);
                record.path = key;
                record.first_discovered = existing.first_discovered.or(Some(now));
                self.database.update_discovered_item(&record)?;
                Ok(PublishOutcome::Updated)
            } else {
                self.database.verify_discovered_item(&key, now)?;
                Ok(PublishOutcome::Verified)
            }
        } else {
            let mut record = item_to_record(item, now, Some(now));
            record.path = key;
            self.database.insert_discovered_item(&record)?;
            Ok(PublishOutcome::Inserted)
        }
    }

    fn verify(&self, path: &Path, at: i64) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let key = normalize_path(path)?.to_string_lossy().into_owned();
        self.database.verify_discovered_item(&key, at)
    }

    fn rename(&self, old_path: &Path, item: &KnowledgeItem, now: i64) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let old_key = normalize_path(old_path)?.to_string_lossy().into_owned();
        let first = self
            .database
            .get_discovered_item(&old_key)?
            .and_then(|row| row.first_discovered);
        let mut record = item_to_record(item, now, first);
        record.path = normalize_path(&item.path)?.to_string_lossy().into_owned();
        record.first_discovered = first.or(Some(now));
        self.database.rename_discovered_item(&old_key, &record)
    }

    fn remove(&self, path: &Path) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let key = normalize_path(path)?.to_string_lossy().into_owned();
        self.database.remove_discovered_path(&key)
    }

    fn items_under_root(&self, root: &Path) -> JaymiResult<Vec<KnowledgeItem>> {
        self.ensure_initialized()?;
        let key = normalize_path(root)?.to_string_lossy().into_owned();
        let records = self.database.items_under_root(&key)?;
        Ok(records.into_iter().map(record_to_item).collect())
    }

    fn record_scan(&self, summary: &ScanSummary) -> JaymiResult<i64> {
        self.ensure_initialized()?;
        self.database.record_scan(&DiscoveryScanInput {
            started_at: summary.started_at,
            finished_at: summary.finished_at,
            duration_ms: summary.duration_ms,
            roots: summary.roots.clone(),
            files_seen: summary.files_seen,
            folders_seen: summary.folders_seen,
            files_added: summary.files_added,
            files_updated: summary.files_updated,
            files_removed: summary.files_removed,
            files_unchanged: summary.files_unchanged,
            status: summary.status.clone(),
        })
    }
}

impl Lifecycle for SqliteKnowledgeStore {
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
        jaymi_logging::info("knowledge", "knowledge store initialized");
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let counts = self
            .initialized
            .then(|| self.database.discovered_counts().ok())
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
            ("files".to_string(), counts.files.to_string()),
            ("folders".to_string(), counts.folders.to_string()),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

fn to_db_query(filter: &KnowledgeQuery) -> DiscoveredQuery {
    DiscoveredQuery {
        path_prefix: filter.path_prefix.clone(),
        parent: filter.parent.clone(),
        name_contains: filter.name_contains.clone(),
        extension: filter.extension.clone(),
        files_only: filter.files_only,
        directories_only: filter.directories_only,
        hidden_only: filter.hidden_only,
        empty_folders: filter.empty_folders,
        sort: match filter.sort {
            KnowledgeSort::Path => DiscoverySort::Path,
            KnowledgeSort::RecentlyModified => DiscoverySort::RecentlyModified,
            KnowledgeSort::RecentlyCreated => DiscoverySort::RecentlyCreated,
            KnowledgeSort::Largest => DiscoverySort::Largest,
        },
        limit: filter.limit,
    }
}

fn query_label(filter: &KnowledgeQuery) -> String {
    if filter.empty_folders {
        return "empty_folders".to_string();
    }
    if filter.hidden_only {
        return "hidden".to_string();
    }
    if let Some(extension) = &filter.extension {
        return format!("extension:{extension}");
    }
    if filter.name_contains.is_some() {
        return "by_name".to_string();
    }
    if filter.parent.is_some() {
        return "by_folder".to_string();
    }
    if filter.path_prefix.is_some() {
        return "under_folder".to_string();
    }
    match filter.sort {
        KnowledgeSort::RecentlyModified => "recently_modified".to_string(),
        KnowledgeSort::RecentlyCreated => "recently_created".to_string(),
        KnowledgeSort::Largest => "largest".to_string(),
        KnowledgeSort::Path => "all".to_string(),
    }
}

fn metadata_changed(existing: &DiscoveredItemRecord, item: &KnowledgeItem) -> bool {
    existing.size != item.size
        || existing.modified != item.modified
        || existing.created != item.created
        || existing.is_directory != item.is_directory
        || existing.hidden != item.hidden
        || existing.filename != item.filename
        || existing.extension != item.extension
}

fn item_to_record(
    item: &KnowledgeItem,
    now: i64,
    first_discovered: Option<i64>,
) -> DiscoveredItemRecord {
    DiscoveredItemRecord {
        path: item.path.to_string_lossy().into_owned(),
        filename: item.filename.clone(),
        extension: item.extension.clone(),
        size: item.size,
        created: item.created,
        modified: item.modified,
        is_directory: item.is_directory,
        hidden: item.hidden,
        parent: item
            .parent
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        first_discovered: first_discovered.or(Some(now)),
        last_indexed: Some(now),
        last_modified: item.modified,
        last_verified: Some(now),
        device_id: item.device_id,
        inode: item.inode,
    }
}

fn record_to_item(record: DiscoveredItemRecord) -> KnowledgeItem {
    KnowledgeItem {
        path: std::path::PathBuf::from(record.path),
        filename: record.filename,
        extension: record.extension,
        size: record.size,
        created: record.created,
        modified: record.modified,
        is_directory: record.is_directory,
        hidden: record.hidden,
        parent: record.parent.map(std::path::PathBuf::from),
        first_discovered: record.first_discovered,
        last_indexed: record.last_indexed,
        last_modified: record.last_modified,
        last_verified: record.last_verified,
        device_id: record.device_id,
        inode: record.inode,
    }
}
