//! Capability discovery — what Jaymi can currently do.
//!
//! Discovery combines the capability catalog with a runtime inventory of tools
//! and providers. It never executes work.

use crate::descriptor::{capability_descriptor, CapabilityDescriptor};
use crate::Capability;

/// A tool observed during discovery (metadata only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTool {
    /// Stable tool id.
    pub id: String,
    /// Capabilities this tool advertises.
    pub capabilities: Vec<Capability>,
}

/// A provider observed during discovery (metadata only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredProvider {
    /// Stable provider id.
    pub id: String,
    /// Capabilities this provider advertises.
    pub capabilities: Vec<Capability>,
}

/// Runtime inventory used for capability discovery.
///
/// Built by the Planner from tool/provider registries. The Capability Engine
/// stays independent of those crates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityInventory {
    /// Registered tools and the capabilities they fulfill.
    pub tools: Vec<DiscoveredTool>,
    /// Registered providers and the capabilities they advertise.
    pub providers: Vec<DiscoveredProvider>,
}

impl CapabilityInventory {
    /// Tools that advertise `capability`.
    pub fn tools_for(&self, capability: Capability) -> Vec<&DiscoveredTool> {
        self.tools
            .iter()
            .filter(|tool| tool.capabilities.contains(&capability))
            .collect()
    }

    /// Providers that advertise `capability`.
    pub fn providers_for(&self, capability: Capability) -> Vec<&DiscoveredProvider> {
        self.providers
            .iter()
            .filter(|provider| provider.capabilities.contains(&capability))
            .collect()
    }
}

/// What a capability needs before Jaymi can currently do it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRequirements {
    /// Capability these requirements apply to.
    pub capability: Capability,
    /// At least one tool must advertise this capability.
    pub requires_tool: bool,
    /// At least one provider must advertise this capability.
    pub requires_provider: bool,
    /// Preferred provider ids (hints for diagnostics; not hard-coded bindings).
    pub preferred_providers: Vec<&'static str>,
    /// Preferred tool ids (hints for diagnostics).
    pub preferred_tools: Vec<&'static str>,
}

impl CapabilityRequirements {
    /// Short summary for diagnostics.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.requires_tool {
            parts.push("tool");
        }
        if self.requires_provider {
            parts.push("provider");
        }
        if parts.is_empty() {
            "none".into()
        } else {
            format!("requires {}", parts.join("+"))
        }
    }
}

/// Built-in requirements for a capability.
///
/// Requirements describe *what class of fulfillment* is needed — never how a
/// specific tool or provider performs the work.
pub fn capability_requirements(capability: Capability) -> CapabilityRequirements {
    match capability {
        Capability::Chat => CapabilityRequirements {
            capability,
            requires_tool: false,
            requires_provider: false,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
        Capability::Search => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["filesystem", "embedding.local"],
            preferred_tools: vec!["search_files", "search_knowledge"],
        },
        Capability::Code => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["filesystem"],
            preferred_tools: vec!["editor", "language_server", "terminal", "git"],
        },
        Capability::Vision => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["ocr.placeholder"],
            preferred_tools: vec![],
        },
        Capability::Ocr => CapabilityRequirements {
            capability,
            requires_tool: false,
            requires_provider: true,
            preferred_providers: vec!["ocr.placeholder"],
            preferred_tools: vec![],
        },
        Capability::Embeddings => CapabilityRequirements {
            capability,
            requires_tool: false,
            requires_provider: true,
            preferred_providers: vec!["embedding.local"],
            preferred_tools: vec![],
        },
        Capability::GenerateImages => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
        Capability::BrowseTheWeb => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
        Capability::ReadDocuments => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["filesystem"],
            preferred_tools: vec!["read_file"],
        },
        Capability::Discover => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: false,
            preferred_providers: vec![],
            preferred_tools: vec!["query_inventory"],
        },
        Capability::Index => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["filesystem"],
            preferred_tools: vec!["scan_filesystem"],
        },
        Capability::OrganizeFiles => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["filesystem"],
            preferred_tools: vec![],
        },
        Capability::ExecuteTerminalCommands => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
        Capability::AutomateTasks => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: false,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
        Capability::FileManagement => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec!["filesystem"],
            preferred_tools: vec![],
        },
        Capability::Internet => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: true,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
        Capability::Automation => CapabilityRequirements {
            capability,
            requires_tool: true,
            requires_provider: false,
            preferred_providers: vec![],
            preferred_tools: vec![],
        },
    }
}

/// Why a capability is not currently usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityBlocker {
    /// Capability Engine has not been initialized.
    EngineNotReady,
    /// Capability is not registered with the engine.
    NotRegistered,
    /// A fulfilling tool is required but none is registered.
    MissingTool,
    /// A fulfilling provider is required but none is registered.
    MissingProvider,
}

impl CapabilityBlocker {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EngineNotReady => "engine_not_ready",
            Self::NotRegistered => "not_registered",
            Self::MissingTool => "missing_tool",
            Self::MissingProvider => "missing_provider",
        }
    }
}

/// Runtime status of one capability after discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityStatus {
    /// Catalog metadata.
    pub descriptor: CapabilityDescriptor,
    /// Declared tool/provider requirements.
    pub requirements: CapabilityRequirements,
    /// True when registered in the Capability Engine.
    pub registered: bool,
    /// Tool ids that currently fulfill this capability.
    pub fulfilling_tools: Vec<String>,
    /// Provider ids that currently advertise this capability.
    pub fulfilling_providers: Vec<String>,
    /// Blockers that prevent current use (empty ⇒ available).
    pub blockers: Vec<CapabilityBlocker>,
}

impl CapabilityStatus {
    /// True when Jaymi can currently use this capability.
    pub fn is_available(&self) -> bool {
        self.blockers.is_empty()
    }

    /// Short status label.
    pub fn status_label(&self) -> &'static str {
        if self.is_available() {
            "available"
        } else {
            "unavailable"
        }
    }

    /// Human-readable detail for diagnostics.
    pub fn detail(&self) -> String {
        let req = self.requirements.summary();
        if self.is_available() {
            format!(
                "{} · {} · tools=[{}] · providers=[{}]",
                self.descriptor.id,
                req,
                self.fulfilling_tools.join(","),
                self.fulfilling_providers.join(",")
            )
        } else {
            let blockers: Vec<_> = self.blockers.iter().map(|b| b.as_str()).collect();
            format!(
                "{} · {} · blocked=[{}] · tools=[{}] · providers=[{}]",
                self.descriptor.id,
                req,
                blockers.join(","),
                self.fulfilling_tools.join(","),
                self.fulfilling_providers.join(",")
            )
        }
    }
}

/// Full discovery report: what Jaymi can and cannot currently do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilityDiscoveryReport {
    /// Capabilities that are registered and fulfillable right now.
    pub available: Vec<CapabilityStatus>,
    /// Capabilities that are known but not currently usable.
    pub unavailable: Vec<CapabilityStatus>,
}

impl CapabilityDiscoveryReport {
    /// All assessed statuses (available then unavailable).
    pub fn all(&self) -> Vec<&CapabilityStatus> {
        self.available
            .iter()
            .chain(self.unavailable.iter())
            .collect()
    }

    /// Number of available capabilities.
    pub fn available_count(&self) -> usize {
        self.available.len()
    }

    /// Number of unavailable capabilities.
    pub fn unavailable_count(&self) -> usize {
        self.unavailable.len()
    }

    /// Find status for a capability id.
    pub fn get(&self, id: &str) -> Option<&CapabilityStatus> {
        self.all()
            .into_iter()
            .find(|status| status.descriptor.id == id)
    }

    /// Compact summary for logs / diagnostics.
    pub fn summary(&self) -> String {
        let available_ids: Vec<_> = self
            .available
            .iter()
            .map(|status| status.descriptor.id)
            .collect();
        let unavailable_ids: Vec<_> = self
            .unavailable
            .iter()
            .map(|status| status.descriptor.id)
            .collect();
        format!(
            "available={} [{}] · unavailable={} [{}]",
            self.available_count(),
            available_ids.join(","),
            self.unavailable_count(),
            unavailable_ids.join(",")
        )
    }
}

/// Assess one capability against registration + inventory.
pub(crate) fn assess_capability(
    capability: Capability,
    registered: bool,
    engine_ready: bool,
    inventory: &CapabilityInventory,
) -> CapabilityStatus {
    let descriptor = capability_descriptor(capability);
    let requirements = capability_requirements(capability);
    let fulfilling_tools: Vec<String> = inventory
        .tools_for(capability)
        .into_iter()
        .map(|tool| tool.id.clone())
        .collect();
    let fulfilling_providers: Vec<String> = inventory
        .providers_for(capability)
        .into_iter()
        .map(|provider| provider.id.clone())
        .collect();

    let mut blockers = Vec::new();
    if !engine_ready {
        blockers.push(CapabilityBlocker::EngineNotReady);
    } else if !registered {
        blockers.push(CapabilityBlocker::NotRegistered);
    } else {
        if requirements.requires_tool && fulfilling_tools.is_empty() {
            blockers.push(CapabilityBlocker::MissingTool);
        }
        if requirements.requires_provider && fulfilling_providers.is_empty() {
            blockers.push(CapabilityBlocker::MissingProvider);
        }
    }

    CapabilityStatus {
        descriptor,
        requirements,
        registered,
        fulfilling_tools,
        fulfilling_providers,
        blockers,
    }
}
