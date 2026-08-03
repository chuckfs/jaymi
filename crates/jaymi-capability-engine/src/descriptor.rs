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

/// How far along a capability is — conceptual catalog vs executable now.
///
/// Capabilities always remain in the catalog. Availability says whether Jaymi
/// can currently fulfill them (Ready / Experimental), intends them for later
/// (Planned), or cannot fulfill them right now (Unavailable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityAvailability {
    /// Conceptual + currently executable (stable fulfillment).
    Ready,
    /// Conceptual + currently executable (partial / stub fulfillment).
    Experimental,
    /// Conceptual catalog support; intentionally not executable yet.
    Planned,
    /// Known, but blocked right now (engine down, missing tools/providers).
    Unavailable,
    /// Capability id is unknown to Jaymi.
    Unknown,
}

impl CapabilityAvailability {
    /// True when the Planner may execute tools for this capability.
    pub fn is_executable_tier(self) -> bool {
        matches!(self, Self::Ready | Self::Experimental)
    }

    /// Alias used by plans: executable-tier capabilities are "available".
    pub fn is_available(self) -> bool {
        self.is_executable_tier()
    }

    /// Stable label for diagnostics and UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Experimental => "experimental",
            Self::Planned => "planned",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

/// Catalog maturity default for a capability (ignores live inventory).
pub fn catalog_availability(capability: Capability) -> CapabilityAvailability {
    match capability {
        Capability::Search
        | Capability::ReadDocuments
        | Capability::Discover
        | Capability::Index => CapabilityAvailability::Ready,
        Capability::Code
        | Capability::Ocr
        | Capability::Embeddings
        | Capability::Vision => CapabilityAvailability::Experimental,
        Capability::Chat
        | Capability::GenerateImages
        | Capability::BrowseTheWeb
        | Capability::OrganizeFiles
        | Capability::ExecuteTerminalCommands
        | Capability::AutomateTasks
        | Capability::FileManagement
        | Capability::Internet
        | Capability::Automation => CapabilityAvailability::Planned,
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
    /// Catalog maturity / availability default.
    pub availability: CapabilityAvailability,
    /// True when fulfilling this capability typically needs the network.
    pub requires_internet: bool,
    /// True when useful work can happen offline.
    pub offline_capable: bool,
}

fn descriptor(
    capability: Capability,
    name: &'static str,
    description: &'static str,
    category: CapabilityCategory,
    requires_internet: bool,
    offline_capable: bool,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: capability.id(),
        capability,
        name,
        description,
        category,
        availability: catalog_availability(capability),
        requires_internet,
        offline_capable,
    }
}

/// Built-in metadata for a capability.
pub fn capability_descriptor(capability: Capability) -> CapabilityDescriptor {
    match capability {
        Capability::Chat => descriptor(
            capability,
            "Chat",
            "Hold a conversational exchange with the user.",
            CapabilityCategory::Conversation,
            false,
            true,
        ),
        Capability::Search => descriptor(
            capability,
            "Search",
            "Find information across local knowledge by meaning or keywords.",
            CapabilityCategory::Knowledge,
            false,
            true,
        ),
        Capability::Code => descriptor(
            capability,
            "Code",
            "Assist with software development inside a project.",
            CapabilityCategory::Development,
            false,
            true,
        ),
        Capability::Vision => descriptor(
            capability,
            "Vision",
            "Understand visual content such as images and screenshots.",
            CapabilityCategory::Media,
            false,
            true,
        ),
        Capability::Ocr => descriptor(
            capability,
            "OCR",
            "Extract text from images through optical character recognition.",
            CapabilityCategory::Media,
            false,
            true,
        ),
        Capability::Embeddings => descriptor(
            capability,
            "Embeddings",
            "Generate and compare semantic embeddings for knowledge retrieval.",
            CapabilityCategory::Knowledge,
            false,
            true,
        ),
        Capability::GenerateImages => descriptor(
            capability,
            "Generate Images",
            "Create images from descriptions.",
            CapabilityCategory::Media,
            false,
            true,
        ),
        Capability::BrowseTheWeb => descriptor(
            capability,
            "Browse the Web",
            "Browse and retrieve information from the web.",
            CapabilityCategory::Network,
            true,
            false,
        ),
        Capability::ReadDocuments => descriptor(
            capability,
            "Read Documents",
            "Read and understand local documents and files.",
            CapabilityCategory::Knowledge,
            false,
            true,
        ),
        Capability::Discover => descriptor(
            capability,
            "Discover",
            "Query what exists in the local knowledge inventory.",
            CapabilityCategory::Knowledge,
            false,
            true,
        ),
        Capability::Index => descriptor(
            capability,
            "Index",
            "Index directories into Jaymi's local knowledge inventory.",
            CapabilityCategory::Filesystem,
            false,
            true,
        ),
        Capability::OrganizeFiles => descriptor(
            capability,
            "Organize Files",
            "Organize files and folders on the local filesystem.",
            CapabilityCategory::Filesystem,
            false,
            true,
        ),
        Capability::ExecuteTerminalCommands => descriptor(
            capability,
            "Execute Terminal Commands",
            "Run terminal commands on behalf of the user.",
            CapabilityCategory::Automation,
            false,
            true,
        ),
        Capability::AutomateTasks => descriptor(
            capability,
            "Automate Tasks",
            "Automate multi-step tasks under user control.",
            CapabilityCategory::Automation,
            false,
            true,
        ),
        Capability::FileManagement => descriptor(
            capability,
            "File Management",
            "Manage local files as a general filesystem capability.",
            CapabilityCategory::Filesystem,
            false,
            true,
        ),
        Capability::Internet => descriptor(
            capability,
            "Internet",
            "Access the internet when explicitly allowed.",
            CapabilityCategory::Network,
            true,
            false,
        ),
        Capability::Automation => descriptor(
            capability,
            "Automation",
            "Coordinate automated work without performing it directly.",
            CapabilityCategory::Automation,
            false,
            true,
        ),
    }
}

/// Compute effective availability from catalog default + runtime state.
pub fn effective_availability(
    catalog: CapabilityAvailability,
    engine_ready: bool,
    registered: bool,
    tools_ok: bool,
    providers_ok: bool,
) -> CapabilityAvailability {
    if !engine_ready {
        return CapabilityAvailability::Unavailable;
    }
    if matches!(catalog, CapabilityAvailability::Unknown) {
        return CapabilityAvailability::Unknown;
    }
    if !registered {
        return CapabilityAvailability::Unavailable;
    }
    if catalog == CapabilityAvailability::Planned {
        return CapabilityAvailability::Planned;
    }
    // Ready or Experimental catalog defaults — inventory must satisfy requirements.
    if tools_ok && providers_ok {
        catalog
    } else {
        CapabilityAvailability::Unavailable
    }
}
