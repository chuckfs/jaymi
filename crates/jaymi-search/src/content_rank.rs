//! Deterministic content ranking helpers for full-text search.
//!
//! Emits independent title vs full-text signals for hybrid fusion.
//! Priority within content: exact phrase > frequency > title matches.
//! No AI ranking.

use jaymi_understanding::Section;

use crate::hybrid_rank::RankSignals;
use crate::result::MatchReason;

/// Ranking / localization outcome for one content document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRank {
    /// Independent title / full-text signals (other channels left zero).
    pub signals: RankSignals,
    /// Primary match reason for content.
    pub reason: MatchReason,
    /// Section title containing the best match, when known.
    pub matching_section: Option<String>,
    /// Snippet around the best match.
    pub snippet: Option<String>,
}

impl ContentRank {
    /// Raw content contribution before hybrid normalization.
    pub fn raw_score(&self) -> u32 {
        self.signals.title.saturating_add(self.signals.full_text)
    }
}

const SNIPPET_RADIUS: usize = 80;

/// Score a content document against a free-text query.
///
/// Ranking priority:
/// 1. Exact phrase in body → full-text signal
/// 2. Exact phrase / strong match in title → title signal
/// 3. Term frequency in body → full-text signal
pub fn rank_content_match(
    query: &str,
    title: Option<&str>,
    plain_text: &str,
    sections: &[Section],
) -> Option<ContentRank> {
    let needle = normalize_query(query);
    if needle.is_empty() {
        return None;
    }

    let text_lower = plain_text.to_ascii_lowercase();
    let title_lower = title.map(|value| value.to_ascii_lowercase());
    let phrase = needle.as_str();

    let phrase_in_body = text_lower.find(phrase);
    let phrase_in_title = title_lower.as_ref().and_then(|title| title.find(phrase));

    let frequency = count_occurrences(&text_lower, phrase);
    let title_token_hits = title_token_match_count(title_lower.as_deref(), phrase);

    let mut signals = RankSignals::default();
    let mut reason = MatchReason::FreeTextContent;

    if phrase_in_body.is_some() {
        // Exact phrase in body is the strongest full-text signal.
        signals.full_text = signals.full_text.saturating_add(120);
        reason = MatchReason::FreeTextPhrase;
    }

    if phrase_in_title.is_some() {
        signals.title = signals.title.saturating_add(110);
        if !matches!(reason, MatchReason::FreeTextPhrase) {
            reason = MatchReason::FreeTextTitle;
        }
    } else if title_token_hits > 0 {
        signals.title = signals.title.saturating_add(90);
        if matches!(reason, MatchReason::FreeTextContent) {
            reason = MatchReason::FreeTextTitle;
        }
    }

    if frequency > 0 {
        let freq_score =
            40u32.saturating_add((frequency.saturating_sub(1) as u32).saturating_mul(8));
        signals.full_text = signals.full_text.saturating_add(freq_score.min(80));
        if signals.title == 0 && signals.full_text == freq_score.min(80) {
            reason = MatchReason::FreeTextContent;
        }
    }

    if signals.title == 0 && signals.full_text == 0 {
        return None;
    }

    let match_offset = phrase_in_body
        .or_else(|| first_token_offset(&text_lower, phrase))
        .unwrap_or(0);

    let matching_section = section_for_offset(sections, match_offset)
        .map(|section| section.title.clone())
        .or_else(|| {
            sections
                .iter()
                .find(|section| section.title.to_ascii_lowercase().contains(phrase))
                .map(|section| section.title.clone())
        });

    if matching_section
        .as_ref()
        .map(|title| title.to_ascii_lowercase().contains(phrase))
        .unwrap_or(false)
    {
        signals.title = signals.title.saturating_add(25);
    }

    let snippet = snippet_around(plain_text, match_offset, phrase.chars().count());

    Some(ContentRank {
        signals,
        reason,
        matching_section,
        snippet,
    })
}

fn normalize_query(query: &str) -> String {
    let trimmed = query.trim();
    let inner = if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    inner
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn count_occurrences(haystack_lower: &str, needle_lower: &str) -> usize {
    if needle_lower.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut start = 0;
    while let Some(idx) = haystack_lower[start..].find(needle_lower) {
        count += 1;
        start += idx + needle_lower.len();
        if start >= haystack_lower.len() {
            break;
        }
    }
    count
}

fn title_token_match_count(title_lower: Option<&str>, phrase: &str) -> usize {
    let Some(title) = title_lower else {
        return 0;
    };
    phrase
        .split_whitespace()
        .filter(|token| title.contains(token))
        .count()
}

fn first_token_offset(text_lower: &str, phrase: &str) -> Option<usize> {
    phrase
        .split_whitespace()
        .find_map(|token| text_lower.find(token))
}

fn section_for_offset(sections: &[Section], offset: usize) -> Option<&Section> {
    sections
        .iter()
        .find(|section| offset >= section.start_offset && offset < section.end_offset)
        .or_else(|| sections.last())
}

fn snippet_around(text: &str, byte_offset: usize, needle_chars: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let offset = byte_offset.min(text.len());
    let start = floor_char_boundary(text, offset.saturating_sub(SNIPPET_RADIUS));
    let end_target = offset
        .saturating_add(needle_chars.saturating_mul(4).max(needle_chars))
        .saturating_add(SNIPPET_RADIUS)
        .min(text.len());
    let end = ceil_char_boundary(text, end_target);
    let mut snippet = text[start..end].trim().to_string();
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < text.len() {
        snippet.push('…');
    }
    if snippet.is_empty() {
        None
    } else {
        Some(snippet)
    }
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut idx = index;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn ceil_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut idx = index;
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_understanding::Section;

    #[test]
    fn ranks_phrase_above_title_and_frequency() {
        let sections = vec![Section {
            title: "Habitat".into(),
            level: 1,
            start_offset: 0,
            end_offset: 80,
        }];
        let ranked = rank_content_match(
            "damp soil",
            Some("Notes"),
            "Fungi grow in damp soil. More damp soil here.",
            &sections,
        )
        .unwrap();
        assert_eq!(ranked.reason, MatchReason::FreeTextPhrase);
        assert!(ranked.signals.full_text >= 120);
        assert!(ranked.raw_score() >= 120);
        assert_eq!(ranked.matching_section.as_deref(), Some("Habitat"));
        assert!(ranked.snippet.unwrap().contains("damp soil"));
    }

    #[test]
    fn title_match_without_body_phrase() {
        let ranked = rank_content_match(
            "biology",
            Some("Biology Paper"),
            "An unrelated abstract about chemistry.",
            &[],
        )
        .unwrap();
        assert_eq!(ranked.reason, MatchReason::FreeTextTitle);
        assert!(ranked.signals.title >= 90);
        assert_eq!(ranked.signals.full_text, 0);
    }

    #[test]
    fn separates_title_and_full_text_signals() {
        let ranked =
            rank_content_match("fungi", Some("Fungi Notes"), "Fungi grow everywhere.", &[])
                .unwrap();
        assert!(ranked.signals.title > 0);
        assert!(ranked.signals.full_text > 0);
    }
}
