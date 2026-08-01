//! Provider Manager resolves providers by capability.

use jaymi_capabilities::Capability;
use jaymi_core::JaymiResult;

/// Resolves the best provider match for a requested capability.
#[derive(Debug, Default)]
pub struct ProviderManager;

impl ProviderManager {
    /// Find a healthy registered provider for the given capability.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn resolve(&self, _capability: Capability) -> JaymiResult<Option<String>> {
        Ok(None)
    }
}
