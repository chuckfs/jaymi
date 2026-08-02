//! Integration tests for Layer 2 Slice 5 — Image Content Pipeline.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_parsers::fixtures;
use jaymi_understanding::{UnderstandOutcome, UnderstandingEngine};

#[test]
fn image_metadata_enters_unified_content_pipeline() {
    let data_dir = temp_dir("image-data");
    let root = temp_dir("image-root");

    let png_path = root.join("shot.png");
    let jpeg_path = root.join("camera.jpg");
    fs::write(&png_path, fixtures::minimal_png()).unwrap();
    fs::write(&jpeg_path, fixtures::minimal_jpeg_with_exif()).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");

    let png = match understanding
        .understand_path(&png_path)
        .unwrap()
        .expect("inventoried")
    {
        UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => content,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(png.content_type, "image");
    assert_eq!(png.parser_used, "image");
    let image = png.image.expect("image metadata");
    assert_eq!(image.width, 2);
    assert_eq!(image.height, 3);
    assert_eq!(image.format, "png");
    assert!(image.color_profile.is_some());
    assert!(image.thumbnail_path.is_some());
    let thumb = PathBuf::from(image.thumbnail_path.as_ref().unwrap());
    assert!(thumb.exists(), "thumbnail should exist at {}", thumb.display());
    assert!(thumb.starts_with(data_dir.join("thumbnails")));

    let jpeg = match understanding
        .understand_path(&jpeg_path)
        .unwrap()
        .expect("inventoried")
    {
        UnderstandOutcome::Parsed(content) | UnderstandOutcome::Cached(content) => content,
        other => panic!("unexpected {other:?}"),
    };
    let jpeg_image = jpeg.image.expect("jpeg image metadata");
    assert_eq!(jpeg_image.format, "jpeg");
    assert_eq!(
        jpeg_image.capture_date.as_deref(),
        Some("2024-01-15 12:30:00")
    );
    assert!(jpeg_image.exif.contains_key("DateTimeOriginal"));
    assert!(jpeg_image.thumbnail_path.is_some());

    // Planner read surfaces image metadata through Document.metadata.
    let read = app.read_file(&png_path).expect("read image");
    assert!(!read.blocked);
    let document = read.document.expect("document");
    assert_eq!(document.file_type.id(), "image");
    assert_eq!(document.metadata.get("width"), Some("2"));
    assert_eq!(document.metadata.get("height"), Some("3"));
    assert_eq!(document.metadata.get("image_format"), Some("png"));
    assert!(document.metadata.get("thumbnail_path").is_some());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-image-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
