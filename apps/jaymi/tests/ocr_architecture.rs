//! Integration tests for Layer 2 Slice 4 — OCR Architecture.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_providers::{
    OcrImage, OcrProvider, PlaceholderOcrProvider, OCR_ENGINE_NONE, OCR_PROVIDER_ID,
};

#[test]
fn ocr_provider_registers_without_planner_or_engine() {
    let data_dir = temp_dir("ocr-arch");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let ocr = app
        .container()
        .resolve::<Arc<PlaceholderOcrProvider>>()
        .expect("ocr provider in container");
    let status = ocr.ocr_status();
    assert_eq!(status.provider_id, OCR_PROVIDER_ID);
    assert!(status.placeholder);
    assert!(!status.available);
    assert_eq!(status.engine, OCR_ENGINE_NONE);
    assert!(status.initialized);

    let error = ocr
        .extract_text(&OcrImage::from_bytes(b"fake-image"))
        .expect_err("placeholder must not extract text");
    assert!(error.message().contains("not integrated"));

    let snapshot = app.diagnostics().expect("diagnostics");
    assert!(snapshot.provider_ids.iter().any(|id| id == OCR_PROVIDER_ID));
    assert_eq!(snapshot.provider_count, 3);

    let row = snapshot.subsystem("OCR Provider").expect("OCR row");
    assert_eq!(row.status, OperationalStatus::Stub);
    assert!(row.detail.contains(OCR_PROVIDER_ID));
    assert!(row.detail.contains("engine=none"));
    assert!(row.detail.contains("available=false"));

    // Capability registry (Planner surface) is unchanged — OCR is provider-side only.
    assert_eq!(snapshot.capability_count, 5);
    assert!(!snapshot.capability_ids.iter().any(|id| id == "ocr"));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-ocr-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
