//! Execution plans — capability selections without execution.
//!
//! A plan describes *what* Jaymi intends to do: which capability, which tools,
//! which providers, and which permissions are required. Tools are never run
//! here — the Planner decides whether and when to execute.

use crate::descriptor::{capability_descriptor, CapabilityAvailability, CapabilityDescriptor};
use crate::discovery::{capability_requirements, CapabilityInventory, CapabilityRequirements};
use crate::Capability;

/// One permission needed before a capability plan may execute.
///
/// Kept as stable string labels so the Capability Engine stays independent of
/// the Permission Engine crate. The Planner maps these to permission checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequirement {
    /// Permission category (`filesystem`, `terminal`, `internet`, …).
    pub category: &'static str,
    /// Action within the category (`read`, `write`, `execute`, …).
    pub action: &'static str,
    /// Why this permission is needed for the capability.
    pub reason: &'static str,
}

impl PermissionRequirement {
    /// Compact `category:action` label.
    pub fn label(&self) -> String {
        format!("{}:{}", self.category, self.action)
    }
}

/// Built-in permission requirements for a capability.
pub fn capability_permission_requirements(capability: Capability) -> Vec<PermissionRequirement> {
    match capability {
        Capability::Chat => vec![],
        Capability::Search | Capability::Discover | Capability::ReadDocuments => {
            vec![PermissionRequirement {
                category: "filesystem",
                action: "read",
                reason: "read local files and knowledge",
            }]
        }
        Capability::Index => vec![PermissionRequirement {
            category: "filesystem",
            action: "read",
            reason: "scan directories into the inventory",
        }],
        Capability::Code => vec![
            PermissionRequirement {
                category: "filesystem",
                action: "read",
                reason: "read project source and documents",
            },
            PermissionRequirement {
                category: "filesystem",
                action: "write",
                reason: "edit project files",
            },
            PermissionRequirement {
                category: "terminal",
                action: "execute",
                reason: "run build and development commands",
            },
        ],
        Capability::Vision | Capability::Ocr | Capability::Embeddings => {
            vec![PermissionRequirement {
                category: "filesystem",
                action: "read",
                reason: "read local media or documents",
            }]
        }
        Capability::GenerateImages => vec![PermissionRequirement {
            category: "ai_providers",
            action: "execute",
            reason: "invoke an image generation provider",
        }],
        Capability::BrowseTheWeb | Capability::Internet => vec![PermissionRequirement {
            category: "internet",
            action: "network",
            reason: "access remote network resources",
        }],
        Capability::OrganizeFiles | Capability::FileManagement => vec![
            PermissionRequirement {
                category: "filesystem",
                action: "read",
                reason: "inspect files before organizing",
            },
            PermissionRequirement {
                category: "filesystem",
                action: "write",
                reason: "move or rename files",
            },
        ],
        Capability::ExecuteTerminalCommands => vec![PermissionRequirement {
            category: "terminal",
            action: "execute",
            reason: "run terminal commands",
        }],
        Capability::AutomateTasks | Capability::Automation => vec![PermissionRequirement {
            category: "system",
            action: "execute",
            reason: "coordinate automated work",
        }],
    }
}

/// One step in a capability execution plan.
///
/// Steps describe *what* should happen. Tools and providers decide *how*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityPlanStep {
    /// Requested capability.
    pub capability: Capability,
    /// Metadata describing the capability.
    pub descriptor: CapabilityDescriptor,
    /// Effective availability (Ready / Experimental / Planned / Unavailable).
    pub availability: CapabilityAvailability,
    /// Declared tool/provider requirement flags.
    pub requirements: CapabilityRequirements,
    /// Tools required to fulfill this capability (inventory hits or preferred).
    pub required_tools: Vec<String>,
    /// Providers required to fulfill this capability (inventory hits or preferred).
    pub required_providers: Vec<String>,
    /// Permissions required before execution.
    pub required_permissions: Vec<PermissionRequirement>,
    /// True when required tools are satisfied by the live inventory (or not required).
    pub tools_resolved: bool,
    /// True when required providers are satisfied by the live inventory (or not required).
    pub providers_resolved: bool,
}

impl CapabilityPlanStep {
    /// True when the step is in an executable tier and inventory requirements are met.
    pub fn is_executable(&self) -> bool {
        self.availability.is_executable_tier() && self.tools_resolved && self.providers_resolved
    }

    /// Short detail line for logs and responses.
    pub fn detail(&self) -> String {
        let perms: Vec<_> = self
            .required_permissions
            .iter()
            .map(PermissionRequirement::label)
            .collect();
        format!(
            "{} · tools=[{}] · providers=[{}] · permissions=[{}] · {}",
            self.descriptor.id,
            self.required_tools.join(","),
            self.required_providers.join(","),
            perms.join(","),
            self.availability.as_str()
        )
    }
}

/// Planned capabilities for a request.
///
/// An execution plan never runs tools. The Planner uses it to understand what
/// is required before any tool selection or execution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionPlan {
    /// Optional natural-language goal that produced this plan.
    pub goal: Option<String>,
    /// Ordered capability steps.
    pub steps: Vec<CapabilityPlanStep>,
}

impl ExecutionPlan {
    /// True when every step is currently in an executable availability tier.
    ///
    /// Planned steps keep the plan honest but incomplete — they are included
    /// without making the plan "ready" for tool execution.
    pub fn is_ready(&self) -> bool {
        !self.steps.is_empty()
            && self
                .steps
                .iter()
                .all(|step| step.availability.is_executable_tier())
    }

    /// True when every step can run (executable tier + inventory satisfied).
    pub fn is_executable(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(CapabilityPlanStep::is_executable)
    }

    /// Capabilities included in the plan (request order).
    pub fn capabilities(&self) -> Vec<Capability> {
        self.steps.iter().map(|step| step.capability).collect()
    }

    /// All required tool ids across steps (deterministic, de-duplicated, stable order).
    pub fn required_tools(&self) -> Vec<String> {
        unique_preserve_order(
            self.steps
                .iter()
                .flat_map(|step| step.required_tools.iter().cloned()),
        )
    }

    /// All required provider ids across steps.
    pub fn required_providers(&self) -> Vec<String> {
        unique_preserve_order(
            self.steps
                .iter()
                .flat_map(|step| step.required_providers.iter().cloned()),
        )
    }

    /// All required permissions across steps.
    pub fn required_permissions(&self) -> Vec<PermissionRequirement> {
        let mut out = Vec::new();
        for step in &self.steps {
            for permission in &step.required_permissions {
                if !out.iter().any(|existing: &PermissionRequirement| {
                    existing.category == permission.category && existing.action == permission.action
                }) {
                    out.push(permission.clone());
                }
            }
        }
        out
    }

    /// Steps that are not currently executable (Planned / Unavailable / …).
    pub fn unavailable(&self) -> Vec<&CapabilityPlanStep> {
        self.steps
            .iter()
            .filter(|step| !step.availability.is_executable_tier())
            .collect()
    }

    /// Short summary for logs and diagnostics.
    pub fn summary(&self) -> String {
        if self.steps.is_empty() {
            return "execution plan: empty".into();
        }
        let ids: Vec<_> = self.steps.iter().map(|step| step.descriptor.id).collect();
        let status = if self.is_executable() {
            "executable"
        } else if self.is_ready() {
            "ready"
        } else {
            "incomplete"
        };
        format!(
            "execution plan ({status}): {} capability(ies) [{}] · tools=[{}] · providers=[{}] · permissions=[{}]",
            self.steps.len(),
            ids.join(", "),
            self.required_tools().join(", "),
            self.required_providers().join(", "),
            self.required_permissions()
                .iter()
                .map(PermissionRequirement::label)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Human-readable plan body for Planner responses (no execution).
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        if let Some(goal) = &self.goal {
            lines.push(format!("Goal: {goal}"));
        }
        lines.push(self.summary());
        for (index, step) in self.steps.iter().enumerate() {
            lines.push(format!("{}. {}", index + 1, step.detail()));
        }
        lines.join("\n")
    }
}

/// Build one plan step from registration state and inventory.
pub fn build_plan_step(
    capability: Capability,
    availability: CapabilityAvailability,
    inventory: &CapabilityInventory,
) -> CapabilityPlanStep {
    let requirements = capability_requirements(capability);
    let inventory_tools: Vec<String> = inventory
        .tools_for(capability)
        .into_iter()
        .map(|tool| tool.id.clone())
        .collect();
    let inventory_providers: Vec<String> = inventory
        .providers_for(capability)
        .into_iter()
        .map(|provider| provider.id.clone())
        .collect();

    // Prefer live fulfillments; fall back to declared preferred ids so plans
    // remain deterministic even before tools/providers exist.
    let tools_resolved = !requirements.requires_tool || !inventory_tools.is_empty();
    let providers_resolved = !requirements.requires_provider || !inventory_providers.is_empty();
    let required_tools = if inventory_tools.is_empty() {
        requirements
            .preferred_tools
            .iter()
            .map(|id| (*id).to_string())
            .collect()
    } else {
        inventory_tools
    };
    let required_providers = if inventory_providers.is_empty() {
        requirements
            .preferred_providers
            .iter()
            .map(|id| (*id).to_string())
            .collect()
    } else {
        inventory_providers
    };

    CapabilityPlanStep {
        capability,
        descriptor: capability_descriptor(capability),
        availability,
        requirements,
        required_tools,
        required_providers,
        required_permissions: capability_permission_requirements(capability),
        tools_resolved,
        providers_resolved,
    }
}

fn unique_preserve_order(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}
