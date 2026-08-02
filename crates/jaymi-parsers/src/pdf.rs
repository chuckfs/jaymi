//! PDF parser (`.pdf`).

use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiError, JaymiResult};
use lopdf::Document as PdfDocument;

use crate::parser::FileParser;
use crate::util::{
    build_document, insert_author, insert_page_count, title_from_path,
};

/// Parser for PDF documents.
///
/// Extracts plain text and lightweight Info-dictionary metadata. Does not
/// perform OCR — scanned image-only PDFs may yield empty text.
#[derive(Debug, Default)]
pub struct PdfParser;

impl FileParser for PdfParser {
    fn id(&self) -> &'static str {
        "pdf"
    }

    fn name(&self) -> &'static str {
        "PDF"
    }

    fn supported_types(&self) -> &[FileType] {
        &[FileType::Pdf]
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document> {
        if bytes.is_empty() {
            return Err(JaymiError::new("PDF file is empty"));
        }
        if !looks_like_pdf(bytes) {
            return Err(JaymiError::new(
                "file does not look like a PDF (missing %PDF header)",
            ));
        }

        let text = pdf_extract::extract_text_from_mem(bytes).map_err(|error| {
            JaymiError::new(format!("failed to extract PDF text: {error}"))
        })?;

        let (page_count, title, author, creation_date, modification_date) =
            read_pdf_info(bytes).unwrap_or((None, None, None, None, None));

        let mut metadata = DocumentMetadata::new();
        insert_page_count(&mut metadata, page_count);
        insert_author(&mut metadata, author.as_deref());
        if let Some(value) = creation_date {
            metadata.insert("creation_date", value);
            metadata.insert("creation_date_source", "pdf_info");
        }
        if let Some(value) = modification_date {
            metadata.insert("modification_date", value);
            metadata.insert("modification_date_source", "pdf_info");
        }
        if text.trim().is_empty() {
            metadata.insert("text_empty", "true");
            metadata.insert(
                "note",
                "no extractable text (image-only or empty PDF; OCR is out of scope)",
            );
        }

        let title = title
            .filter(|value| !value.trim().is_empty())
            .or_else(|| title_from_path(path));

        Ok(build_document(
            path,
            FileType::Pdf,
            title,
            text,
            metadata,
            self.id(),
        ))
    }
}

fn looks_like_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

fn read_pdf_info(
    bytes: &[u8],
) -> JaymiResult<(
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
)> {
    let document = PdfDocument::load_mem(bytes).map_err(|error| {
        JaymiError::new(format!("failed to open PDF structure: {error}"))
    })?;
    let page_count = Some(document.get_pages().len() as u64);
    let mut title = None;
    let mut author = None;
    let mut creation_date = None;
    let mut modification_date = None;

    if let Ok(info) = document.trailer.get(b"Info") {
        if let Ok(id) = info.as_reference() {
            if let Ok(dict) = document.get_dictionary(id) {
                title = dict_string(dict, b"Title");
                author = dict_string(dict, b"Author");
                creation_date = dict_string(dict, b"CreationDate");
                modification_date = dict_string(dict, b"ModDate");
            }
        }
    }

    Ok((
        page_count,
        title,
        author,
        creation_date,
        modification_date,
    ))
}

fn dict_string(dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
    let object = dict.get(key).ok()?;
    match object {
        lopdf::Object::String(bytes, _) => Some(decode_pdf_string(bytes)),
        lopdf::Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
        _ => None,
    }
}

fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let pairs = bytes[2..].chunks_exact(2);
        let units: Vec<u16> = pairs
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn parses_simple_pdf_fixture() {
        let parser = PdfParser;
        let bytes = fixtures::minimal_pdf();
        let document = parser
            .parse(Path::new("demo.pdf"), &bytes)
            .expect("pdf parse");
        assert_eq!(document.file_type, FileType::Pdf);
        assert!(document.text.to_ascii_lowercase().contains("hello"));
        assert_eq!(document.metadata.get("page_count"), Some("1"));
        assert_eq!(document.parser_id, "pdf");
        assert_eq!(document.title.as_deref(), Some("Demo PDF"));
        assert_eq!(document.metadata.get("author"), Some("Jaymi"));
    }

    #[test]
    fn rejects_non_pdf_bytes() {
        let parser = PdfParser;
        let error = parser
            .parse(Path::new("fake.pdf"), b"not a pdf")
            .unwrap_err();
        assert!(error.message().contains("does not look like a PDF"));
    }

    #[test]
    fn rejects_empty_pdf() {
        let parser = PdfParser;
        let error = parser.parse(Path::new("empty.pdf"), b"").unwrap_err();
        assert!(error.message().contains("empty"));
    }

    #[test]
    fn rejects_corrupt_pdf_fixture() {
        let parser = PdfParser;
        let error = parser
            .parse(Path::new("broken.pdf"), fixtures::corrupt_pdf())
            .unwrap_err();
        assert!(!error.message().is_empty());
    }
}