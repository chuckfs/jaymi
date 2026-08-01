//! Capability registration surface.

use std::collections::BTreeSet;

use crate::{ensure_initialized, Capability};
use jaymi_core::JaymiResult;

/// Registry of capabilities known to Jaymi.
///
/// Capabilities are registered independently from tools. The Planner queries
/// this registry rather than hardcoding available behaviors.
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

    /// List all registered capabilities in registration order.
    pub fn list(&self) -> Vec<Capability> {
        self.entries.clone()
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

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::Lifecycle;

    #[test]
    fn register_requires_initialization() {
        let mut registry = CapabilityRegistry::new();
        assert!(registry.register(Capability::Chat).is_err());
        registry.initialize().unwrap();
        registry.register(Capability::Chat).unwrap();
        registry.register(Capability::Chat).unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.contains(Capability::Chat));
    }
}
