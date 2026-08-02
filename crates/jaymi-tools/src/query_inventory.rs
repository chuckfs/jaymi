//! Query Inventory Tool — answer discovery questions from the Knowledge API.

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{DiscoveryQueryKind, EntryType, FileEntry, JaymiResult};
use jaymi_discovery::normalize_path;
use jaymi_knowledge::{
    KnowledgeQuery, KnowledgeSort, KnowledgeStore, RecentKind, SqliteKnowledgeStore,
};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const QUERY_INVENTORY_TOOL_ID: &str = "query_inventory";

/// Tool that queries indexed knowledge without scanning the filesystem.
pub struct QueryInventoryTool {
    metadata: ToolMetadata,
    knowledge: Arc<SqliteKnowledgeStore>,
}

impl QueryInventoryTool {
    /// Create a query tool bound to the Knowledge API.
    pub fn new(knowledge: Arc<SqliteKnowledgeStore>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: QUERY_INVENTORY_TOOL_ID.to_string(),
                name: "Query Inventory".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Answer discovery questions from the local knowledge database without filesystem traversal"
                        .to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Discover],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Fast,
                resource_cost: ResourceCost::VeryLow,
                memory_usage: MemoryUsage::Tiny,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::SearchResults,
            },
            knowledge,
        }
    }
}

impl Tool for QueryInventoryTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let kind = input
            .discovery
            .clone()
            .unwrap_or(DiscoveryQueryKind::All);

        match &kind {
            DiscoveryQueryKind::Collections => self.execute_collections(),
            DiscoveryQueryKind::ByCollection { name, immediate } => {
                self.execute_collection(name, *immediate)
            }
            other => {
                let items = match other {
                    DiscoveryQueryKind::All => self.knowledge.query(KnowledgeQuery {
                        limit: Some(10_000),
                        ..KnowledgeQuery::default()
                    })?,
                    DiscoveryQueryKind::ByExtension { extension } => {
                        self.knowledge.by_extension(extension, Some(10_000))?
                    }
                    DiscoveryQueryKind::ByFolder { path, immediate } => {
                        let normalized = normalize_path(input.path.as_ref().unwrap_or(path))?;
                        let key = normalized.to_string_lossy().into_owned();
                        if *immediate {
                            self.knowledge.query(KnowledgeQuery {
                                parent: Some(key),
                                limit: Some(10_000),
                                ..KnowledgeQuery::default()
                            })?
                        } else {
                            self.knowledge.query(KnowledgeQuery {
                                path_prefix: Some(key),
                                limit: Some(10_000),
                                ..KnowledgeQuery::default()
                            })?
                        }
                    }
                    DiscoveryQueryKind::RecentlyModified => {
                        self.knowledge.recent(RecentKind::Modified, 100)?
                    }
                    DiscoveryQueryKind::RecentlyCreated => {
                        self.knowledge.recent(RecentKind::Created, 100)?
                    }
                    DiscoveryQueryKind::Largest => self.knowledge.query(KnowledgeQuery {
                        files_only: true,
                        sort: KnowledgeSort::Largest,
                        limit: Some(100),
                        ..KnowledgeQuery::default()
                    })?,
                    DiscoveryQueryKind::Hidden => self.knowledge.query(KnowledgeQuery {
                        hidden_only: true,
                        limit: Some(10_000),
                        ..KnowledgeQuery::default()
                    })?,
                    DiscoveryQueryKind::EmptyFolders => self.knowledge.query(KnowledgeQuery {
                        empty_folders: true,
                        directories_only: true,
                        limit: Some(10_000),
                        ..KnowledgeQuery::default()
                    })?,
                    DiscoveryQueryKind::Collections
                    | DiscoveryQueryKind::ByCollection { .. } => unreachable!(),
                };
                let rows = items.len();
                let entries = knowledge_to_entries(items);
                success_or_empty(&kind, entries, rows)
            }
        }
    }
}

impl QueryInventoryTool {
    fn execute_collections(&self) -> JaymiResult<ToolOutput> {
        let collections = self.knowledge.list_collections()?;
        let rows = collections.len();
        let entries = collections
            .into_iter()
            .map(|collection| {
                FileEntry::new(
                    collection.name,
                    EntryType::Directory,
                    collection.root,
                    collection.item_count,
                    None,
                )
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Ok(ToolOutput {
                success: true,
                entries,
                document: None,
                parser_id: None,
                message: Some(
                    "No active collections yet (database only). Index well-known folders first."
                        .to_string(),
                ),
            });
        }
        Ok(ToolOutput {
            success: true,
            entries,
            document: None,
            parser_id: None,
            message: Some(format!(
                "Found {rows} collections via discover → query_inventory (database only)"
            )),
        })
    }

    fn execute_collection(&self, name: &str, immediate: bool) -> JaymiResult<ToolOutput> {
        let Some(collection) = self.knowledge.resolve_collection(name)? else {
            return Ok(ToolOutput {
                success: true,
                entries: Vec::new(),
                document: None,
                parser_id: None,
                message: Some(format!(
                    "Collection '{name}' is unknown or not present in the inventory yet (database only)."
                )),
            });
        };
        let items =
            self.knowledge
                .items_in_collection(name, immediate, Some(10_000))?;
        let rows = items.len();
        let entries = knowledge_to_entries(items);
        let kind = DiscoveryQueryKind::ByCollection {
            name: collection.name.clone(),
            immediate,
        };
        if entries.is_empty() {
            return Ok(ToolOutput {
                success: true,
                entries,
                document: None,
                parser_id: None,
                message: Some(format!(
                    "Collection '{}' is empty in the inventory (database only).",
                    collection.name
                )),
            });
        }
        Ok(ToolOutput {
            success: true,
            entries,
            document: None,
            parser_id: None,
            message: Some(format!(
                "Found {rows} inventoried entries for {} via discover → query_inventory (database only)",
                kind.label()
            )),
        })
    }
}

fn knowledge_to_entries(items: Vec<jaymi_knowledge::KnowledgeItem>) -> Vec<FileEntry> {
    items
        .into_iter()
        .map(|item| {
            FileEntry::new(
                item.filename,
                if item.is_directory {
                    EntryType::Directory
                } else {
                    EntryType::File
                },
                item.path,
                item.size,
                item.modified.map(|value| value as u64),
            )
        })
        .collect()
}

fn success_or_empty(
    kind: &DiscoveryQueryKind,
    entries: Vec<FileEntry>,
    rows: usize,
) -> JaymiResult<ToolOutput> {
    if entries.is_empty() {
        return Ok(ToolOutput {
            success: true,
            entries,
            document: None,
            parser_id: None,
            message: Some(format!(
                "No inventory matches for {} (database only). Index files first if the inventory is empty.",
                kind.label()
            )),
        });
    }
    Ok(ToolOutput {
        success: true,
        entries,
        document: None,
        parser_id: None,
        message: Some(format!(
            "Found {rows} inventoried entries for {} via discover → query_inventory (database only)",
            kind.label()
        )),
    })
}
