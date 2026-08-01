//! Diagnostics snapshot shown by the temporary desktop UI.

use std::path::PathBuf;

use jaymi_core::{AppState, FileEntry};

/// Read-only view of runtime health, listing results, and document reads.
#[derive(Debug, Clone)]
pub struct DiagnosticsSnapshot {
    /// Current application lifecycle state.
    pub app_state: AppState,
    /// Whether the Planner reports healthy.
    pub planner_healthy: bool,
    /// Number of registered providers.
    pub provider_count: usize,
    /// Number of registered tools.
    pub tool_count: usize,
    /// Number of registered capabilities.
    pub capability_count: usize,
    /// Whether the database reports an active connection.
    pub database_connected: bool,
    /// Directory that was listed, when a listing has been performed.
    pub listed_path: Option<PathBuf>,
    /// Summary produced by the Planner for the latest listing.
    pub listing_summary: Option<String>,
    /// Structured file metadata returned through the architecture.
    pub entries: Vec<FileEntry>,
    /// Path of the file that was read.
    pub read_path: Option<PathBuf>,
    /// Detected file type label.
    pub read_file_type: Option<String>,
    /// Parser selected for the read.
    pub read_parser: Option<String>,
    /// Whether parsing completed successfully.
    pub read_success: bool,
    /// Character count of parsed text.
    pub read_character_count: Option<usize>,
    /// Planner summary for the latest read.
    pub read_summary: Option<String>,
    /// Parsed text content for the scrollable viewer.
    pub read_text: Option<String>,
}

impl DiagnosticsSnapshot {
    /// Format planner health for display.
    pub fn planner_label(&self) -> &'static str {
        if self.planner_healthy {
            "Healthy"
        } else {
            "Unhealthy"
        }
    }

    /// Format database connection status for display.
    pub fn database_label(&self) -> &'static str {
        if self.database_connected {
            "Connected"
        } else {
            "Disconnected"
        }
    }

    /// Format read success for display.
    pub fn read_success_label(&self) -> &'static str {
        if self.read_success {
            "yes"
        } else {
            "no"
        }
    }
}
