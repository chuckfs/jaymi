//! Provider interface and identity metadata.

use crate::categories::ProviderCategory;
use jaymi_capabilities::Capability;
use jaymi_core::JaymiResult;

/// Metadata every provider must expose for discovery.
#[derive(Debug, Clone)]
pub struct ProviderIdentity {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: ProviderCategory,
    pub author: String,
    pub capabilities: Vec<Capability>,
}

/// Provider trait — connect, advertise, execute, report.
///
/// Providers do not plan, reason, store memory, build context, or decide.
pub trait Provider: Send + Sync {
    /// Return provider identity and advertised capabilities.
    fn identity(&self) -> &ProviderIdentity;

    /// Initialize the provider after registration.
    fn initialize(&mut self) -> JaymiResult<()>;

    /// Perform a health check before serving requests.
    fn health_check(&self) -> JaymiResult<()>;

    /// Shut down the provider cleanly.
    fn shutdown(&mut self) -> JaymiResult<()>;
}
