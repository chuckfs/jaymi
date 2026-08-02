//! Aggregate knowledge statistics for diagnostics.

/// Inventory statistics exposed through the Knowledge API.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InventoryStats {
    /// Indexed file count.
    pub files: u64,
    /// Indexed folder count.
    pub folders: u64,
    /// Last scan finished_at unix seconds, when any.
    pub last_scan_at: Option<i64>,
    /// Last scan duration in milliseconds, when any.
    pub last_scan_duration_ms: Option<u64>,
    /// Files added in the last successful scan.
    pub last_added: Option<u64>,
    /// Files updated in the last successful scan.
    pub last_updated: Option<u64>,
    /// Files removed in the last successful scan.
    pub last_removed: Option<u64>,
    /// Files unchanged in the last successful scan.
    pub last_unchanged: Option<u64>,
    /// On-disk SQLite database size in bytes.
    pub database_size_bytes: u64,
    /// Total knowledge queries executed since boot.
    pub query_count: u64,
    /// Label of the last knowledge query.
    pub last_query_label: Option<String>,
    /// Rows returned by the last knowledge query.
    pub last_query_rows: Option<u64>,
    /// Duration of the last knowledge query in milliseconds.
    pub last_query_duration_ms: Option<u64>,
}

/// Aggregate collection statistics for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollectionStats {
    /// Number of active collections with inventory coverage.
    pub collection_count: u64,
    /// Combined inventoried items across collection roots.
    pub total_items: u64,
    /// Active collection display names, sorted.
    pub names: Vec<String>,
}
