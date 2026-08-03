//! Diagnostics snapshot for the developer dashboard.
//!
//! Health indicators reflect real subsystem state. Stub and unimplemented
//! components are labeled honestly — never as healthy/operational.

use std::path::PathBuf;

use jaymi_capabilities::CapabilityInspectorReport;
use jaymi_core::{AppState, FileEntry};

/// Operational readiness of a subsystem, distinct from lifecycle initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationalStatus {
    /// Feature-complete and usable for its Layer 0 role.
    Operational,
    /// Present and enforcing, but with known limitations.
    Degraded,
    /// Lifecycle-initialized with stub behavior only.
    Stub,
    /// Not wired into the running process yet.
    NotImplemented,
    /// Expected to work but currently failing or disconnected.
    Unavailable,
}

impl OperationalStatus {
    /// Short label for UI / CLI display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Operational => "Operational",
            Self::Degraded => "Degraded",
            Self::Stub => "Stub",
            Self::NotImplemented => "Not implemented",
            Self::Unavailable => "Unavailable",
        }
    }

    /// Whether this status should be treated as a green/healthy indicator.
    pub fn is_operational(self) -> bool {
        matches!(self, Self::Operational)
    }
}

/// One row in the diagnostics dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsystemStatus {
    /// Display name (e.g. "Planner", "Database").
    pub name: String,
    /// Honest operational state.
    pub status: OperationalStatus,
    /// Short human-readable detail.
    pub detail: String,
}

impl SubsystemStatus {
    /// Construct a subsystem status row.
    pub fn new(
        name: impl Into<String>,
        status: OperationalStatus,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
        }
    }
}

/// Read-only view of runtime health, listing results, and document reads.
#[derive(Debug, Clone)]
pub struct DiagnosticsSnapshot {
    /// Current application lifecycle state.
    pub app_state: AppState,
    /// Ordered subsystem rows for the developer dashboard.
    pub subsystems: Vec<SubsystemStatus>,
    /// Whether the Planner reports lifecycle+operational health.
    pub planner_healthy: bool,
    /// Number of registered providers.
    pub provider_count: usize,
    /// Registered provider identity ids.
    pub provider_ids: Vec<String>,
    /// Number of registered tools.
    pub tool_count: usize,
    /// Registered tool ids.
    pub tool_ids: Vec<String>,
    /// Number of registered capabilities.
    pub capability_count: usize,
    /// Registered capability ids.
    pub capability_ids: Vec<String>,
    /// Capabilities currently available (registered + requirements met).
    pub available_capability_ids: Vec<String>,
    /// Capabilities currently unavailable.
    pub unavailable_capability_ids: Vec<String>,
    /// Per-capability status detail lines for the dashboard.
    pub capability_status_details: Vec<String>,
    /// Developer-facing capability inspector (registered / active / requirements).
    pub capability_inspector: Option<CapabilityInspectorReport>,
    /// Number of registered parsers.
    pub parser_count: usize,
    /// Registered parser ids.
    pub parser_ids: Vec<String>,
    /// Whether the database reports an active connection.
    pub database_connected: bool,
    /// Absolute path to the SQLite database file.
    pub database_path: Option<String>,
    /// Applied schema version.
    pub database_schema_version: Option<u32>,
    /// Migration status label (`applied`, `failed: …`, etc.).
    pub database_migration_status: Option<String>,
    /// Whether local file logging reports healthy.
    pub logging_healthy: bool,
    /// Absolute path to the active log file.
    pub logging_path: Option<String>,
    /// Directory containing rotating log files.
    pub logging_dir: Option<String>,
    /// Configured / active minimum log level.
    pub logging_level: Option<String>,
    /// Absolute path to the persisted configuration file.
    pub config_path: Option<String>,
    /// Configured log level label.
    pub config_log_level: Option<String>,
    /// Configured theme label.
    pub config_theme: Option<String>,
    /// Whether indexing is enabled in configuration.
    pub config_indexing_enabled: Option<bool>,
    /// Active policy names.
    pub active_policies: Vec<String>,
    /// Permission engine mode summary.
    pub permission_mode: Option<String>,
    /// Latest permission decision label (`allowed`, `denied`, …).
    pub permission_decision: Option<String>,
    /// Explanation associated with the latest permission check.
    pub permission_explanation: Option<String>,
    /// Whether policy evaluation allowed the selected tool.
    pub policy_allowed: Option<bool>,
    /// Summary of the latest policy evaluation.
    pub policy_summary: Option<String>,
    /// Whether the latest request was blocked before tool execution.
    pub request_blocked: bool,
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
    /// Look up a subsystem row by display name.
    pub fn subsystem(&self, name: &str) -> Option<&SubsystemStatus> {
        self.subsystems.iter().find(|row| row.name == name)
    }

    /// Format planner status for display (never claims healthy for stubs).
    pub fn planner_label(&self) -> &str {
        self.subsystem("Planner")
            .map(|row| row.status.label())
            .unwrap_or(if self.planner_healthy {
                OperationalStatus::Operational.label()
            } else {
                OperationalStatus::Unavailable.label()
            })
    }

    /// Format database connection status for display.
    pub fn database_label(&self) -> &str {
        self.subsystem("Database")
            .map(|row| row.status.label())
            .unwrap_or(if self.database_connected {
                OperationalStatus::Operational.label()
            } else {
                OperationalStatus::Unavailable.label()
            })
    }

    /// Format database path for display.
    pub fn database_path_label(&self) -> &str {
        self.database_path.as_deref().unwrap_or("-")
    }

    /// Format schema version for display.
    pub fn database_schema_version_label(&self) -> String {
        self.database_schema_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    /// Format migration status for display.
    pub fn database_migration_status_label(&self) -> &str {
        self.database_migration_status.as_deref().unwrap_or("-")
    }

    /// Format logging status for display.
    pub fn logging_label(&self) -> &str {
        self.subsystem("Logging")
            .map(|row| row.status.label())
            .unwrap_or(if self.logging_healthy {
                OperationalStatus::Operational.label()
            } else {
                OperationalStatus::Unavailable.label()
            })
    }

    /// Format log file path for display.
    pub fn logging_path_label(&self) -> &str {
        self.logging_path.as_deref().unwrap_or("-")
    }

    /// Format log directory for display.
    pub fn logging_dir_label(&self) -> &str {
        self.logging_dir.as_deref().unwrap_or("-")
    }

    /// Format configuration path for display.
    pub fn config_path_label(&self) -> &str {
        self.config_path.as_deref().unwrap_or("-")
    }

    /// Format configured log level for display.
    pub fn config_log_level_label(&self) -> &str {
        self.config_log_level.as_deref().unwrap_or("-")
    }

    /// Format configured theme for display.
    pub fn config_theme_label(&self) -> &str {
        self.config_theme.as_deref().unwrap_or("-")
    }

    /// Format indexing enabled flag for display.
    pub fn config_indexing_enabled_label(&self) -> &str {
        match self.config_indexing_enabled {
            Some(true) => "true",
            Some(false) => "false",
            None => "-",
        }
    }

    /// Format permission decision for display.
    pub fn permission_decision_label(&self) -> &str {
        self.permission_decision.as_deref().unwrap_or("-")
    }

    /// Format permission explanation for display.
    pub fn permission_explanation_label(&self) -> &str {
        self.permission_explanation.as_deref().unwrap_or("-")
    }

    /// Format policy allowance for display.
    pub fn policy_allowed_label(&self) -> &str {
        match self.policy_allowed {
            Some(true) => "allowed",
            Some(false) => "denied",
            None => "-",
        }
    }

    /// Format policy summary for display.
    pub fn policy_summary_label(&self) -> &str {
        self.policy_summary.as_deref().unwrap_or("-")
    }

    /// Format blocked flag for display.
    pub fn request_blocked_label(&self) -> &'static str {
        if self.request_blocked {
            "yes"
        } else {
            "no"
        }
    }

    /// Render the capability inspector section, when present.
    pub fn render_capability_inspector(&self) -> Option<String> {
        self.capability_inspector
            .as_ref()
            .map(CapabilityInspectorReport::render)
    }

    /// Format read success for display.
    pub fn read_success_label(&self) -> &'static str {
        if self.read_success {
            "yes"
        } else {
            "no"
        }
    }

    /// Render the full subsystem dashboard as plain text (headless / tests).
    pub fn render_dashboard(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Jaymi Diagnostics".to_string());
        lines.push(format!("App state: {}", self.app_state.label()));
        lines.push(String::new());
        lines.push(format!(
            "{:<18} {:<16} {}",
            "Subsystem", "Status", "Detail"
        ));
        lines.push("-".repeat(72));
        for row in &self.subsystems {
            lines.push(format!(
                "{:<18} {:<16} {}",
                row.name,
                row.status.label(),
                row.detail
            ));
        }
        if let Some(inspector) = self.render_capability_inspector() {
            lines.push(String::new());
            lines.push(inspector);
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_status_labels_are_honest() {
        assert_eq!(OperationalStatus::Operational.label(), "Operational");
        assert_eq!(OperationalStatus::Stub.label(), "Stub");
        assert_eq!(
            OperationalStatus::NotImplemented.label(),
            "Not implemented"
        );
        assert!(!OperationalStatus::Stub.is_operational());
        assert!(!OperationalStatus::NotImplemented.is_operational());
    }

    #[test]
    fn render_dashboard_includes_subsystem_rows() {
        let snapshot = DiagnosticsSnapshot {
            app_state: AppState::Ready,
            subsystems: vec![
                SubsystemStatus::new("Planner", OperationalStatus::Operational, "ready"),
                SubsystemStatus::new(
                    "Memory Status",
                    OperationalStatus::Stub,
                    "retrieve not implemented",
                ),
            ],
            planner_healthy: true,
            provider_count: 0,
            provider_ids: vec![],
            tool_count: 0,
            tool_ids: vec![],
            capability_count: 0,
            capability_ids: vec![],
            available_capability_ids: vec![],
            unavailable_capability_ids: vec![],
            capability_status_details: vec![],
            capability_inspector: None,
            parser_count: 0,
            parser_ids: vec![],
            database_connected: false,
            database_path: None,
            database_schema_version: None,
            database_migration_status: None,
            logging_healthy: false,
            logging_path: None,
            logging_dir: None,
            logging_level: None,
            config_path: None,
            config_log_level: None,
            config_theme: None,
            config_indexing_enabled: None,
            active_policies: vec![],
            permission_mode: None,
            permission_decision: None,
            permission_explanation: None,
            policy_allowed: None,
            policy_summary: None,
            request_blocked: false,
            listed_path: None,
            listing_summary: None,
            entries: vec![],
            read_path: None,
            read_file_type: None,
            read_parser: None,
            read_success: false,
            read_character_count: None,
            read_summary: None,
            read_text: None,
        };

        let rendered = snapshot.render_dashboard();
        assert!(rendered.contains("Planner"));
        assert!(rendered.contains("Operational"));
        assert!(rendered.contains("Memory Status"));
        assert!(rendered.contains("Stub"));
        assert!(!rendered.contains("Healthy"));
    }
}
