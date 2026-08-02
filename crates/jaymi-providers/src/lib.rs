//! Provider framework for Jaymi.
//!
//! Providers connect Jaymi to resources. They expose consistent interfaces and
//! never make decisions. The Planner never communicates with external systems
//! directly — every interaction flows through providers via tools.

#![forbid(unsafe_code)]

pub mod categories;
pub mod filesystem;
pub mod lifecycle;
pub mod manager;
pub mod ocr;
pub mod provider;
pub mod registry;

pub use categories::ProviderCategory;
pub use filesystem::{FilesystemProvider, FILESYSTEM_PROVIDER_ID};
pub use lifecycle::ProviderLifecycle;
pub use manager::ProviderManager;
pub use ocr::{
    OcrExtraction, OcrImage, OcrProvider, OcrProviderStatus, PlaceholderOcrProvider,
    OCR_ENGINE_NONE, OCR_PROVIDER_ID,
};
pub use provider::{Provider, ProviderIdentity};
pub use registry::ProviderRegistry;

use jaymi_core::{HealthReport, JaymiResult, Lifecycle};

const NAME: &str = "provider_registry";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
    "context_engine",
    "capability_registry",
];

impl Lifecycle for ProviderRegistry {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.mark_initialized();
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.is_initialized(),
            self.is_initialized(),
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.clear()
    }
}
