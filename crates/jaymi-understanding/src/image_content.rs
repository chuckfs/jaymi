//! Image content abstraction for Layer 2 Slice 5.
//!
//! Structural image metadata only — no vision understanding or captions.

use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use jaymi_core::{Document, JaymiError, JaymiResult};
use jaymi_parsers::image::keys as image_keys;
use serde::{Deserialize, Serialize};

/// Maximum edge length for generated thumbnails (pixels).
pub const THUMBNAIL_MAX_EDGE: u32 = 128;

/// First-class image metadata stored alongside normalized content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageContent {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Color profile / color type label when known.
    pub color_profile: Option<String>,
    /// Image format (`png`, `jpeg`, …).
    pub format: String,
    /// Capture date from EXIF when available.
    pub capture_date: Option<String>,
    /// EXIF tag → value map (stringified, deterministic order).
    pub exif: BTreeMap<String, String>,
    /// Absolute path to a generated thumbnail, when created.
    pub thumbnail_path: Option<String>,
}

impl ImageContent {
    /// Build image content from a parsed image document's metadata.
    pub fn from_document(document: &Document) -> JaymiResult<Self> {
        if document.file_type.id() != "image" {
            return Err(JaymiError::new(
                "ImageContent requires an image document",
            ));
        }
        let width = document
            .metadata
            .get(image_keys::WIDTH)
            .ok_or_else(|| JaymiError::new("image metadata missing width"))?
            .parse::<u32>()
            .map_err(|error| JaymiError::new(format!("invalid image width: {error}")))?;
        let height = document
            .metadata
            .get(image_keys::HEIGHT)
            .ok_or_else(|| JaymiError::new("image metadata missing height"))?
            .parse::<u32>()
            .map_err(|error| JaymiError::new(format!("invalid image height: {error}")))?;
        let format = document
            .metadata
            .get(image_keys::FORMAT)
            .unwrap_or("unknown")
            .to_string();
        let color_profile = document
            .metadata
            .get(image_keys::COLOR_PROFILE)
            .map(str::to_string);
        let capture_date = document
            .metadata
            .get(image_keys::CAPTURE_DATE)
            .map(str::to_string);
        let exif = match document.metadata.get(image_keys::EXIF_JSON) {
            Some(json) => serde_json::from_str(json)
                .map_err(|error| JaymiError::new(format!("invalid exif_json: {error}")))?,
            None => BTreeMap::new(),
        };

        Ok(Self {
            width,
            height,
            color_profile,
            format,
            capture_date,
            exif,
            thumbnail_path: None,
        })
    }

    /// Write a small JPEG thumbnail under `thumbnail_dir` and record its path.
    pub fn ensure_thumbnail(
        &mut self,
        source_bytes: &[u8],
        source_id: &str,
        thumbnail_dir: &Path,
    ) -> JaymiResult<()> {
        fs::create_dir_all(thumbnail_dir).map_err(|error| {
            JaymiError::new(format!(
                "failed to create thumbnail dir {}: {error}",
                thumbnail_dir.display()
            ))
        })?;

        let path = thumbnail_path_for(source_id, thumbnail_dir);
        let dyn_image = image::load_from_memory(source_bytes).map_err(|error| {
            JaymiError::new(format!("thumbnail decode failed: {error}"))
        })?;
        let thumb = dyn_image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE);
        thumb
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .map_err(|error| JaymiError::new(format!("thumbnail write failed: {error}")))?;
        self.thumbnail_path = Some(path.to_string_lossy().into_owned());
        Ok(())
    }
}

fn thumbnail_path_for(source_id: &str, thumbnail_dir: &Path) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source_id.hash(&mut hasher);
    let digest = hasher.finish();
    thumbnail_dir.join(format!("{digest:016x}.jpg"))
}
