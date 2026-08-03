//! Scan Filesystem Tool — recursive discovery into the knowledge inventory.

use std::path::PathBuf;
use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{EntryType, FileEntry, JaymiError, JaymiResult};
use jaymi_discovery::DiscoveryEngine;
use jaymi_providers::FILESYSTEM_PROVIDER_ID;

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const SCAN_FILESYSTEM_TOOL_ID: &str = "scan_filesystem";

/// Tool that recursively scans directories into the discovery inventory.
pub struct ScanFilesystemTool {
    metadata: ToolMetadata,
    discovery: Arc<DiscoveryEngine>,
}

impl ScanFilesystemTool {
    /// Create a scan tool bound to the discovery engine.
    pub fn new(discovery: Arc<DiscoveryEngine>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: SCAN_FILESYSTEM_TOOL_ID.to_string(),
                name: "Scan Filesystem".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Recursively discover files and folders into the knowledge inventory"
                    .to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Index],
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
            discovery,
        }
    }
}

impl Tool for ScanFilesystemTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        if let Some(path) = &input.path {
            if path.as_os_str().is_empty() {
                return Err(JaymiError::new("scan root must not be empty"));
            }
            return Ok(());
        }
        if self.discovery.configured_roots().is_empty() {
            return Err(JaymiError::new(
                "scan requires a path or configured discovery_roots",
            ));
        }
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let roots: Vec<PathBuf> = if let Some(path) = &input.path {
            vec![path.clone()]
        } else {
            self.discovery.configured_roots().to_vec()
        };

        let report = self.discovery.scan(&roots)?;
        let message = format!(
            "Indexed under {} root(s) in {}ms: visited_files={} visited_folders={} added={} updated={} removed={} unchanged={}",
            report.roots.len(),
            report.duration.as_millis(),
            report.files_seen,
            report.folders_seen,
            report.added,
            report.updated,
            report.removed,
            report.unchanged
        );
        Ok(ToolOutput {
            success: true,
            entries: Vec::new(),
            citations: Vec::new(),
            document: None,
            parser_id: None,
            message: Some(message),
            listed_path: None,
            project_knowledge: Vec::new(),
                    ..Default::default()
        })
    }
}

/// Map discovery items into FileEntry rows for Planner responses.
pub fn discovered_to_entries(
    items: impl IntoIterator<Item = jaymi_discovery::DiscoveredItem>,
) -> Vec<FileEntry> {
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
