//! Content registry — maps content types to replaceable content parsers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use jaymi_core::{ContentType, JaymiError, JaymiResult};

use crate::parser::ContentParser;

/// Registry of content parsers keyed by [`ContentType`] identity.
///
/// New formats (PDF, DOCX, messages, …) are added by implementing
/// [`ContentParser`] and registering here — no Planner changes required.
#[derive(Default)]
pub struct ContentRegistry {
    initialized: bool,
    parsers: RwLock<HashMap<String, Arc<dyn ContentParser>>>,
    by_type: RwLock<HashMap<String, String>>,
}

impl std::fmt::Debug for ContentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentRegistry")
            .field("initialized", &self.initialized)
            .field("parser_count", &self.len())
            .finish()
    }
}

impl ContentRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the registry ready for registration and lookup.
    pub fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    /// Returns true after initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Register a parser for each of its supported content types.
    ///
    /// Later registrations for the same content type replace earlier ones.
    pub fn register(&self, parser: Arc<dyn ContentParser>) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let parser_id = parser.id().to_string();
        let supported = parser.supported_types().to_vec();

        {
            let mut parsers = self
                .parsers
                .write()
                .map_err(|_| JaymiError::new("content registry lock poisoned"))?;
            parsers.insert(parser_id.clone(), parser);
        }

        let mut by_type = self
            .by_type
            .write()
            .map_err(|_| JaymiError::new("content registry lock poisoned"))?;
        for content_type in supported {
            by_type.insert(content_type.id().to_string(), parser_id.clone());
        }
        Ok(())
    }

    /// Resolve the parser registered for a content type.
    pub fn resolve(&self, content_type: &ContentType) -> JaymiResult<Arc<dyn ContentParser>> {
        self.ensure_initialized()?;
        let by_type = self
            .by_type
            .read()
            .map_err(|_| JaymiError::new("content registry lock poisoned"))?;
        let parser_id = by_type.get(content_type.id()).ok_or_else(|| {
            JaymiError::new(format!(
                "no content parser registered for type {}",
                content_type.id()
            ))
        })?;
        let parsers = self
            .parsers
            .read()
            .map_err(|_| JaymiError::new("content registry lock poisoned"))?;
        parsers
            .get(parser_id)
            .cloned()
            .ok_or_else(|| JaymiError::new(format!("content parser not found: {parser_id}")))
    }

    /// Detect a content type from a filesystem path extension.
    pub fn detect_type(path: &Path) -> Option<ContentType> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "txt" => Some(ContentType::PlainText),
            "md" | "markdown" => Some(ContentType::Markdown),
            "json" => Some(ContentType::Json),
            other => Some(ContentType::Other(other.to_string())),
        }
    }

    /// Number of registered parser implementations.
    pub fn len(&self) -> usize {
        self.parsers.read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Returns true when no parsers are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all registrations and mark uninitialized.
    pub fn clear(&mut self) -> JaymiResult<()> {
        self.parsers
            .write()
            .map_err(|_| JaymiError::new("content registry lock poisoned"))?
            .clear();
        self.by_type
            .write()
            .map_err(|_| JaymiError::new("content registry lock poisoned"))?
            .clear();
        self.initialized = false;
        Ok(())
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("content registry is not initialized"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::JsonParser;
    use crate::markdown::MarkdownParser;
    use crate::plain_text::PlainTextParser;

    #[test]
    fn register_and_resolve_by_type() {
        let mut registry = ContentRegistry::new();
        registry.initialize().unwrap();
        registry.register(Arc::new(PlainTextParser)).unwrap();
        registry.register(Arc::new(MarkdownParser)).unwrap();
        registry.register(Arc::new(JsonParser)).unwrap();

        assert_eq!(registry.len(), 3);
        assert_eq!(
            registry.resolve(&ContentType::PlainText).unwrap().id(),
            "plain_text"
        );
        assert_eq!(
            registry.resolve(&ContentType::Markdown).unwrap().id(),
            "markdown"
        );
        assert_eq!(registry.resolve(&ContentType::Json).unwrap().id(), "json");
    }

    #[test]
    fn detect_type_from_extension() {
        assert_eq!(
            ContentRegistry::detect_type(Path::new("notes.txt")),
            Some(ContentType::PlainText)
        );
        assert_eq!(
            ContentRegistry::detect_type(Path::new("README.md")),
            Some(ContentType::Markdown)
        );
        assert_eq!(
            ContentRegistry::detect_type(Path::new("data.json")),
            Some(ContentType::Json)
        );
        assert_eq!(
            ContentRegistry::detect_type(Path::new("doc.pdf")),
            Some(ContentType::Other("pdf".to_string()))
        );
    }

    #[test]
    fn resolve_requires_registration() {
        let mut registry = ContentRegistry::new();
        registry.initialize().unwrap();
        match registry.resolve(&ContentType::Json) {
            Ok(_) => panic!("expected missing parser"),
            Err(error) => assert!(error.message().contains("no content parser registered")),
        }
    }
}
