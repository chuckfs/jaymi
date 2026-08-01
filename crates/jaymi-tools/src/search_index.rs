//! Search Index Tool — answers “what exists?” from the local knowledge DB.
//!
//! Architecture path:
//! Planner → Search → Search Index Tool → Database

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::JaymiResult;
use jaymi_database::{IndexQuery, Database};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const SEARCH_INDEX_TOOL_ID: &str = "search_index";

/// Tool that queries indexed filesystem metadata.
#[derive(Debug)]
pub struct SearchIndexTool {
    metadata: ToolMetadata,
    database: Arc<Database>,
}

impl SearchIndexTool {
    /// Create a search-index tool bound to the knowledge database.
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: SEARCH_INDEX_TOOL_ID.to_string(),
                name: "Search Index".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Query the local knowledge index for files and folders".to_string(),
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
            database,
        }
    }
}

impl Tool for SearchIndexTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let limit = input.limit.unwrap_or(40);
        let mut query = IndexQuery::new().with_limit(limit);
        if let Some(text) = &input.query {
            query = query.with_text(text.clone());
        }
        if let Some(root) = &input.source_root {
            query = query.with_source_root(root.clone());
        }

        let indexed = self.database.query_files(&query)?;
        let total = self.database.count_files()?;
        let entries = indexed
            .iter()
            .map(|file| file.to_file_entry())
            .collect::<Vec<_>>();

        let scope = input
            .source_root
            .as_deref()
            .unwrap_or("all indexed roots");
        let message = if entries.is_empty() {
            format!(
                "I don't have anything matching that in the knowledge index yet ({scope}). Try asking me to index your files."
            )
        } else {
            format!(
                "Found {} matching item{} in {scope} ({} indexed total).",
                entries.len(),
                if entries.len() == 1 { "" } else { "s" },
                total
            )
        };

        Ok(ToolOutput::index_results(entries, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::{EntryType, FileEntry, Lifecycle};
    use jaymi_database::IndexedFile;
    use std::path::Path;

    #[test]
    fn queries_indexed_metadata() {
        let mut database = Database::new();
        database.initialize().unwrap();
        let entry = FileEntry::new(
            "report.pdf",
            EntryType::File,
            "/tmp/Documents/report.pdf",
            10,
            None,
        );
        let indexed = IndexedFile::from_entry(&entry, "documents");
        database
            .replace_root_files("documents", Path::new("/tmp/Documents"), &[indexed])
            .unwrap();

        let tool = SearchIndexTool::new(Arc::new(database));
        let output = tool
            .execute(&ToolInput::search_index(
                Some("report".into()),
                Some("documents".into()),
                Some(10),
            ))
            .unwrap();
        assert!(output.success);
        assert_eq!(output.entries.len(), 1);
        assert_eq!(output.entries[0].name, "report.pdf");
    }
}
