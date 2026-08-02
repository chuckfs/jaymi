//! User-facing request types that enter the Planner.

use std::path::PathBuf;

use crate::search::SearchRequest;

/// Structured discovery query kinds answered from the knowledge database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiscoveryQueryKind {
    /// All inventoried entries.
    #[default]
    All,
    /// Files with a specific extension (no leading dot).
    ByExtension {
        /// Lowercased extension without a leading dot.
        extension: String,
    },
    /// Entries under or in a folder.
    ByFolder {
        /// Folder path.
        path: PathBuf,
        /// When true, only immediate children (`parent = path`).
        immediate: bool,
    },
    /// List active logical collections.
    Collections,
    /// Entries in a named logical collection.
    ByCollection {
        /// Collection name or slug (for example `downloads`).
        name: String,
        /// When true, only immediate children of the collection root.
        immediate: bool,
    },
    /// Files ordered by newest modification time.
    RecentlyModified,
    /// Files ordered by newest creation time.
    RecentlyCreated,
    /// Files ordered by largest size.
    Largest,
    /// Hidden files and folders.
    Hidden,
    /// Folders with no inventoried children.
    EmptyFolders,
}

impl DiscoveryQueryKind {
    /// Stable label for diagnostics and tool messages.
    pub fn label(&self) -> String {
        match self {
            Self::All => "all".to_string(),
            Self::ByExtension { extension } => format!("extension:{extension}"),
            Self::ByFolder { immediate: true, .. } => "by_folder".to_string(),
            Self::ByFolder {
                immediate: false, ..
            } => "under_folder".to_string(),
            Self::Collections => "collections".to_string(),
            Self::ByCollection {
                name,
                immediate: true,
            } => format!("collection:{name}"),
            Self::ByCollection {
                name,
                immediate: false,
            } => format!("collection_under:{name}"),
            Self::RecentlyModified => "recently_modified".to_string(),
            Self::RecentlyCreated => "recently_created".to_string(),
            Self::Largest => "largest".to_string(),
            Self::Hidden => "hidden".to_string(),
            Self::EmptyFolders => "empty_folders".to_string(),
        }
    }
}

/// A request originating from the conversation interface.
///
/// Every interaction begins with understanding intent. The Planner receives
/// this request and coordinates the rest of the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRequest {
    /// Natural-language content provided by the user.
    pub content: String,
    /// Optional structured directory path for list-directory intents.
    ///
    /// When set, the Decision Engine treats this as an explicit list-directory
    /// request without requiring natural-language parsing.
    pub directory: Option<PathBuf>,
    /// Optional structured file path for read-file intents.
    pub file: Option<PathBuf>,
    /// When true, query the persistent discovery inventory.
    pub discover: bool,
    /// Optional structured discovery query kind.
    pub discovery_kind: Option<DiscoveryQueryKind>,
    /// Optional structured root path for an index/discovery scan.
    pub index_root: Option<PathBuf>,
    /// Optional structured Search Engine request.
    pub search: Option<SearchRequest>,
}

impl UserRequest {
    /// Create a new user request from free-form content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            directory: None,
            file: None,
            discover: false,
            discovery_kind: None,
            index_root: None,
            search: None,
        }
    }

    /// Create a structured request to list a single directory.
    pub fn list_directory(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("list {}", path.display()),
            directory: Some(path),
            file: None,
            discover: false,
            discovery_kind: None,
            index_root: None,
            search: None,
        }
    }

    /// Create a structured request to read a single file.
    pub fn read_file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("read {}", path.display()),
            directory: None,
            file: Some(path),
            discover: false,
            discovery_kind: None,
            index_root: None,
            search: None,
        }
    }

    /// Create a structured Search Engine request.
    pub fn search(request: SearchRequest) -> Self {
        let content = request
            .free_text
            .as_ref()
            .map(|text| format!("search {text}"))
            .or_else(|| {
                request
                    .filename
                    .as_ref()
                    .map(|name| format!("find file {name}"))
            })
            .unwrap_or_else(|| "search".to_string());
        Self {
            content,
            directory: None,
            file: None,
            discover: false,
            discovery_kind: None,
            index_root: None,
            search: Some(request),
        }
    }

    /// Create a structured request to query the discovery inventory.
    pub fn discover_inventory() -> Self {
        Self {
            content: "what files exist?".to_string(),
            directory: None,
            file: None,
            discover: true,
            discovery_kind: Some(DiscoveryQueryKind::All),
            index_root: None,
            search: None,
        }
    }

    /// Create a structured discovery query request.
    pub fn discover_query(kind: DiscoveryQueryKind) -> Self {
        let content = match &kind {
            DiscoveryQueryKind::All => "what files exist?".to_string(),
            DiscoveryQueryKind::ByExtension { extension } => {
                format!("{extension} files")
            }
            DiscoveryQueryKind::ByFolder { path, immediate } => {
                if *immediate {
                    format!("files in {}", path.display())
                } else {
                    format!("files under {}", path.display())
                }
            }
            DiscoveryQueryKind::Collections => "show collections".to_string(),
            DiscoveryQueryKind::ByCollection { name, immediate } => {
                if *immediate {
                    format!("what's in {name}?")
                } else {
                    format!("files under {name}")
                }
            }
            DiscoveryQueryKind::RecentlyModified => "recently modified files".to_string(),
            DiscoveryQueryKind::RecentlyCreated => "recently created files".to_string(),
            DiscoveryQueryKind::Largest => "largest files".to_string(),
            DiscoveryQueryKind::Hidden => "hidden files".to_string(),
            DiscoveryQueryKind::EmptyFolders => "empty folders".to_string(),
        };
        Self {
            content,
            directory: None,
            file: None,
            discover: true,
            discovery_kind: Some(kind),
            index_root: None,
            search: None,
        }
    }

    /// Create a structured request to scan a root into the inventory.
    pub fn index_root(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            content: format!("index {}", path.display()),
            directory: None,
            file: None,
            discover: false,
            discovery_kind: None,
            index_root: Some(path),
            search: None,
        }
    }
}
