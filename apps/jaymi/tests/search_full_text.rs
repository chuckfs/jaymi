//! Integration tests for Layer 3 Slice 2 — Full Text Search.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::SearchRequest;
use jaymi_search::{MatchReason, SearchEngine, SearchEngineApi};
use jaymi_understanding::{ContentIntelligence, ContentIntelligenceApi, UnderstandingEngine};

#[test]
fn full_text_search_finds_words_phrases_and_sections() {
    let data_dir = temp_dir("fts-data");
    let root = temp_dir("fts-root");
    let docs = root.join("Documents");
    fs::create_dir_all(&docs).unwrap();

    // Filename deliberately does NOT contain the search terms.
    let paper = docs.join("report_final_v7.md");
    let notes = docs.join("meeting.md");
    let shopping = docs.join("errands.md");

    fs::write(
        &paper,
        "# Biology Paper\n\n## Habitat\n\nFungi grow in damp soil near oak trees.\n\n## Methods\n\nSamples were collected weekly.\n",
    )
    .unwrap();
    fs::write(
        &notes,
        "# Weekly Sync\n\nDiscussed fungi again. Fungi appear frequently in the notes.\n",
    )
    .unwrap();
    fs::write(&shopping, "# Errands\n\nBuy milk and bread.\n").unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    app.index_root(&root).expect("index");

    let understanding = app
        .container()
        .resolve::<Arc<UnderstandingEngine>>()
        .expect("understanding");
    // Warm normalized content (FTS indexes on upsert).
    for path in [&paper, &notes, &shopping] {
        understanding.understand_path(path).unwrap().unwrap();
    }

    let engine = app
        .container()
        .resolve::<Arc<SearchEngine>>()
        .expect("search");

    // Word search — content only (filename has no "fungi").
    let word = engine
        .search(&SearchRequest::free_text("fungi"))
        .expect("word");
    assert!(
        word.hits.iter().any(|hit| hit.path.ends_with("report_final_v7.md")),
        "expected content hit, got {:?}",
        word.hits
    );
    let paper_hit = word
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("report_final_v7.md"))
        .unwrap();
    assert!(
        matches!(
            paper_hit.match_reason,
            MatchReason::FreeTextPhrase
                | MatchReason::FreeTextContent
                | MatchReason::FreeTextTitle
                | MatchReason::Combined { .. }
                | MatchReason::Semantic
        ),
        "unexpected reason {:?}",
        paper_hit.match_reason
    );
    assert_eq!(paper_hit.matching_section.as_deref(), Some("Habitat"));
    assert!(
        paper_hit
            .snippet
            .as_ref()
            .unwrap()
            .to_ascii_lowercase()
            .contains("fungi")
    );

    // Phrase / exact match.
    let phrase = engine
        .search(&SearchRequest::free_text("\"damp soil\""))
        .expect("phrase");
    assert_eq!(phrase.hits.len(), 1);
    assert!(
        matches!(
            phrase.hits[0].match_reason,
            MatchReason::FreeTextPhrase | MatchReason::Combined { .. }
        ),
        "unexpected reason {:?}",
        phrase.hits[0].match_reason
    );
    assert!(phrase.hits[0]
        .snippet
        .as_ref()
        .unwrap()
        .contains("damp soil"));

    // Multi-word phrase without quotes.
    let multi = engine
        .search(&SearchRequest::free_text("damp soil"))
        .expect("multi");
    assert_eq!(multi.hits.len(), 1);
    assert!(
        matches!(
            multi.hits[0].match_reason,
            MatchReason::FreeTextPhrase | MatchReason::Combined { .. }
        ),
        "unexpected reason {:?}",
        multi.hits[0].match_reason
    );

    // Title match ranking.
    let title = engine
        .search(&SearchRequest::free_text("Biology"))
        .expect("title");
    assert!(title.hits.iter().any(|hit| {
        hit.path.ends_with("report_final_v7.md")
            && matches!(
                hit.match_reason,
                MatchReason::FreeTextTitle
                    | MatchReason::FreeTextPhrase
                    | MatchReason::Combined { .. }
            )
    }));

    // Frequency: notes mention fungi twice → full-text signal at or above single-mention paper.
    let ranked = engine
        .search(&SearchRequest::free_text("fungi"))
        .expect("ranked");
    let notes_ft = ranked
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("meeting.md"))
        .map(|hit| hit.signals.full_text)
        .expect("notes hit");
    let paper_ft = ranked
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("report_final_v7.md"))
        .map(|hit| hit.signals.full_text)
        .expect("paper hit");
    assert!(
        notes_ft >= paper_ft,
        "frequency should boost notes full-text ({notes_ft}) vs paper ({paper_ft})"
    );

    // Determinism.
    let again = engine
        .search(&SearchRequest::free_text("\"damp soil\""))
        .expect("again");
    assert_eq!(phrase.hits, again.hits);

    // Non-matching document must not appear.
    assert!(!ranked.hits.iter().any(|hit| hit.path.ends_with("errands.md")));

    // Content Intelligence API surface.
    let api = app
        .container()
        .resolve::<Arc<ContentIntelligenceApi>>()
        .expect("api");
    let api_hits = api.search_full_text("fungi", 10).expect("api fts");
    assert!(api_hits.iter().any(|hit| hit.source_id.ends_with("report_final_v7.md")));

    // Planner path.
    let response = app
        .search(SearchRequest::free_text("\"damp soil\""))
        .expect("planner");
    assert_eq!(response.tool_id.as_deref(), Some("search_knowledge"));
    assert!(response
        .entries
        .iter()
        .any(|entry| entry.name.contains("Biology") || entry.path.ends_with("report_final_v7.md")));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-fts-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
