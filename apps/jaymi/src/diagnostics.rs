//! Diagnostics snapshot shown by the temporary desktop UI.

use jaymi_core::AppState;

/// Read-only view of runtime health for the diagnostics window.
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
}
