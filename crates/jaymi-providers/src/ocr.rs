//! OCR Provider interface and placeholder implementation.
//!
//! OCR engines plug in behind [`OcrProvider`]. The Planner never depends on a
//! concrete engine — tools/understanding select a registered provider later.

use std::path::Path;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Stable provider identity for the placeholder OCR provider.
pub const OCR_PROVIDER_ID: &str = "ocr.placeholder";

/// Engine label used when no OCR backend is integrated.
pub const OCR_ENGINE_NONE: &str = "none";

/// Input image for an OCR extraction request.
#[derive(Debug, Clone)]
pub struct OcrImage<'a> {
    /// Raw image bytes.
    pub bytes: &'a [u8],
    /// Optional MIME type hint (`image/png`, `image/jpeg`, …).
    pub mime_hint: Option<&'a str>,
    /// Optional source path for diagnostics / logging.
    pub path: Option<&'a Path>,
}

impl<'a> OcrImage<'a> {
    /// Build an OCR image request from raw bytes.
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            mime_hint: None,
            path: None,
        }
    }

    /// Attach a MIME hint.
    pub fn with_mime_hint(mut self, mime: &'a str) -> Self {
        self.mime_hint = Some(mime);
        self
    }

    /// Attach a source path.
    pub fn with_path(mut self, path: &'a Path) -> Self {
        self.path = Some(path);
        self
    }
}

/// Successful OCR extraction result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrExtraction {
    /// Extracted plain text.
    pub text: String,
    /// Provider that produced the result.
    pub provider_id: String,
    /// Engine identifier behind the provider.
    pub engine_id: String,
}

/// Runtime status of an OCR provider (for diagnostics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrProviderStatus {
    /// Provider identity id.
    pub provider_id: String,
    /// Human-readable provider name.
    pub name: String,
    /// Declared OCR engine (`none` for the placeholder).
    pub engine: String,
    /// True when an OCR engine is integrated and ready.
    pub available: bool,
    /// True when this is an intentional placeholder.
    pub placeholder: bool,
    /// Whether the provider finished initialization.
    pub initialized: bool,
    /// Short detail string for diagnostics.
    pub detail: String,
}

/// OCR-specific provider surface.
///
/// Implementations also implement [`Provider`] so they register through the
/// shared Provider framework. Swapping engines does not require Planner changes.
pub trait OcrProvider: Provider {
    /// Current OCR readiness for diagnostics.
    fn ocr_status(&self) -> OcrProviderStatus;

    /// Extract text from an image.
    ///
    /// Placeholder providers return an error explaining that no engine is
    /// integrated. Real engines return structured [`OcrExtraction`].
    fn extract_text(&self, image: &OcrImage<'_>) -> JaymiResult<OcrExtraction>;
}

/// Placeholder OCR provider — architecture only, no OCR engine.
#[derive(Debug)]
pub struct PlaceholderOcrProvider {
    identity: ProviderIdentity,
    initialized: bool,
}

impl PlaceholderOcrProvider {
    /// Create an uninitialized placeholder OCR provider.
    pub fn new() -> Self {
        Self {
            identity: ProviderIdentity {
                id: OCR_PROVIDER_ID.to_string(),
                name: "Placeholder OCR".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "OCR provider placeholder — no engine integrated".to_string(),
                category: ProviderCategory::Local,
                author: "jaymi".to_string(),
                capabilities: vec![Capability::Ocr, Capability::Vision],
            },
            initialized: false,
        }
    }

    /// Returns true after initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Default for PlaceholderOcrProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for PlaceholderOcrProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> JaymiResult<()> {
        if self.initialized {
            // Healthy as a registered placeholder; engine availability is separate.
            Ok(())
        } else {
            Err(JaymiError::new(
                "placeholder OCR provider is not initialized",
            ))
        }
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

impl OcrProvider for PlaceholderOcrProvider {
    fn ocr_status(&self) -> OcrProviderStatus {
        OcrProviderStatus {
            provider_id: self.identity.id.clone(),
            name: self.identity.name.clone(),
            engine: OCR_ENGINE_NONE.to_string(),
            available: false,
            placeholder: true,
            initialized: self.initialized,
            detail: if self.initialized {
                "placeholder · engine=none · no OCR engine integrated".to_string()
            } else {
                "uninitialized placeholder".to_string()
            },
        }
    }

    fn extract_text(&self, image: &OcrImage<'_>) -> JaymiResult<OcrExtraction> {
        if !self.initialized {
            return Err(JaymiError::new(
                "placeholder OCR provider is not initialized",
            ));
        }
        let source = image
            .path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| format!("{} bytes", image.bytes.len()));
        Err(JaymiError::new(format!(
            "OCR engine not integrated (provider={OCR_PROVIDER_ID}, source={source})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ProviderRegistry;
    use jaymi_core::Lifecycle;

    #[test]
    fn placeholder_registers_through_provider_registry() {
        let mut registry = ProviderRegistry::new();
        registry.initialize().unwrap();

        let mut ocr = PlaceholderOcrProvider::new();
        ocr.initialize().unwrap();
        registry.register(&ocr).unwrap();

        let listed = registry.list().unwrap();
        assert!(listed.iter().any(|identity| identity.id == OCR_PROVIDER_ID));
        assert!(listed
            .iter()
            .any(|identity| identity.capabilities.contains(&Capability::Ocr)));
    }

    #[test]
    fn placeholder_extract_text_fails_without_engine() {
        let mut ocr = PlaceholderOcrProvider::new();
        ocr.initialize().unwrap();
        let bytes = b"not-an-image";
        let error = ocr
            .extract_text(&OcrImage::from_bytes(bytes))
            .unwrap_err();
        assert!(error.message().contains("not integrated"));
        let status = ocr.ocr_status();
        assert!(status.placeholder);
        assert!(!status.available);
        assert_eq!(status.engine, OCR_ENGINE_NONE);
    }

    #[test]
    fn uninitialized_provider_fails_health_and_extract() {
        let ocr = PlaceholderOcrProvider::new();
        assert!(ocr.health_check().is_err());
        assert!(ocr
            .extract_text(&OcrImage::from_bytes(b""))
            .unwrap_err()
            .message()
            .contains("not initialized"));
    }
}
