//! Search Engine diagnostics and health.

/// Aggregate Search Engine statistics for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchStats {
    /// Total searches executed since boot.
    pub search_count: u64,
    /// Average query time in milliseconds (0 when no searches yet).
    pub average_query_time_ms: u64,
    /// Last strategy selected, when any.
    pub last_strategy: Option<String>,
    /// Last query duration in milliseconds.
    pub last_duration_ms: Option<u64>,
    /// Last hit count returned.
    pub last_hit_count: Option<usize>,
    /// Citations generated for the last search.
    pub last_citation_count: Option<usize>,
    /// Total citations generated since boot.
    pub citations_generated: u64,
}

/// Health snapshot for the Search Engine subsystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHealth {
    /// Whether the engine completed initialization.
    pub initialized: bool,
    /// Whether the engine is healthy for searches.
    pub healthy: bool,
    /// Engine version string.
    pub version: String,
    /// Short detail string for diagnostics.
    pub detail: String,
    /// Latest statistics snapshot.
    pub statistics: SearchStats,
}
