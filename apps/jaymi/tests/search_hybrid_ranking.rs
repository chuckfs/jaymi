//! Integration tests for Layer 3 Slice 4 — Hybrid Ranking.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::{MetadataFilters, SearchRequest};
use jaymi_knowledge::{normalize_path, KnowledgeStore, SqliteKnowledgeStore};
use jaymi_search::{
    fuse_relevance, MatchReason, RankSignals, SearchEngine, SearchEngineApi, SearchStrategy,
    SCORE_SCALE,
};
use jaymi_understanding::{ContentStore, UnderstandingEngine};

#[test]
fn hybrid_ranking_fuses_independent_strategies_consistently() {
    let data_dir = temp_dir("hybrid-data");
    let root = temp_dir("hybrid-root");
    let docs = root.join("Documents");
    fs::create_dir_all(&docs).unwrap();

    // Multi-signal winner: filename + body + title all mention fungi, and freshest mtime.
    let best = docs.join("fungi_field_guide.md");
    // Full-text only (filename unrelated).
    let body_only = docs.join("report_final_v7.md");
    // Filename only (body unrelated).
    let name_only = docs.join("fungi_todo.txt");
    // Stale body hit — older than best.
    let stale = docs.join("old_notes.md");

    fs::write(
        &best,
        "# Fungi Field Guide\n\nFungi grow in damp soil near oak trees. Fungi appear again here.\n",
    )
    .unwrap();
    fs::write(
        &body_only,
        "# Biology Paper\n\nFungi grow in damp soil near oak trees.\n",
    )
    .unwrap();
    fs::write(&name_only, "# Errands\n\nBuy milk and bread tomorrow.\n").unwrap();
    fs::write(
        &stale,
        "# Archive\n\nFungi were mentioned once in an old notebook.\n",
    )
    .unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    for path in [&best, &body_only, &name_only, &stale] {
        understanding.understand_path(path).unwrap().unwrap();
    }

    // Stamp deterministic recency: best is newest, stale is oldest.
    let knowledge = app
        .container()
        .resolve::<Arc<SqliteKnowledgeStore>>()
        .expect("knowledge");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    stamp_modified(&knowledge, &best, now);
    stamp_modified(&knowledge, &body_only, now - 7 * 24 * 3600);
    stamp_modified(&knowledge, &name_only, now - 30 * 24 * 3600);
    stamp_modified(&knowledge, &stale, now - 365 * 24 * 3600);

    let engine = app
        .container()
        .resolve::<Arc<SearchEngine>>()
        .expect("search");

    let results = engine
        .search(&SearchRequest::free_text("fungi"))
        .expect("hybrid search");
    assert_eq!(results.strategy, SearchStrategy::Semantic);
    assert!(results.hits.len() >= 3);

    // One ordered list: best multi-signal doc ranks first.
    assert!(
        results.hits[0].path.ends_with("fungi_field_guide.md"),
        "expected multi-signal doc first, got {:?}",
        results
            .hits
            .iter()
            .map(|hit| (
                hit.path.file_name().map(|n| n.to_string_lossy().into_owned()),
                hit.score,
                &hit.signals
            ))
            .collect::<Vec<_>>()
    );

    let best_hit = &results.hits[0];
    assert!(best_hit.signals.filename > 0, "filename signal");
    assert!(best_hit.signals.full_text > 0, "full-text signal");
    assert!(best_hit.signals.title > 0, "title signal");
    assert!(best_hit.signals.recency > 0, "recency signal");
    assert_eq!(best_hit.score, fuse_relevance(&best_hit.signals));
    assert!(best_hit.score <= SCORE_SCALE);

    // Body-only still ranks above filename-only when full-text weight dominates weakly-matched names...
    // Filename-only has filename signal; body-only has full-text. Ordering between them is
    // weight-dependent — assert both appear and scores are fused consistently.
    let body_hit = results
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("report_final_v7.md"))
        .expect("body-only hit");
    let name_hit = results
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("fungi_todo.txt"))
        .expect("filename-only hit");
    assert!(body_hit.signals.full_text > 0);
    assert_eq!(body_hit.signals.filename, 0);
    assert!(name_hit.signals.filename > 0);
    assert_eq!(name_hit.signals.full_text, 0);
    assert_eq!(body_hit.score, fuse_relevance(&body_hit.signals));
    assert_eq!(name_hit.score, fuse_relevance(&name_hit.signals));

    // Fresher body hit outranks stale body hit when other signals are comparable.
    let stale_hit = results
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("old_notes.md"))
        .expect("stale hit");
    assert!(
        body_hit.score > stale_hit.score,
        "recency should prefer newer body hit ({} vs {})",
        body_hit.score,
        stale_hit.score
    );
    assert!(body_hit.signals.recency > stale_hit.signals.recency);

    // Combined free-text + metadata still one ordered list; metadata adds a signal.
    let content_store = app
        .container()
        .resolve::<Arc<jaymi_understanding::SqliteContentStore>>()
        .expect("content store");
    let source_id = normalize_path(&best).unwrap().to_string_lossy().into_owned();
    let mut content = content_store
        .get_by_source_id(&source_id)
        .unwrap()
        .expect("best content");
    content.tags = vec!["biology".into()];
    content_store.upsert(&content).unwrap();

    let combined = engine
        .search(&SearchRequest {
            free_text: Some("fungi".into()),
            metadata: MetadataFilters {
                tag: Some("biology".into()),
                ..MetadataFilters::default()
            },
            limit: Some(10),
            ..SearchRequest::default()
        })
        .expect("combined");
    assert_eq!(combined.strategy, SearchStrategy::Combined);
    assert_eq!(combined.hits.len(), 1);
    assert!(combined.hits[0].path.ends_with("fungi_field_guide.md"));
    assert!(combined.hits[0].signals.metadata > 0);
    assert!(combined.hits[0].signals.full_text > 0);
    assert!(matches!(
        combined.hits[0].match_reason,
        MatchReason::Combined { .. }
            | MatchReason::FreeTextPhrase
            | MatchReason::FreeTextTitle
            | MatchReason::FreeTextContent
            | MatchReason::MetadataTag
    ));

    // Deterministic / consistent ranking across repeated queries.
    let again = engine
        .search(&SearchRequest::free_text("fungi"))
        .expect("again");
    assert_eq!(
        results
            .hits
            .iter()
            .map(|hit| (&hit.item_id, hit.score, &hit.signals))
            .collect::<Vec<_>>(),
        again
            .hits
            .iter()
            .map(|hit| (&hit.item_id, hit.score, &hit.signals))
            .collect::<Vec<_>>()
    );

    // Multi-signal fused score beats single-channel equivalent.
    let multi = RankSignals {
        filename: 80,
        title: 110,
        full_text: 160,
        recency: 100,
        metadata: 0,
        semantic: 0,
    };
    let single = RankSignals {
        full_text: 160,
        ..RankSignals::default()
    };
    assert!(fuse_relevance(&multi) > fuse_relevance(&single));
}

fn stamp_modified(knowledge: &SqliteKnowledgeStore, path: &std::path::Path, modified: i64) {
    let path = normalize_path(path).unwrap();
    let mut item = knowledge
        .get_by_path(&path)
        .unwrap()
        .expect("inventory item");
    item.modified = Some(modified);
    item.last_modified = Some(modified);
    knowledge.publish(&item, modified).unwrap();
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-hybrid-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
