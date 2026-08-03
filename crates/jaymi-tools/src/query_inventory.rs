//! Query Inventory Tool — answer discovery questions through the Search Engine.

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{format_citations, DiscoveryQueryKind, JaymiResult};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;
use jaymi_search::{
    hits_to_citations, hits_to_entries, request_from_discovery, SearchEngine, SearchEngineApi,
};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const QUERY_INVENTORY_TOOL_ID: &str = "query_inventory";

/// Tool that queries indexed knowledge through the Search Engine.
///
/// Architecture path:
/// Planner → Discover → Query Inventory Tool → Search Engine → Knowledge Store
pub struct QueryInventoryTool {
    metadata: ToolMetadata,
    search: Arc<SearchEngine>,
}

impl QueryInventoryTool {
    /// Create a query tool bound to the Search Engine.
    pub fn new(search: Arc<SearchEngine>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: QUERY_INVENTORY_TOOL_ID.to_string(),
                name: "Query Inventory".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Answer discovery questions through the Search Engine without filesystem traversal"
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
            search,
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
        let kind = input.discovery.clone().unwrap_or(DiscoveryQueryKind::All);
        let request = request_from_discovery(&kind);
        let results = self.search.search(&request)?;
        let rows = results.len();
        let entries = hits_to_entries(&results.hits);
        let citations = hits_to_citations(&results.hits);
        if entries.is_empty() {
            let message = match &kind {
                DiscoveryQueryKind::Collections => {
                    "No active collections yet. Index well-known folders first.".to_string()
                }
                DiscoveryQueryKind::ByCollection { name, .. } => {
                    format!("Collection '{name}' is unknown or empty in the inventory.")
                }
                other => format!(
                    "No inventory matches for {} via search engine (strategy={}). Index files first if the inventory is empty.",
                    other.label(),
                    results.strategy
                ),
            };
            return Ok(ToolOutput {
                success: true,
                entries,
                citations,
                document: None,
                parser_id: None,
                message: Some(message),
                listed_path: None,
                project_knowledge: Vec::new(),
                        ..Default::default()
        });
        }
        let mut message = format!(
            "Found {rows} inventoried entries for {} via discover → query_inventory → search (strategy={} citations={})",
            kind.label(),
            results.strategy,
            citations.len()
        );
        let cite_block = format_citations(&citations);
        if !cite_block.is_empty() {
            message.push_str("\n\n");
            message.push_str(&cite_block);
        }
        Ok(ToolOutput {
            success: true,
            entries,
            citations,
            document: None,
            parser_id: None,
            message: Some(message),
            listed_path: None,
            project_knowledge: Vec::new(),
                    ..Default::default()
        })
    }
}
