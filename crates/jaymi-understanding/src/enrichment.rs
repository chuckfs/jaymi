//! Deterministic structural metadata enrichment (Layer 2 Slice 3).
//!
//! No AI / LLM involvement. The same input always yields the same enrichment.

use serde::{Deserialize, Serialize};

/// Words-per-minute used for estimated reading time (deterministic constant).
pub const READING_WORDS_PER_MINUTE: u64 = 200;

/// Enrichment algorithm version stored with content.
pub const ENRICHMENT_VERSION: &str = "1";

/// A document heading extracted from structure or text heuristics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    /// Heading level (`1` = highest). Plain-text heuristics use `1`.
    pub level: u8,
    /// Heading text without markup markers.
    pub text: String,
    /// Byte offset of the heading line start in `plain_text`.
    pub offset: usize,
}

/// A section bounded by headings (or the full document when none exist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// Section title (heading text, document title, or `"Body"`).
    pub title: String,
    /// Heading level that opened the section (`0` for synthetic body).
    pub level: u8,
    /// Byte offset where the section body begins.
    pub start_offset: usize,
    /// Exclusive byte offset where the section ends.
    pub end_offset: usize,
}

/// Deterministic structural metadata for normalized content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentEnrichment {
    /// Extracted headings in document order.
    pub headings: Vec<Heading>,
    /// Sections derived from headings (or one synthetic body section).
    pub sections: Vec<Section>,
    /// Estimated reading time in whole seconds.
    pub reading_time_seconds: u64,
    /// Whitespace-delimited word count.
    pub word_count: u64,
    /// Unicode scalar character count.
    pub character_count: u64,
    /// Detected language tag (`en`, `es`, `fr`, `de`) when confident.
    pub language: Option<String>,
    /// Relative / anchor / path links.
    pub internal_links: Vec<String>,
    /// Absolute URL links (`http`, `https`, `mailto`, …).
    pub external_links: Vec<String>,
    /// Enrichment algorithm version.
    pub version: String,
}

impl ContentEnrichment {
    /// Extract enrichment from normalized plain text and content type.
    pub fn extract(plain_text: &str, content_type: &str, title: Option<&str>) -> Self {
        let character_count = plain_text.chars().count() as u64;
        let word_count = count_words(plain_text);
        let reading_time_seconds = estimate_reading_time_seconds(word_count);
        let headings = extract_headings(plain_text, content_type, title);
        let sections = build_sections(plain_text, &headings, title);
        let (internal_links, external_links) = extract_links(plain_text, content_type);
        let language = detect_language(plain_text);

        Self {
            headings,
            sections,
            reading_time_seconds,
            word_count,
            character_count,
            language,
            internal_links,
            external_links,
            version: ENRICHMENT_VERSION.to_string(),
        }
    }

    /// Empty enrichment used when migrating legacy rows without enrichment.
    pub fn empty() -> Self {
        Self {
            headings: Vec::new(),
            sections: Vec::new(),
            reading_time_seconds: 0,
            word_count: 0,
            character_count: 0,
            language: None,
            internal_links: Vec::new(),
            external_links: Vec::new(),
            version: ENRICHMENT_VERSION.to_string(),
        }
    }

    /// Serialize to JSON for persistence.
    pub fn to_json(&self) -> JaymiJsonResult {
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    /// Deserialize from JSON persistence.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }
}

type JaymiJsonResult = Result<String, String>;

fn count_words(text: &str) -> u64 {
    text.split_whitespace().filter(|token| !token.is_empty()).count() as u64
}

fn estimate_reading_time_seconds(word_count: u64) -> u64 {
    if word_count == 0 {
        return 0;
    }
    // Ceil(words / WPM) minutes, converted to seconds.
    let minutes = word_count.div_ceil(READING_WORDS_PER_MINUTE).max(1);
    minutes.saturating_mul(60)
}

fn extract_headings(plain_text: &str, content_type: &str, title: Option<&str>) -> Vec<Heading> {
    match content_type {
        "markdown" => extract_markdown_headings(plain_text),
        "json" => extract_json_headings(plain_text),
        _ => extract_generic_headings(plain_text, title),
    }
}

fn extract_markdown_headings(plain_text: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut offset = 0usize;
    for line in plain_text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\r', '\n']).trim();
        if let Some(heading) = parse_atx_heading(trimmed, line_start) {
            headings.push(heading);
        }
    }
    headings
}

fn parse_atx_heading(line: &str, offset: usize) -> Option<Heading> {
    if !line.starts_with('#') {
        return None;
    }
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = line.get(hashes..)?.trim();
    let rest = rest.trim_end_matches('#').trim();
    if rest.is_empty() {
        return None;
    }
    // Require a space after hashes for ATX (CommonMark), except bare `#Title`.
    let after_hashes = line.get(hashes..)?;
    if !after_hashes.starts_with(' ') && !after_hashes.starts_with('\t') {
        // Allow `#Title` only when the remainder is non-empty text without space requirement
        // for older fixtures; still accept `# Title`.
        if after_hashes.starts_with('#') {
            return None;
        }
    }
    Some(Heading {
        level: hashes as u8,
        text: rest.to_string(),
        offset,
    })
}

fn extract_json_headings(plain_text: &str) -> Vec<Heading> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(plain_text.trim()) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    let mut keys: Vec<&String> = object.keys().collect();
    keys.sort();
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| Heading {
            level: 1,
            text: key.clone(),
            offset: index,
        })
        .collect()
}

fn extract_generic_headings(plain_text: &str, title: Option<&str>) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut offset = 0usize;
    for line in plain_text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\r', '\n']).trim();
        if trimmed.is_empty() {
            continue;
        }
        if looks_like_generic_heading(trimmed) {
            headings.push(Heading {
                level: 1,
                text: trimmed.to_string(),
                offset: line_start,
            });
        }
    }
    if headings.is_empty() {
        if let Some(title) = title.map(str::trim).filter(|value| !value.is_empty()) {
            headings.push(Heading {
                level: 1,
                text: title.to_string(),
                offset: 0,
            });
        }
    }
    headings
}

fn looks_like_generic_heading(line: &str) -> bool {
    if line.chars().count() > 80 || line.chars().count() < 2 {
        return false;
    }
    if line.ends_with('.') || line.ends_with(',') || line.ends_with(';') {
        return false;
    }
    // Numbered section: "1. Introduction" / "1) Intro" / "Chapter 2"
    if looks_numbered_heading(line) {
        return true;
    }
    // ALL-CAPS short line of letters/spaces
    let letters = line.chars().filter(|ch| ch.is_alphabetic()).count();
    if letters >= 3 && line.chars().filter(|ch| ch.is_alphabetic()).all(|ch| ch.is_uppercase()) {
        return true;
    }
    false
}

fn looks_numbered_heading(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() {
        return false;
    }
    matches!(bytes[i], b'.' | b')') && bytes.get(i + 1) == Some(&b' ')
}

fn build_sections(plain_text: &str, headings: &[Heading], title: Option<&str>) -> Vec<Section> {
    let end = plain_text.len();
    if headings.is_empty() {
        let section_title = title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Body")
            .to_string();
        return vec![Section {
            title: section_title,
            level: 0,
            start_offset: 0,
            end_offset: end,
        }];
    }

    let mut sections = Vec::with_capacity(headings.len());
    for (index, heading) in headings.iter().enumerate() {
        let next_start = headings
            .get(index + 1)
            .map(|next| next.offset)
            .unwrap_or(end);
        let body_start = skip_heading_line(plain_text, heading.offset);
        sections.push(Section {
            title: heading.text.clone(),
            level: heading.level,
            start_offset: body_start.min(next_start),
            end_offset: next_start,
        });
    }
    sections
}

fn skip_heading_line(plain_text: &str, offset: usize) -> usize {
    let rest = plain_text.get(offset..).unwrap_or("");
    if let Some(newline) = rest.find('\n') {
        offset + newline + 1
    } else {
        plain_text.len()
    }
}

fn extract_links(plain_text: &str, content_type: &str) -> (Vec<String>, Vec<String>) {
    let mut internal = Vec::new();
    let mut external = Vec::new();

    if content_type == "markdown" {
        collect_markdown_links(plain_text, &mut internal, &mut external);
    }
    collect_bare_urls(plain_text, &mut internal, &mut external);

    dedupe_preserve_order(&mut internal);
    dedupe_preserve_order(&mut external);
    (internal, external)
}

fn collect_markdown_links(text: &str, internal: &mut Vec<String>, external: &mut Vec<String>) {
    let mut rest = text;
    while let Some(start) = rest.find(']') {
        let after = &rest[start + 1..];
        if !after.starts_with('(') {
            rest = &rest[start + 1..];
            continue;
        }
        let url_part = &after[1..];
        if let Some(end) = url_part.find(')') {
            let target = url_part[..end].trim();
            let target = target.split_once(' ').map(|(url, _)| url).unwrap_or(target);
            let target = target.trim_matches(|ch| ch == '"' || ch == '\'');
            if !target.is_empty() {
                classify_link(target, internal, external);
            }
            rest = &url_part[end + 1..];
        } else {
            break;
        }
    }
}

fn collect_bare_urls(text: &str, internal: &mut Vec<String>, external: &mut Vec<String>) {
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|ch: char| {
            matches!(ch, '.' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '"' | '\'')
        });
        if cleaned.starts_with("http://")
            || cleaned.starts_with("https://")
            || cleaned.starts_with("mailto:")
            || cleaned.starts_with("ftp://")
        {
            classify_link(cleaned, internal, external);
        }
    }
}

fn classify_link(target: &str, internal: &mut Vec<String>, external: &mut Vec<String>) {
    if is_external_link(target) {
        if !external.iter().any(|existing| existing == target) {
            external.push(target.to_string());
        }
    } else if !internal.iter().any(|existing| existing == target) {
        internal.push(target.to_string());
    }
}

fn is_external_link(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("ftp://")
        || lower.starts_with("//")
}

fn dedupe_preserve_order(values: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn detect_language(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| !ch.is_alphabetic())
                .to_ascii_lowercase()
        })
        .filter(|token| token.len() >= 2)
        .collect();
    if words.len() < 12 {
        return None;
    }

    let candidates: &[(&str, &[&str])] = &[
        (
            "en",
            &[
                "the", "and", "of", "to", "a", "in", "is", "that", "for", "on", "with", "as",
                "this", "be", "are", "by", "from", "or", "an", "it",
            ],
        ),
        (
            "es",
            &[
                "de", "la", "que", "el", "en", "y", "los", "del", "se", "las", "por", "un", "con",
                "una", "para", "es", "al", "lo", "como", "más",
            ],
        ),
        (
            "fr",
            &[
                "de", "la", "et", "le", "les", "des", "en", "un", "une", "du", "est", "que", "pour",
                "dans", "qui", "pas", "sur", "par", "plus", "avec",
            ],
        ),
        (
            "de",
            &[
                "der", "die", "und", "den", "das", "von", "zu", "mit", "sich", "auf", "für", "ist",
                "im", "dem", "nicht", "ein", "eine", "als", "auch", "es",
            ],
        ),
    ];

    let mut best: Option<(&str, usize)> = None;
    for (code, stops) in candidates {
        let score = words
            .iter()
            .filter(|word| stops.iter().any(|stop| stop == word))
            .count();
        match best {
            Some((_, best_score)) if score <= best_score => {}
            _ => best = Some((code, score)),
        }
    }

    let (code, score) = best?;
    // Require a meaningful stopword density.
    if score * 10 < words.len() || score < 3 {
        return None;
    }
    Some(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrichment_is_deterministic() {
        let text = "# Intro\n\nSee [docs](./guide.md) and https://example.com/a.\n\n## Details\n\nThe and of to a in is that for on with as this be are by from.";
        let first = ContentEnrichment::extract(text, "markdown", Some("Intro"));
        let second = ContentEnrichment::extract(text, "markdown", Some("Intro"));
        assert_eq!(first, second);
    }

    #[test]
    fn extracts_markdown_structure_and_links() {
        let text = "# Title\n\nBody with [local](../x.md) and [web](https://jaymi.dev).\n\n## Section\n\nMore text.";
        let enrichment = ContentEnrichment::extract(text, "markdown", Some("Title"));
        assert_eq!(enrichment.headings.len(), 2);
        assert_eq!(enrichment.headings[0].text, "Title");
        assert_eq!(enrichment.headings[0].level, 1);
        assert_eq!(enrichment.headings[1].text, "Section");
        assert_eq!(enrichment.sections.len(), 2);
        assert_eq!(enrichment.internal_links, vec!["../x.md".to_string()]);
        assert_eq!(
            enrichment.external_links,
            vec!["https://jaymi.dev".to_string()]
        );
        assert!(enrichment.word_count > 0);
        assert_eq!(
            enrichment.character_count,
            text.chars().count() as u64
        );
        assert_eq!(
            enrichment.reading_time_seconds,
            estimate_reading_time_seconds(enrichment.word_count)
        );
    }

    #[test]
    fn json_top_level_keys_become_headings() {
        let text = r#"{"title":"X","count":1}"#;
        let enrichment = ContentEnrichment::extract(text, "json", Some("X"));
        assert_eq!(
            enrichment
                .headings
                .iter()
                .map(|heading| heading.text.as_str())
                .collect::<Vec<_>>(),
            vec!["count", "title"]
        );
    }

    #[test]
    fn detects_english_from_stopwords() {
        let text = "The quick brown fox and the lazy dog are in the yard of the farm for a walk with a friend that is on the path.";
        let enrichment = ContentEnrichment::extract(text, "plain_text", None);
        assert_eq!(enrichment.language.as_deref(), Some("en"));
    }

    #[test]
    fn empty_text_has_zero_reading_time() {
        let enrichment = ContentEnrichment::extract("", "plain_text", None);
        assert_eq!(enrichment.word_count, 0);
        assert_eq!(enrichment.reading_time_seconds, 0);
        assert_eq!(enrichment.sections.len(), 1);
        assert_eq!(enrichment.sections[0].title, "Body");
    }
}
