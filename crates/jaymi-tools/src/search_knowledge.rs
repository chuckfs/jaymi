//! Search Knowledge Tool — retrieve inventory hits through the Search Engine.

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{format_citations, JaymiResult, SearchRequest};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;
use jaymi_search::{hits_to_citations, hits_to_entries, SearchEngine, SearchEngineApi};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const SEARCH_KNOWLEDGE_TOOL_ID: &str = "search_knowledge";

/// Tool that runs structured searches through the Search Engine.
///
/// Architecture path:
/// Planner → Search capability → Search Knowledge Tool → Search Engine → Knowledge Store
pub struct SearchKnowledgeTool {
    metadata: ToolMetadata,
    search: Arc<SearchEngine>,
}

impl SearchKnowledgeTool {
    /// Create a search tool bound to the Search Engine.
    pub fn new(search: Arc<SearchEngine>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: SEARCH_KNOWLEDGE_TOOL_ID.to_string(),
                name: "Search Knowledge".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Search the local knowledge inventory through the unified Search Engine"
                        .to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Search],
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

impl Tool for SearchKnowledgeTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        match &input.search {
            Some(request) if request.has_primary_dimension() => Ok(()),
            Some(_) => Ok(()), // empty request browses inventory via metadata strategy
            None => Ok(()),
        }
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let request = input.search.clone().unwrap_or_else(|| SearchRequest {
            limit: Some(100),
            ..SearchRequest::default()
        });
        let results = self.search.search(&request)?;
        let rows = results.len();
        let entries = hits_to_entries(&results.hits);
        let citations = hits_to_citations(&results.hits);
        if entries.is_empty() {
            return Ok(ToolOutput {
                success: true,
                entries,
                citations,
                message: Some(format!(
                    "No search matches via search → search_knowledge (strategy={})",
                    results.strategy
                )),
                ..Default::default()
            });
        }
        let mut message = format!(
            "Found {rows} search hits via search → search_knowledge (strategy={} duration_ms={} citations={})",
            results.strategy,
            results.duration_ms,
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
