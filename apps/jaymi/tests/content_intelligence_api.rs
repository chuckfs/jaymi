//! Integration tests for Layer 2 Slice 6 — Content Intelligence API.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_parsers::fixtures;
use jaymi_understanding::{
    ContentIntelligence, ContentIntelligenceApi, ContentSource, ENRICHMENT_VERSION,
};

#[test]
fn content_intelligence_api_exposes_every_operation() {
    let data_dir = temp_dir("cia-data");
    let root = temp_dir("cia-root");
    let md_path = root.join("note.md");
    let img_path = root.join("shot.png");
    fs::write(
        &md_path,
        "# API Note\n\nThe and of to a in is that for on with as this be are by from.\n\nSee [local](./x.md).\n",
    )
    .unwrap();
    fs::write(&img_path, fixtures::minimal_png()).unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let api = app
        .container()
        .resolve::<Arc<ContentIntelligenceApi>>()
        .expect("content intelligence api");

    // load content
    let loaded = api.load_content(&md_path).expect("load");
    assert_eq!(loaded.source, ContentSource::Parsed);
    assert_eq!(loaded.content.content_type, "markdown");
    assert!(loaded.content.plain_text.contains("API Note"));

    let cached = api.load_content(&md_path).expect("reload");
    assert_eq!(cached.source, ContentSource::Stored);

    // retrieve metadata
    let metadata = api.retrieve_metadata(&md_path).expect("metadata");
    assert_eq!(metadata.content_type, "markdown");
    assert_eq!(metadata.title.as_deref(), Some("API Note"));
    assert_eq!(metadata.enrichment.version, ENRICHMENT_VERSION);
    assert!(!metadata.enrichment.headings.is_empty());
    assert!(metadata.image.is_none());

    // retrieve plain text
    let text = api.retrieve_plain_text(&md_path).expect("plain text");
    assert!(text.contains("API Note"));
    assert!(text.contains("local"));

    // retrieve parser information (identity only — no parser objects)
    let parser = api.retrieve_parser_info(&md_path).expect("parser info");
    assert_eq!(parser.parser_used, "markdown");
    assert!(!parser.parser_version.is_empty());
    assert!(parser.extraction_timestamp > 0);

    // image path through the same API
    let image_loaded = api.load_content(&img_path).expect("load image");
    assert_eq!(image_loaded.content.content_type, "image");
    let image_meta = api.retrieve_metadata(&img_path).expect("image metadata");
    assert!(image_meta.image.is_some());
    assert_eq!(image_meta.image.as_ref().unwrap().format, "png");

    // get_by_source_id
    let source_id = image_loaded.content.source_id.clone();
    let by_id = api
        .get_by_source_id(&source_id)
        .expect("by id")
        .expect("present");
    assert_eq!(by_id.content_type, "image");

    // retrieve statistics
    let stats = api.retrieve_statistics().expect("stats");
    assert!(stats.document_count >= 2);
    assert!(stats.enriched_count >= 2);
    assert!(stats.image_count >= 1);
    assert!(stats
        .parser_usage
        .iter()
        .any(|(parser, _)| parser == "markdown"));
    assert!(stats
        .parser_usage
        .iter()
        .any(|(parser, _)| parser == "image"));

    // retrieve health
    let health = api.retrieve_health().expect("health");
    assert!(health.initialized);
    assert!(health.healthy);
    assert!(!health.version.is_empty());
    assert!(health.detail.contains("documents="));
    assert!(health.detail.contains("images="));
    assert_eq!(health.statistics.document_count, stats.document_count);

    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot
        .subsystem("Content Intelligence")
        .expect("content intelligence row");
    assert_eq!(row.status, OperationalStatus::Operational);
    assert!(row.detail.contains("documents="));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-cia-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
