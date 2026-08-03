//! Capability Inspector — developer-facing runtime capability view.
//!
//! Surfaces registered vs active capabilities, workspace associations, and
//! required tools/providers so diagnostics reflect real runtime state.

use crate::discovery::{
    capability_requirements, CapabilityBlocker, CapabilityDiscoveryReport, CapabilityStatus,
};
use crate::workspace::{capability_workspace, WorkspaceKind};
use crate::Capability;

/// One capability row in the inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedCapability {
    /// Stable capability id.
    pub id: String,
    /// Capability enum value.
    pub capability: Capability,
    /// True when registered in the Capability Engine.
    pub registered: bool,
    /// True when currently usable (registered + requirements met).
    pub active: bool,
    /// Workspace this capability expands, when any.
    pub workspace: Option<WorkspaceKind>,
    /// Declared required tool ids (preferred / planned).
    pub required_tools: Vec<String>,
    /// Declared required provider ids (preferred / planned).
    pub required_providers: Vec<String>,
    /// Tools currently fulfilling this capability in the live inventory.
    pub fulfilling_tools: Vec<String>,
    /// Providers currently advertising this capability.
    pub fulfilling_providers: Vec<String>,
    /// Blockers preventing activation (empty when active).
    pub blockers: Vec<CapabilityBlocker>,
}

impl InspectedCapability {
    /// Build an inspector row from discovery status.
    pub fn from_status(status: &CapabilityStatus) -> Self {
        let requirements = &status.requirements;
        Self {
            id: status.descriptor.id.to_string(),
            capability: requirements.capability,
            registered: status.registered,
            active: status.is_available(),
            workspace: capability_workspace(requirements.capability),
            required_tools: requirements
                .preferred_tools
                .iter()
                .map(|id| (*id).to_string())
                .collect(),
            required_providers: requirements
                .preferred_providers
                .iter()
                .map(|id| (*id).to_string())
                .collect(),
            fulfilling_tools: status.fulfilling_tools.clone(),
            fulfilling_providers: status.fulfilling_providers.clone(),
            blockers: status.blockers.clone(),
        }
    }

    /// Compact one-line detail for dashboards.
    pub fn detail(&self) -> String {
        let workspace = self
            .workspace
            .map(|kind| kind.id())
            .unwrap_or("conversation");
        let status = if self.active {
            "active"
        } else if self.registered {
            "registered"
        } else {
            "inactive"
        };
        format!(
            "{} · {} · workspace={} · tools=[{}] · providers=[{}]",
            self.id,
            status,
            workspace,
            self.required_tools.join(","),
            self.required_providers.join(",")
        )
    }
}

/// Developer-facing snapshot of the capability system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilityInspectorReport {
    /// Capability ids registered with the engine (registration order).
    pub registered: Vec<String>,
    /// Capability ids currently active (available at runtime).
    pub active: Vec<String>,
    /// Per-capability inspector rows (catalog order via discovery).
    pub entries: Vec<InspectedCapability>,
    /// Session workspace kind currently expanded, when known.
    pub active_workspace: Option<WorkspaceKind>,
}

impl CapabilityInspectorReport {
    /// Build from a discovery report and the registered capability list.
    pub fn from_discovery(
        registered: &[Capability],
        discovery: &CapabilityDiscoveryReport,
    ) -> Self {
        let registered_ids: Vec<String> = registered.iter().map(|c| c.id().to_string()).collect();
        let active: Vec<String> = discovery
            .available
            .iter()
            .map(|status| status.descriptor.id.to_string())
            .collect();
        let entries: Vec<InspectedCapability> = discovery
            .all()
            .into_iter()
            .map(InspectedCapability::from_status)
            .collect();
        Self {
            registered: registered_ids,
            active,
            entries,
            active_workspace: None,
        }
    }

    /// Attach the session's active workspace without changing capability rows.
    pub fn with_active_workspace(mut self, workspace: Option<WorkspaceKind>) -> Self {
        self.active_workspace = workspace;
        self
    }

    /// Look up one inspected capability by id.
    pub fn get(&self, id: &str) -> Option<&InspectedCapability> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Registered inspector rows only.
    pub fn registered_entries(&self) -> Vec<&InspectedCapability> {
        self.entries
            .iter()
            .filter(|entry| entry.registered)
            .collect()
    }

    /// Active (currently usable) inspector rows only.
    pub fn active_entries(&self) -> Vec<&InspectedCapability> {
        self.entries.iter().filter(|entry| entry.active).collect()
    }

    /// Compact summary for logs / diagnostics subsystem detail.
    pub fn summary(&self) -> String {
        let workspace = self
            .active_workspace
            .map(|kind| kind.id())
            .unwrap_or("none");
        format!(
            "registered={} [{}] · active={} [{}] · workspace={}",
            self.registered.len(),
            self.registered.join(", "),
            self.active.len(),
            self.active.join(", "),
            workspace
        )
    }

    /// Full developer-facing text report.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Capability Inspector".to_string());
        lines.push(self.summary());
        lines.push(String::new());
        lines.push(format!(
            "{:<18} {:<10} {:<12} {:<18} {}",
            "Capability", "Status", "Workspace", "Required tools", "Required providers"
        ));
        lines.push("-".repeat(88));

        // Registered capabilities first (developer focus), then catalog rest.
        let mut ordered: Vec<&InspectedCapability> = Vec::new();
        for id in &self.registered {
            if let Some(entry) = self.get(id) {
                ordered.push(entry);
            }
        }
        for entry in &self.entries {
            if !self.registered.iter().any(|id| id == &entry.id) {
                ordered.push(entry);
            }
        }

        for entry in ordered {
            let status = if entry.active {
                "active"
            } else if entry.registered {
                "registered"
            } else {
                "inactive"
            };
            let workspace = entry
                .workspace
                .map(|kind| kind.id())
                .unwrap_or("conversation");
            let tools = if entry.required_tools.is_empty() {
                "-".to_string()
            } else {
                entry.required_tools.join(",")
            };
            let providers = if entry.required_providers.is_empty() {
                "-".to_string()
            } else {
                entry.required_providers.join(",")
            };
            lines.push(format!(
                "{:<18} {:<10} {:<12} {:<18} {}",
                entry.id, status, workspace, tools, providers
            ));
        }
        lines.join("\n")
    }
}

/// Build an inspector report from registration + live inventory.
///
/// Pure helper used by [`crate::CapabilityEngineApi::inspect`].
pub fn build_inspector_report(
    registered: &[Capability],
    discovery: &CapabilityDiscoveryReport,
) -> CapabilityInspectorReport {
    CapabilityInspectorReport::from_discovery(registered, discovery)
}

/// Inspect declared requirements for one capability without discovery.
pub fn inspect_requirements(capability: Capability) -> (Vec<String>, Vec<String>) {
    let requirements = capability_requirements(capability);
    (
        requirements
            .preferred_tools
            .iter()
            .map(|id| (*id).to_string())
            .collect(),
        requirements
            .preferred_providers
            .iter()
            .map(|id| (*id).to_string())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{
        assess_capability, CapabilityDiscoveryReport, CapabilityInventory, DiscoveredProvider,
        DiscoveredTool,
    };

    fn inventory_with_search() -> CapabilityInventory {
        CapabilityInventory {
            tools: vec![DiscoveredTool {
                id: "search_files".into(),
                capabilities: vec![Capability::Search],
            }],
            providers: vec![DiscoveredProvider {
                id: "filesystem".into(),
                capabilities: vec![Capability::Search, Capability::Code],
            }],
        }
    }

    #[test]
    fn inspector_separates_registered_active_and_workspace() {
        let inventory = inventory_with_search();
        let search = assess_capability(Capability::Search, true, true, &inventory);
        let code = assess_capability(Capability::Code, true, true, &inventory);
        let chat = assess_capability(Capability::Chat, false, true, &inventory);
        let discovery = CapabilityDiscoveryReport {
            available: vec![search],
            unavailable: vec![code, chat],
        };
        let report = CapabilityInspectorReport::from_discovery(
            &[Capability::Search, Capability::Code],
            &discovery,
        );

        assert_eq!(report.registered, vec!["search", "code"]);
        assert_eq!(report.active, vec!["search"]);
        let search_row = report.get("search").expect("search");
        assert!(search_row.registered);
        assert!(search_row.active);
        assert_eq!(search_row.workspace, Some(WorkspaceKind::Research));
        assert!(search_row.required_tools.contains(&"search_files".into()));
        assert!(search_row
            .required_providers
            .iter()
            .any(|id| id == "filesystem"));

        let code_row = report.get("code").expect("code");
        assert!(code_row.registered);
        assert!(!code_row.active);
        assert_eq!(code_row.workspace, Some(WorkspaceKind::Coding));
        assert!(code_row.required_tools.contains(&"editor".into()));

        let rendered = report.render();
        assert!(rendered.contains("Capability Inspector"));
        assert!(rendered.contains("search"));
        assert!(rendered.contains("research"));
        assert!(rendered.contains("coding"));
    }
}
