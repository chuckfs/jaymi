//! Capability metadata — what Jaymi knows how to do.

use crate::Capability;

/// Broad grouping for capability catalogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityCategory {
    /// Conversational interaction.
    Conversation,
    /// Knowledge discovery and retrieval.
    Knowledge,
    /// Software and project work.
    Development,
    /// Media understanding and creation.
    Media,
    /// Filesystem and local resources.
    Filesystem,
    /// Network / internet access.
    Network,
    /// Automation and orchestration.
    Automation,
}

impl CapabilityCategory {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Knowledge => "knowledge",
            Self::Development => "development",
            Self::Media => "media",
            Self::Filesystem => "filesystem",
            Self::Network => "network",
            Self::Automation => "automation",
        }
    }
}

/// Availability of a registered (or requested) capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityAvailability {
    /// Capability is registered and ready for planning.
    Available,
    /// Capability exists in the catalog but is not registered.
    Unregistered,
    /// Capability Engine is not ready.
    EngineNotReady,
    /// Capability id is unknown to Jaymi.
    Unknown,
}

impl CapabilityAvailability {
    /// True when the Planner may include this capability in a plan.
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unregistered => "unregistered",
            Self::EngineNotReady => "engine_not_ready",
            Self::Unknown => "unknown",
        }
    }
}

/// Description of an abstract capability.
///
/// Descriptors never include tool or provider details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDescriptor {
    /// Stable capability id (`search`, `read_documents`, …).
    pub id: &'static str,
    /// Capability enum value.
    pub capability: Capability,
    /// Short human-readable name.
    pub name: &'static str,
    /// What Jaymi knows how to do (not how).
    pub description: &'static str,
    /// Catalog category.
    pub category: CapabilityCategory,
    /// True when fulfilling this capability typically needs the network.
    pub requires_internet: bool,
    /// True when useful work can happen offline.
    pub offline_capable: bool,
}

/// Built-in metadata for a capability.
pub fn capability_descriptor(capability: Capability) -> CapabilityDescriptor {
    match capability {
        Capability::Chat => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Chat",
            description: "Hold a conversational exchange with the user.",
            category: CapabilityCategory::Conversation,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Search => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Search",
            description: "Find information across local knowledge by meaning or keywords.",
            category: CapabilityCategory::Knowledge,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Code => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Code",
            description: "Assist with software development inside a project.",
            category: CapabilityCategory::Development,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Vision => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Vision",
            description: "Understand visual content such as images and screenshots.",
            category: CapabilityCategory::Media,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Ocr => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "OCR",
            description: "Extract text from images through optical character recognition.",
            category: CapabilityCategory::Media,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Embeddings => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Embeddings",
            description: "Generate and compare semantic embeddings for knowledge retrieval.",
            category: CapabilityCategory::Knowledge,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::GenerateImages => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Generate Images",
            description: "Create images from descriptions.",
            category: CapabilityCategory::Media,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::BrowseTheWeb => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Browse the Web",
            description: "Browse and retrieve information from the web.",
            category: CapabilityCategory::Network,
            requires_internet: true,
            offline_capable: false,
        },
        Capability::ReadDocuments => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Read Documents",
            description: "Read and understand local documents and files.",
            category: CapabilityCategory::Knowledge,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Discover => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Discover",
            description: "Query what exists in the local knowledge inventory.",
            category: CapabilityCategory::Knowledge,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Index => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Index",
            description: "Index directories into Jaymi's local knowledge inventory.",
            category: CapabilityCategory::Filesystem,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::OrganizeFiles => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Organize Files",
            description: "Organize files and folders on the local filesystem.",
            category: CapabilityCategory::Filesystem,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::ExecuteTerminalCommands => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Execute Terminal Commands",
            description: "Run terminal commands on behalf of the user.",
            category: CapabilityCategory::Automation,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::AutomateTasks => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Automate Tasks",
            description: "Automate multi-step tasks under user control.",
            category: CapabilityCategory::Automation,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::FileManagement => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "File Management",
            description: "Manage local files as a general filesystem capability.",
            category: CapabilityCategory::Filesystem,
            requires_internet: false,
            offline_capable: true,
        },
        Capability::Internet => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Internet",
            description: "Access the internet when explicitly allowed.",
            category: CapabilityCategory::Network,
            requires_internet: true,
            offline_capable: false,
        },
        Capability::Automation => CapabilityDescriptor {
            id: capability.id(),
            capability,
            name: "Automation",
            description: "Coordinate automated work without performing it directly.",
            category: CapabilityCategory::Automation,
            requires_internet: false,
            offline_capable: true,
        },
    }
}
