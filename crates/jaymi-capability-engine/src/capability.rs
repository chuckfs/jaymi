//! Stable capability identifiers.

/// Stable capability identifiers.
///
/// Capabilities describe behavior independently from tools and providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Conversational interaction.
    Chat,
    /// Semantic or keyword search.
    Search,
    /// Software development assistance.
    Code,
    /// Visual understanding.
    Vision,
    /// Optical character recognition (image → text).
    Ocr,
    /// Generate and compare semantic embeddings (model-agnostic).
    Embeddings,
    /// Image generation.
    GenerateImages,
    /// Web browsing.
    BrowseTheWeb,
    /// Document reading and parsing.
    ReadDocuments,
    /// Query the local knowledge inventory of discovered files.
    Discover,
    /// Recursively scan directories into the knowledge inventory.
    Index,
    /// File organization.
    OrganizeFiles,
    /// Terminal command execution.
    ExecuteTerminalCommands,
    /// Task automation.
    AutomateTasks,
    /// General file management.
    FileManagement,
    /// Internet access.
    Internet,
    /// General automation.
    Automation,
}

impl Capability {
    /// Stable string identity for diagnostics and registries.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Search => "search",
            Self::Code => "code",
            Self::Vision => "vision",
            Self::Ocr => "ocr",
            Self::Embeddings => "embeddings",
            Self::GenerateImages => "generate_images",
            Self::BrowseTheWeb => "browse_the_web",
            Self::ReadDocuments => "read_documents",
            Self::Discover => "discover",
            Self::Index => "index",
            Self::OrganizeFiles => "organize_files",
            Self::ExecuteTerminalCommands => "execute_terminal_commands",
            Self::AutomateTasks => "automate_tasks",
            Self::FileManagement => "file_management",
            Self::Internet => "internet",
            Self::Automation => "automation",
        }
    }

    /// Parse a stable capability id.
    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim() {
            "chat" => Some(Self::Chat),
            "search" => Some(Self::Search),
            "code" => Some(Self::Code),
            "vision" => Some(Self::Vision),
            "ocr" => Some(Self::Ocr),
            "embeddings" => Some(Self::Embeddings),
            "generate_images" => Some(Self::GenerateImages),
            "browse_the_web" => Some(Self::BrowseTheWeb),
            "read_documents" => Some(Self::ReadDocuments),
            "discover" => Some(Self::Discover),
            "index" => Some(Self::Index),
            "organize_files" => Some(Self::OrganizeFiles),
            "execute_terminal_commands" => Some(Self::ExecuteTerminalCommands),
            "automate_tasks" => Some(Self::AutomateTasks),
            "file_management" => Some(Self::FileManagement),
            "internet" => Some(Self::Internet),
            "automation" => Some(Self::Automation),
            _ => None,
        }
    }

    /// All known built-in capabilities (registration order for diagnostics).
    pub fn all() -> &'static [Capability] {
        &[
            Self::Chat,
            Self::Search,
            Self::Code,
            Self::Vision,
            Self::Ocr,
            Self::Embeddings,
            Self::GenerateImages,
            Self::BrowseTheWeb,
            Self::ReadDocuments,
            Self::Discover,
            Self::Index,
            Self::OrganizeFiles,
            Self::ExecuteTerminalCommands,
            Self::AutomateTasks,
            Self::FileManagement,
            Self::Internet,
            Self::Automation,
        ]
    }
}
