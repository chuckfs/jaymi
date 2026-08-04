//! Stable Knowledge API surface.
//!
//! Consumers (Planner tools, discovery indexing, future providers) use this
//! trait and never talk to SQLite directly.

use std::path::Path;

use jaymi_core::JaymiResult;

use crate::collections::Collection;
use crate::stats::{CollectionStats, InventoryStats};
use crate::types::{KnowledgeItem, KnowledgeQuery, PublishOutcome, RecentKind, ScanSummary};

/// Stable internal API for Jaymi's indexed knowledge.
pub trait KnowledgeStore: Send + Sync {
    /// Find one item by absolute path.
    fn get_by_path(&self, path: &Path) -> JaymiResult<Option<KnowledgeItem>>;

    /// True when the path exists in the knowledge inventory.
    fn exists(&self, path: &Path) -> JaymiResult<bool>;

    /// Find items whose filename contains `name` (case-insensitive).
    fn find_by_name(&self, name: &str, limit: Option<usize>) -> JaymiResult<Vec<KnowledgeItem>>;

    /// General filtered query.
    fn query(&self, filter: KnowledgeQuery) -> JaymiResult<Vec<KnowledgeItem>>;

    /// Recently modified or created files.
    fn recent(&self, kind: RecentKind, limit: usize) -> JaymiResult<Vec<KnowledgeItem>>;

    /// Files with a given extension (no leading dot).
    fn by_extension(
        &self,
        extension: &str,
        limit: Option<usize>,
    ) -> JaymiResult<Vec<KnowledgeItem>>;

    /// Active logical collections with inventory coverage.
    fn list_collections(&self) -> JaymiResult<Vec<Collection>>;

    /// Resolve one collection by name/slug when active.
    fn resolve_collection(&self, name: &str) -> JaymiResult<Option<Collection>>;

    /// Items belonging to a named collection.
    fn items_in_collection(
        &self,
        name: &str,
        immediate: bool,
        limit: Option<usize>,
    ) -> JaymiResult<Vec<KnowledgeItem>>;

    /// Aggregate inventory statistics.
    fn stats(&self) -> JaymiResult<InventoryStats>;

    /// Aggregate collection statistics.
    fn collection_stats(&self) -> JaymiResult<CollectionStats>;

    /// Insert or update an inventory item (publish path for providers).
    fn publish(&self, item: &KnowledgeItem, now: i64) -> JaymiResult<PublishOutcome>;

    /// Confirm an existing path is still present without rewriting metadata.
    fn verify(&self, path: &Path, at: i64) -> JaymiResult<()>;

    /// Rename an inventory identity from `old_path` to the item's path.
    fn rename(&self, old_path: &Path, item: &KnowledgeItem, now: i64) -> JaymiResult<()>;

    /// Remove a path from the inventory.
    fn remove(&self, path: &Path) -> JaymiResult<()>;

    /// All inventory rows under a root (including the root).
    fn items_under_root(&self, root: &Path) -> JaymiResult<Vec<KnowledgeItem>>;

    /// Persist a scan summary for diagnostics.
    fn record_scan(&self, summary: &ScanSummary) -> JaymiResult<i64>;
}
