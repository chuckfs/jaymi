//! DOCX parser (`.docx` — Office Open XML).

use std::io::{Cursor, Read};
use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiError, JaymiResult};
use zip::ZipArchive;

use crate::parser::FileParser;
use crate::util::{build_document, insert_author, insert_page_count, title_from_path};

/// Parser for DOCX documents.
///
/// Reads `word/document.xml` for plain text and `docProps/core.xml` for
/// title/author/dates when present. Corrupted archives fail gracefully.
#[derive(Debug, Default)]
pub struct DocxParser;

impl FileParser for DocxParser {
    fn id(&self) -> &'static str {
        "docx"
    }

    fn name(&self) -> &'static str {
        "DOCX"
    }

    fn supported_types(&self) -> &[FileType] {
        &[FileType::Docx]
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document> {
        if bytes.is_empty() {
            return Err(JaymiError::new("DOCX file is empty"));
        }
        if !looks_like_zip(bytes) {
            return Err(JaymiError::new(
                "file does not look like a DOCX archive (missing ZIP header)",
            ));
        }

        let mut archive = ZipArchive::new(Cursor::new(bytes))
            .map_err(|error| JaymiError::new(format!("failed to open DOCX archive: {error}")))?;

        let document_xml = read_zip_entry(&mut archive, "word/document.xml").map_err(|error| {
            JaymiError::new(format!(
                "DOCX is missing word/document.xml or is corrupted: {error}"
            ))
        })?;
        let text = extract_docx_text(&document_xml);
        if text.trim().is_empty() {
            return Err(JaymiError::new(
                "DOCX contains no extractable text in word/document.xml",
            ));
        }

        let core = read_zip_entry(&mut archive, "docProps/core.xml").ok();
        let (title, author, created, modified) = core
            .as_deref()
            .map(parse_core_properties)
            .unwrap_or((None, None, None, None));

        let page_count = read_zip_entry(&mut archive, "docProps/app.xml")
            .ok()
            .and_then(|xml| extract_xml_tag(&xml, "Pages"))
            .and_then(|value| value.parse::<u64>().ok());

        let mut metadata = DocumentMetadata::new();
        insert_author(&mut metadata, author.as_deref());
        insert_page_count(&mut metadata, page_count);
        if let Some(value) = created {
            metadata.insert("creation_date", value);
            metadata.insert("creation_date_source", "docx_core");
        }
        if let Some(value) = modified {
            metadata.insert("modification_date", value);
            metadata.insert("modification_date_source", "docx_core");
        }

        let title = title
            .filter(|value| !value.trim().is_empty())
            .or_else(|| title_from_path(path));

        Ok(build_document(
            path,
            FileType::Docx,
            title,
            text,
            metadata,
            self.id(),
        ))
    }
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK")
}

fn read_zip_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> JaymiResult<String> {
    let mut file = archive
        .by_name(name)
        .map_err(|error| JaymiError::new(format!("missing {name}: {error}")))?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .map_err(|error| JaymiError::new(format!("failed reading {name}: {error}")))?;
    Ok(buffer)
}

fn extract_docx_text(document_xml: &str) -> String {
    // Collect text from <w:t>...</w:t> runs; insert newlines at paragraph ends.
    let mut text = String::new();
    let mut rest = document_xml;
    while let Some(next_t) = rest.find("<w:t") {
        let before = &rest[..next_t];
        if before.contains("</w:p>") && !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        rest = &rest[next_t..];
        let Some(close) = rest.find('>') else {
            break;
        };
        rest = &rest[close + 1..];
        let Some(end) = rest.find("</w:t>") else {
            break;
        };
        let chunk = &rest[..end];
        text.push_str(&decode_xml_entities(chunk));
        rest = &rest[end + "</w:t>".len()..];
    }
    text.trim().to_string()
}

fn parse_core_properties(
    xml: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    (
        extract_xml_tag(xml, "dc:title").or_else(|| extract_xml_tag(xml, "title")),
        extract_xml_tag(xml, "dc:creator").or_else(|| extract_xml_tag(xml, "creator")),
        extract_xml_tag(xml, "dcterms:created").or_else(|| extract_xml_tag(xml, "created")),
        extract_xml_tag(xml, "dcterms:modified").or_else(|| extract_xml_tag(xml, "modified")),
    )
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let after = &xml[start..];
    let gt = after.find('>')?;
    let content_start = &after[gt + 1..];
    let end = content_start.find(&close)?;
    let value = decode_xml_entities(&content_start[..end])
        .trim()
        .to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn parses_simple_docx_fixture() {
        let parser = DocxParser;
        let bytes = fixtures::minimal_docx();
        let document = parser
            .parse(Path::new("memo.docx"), &bytes)
            .expect("docx parse");
        assert_eq!(document.file_type, FileType::Docx);
        assert!(document.text.contains("Hello DOCX"));
        assert_eq!(document.title.as_deref(), Some("Fixture Memo"));
        assert_eq!(document.metadata.get("author"), Some("Jaymi"));
        assert_eq!(document.metadata.get("page_count"), Some("1"));
        assert_eq!(
            document.metadata.get("creation_date"),
            Some("2024-01-01T12:00:00Z")
        );
        assert_eq!(document.parser_id, "docx");
    }

    #[test]
    fn rejects_corrupt_docx_fixture() {
        let parser = DocxParser;
        let error = parser
            .parse(Path::new("broken.docx"), fixtures::corrupt_docx())
            .unwrap_err();
        assert!(!error.message().is_empty());
    }
}
