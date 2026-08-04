//! Shared fixtures for every supported document format.

use std::io::{Cursor, Write};

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
use lopdf::{dictionary, Document, Object, Stream};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Minimal plain-text fixture.
pub fn plain_text() -> &'static [u8] {
    b"Hello plain text.\nLine two.\n"
}

/// Minimal markdown fixture with an ATX title.
pub fn markdown() -> &'static [u8] {
    b"# Fixture Title\n\nBody paragraph.\n"
}

/// Minimal JSON fixture with a title field.
pub fn json() -> &'static [u8] {
    br#"{"title":"Fixture JSON","count":2,"tags":["a","b"]}"#
}

/// Minimal one-page PDF containing the text "Hello PDF".
pub fn minimal_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content = "BT /F1 24 Tf 72 720 Td (Hello PDF) Tj ET\n";
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Demo PDF"),
        "Author" => Object::string_literal("Jaymi"),
        "CreationDate" => Object::string_literal("D:20240101120000Z"),
        "ModDate" => Object::string_literal("D:20240201120000Z"),
    });
    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write fixture pdf");
    out
}

/// Minimal DOCX archive with title, author, and body text.
pub fn minimal_docx() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
  <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
</Types>"#,
        )
        .unwrap();

        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#,
        )
        .unwrap();

        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Hello DOCX</w:t></w:r></w:p>
    <w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:dcterms="http://purl.org/dc/terms/"
 xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dc:title>Fixture Memo</dc:title>
  <dc:creator>Jaymi</dc:creator>
  <dcterms:created xsi:type="dcterms:W3CDTF">2024-01-01T12:00:00Z</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">2024-02-01T12:00:00Z</dcterms:modified>
</cp:coreProperties>"#,
        )
        .unwrap();

        zip.start_file("docProps/app.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
  <Pages>1</Pages>
</Properties>"#,
        )
        .unwrap();

        zip.finish().unwrap();
    }
    cursor.into_inner()
}

/// Corrupted PDF bytes (header only).
pub fn corrupt_pdf() -> &'static [u8] {
    b"%PDF-1.4\nthis is not a real pdf"
}

/// Corrupted DOCX bytes (ZIP header but invalid archive).
pub fn corrupt_docx() -> &'static [u8] {
    b"PK\x03\x04corrupt-docx-payload"
}

/// Minimal 2×3 RGB PNG.
pub fn minimal_png() -> Vec<u8> {
    let buffer = ImageBuffer::from_fn(2, 3, |x, y| Rgb([(x * 40) as u8, (y * 40) as u8, 120]));
    let image = DynamicImage::ImageRgb8(buffer);
    let mut cursor = Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("write png fixture");
    cursor.into_inner()
}

/// Minimal JPEG with an EXIF DateTimeOriginal tag.
pub fn minimal_jpeg_with_exif() -> Vec<u8> {
    let buffer = ImageBuffer::from_pixel(1, 1, Rgb([10u8, 20, 30]));
    let image = DynamicImage::ImageRgb8(buffer);
    let mut jpeg = Cursor::new(Vec::new());
    image
        .write_to(&mut jpeg, ImageFormat::Jpeg)
        .expect("write jpeg fixture");
    let jpeg = jpeg.into_inner();
    inject_exif_app1(jpeg, b"2024:01:15 12:30:00")
}

fn inject_exif_app1(jpeg: Vec<u8>, datetime: &[u8]) -> Vec<u8> {
    // Expect SOI (FFD8). Insert APP1 immediately after SOI.
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return jpeg;
    }
    let app1 = build_exif_app1(datetime);
    let mut out = Vec::with_capacity(jpeg.len() + app1.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn build_exif_app1(datetime: &[u8]) -> Vec<u8> {
    // Minimal TIFF/EXIF with DateTimeOriginal (tag 0x9003) ASCII.
    let mut tiff = Vec::new();
    // TIFF header (little endian)
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes()); // offset to first IFD

    // IFD0 with 1 entry pointing to Exif IFD
    let exif_ifd_offset = 8 + 2 + 12 + 4; // after IFD0
    tiff.extend_from_slice(&1u16.to_le_bytes()); // entry count
                                                 // ExifIFDPointer tag 0x8769, type LONG (4), count 1
    tiff.extend_from_slice(&0x8769u16.to_le_bytes());
    tiff.extend_from_slice(&4u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&(exif_ifd_offset as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD

    // Exif IFD with DateTimeOriginal
    let value_offset = exif_ifd_offset + 2 + 12 + 4;
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x9003u16.to_le_bytes()); // DateTimeOriginal
    tiff.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    tiff.extend_from_slice(&(datetime.len() as u32 + 1).to_le_bytes());
    tiff.extend_from_slice(&(value_offset as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(datetime);
    tiff.push(0);

    let mut app1 = Vec::new();
    app1.extend_from_slice(&[0xFF, 0xE1]);
    let payload_len = 2 + 6 + tiff.len(); // size field includes itself
    app1.extend_from_slice(&(payload_len as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);
    app1
}
