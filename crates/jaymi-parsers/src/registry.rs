//! Parser registry — maps file types to replaceable parsers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use jaymi_core::{FileType, JaymiError, JaymiResult};

use crate::parser::FileParser;

/// Registry of file parsers keyed by [`FileType`] identity.
///
/// New formats (PDF, DOCX, …) are added by implementing [`FileParser`] and
/// registering here — no Planner changes required.
#[derive(Default)]
pub struct ParserRegistry {
    initialized: bool,
    parsers: RwLock<HashMap<String, Arc<dyn FileParser>>>,
    by_type: RwLock<HashMap<String, String>>,
}

impl std::fmt::Debug for ParserRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParserRegistry")
            .field("initialized", &self.initialized)
            .field("parser_count", &self.len())
            .finish()
    }
}

impl ParserRegistry {
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

    /// Register a parser for each of its supported file types.
    ///
    /// Later registrations for the same file type replace earlier ones.
    pub fn register(&self, parser: Arc<dyn FileParser>) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let parser_id = parser.id().to_string();
        let supported = parser.supported_types().to_vec();

        {
            let mut parsers = self
                .parsers
                .write()
                .map_err(|_| JaymiError::new("parser registry lock poisoned"))?;
            parsers.insert(parser_id.clone(), parser);
        }

        let mut by_type = self
            .by_type
            .write()
            .map_err(|_| JaymiError::new("parser registry lock poisoned"))?;
        for file_type in supported {
            by_type.insert(file_type.id().to_string(), parser_id.clone());
        }
        Ok(())
    }

    /// Resolve the parser registered for a file type.
    pub fn resolve(&self, file_type: &FileType) -> JaymiResult<Arc<dyn FileParser>> {
        self.ensure_initialized()?;
        let by_type = self
            .by_type
            .read()
            .map_err(|_| JaymiError::new("parser registry lock poisoned"))?;
        let parser_id = by_type.get(file_type.id()).ok_or_else(|| {
            JaymiError::new(format!(
                "no parser registered for file type {}",
                file_type.id()
            ))
        })?;
        let parsers = self
            .parsers
            .read()
            .map_err(|_| JaymiError::new("parser registry lock poisoned"))?;
        parsers
            .get(parser_id)
            .cloned()
            .ok_or_else(|| JaymiError::new(format!("parser not found: {parser_id}")))
    }

    /// Detect a file type from path extension.
    pub fn detect_type(path: &Path) -> Option<FileType> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "txt" | "rs" | "toml" | "yaml" | "yml" => Some(FileType::PlainText),
            "md" | "markdown" => Some(FileType::Markdown),
            "json" => Some(FileType::Json),
            "pdf" => Some(FileType::Pdf),
            "docx" => Some(FileType::Docx),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "tif" | "tiff" | "bmp" => {
                Some(FileType::Image)
            }
            other => Some(FileType::Other(other.to_string())),
        }
    }

    /// Number of registered parser implementations.
    pub fn len(&self) -> usize {
        self.parsers.read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Registered parser identifiers (sorted).
    pub fn parser_ids(&self) -> Vec<String> {
        self.parsers
            .read()
            .map(|guard| {
                let mut ids: Vec<String> = guard.keys().cloned().collect();
                ids.sort();
                ids
            })
            .unwrap_or_default()
    }

    /// Returns true when no parsers are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all registrations and mark uninitialized.
    pub fn clear(&mut self) -> JaymiResult<()> {
        self.parsers
            .write()
            .map_err(|_| JaymiError::new("parser registry lock poisoned"))?
            .clear();
        self.by_type
            .write()
            .map_err(|_| JaymiError::new("parser registry lock poisoned"))?
            .clear();
        self.initialized = false;
        Ok(())
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("parser registry is not initialized"))
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
        let mut registry = ParserRegistry::new();
        registry.initialize().unwrap();
        registry.register(Arc::new(PlainTextParser)).unwrap();
        registry.register(Arc::new(MarkdownParser)).unwrap();
        registry.register(Arc::new(JsonParser)).unwrap();
        registry.register(Arc::new(crate::PdfParser)).unwrap();
        registry.register(Arc::new(crate::DocxParser)).unwrap();
        registry.register(Arc::new(crate::ImageParser)).unwrap();

        assert_eq!(registry.len(), 6);
        assert_eq!(
            registry.resolve(&FileType::PlainText).unwrap().id(),
            "plain_text"
        );
        assert_eq!(
            registry.resolve(&FileType::Markdown).unwrap().id(),
            "markdown"
        );
        assert_eq!(registry.resolve(&FileType::Json).unwrap().id(), "json");
        assert_eq!(registry.resolve(&FileType::Pdf).unwrap().id(), "pdf");
        assert_eq!(registry.resolve(&FileType::Docx).unwrap().id(), "docx");
        assert_eq!(registry.resolve(&FileType::Image).unwrap().id(), "image");
    }

    #[test]
    fn detect_type_from_extension() {
        assert_eq!(
            ParserRegistry::detect_type(Path::new("notes.txt")),
            Some(FileType::PlainText)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("main.rs")),
            Some(FileType::PlainText)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("Cargo.toml")),
            Some(FileType::PlainText)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("config.yaml")),
            Some(FileType::PlainText)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("config.yml")),
            Some(FileType::PlainText)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("README.md")),
            Some(FileType::Markdown)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("data.json")),
            Some(FileType::Json)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("doc.pdf")),
            Some(FileType::Pdf)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("memo.docx")),
            Some(FileType::Docx)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("photo.png")),
            Some(FileType::Image)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("shot.jpeg")),
            Some(FileType::Image)
        );
        assert_eq!(
            ParserRegistry::detect_type(Path::new("binary.bin")),
            Some(FileType::Other("bin".to_string()))
        );
    }

    #[test]
    fn resolve_requires_registration() {
        let mut registry = ParserRegistry::new();
        registry.initialize().unwrap();
        match registry.resolve(&FileType::Json) {
            Ok(_) => panic!("expected missing parser"),
            Err(error) => assert!(error.message().contains("no parser registered")),
        }
    }
}
