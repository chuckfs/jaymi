//! Provider registration surface.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::provider::{Provider, ProviderIdentity};
use jaymi_core::{JaymiError, JaymiResult};

/// Registry of installed providers.
///
/// Registration only — providers are not executed in this milestone.
#[derive(Debug, Default)]
pub struct ProviderRegistry {
    initialized: bool,
    providers: RwLock<HashMap<String, ProviderIdentity>>,
}

impl ProviderRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self {
            initialized: false,
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register provider identity metadata.
    ///
    /// Accepts a [`Provider`] so future milestones can store the instance.
    /// This milestone records identity only.
    pub fn register(&self, provider: &dyn Provider) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let identity = provider.identity().clone();
        let mut guard = self
            .providers
            .write()
            .map_err(|_| JaymiError::new("provider registry lock poisoned"))?;
        if guard.contains_key(&identity.id) {
            return Err(JaymiError::new(format!(
                "provider already registered: {}",
                identity.id
            )));
        }
        guard.insert(identity.id.clone(), identity);
        Ok(())
    }

    /// Register a provider identity directly (useful for tests and stubs).
    pub fn register_identity(&self, identity: ProviderIdentity) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let mut guard = self
            .providers
            .write()
            .map_err(|_| JaymiError::new("provider registry lock poisoned"))?;
        if guard.contains_key(&identity.id) {
            return Err(JaymiError::new(format!(
                "provider already registered: {}",
                identity.id
            )));
        }
        guard.insert(identity.id.clone(), identity);
        Ok(())
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers
            .read()
            .map(|guard| guard.len())
            .unwrap_or(0)
    }

    /// Returns true when no providers are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// List registered provider identities.
    pub fn list(&self) -> JaymiResult<Vec<ProviderIdentity>> {
        let guard = self
            .providers
            .read()
            .map_err(|_| JaymiError::new("provider registry lock poisoned"))?;
        let mut identities: Vec<ProviderIdentity> = guard.values().cloned().collect();
        identities.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(identities)
    }

    /// Look up a registered provider identity by id.
    pub fn get(&self, id: &str) -> JaymiResult<Option<ProviderIdentity>> {
        let guard = self
            .providers
            .read()
            .map_err(|_| JaymiError::new("provider registry lock poisoned"))?;
        Ok(guard.get(id).cloned())
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
            .providers
            .write()
            .map_err(|_| JaymiError::new("provider registry lock poisoned"))?;
        guard.clear();
        self.initialized = false;
        Ok(())
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new(
                "provider registry is not initialized".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::ProviderCategory;
    use jaymi_core::Lifecycle;

    fn sample_identity(id: &str) -> ProviderIdentity {
        ProviderIdentity {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            description: "test".to_string(),
            category: ProviderCategory::Local,
            author: "jaymi".to_string(),
            capabilities: vec![],
        }
    }

    #[test]
    fn register_identity_after_init() {
        let mut registry = ProviderRegistry::new();
        assert!(registry.register_identity(sample_identity("fs")).is_err());
        registry.initialize().unwrap();
        registry.register_identity(sample_identity("fs")).unwrap();
        assert_eq!(registry.len(), 1);
    }
}
