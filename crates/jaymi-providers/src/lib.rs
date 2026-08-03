//! Provider framework for Jaymi.
//!
//! Providers connect Jaymi to resources. They expose consistent interfaces and
//! never make decisions.
//!
//! Active architecture:
//! - [`ProviderRegistry`] — discovery / diagnostics (identity metadata only)
//! - Concrete provider instances (`Arc`s) — bound into tools at boot
//! - Tools / Capability Engine — select and execute; there is no ProviderManager
//!
//! The Planner never communicates with external systems directly — every
//! interaction flows through tools that hold bound providers.

#![forbid(unsafe_code)]

pub mod categories;
pub mod embedding;
pub mod filesystem;
pub mod git;
pub mod lsp;
pub mod ocr;
pub mod provider;
pub mod registry;
pub mod terminal;

pub use categories::ProviderCategory;
pub use embedding::{
    cosine_similarity, EmbeddingProvider, EmbeddingProviderStatus, EmbeddingVector,
    LocalEmbeddingProvider, EMBEDDING_PROVIDER_ID, LOCAL_EMBEDDING_DIMS, LOCAL_EMBEDDING_MODEL,
};
pub use filesystem::{FilesystemProvider, FILESYSTEM_PROVIDER_ID};
pub use git::{GitProvider, GitStatusSnapshot, GIT_PROVIDER_ID};
pub use lsp::{
    resolve_lsp_command, LspOperationResult, LspProvider, DEFAULT_LSP_COMMAND, LSP_PROVIDER_ID,
    MOCK_LSP_COMMAND,
};
pub use ocr::{
    OcrExtraction, OcrImage, OcrProvider, OcrProviderStatus, PlaceholderOcrProvider,
    OCR_ENGINE_NONE, OCR_PROVIDER_ID,
};
pub use provider::{Provider, ProviderIdentity};
pub use registry::ProviderRegistry;
pub use terminal::{
    TerminalCommandResult, TerminalProvider, DEFAULT_TERMINAL_SESSION_ID, TERMINAL_PROVIDER_ID,
};

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
    "capability_engine",
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
