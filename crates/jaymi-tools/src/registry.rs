//! Tool registration surface.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::metadata::ToolMetadata;
use crate::tool::Tool;
use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};

/// Registry of available tools.
///
/// Stores runnable tool instances so the Tool Orchestrator can execute them.
#[derive(Default)]
pub struct ToolRegistry {
    initialized: bool,
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("initialized", &self.initialized)
            .field("tool_count", &self.len())
            .finish()
    }
}

impl ToolRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self {
            initialized: false,
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Register a runnable tool instance.
    pub fn register_tool(&self, tool: Arc<dyn Tool>) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let id = tool.metadata().id.clone();
        let mut guard = self
            .tools
            .write()
            .map_err(|_| JaymiError::new("tool registry lock poisoned"))?;
        if guard.contains_key(&id) {
            return Err(JaymiError::new(format!("tool already registered: {id}")));
        }
        guard.insert(id, tool);
        Ok(())
    }

    /// Register tool metadata from a [`Tool`] reference.
    pub fn register(&self, tool: &dyn Tool) -> JaymiResult<()> {
        // Metadata-only registration is insufficient for execution. Callers
        // that need execution should use [`Self::register_tool`].
        self.register_metadata(tool.metadata().clone())
    }

    /// Register tool metadata without a runnable instance.
    ///
    /// Prefer [`Self::register_tool`] for executable tools.
    pub fn register_metadata(&self, metadata: ToolMetadata) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let mut guard = self
            .tools
            .write()
            .map_err(|_| JaymiError::new("tool registry lock poisoned"))?;
        if guard.contains_key(&metadata.id) {
            return Err(JaymiError::new(format!(
                "tool already registered: {}",
                metadata.id
            )));
        }
        guard.insert(metadata.id.clone(), Arc::new(MetadataOnlyTool { metadata }));
        Ok(())
    }

    /// Retrieve a registered tool by ID.
    pub fn get(&self, tool_id: &str) -> JaymiResult<Arc<dyn Tool>> {
        let guard = self
            .tools
            .read()
            .map_err(|_| JaymiError::new("tool registry lock poisoned"))?;
        guard
            .get(tool_id)
            .cloned()
            .ok_or_else(|| JaymiError::new(format!("tool not registered: {tool_id}")))
    }

    /// Find the first tool that advertises the given capability.
    pub fn find_for_capability(&self, capability: Capability) -> JaymiResult<Option<Arc<dyn Tool>>> {
        let guard = self
            .tools
            .read()
            .map_err(|_| JaymiError::new("tool registry lock poisoned"))?;
        Ok(guard
            .values()
            .find(|tool| tool.metadata().capabilities.contains(&capability))
            .cloned())
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Returns true when no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// List registered tool metadata.
    pub fn list(&self) -> JaymiResult<Vec<ToolMetadata>> {
        let guard = self
            .tools
            .read()
            .map_err(|_| JaymiError::new("tool registry lock poisoned"))?;
        Ok(guard.values().map(|tool| tool.metadata().clone()).collect())
    }

    /// Returns true after successful initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub(crate) fn mark_initialized(&mut self) {
        self.initialized = true;
    }

    pub(crate) fn clear(&mut self) -> JaymiResult<()> {
        let mut guard = self
            .tools
            .write()
            .map_err(|_| JaymiError::new("tool registry lock poisoned"))?;
        guard.clear();
        self.initialized = false;
        Ok(())
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new(
                "tool registry is not initialized".to_string(),
            ))
        }
    }
}

/// Placeholder tool used when only metadata is registered.
struct MetadataOnlyTool {
    metadata: ToolMetadata,
}

impl Tool for MetadataOnlyTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, _input: &crate::tool::ToolInput) -> JaymiResult<()> {
        Err(JaymiError::new(format!(
            "tool {} has metadata only and cannot execute",
            self.metadata.id
        )))
    }

    fn execute(&self, _input: &crate::tool::ToolInput) -> JaymiResult<crate::tool::ToolOutput> {
        Err(JaymiError::new(format!(
            "tool {} has metadata only and cannot execute",
            self.metadata.id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{
        EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
        PrivacyMode, Reliability, ResourceCost, ResultType,
    };
    use jaymi_core::Lifecycle;

    fn sample_metadata(id: &str) -> ToolMetadata {
        ToolMetadata {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            description: "test".to_string(),
            provider: "none".to_string(),
            capabilities: vec![Capability::Search],
            execution_mode: ExecutionMode::Synchronous,
            estimated_runtime: EstimatedRuntime::Instant,
            resource_cost: ResourceCost::VeryLow,
            memory_usage: MemoryUsage::Tiny,
            gpu_requirements: GpuRequirements::None,
            privacy: PrivacyMode::LocalOnly,
            internet: InternetRequirement::Never,
            reliability: Reliability::Experimental,
            result_type: ResultType::Text,
        }
    }

    #[test]
    fn register_metadata_after_init() {
        let mut registry = ToolRegistry::new();
        assert!(registry.register_metadata(sample_metadata("search")).is_err());
        registry.initialize().unwrap();
        registry
            .register_metadata(sample_metadata("search"))
            .unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry
            .find_for_capability(Capability::Search)
            .unwrap()
            .is_some());
    }
}
