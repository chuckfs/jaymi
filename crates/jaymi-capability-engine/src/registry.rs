//! Capability registration surface.

use std::collections::BTreeSet;

use jaymi_core::{JaymiError, JaymiResult};

use crate::{capability_descriptor, Capability, CapabilityDescriptor};

/// Registry of capabilities known to Jaymi.
///
/// Capabilities are registered independently from tools. The Capability Engine
/// owns this registry; the Planner queries the engine rather than hardcoding
/// available behaviors.
#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    initialized: bool,
    capabilities: BTreeSet<&'static str>,
    entries: Vec<Capability>,
}

impl CapabilityRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a capability. Idempotent for duplicates.
    pub fn register(&mut self, capability: Capability) -> JaymiResult<()> {
        ensure_initialized(self.initialized)?;
        if self.capabilities.insert(capability.id()) {
            self.entries.push(capability);
        }
        Ok(())
    }

    /// Number of registered capabilities.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no capabilities are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns true when the capability is registered.
    pub fn contains(&self, capability: Capability) -> bool {
        self.capabilities.contains(capability.id())
    }

    /// Returns true when a stable id is registered.
    pub fn contains_id(&self, id: &str) -> bool {
        self.capabilities.contains(id)
    }

    /// List all registered capabilities in registration order.
    pub fn list(&self) -> Vec<Capability> {
        self.entries.clone()
    }

    /// Resolve a registered capability by stable id.
    pub fn resolve(&self, id: &str) -> Option<Capability> {
        let id = id.trim();
        if !self.capabilities.contains(id) {
            return None;
        }
        Capability::from_id(id)
    }

    /// Describe registered capabilities (metadata only).
    pub fn list_descriptors(&self) -> Vec<CapabilityDescriptor> {
        self.entries
            .iter()
            .copied()
            .map(capability_descriptor)
            .collect()
    }

    /// Returns true after successful initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub(crate) fn mark_initialized(&mut self) {
        self.initialized = true;
    }

    pub(crate) fn clear(&mut self) {
        self.capabilities.clear();
        self.entries.clear();
        self.initialized = false;
    }
}

/// Ensure a registry has been initialized before mutation.
pub(crate) fn ensure_initialized(initialized: bool) -> JaymiResult<()> {
    if initialized {
        Ok(())
    } else {
        Err(JaymiError::new(
            "capability registry is not initialized".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CapabilityEngineApi;
    use jaymi_core::Lifecycle;
    use crate::CapabilityEngine;

    #[test]
    fn register_requires_initialization() {
        let mut registry = CapabilityRegistry::new();
        assert!(registry.register(Capability::Chat).is_err());

        // Lifecycle lives on the Capability Engine; mark via engine for realism.
        let mut engine = CapabilityEngine::new();
        engine.initialize().unwrap();
        engine.register(Capability::Chat).unwrap();
        engine.register(Capability::Chat).unwrap();
        assert_eq!(engine.len(), 1);
        assert!(engine.contains(Capability::Chat));
    }
}
