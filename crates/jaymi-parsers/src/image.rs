//! Image parser — metadata only (no vision / captions / OCR).

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use jaymi_core::{Document, DocumentMetadata, FileType, JaymiError, JaymiResult};

use crate::parser::FileParser;
use crate::util::{build_document, title_from_path};

/// Metadata keys written into [`Document::metadata`] for image sources.
pub mod keys {
    /// Pixel width.
    pub const WIDTH: &str = "width";
    /// Pixel height.
    pub const HEIGHT: &str = "height";
    /// Color profile / color type label.
    pub const COLOR_PROFILE: &str = "color_profile";
    /// Image format (`png`, `jpeg`, …).
    pub const FORMAT: &str = "image_format";
    /// Capture / shot date when EXIF provides one.
    pub const CAPTURE_DATE: &str = "capture_date";
    /// JSON object of EXIF tag → value strings.
    pub const EXIF_JSON: &str = "exif_json";
}

/// Parser for common raster image formats.
///
/// Extracts structural metadata only. Does not run OCR, vision models, or
/// caption generation.
#[derive(Debug, Default)]
pub struct ImageParser;

impl FileParser for ImageParser {
    fn id(&self) -> &'static str {
        "image"
    }

    fn name(&self) -> &'static str {
        "Image"
    }

    fn supported_types(&self) -> &[FileType] {
        &[FileType::Image]
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> JaymiResult<Document> {
        if bytes.is_empty() {
            return Err(JaymiError::new("image file is empty"));
        }

        let dyn_image = image::load_from_memory(bytes).map_err(|error| {
            JaymiError::new(format!("failed to decode image: {error}"))
        })?;

        let width = dyn_image.width();
        let height = dyn_image.height();
        let format = detect_format(path, bytes);
        let color_profile = color_profile_label(&dyn_image);
        let exif = extract_exif(bytes);
        let capture_date = exif
            .get("DateTimeOriginal")
            .cloned()
            .or_else(|| exif.get("DateTime").cloned())
            .or_else(|| exif.get("DateTimeDigitized").cloned());

        let mut metadata = DocumentMetadata::new();
        metadata.insert(keys::WIDTH, width.to_string());
        metadata.insert(keys::HEIGHT, height.to_string());
        metadata.insert(keys::FORMAT, format.as_str());
        metadata.insert(keys::COLOR_PROFILE, color_profile.as_str());
        if let Some(capture_date) = &capture_date {
            metadata.insert(keys::CAPTURE_DATE, capture_date.as_str());
        }
        let exif_json = serde_json::to_string(&exif)
            .map_err(|error| JaymiError::new(format!("exif encode: {error}")))?;
        metadata.insert(keys::EXIF_JSON, exif_json);

        // Structural descriptor for the unified text pipeline — not a caption.
        let text = format!("Image {format} {width}x{height}");

        Ok(build_document(
            path,
            FileType::Image,
            title_from_path(path),
            text,
            metadata,
            self.id(),
        ))
    }
}

fn detect_format(path: &Path, bytes: &[u8]) -> String {
    if let Ok(format) = image::guess_format(bytes) {
        return format_name(format).to_string();
    }
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| normalize_ext(ext))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_name(format: image::ImageFormat) -> &'static str {
    match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpeg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Tiff => "tiff",
        image::ImageFormat::Bmp => "bmp",
        image::ImageFormat::Ico => "ico",
        _ => "unknown",
    }
}

fn normalize_ext(ext: &str) -> String {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpeg".to_string(),
        "tif" | "tiff" => "tiff".to_string(),
        other => other.to_string(),
    }
}

fn color_profile_label(image: &image::DynamicImage) -> String {
    match image.color() {
        image::ColorType::L8 => "gray8".to_string(),
        image::ColorType::La8 => "gray_alpha8".to_string(),
        image::ColorType::Rgb8 => "rgb8".to_string(),
        image::ColorType::Rgba8 => "rgba8".to_string(),
        image::ColorType::L16 => "gray16".to_string(),
        image::ColorType::La16 => "gray_alpha16".to_string(),
        image::ColorType::Rgb16 => "rgb16".to_string(),
        image::ColorType::Rgba16 => "rgba16".to_string(),
        image::ColorType::Rgb32F => "rgb32f".to_string(),
        image::ColorType::Rgba32F => "rgba32f".to_string(),
        other => format!("{other:?}"),
    }
}

fn extract_exif(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut tags = BTreeMap::new();
    let Ok(reader) = exif::Reader::new().read_from_container(&mut Cursor::new(bytes)) else {
        return tags;
    };
    for field in reader.fields() {
        let key = field.tag.to_string();
        let value = field.display_value().with_unit(&reader).to_string();
        if !key.is_empty() && !value.is_empty() {
            tags.insert(key, value);
        }
    }
    // Prefer canonical names for common capture fields when present.
    if let Some(field) = reader.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
        tags.insert(
            "DateTimeOriginal".to_string(),
            field.display_value().to_string(),
        );
    }
    if let Some(field) = reader.get_field(exif::Tag::DateTime, exif::In::PRIMARY) {
        tags.insert("DateTime".to_string(), field.display_value().to_string());
    }
    if let Some(field) = reader.get_field(exif::Tag::ColorSpace, exif::In::PRIMARY) {
        tags.insert(
            "ColorSpace".to_string(),
            field.display_value().to_string(),
        );
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn parses_png_dimensions_and_format() {
        let parser = ImageParser;
        let bytes = fixtures::minimal_png();
        let document = parser
            .parse(Path::new("shot.png"), &bytes)
            .expect("png parse");
        assert_eq!(document.file_type, FileType::Image);
        assert_eq!(document.metadata.get(keys::WIDTH), Some("2"));
        assert_eq!(document.metadata.get(keys::HEIGHT), Some("3"));
        assert_eq!(document.metadata.get(keys::FORMAT), Some("png"));
        assert!(document.metadata.get(keys::COLOR_PROFILE).is_some());
        assert!(document.text.contains("2x3"));
        assert_eq!(document.parser_id, "image");
    }

    #[test]
    fn parses_jpeg_exif_capture_date() {
        let parser = ImageParser;
        let bytes = fixtures::minimal_jpeg_with_exif();
        let document = parser
            .parse(Path::new("camera.jpg"), &bytes)
            .expect("jpeg parse");
        assert_eq!(document.file_type, FileType::Image);
        assert_eq!(document.metadata.get(keys::FORMAT), Some("jpeg"));
        assert_eq!(
            document.metadata.get(keys::CAPTURE_DATE),
            Some("2024-01-15 12:30:00")
        );
        let exif = document.metadata.get(keys::EXIF_JSON).unwrap();
        assert!(exif.contains("DateTimeOriginal"));
    }

    #[test]
    fn rejects_empty_image() {
        let parser = ImageParser;
        let error = parser.parse(Path::new("empty.png"), b"").unwrap_err();
        assert!(error.message().contains("empty"));
    }

    #[test]
    fn rejects_corrupt_image() {
        let parser = ImageParser;
        let error = parser
            .parse(Path::new("bad.png"), b"\x89PNG\r\nnot-an-image")
            .unwrap_err();
        assert!(error.message().contains("failed to decode"));
    }
}
