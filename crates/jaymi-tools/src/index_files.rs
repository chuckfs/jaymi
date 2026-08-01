//! Index Files Tool — discovers filesystem metadata and stores it in SQLite.
//!
//! Architecture path:
//! Planner → Search → Index Files Tool → Filesystem Provider + Database

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::JaymiResult;
use jaymi_database::{IndexRoot, IndexedFile, Database};
use jaymi_providers::{
    FilesystemProvider, DEFAULT_WALK_DEPTH, DEFAULT_WALK_LIMIT, FILESYSTEM_PROVIDER_ID,
};

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const INDEX_FILES_TOOL_ID: &str = "index_files";

/// Tool that walks configured roots and upserts metadata into the knowledge DB.
#[derive(Debug)]
pub struct IndexFilesTool {
    metadata: ToolMetadata,
    filesystem: Arc<FilesystemProvider>,
    database: Arc<Database>,
    roots: Vec<IndexRoot>,
}

impl IndexFilesTool {
    /// Create an index tool bound to filesystem, database, and configured roots.
    pub fn new(
        filesystem: Arc<FilesystemProvider>,
        database: Arc<Database>,
        roots: Vec<IndexRoot>,
    ) -> Self {
        Self {
            metadata: ToolMetadata {
                id: INDEX_FILES_TOOL_ID.to_string(),
                name: "Index Files".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Discover filesystem metadata and update the local knowledge index"
                    .to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Search],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Medium,
                resource_cost: ResourceCost::Low,
                memory_usage: MemoryUsage::Small,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::StructuredData,
            },
            filesystem,
            database,
            roots,
        }
    }

    /// Scan all enabled roots into the database.
    pub fn index_all(&self) -> JaymiResult<(usize, Vec<String>)> {
        let mut total = 0usize;
        let mut scanned = Vec::new();
        for root in self.roots.iter().filter(|root| root.enabled) {
            let count = self.index_root(root)?;
            total += count;
            scanned.push(format!("{} ({count})", root.label));
        }
        Ok((total, scanned))
    }

    fn index_root(&self, root: &IndexRoot) -> JaymiResult<usize> {
        let entries =
            self.filesystem
                .walk_directory(&root.path, DEFAULT_WALK_DEPTH, DEFAULT_WALK_LIMIT)?;
        let indexed: Vec<IndexedFile> = entries
            .iter()
            .map(|entry| IndexedFile::from_entry(entry, &root.label))
            .collect();
        self.database
            .replace_root_files(&root.label, &root.path, &indexed)
    }
}

impl Tool for IndexFilesTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, _input: &ToolInput) -> JaymiResult<()> {
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let (total, scanned) = self.index_all()?;
        let message = if scanned.is_empty() {
            "No index roots were configured.".to_string()
        } else {
            format!(
                "Indexed {total} entries across {}.",
                scanned.join(", ")
            )
        };
        Ok(ToolOutput::indexed(total, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_providers::Provider;
    use jaymi_core::Lifecycle;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn indexes_a_root_into_sqlite() {
        let dir = temp_dir();
        write!(File::create(dir.join("a.txt")).unwrap(), "a").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        write!(File::create(dir.join("sub").join("b.md")).unwrap(), "b").unwrap();

        let mut filesystem = FilesystemProvider::new();
        filesystem.initialize().unwrap();
        let mut database = Database::new();
        database.initialize().unwrap();
        let tool = IndexFilesTool::new(
            Arc::new(filesystem),
            Arc::new(database),
            vec![IndexRoot::new("workspace", dir.clone())],
        );

        let output = tool.execute(&ToolInput::index_roots()).unwrap();
        assert!(output.success);
        assert!(output.indexed_count.unwrap_or(0) >= 2);
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-index-tool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
