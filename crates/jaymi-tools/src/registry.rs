//! Tool registration surface.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::metadata::ToolMetadata;
use crate::tool::Tool;
use jaymi_core::{JaymiError, JaymiResult};

/// Registry of available tools.
///
/// Registration only — tools are not executed in this milestone.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    initialized: bool,
    tools: RwLock<HashMap<String, ToolMetadata>>,
}

impl ToolRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self {
            initialized: false,
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Register tool metadata from a [`Tool`] implementation.
    pub fn register(&self, tool: &dyn Tool) -> JaymiResult<()> {
        self.register_metadata(tool.metadata().clone())
    }

    /// Register tool metadata directly.
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
        guard.insert(metadata.id.clone(), metadata);
        Ok(())
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
        Ok(guard.values().cloned().collect())
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
            capabilities: vec![],
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
    }
}
