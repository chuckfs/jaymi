//! Content match locating for regex / case / whole-word options.
//!
//! Operates on already-indexed plain text from Content Intelligence — does not
//! build a second index.

use regex::{Regex, RegexBuilder};

use jaymi_core::SearchRequest;

/// One located match inside a document body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedMatch {
    /// Zero-based start line.
    pub line: u32,
    /// Zero-based start column.
    pub column: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end column.
    pub end_column: u32,
    /// Single-line preview around the match.
    pub preview: String,
}

/// Locate matches of `query` in `text` using request match options.
pub fn locate_matches(text: &str, query: &str, request: &SearchRequest) -> Vec<LocatedMatch> {
    let query = query.trim();
    if query.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let Some(re) = build_regex(query, request) else {
        return Vec::new();
    };
    locate_with_regex(text, &re)
}

/// Replace matches of `query` in `text` with `replacement`, using the same
/// match options as [`locate_matches`]. Returns the new text and match count.
///
/// When [`SearchRequest::use_regex`] is set, `replacement` may reference
/// capture groups (`$1`, `${name}`); otherwise it is inserted literally.
pub fn replace_matches(
    text: &str,
    query: &str,
    replacement: &str,
    request: &SearchRequest,
) -> (String, usize) {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() || text.is_empty() {
        return (text.to_string(), 0);
    }
    let Some(re) = build_regex(trimmed_query, request) else {
        return (text.to_string(), 0);
    };
    let count = re.find_iter(text).count();
    if count == 0 {
        return (text.to_string(), 0);
    }
    let replaced = if request.use_regex {
        re.replace_all(text, replacement).into_owned()
    } else {
        re.replace_all(text, regex::NoExpand(replacement)).into_owned()
    };
    (replaced, count)
}

fn build_regex(query: &str, request: &SearchRequest) -> Option<Regex> {
    let pattern = if request.use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let pattern = if request.whole_word {
        format!(r"(?-u:\b(?:{pattern})\b)")
    } else {
        pattern
    };

    RegexBuilder::new(&pattern)
        .case_insensitive(!request.case_sensitive)
        .multi_line(true)
        .build()
        .ok()
}

fn locate_with_regex(text: &str, re: &Regex) -> Vec<LocatedMatch> {
    let mut out = Vec::new();
    for mat in re.find_iter(text) {
        if let Some(located) = span_to_located(text, mat.start(), mat.end()) {
            out.push(located);
        }
        if out.len() >= 200 {
            break;
        }
    }
    out
}

fn span_to_located(text: &str, start: usize, end: usize) -> Option<LocatedMatch> {
    let start = start.min(text.len());
    let end = end.min(text.len()).max(start);
    let before = &text[..start];
    let line = before.bytes().filter(|&b| b == b'\n').count() as u32;
    let line_start = before.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let column = text[line_start..start].chars().count() as u32;
    let matched = &text[start..end];
    let end_line = line + matched.bytes().filter(|&b| b == b'\n').count() as u32;
    let last_nl = matched.rfind('\n').map(|idx| start + idx + 1).unwrap_or(start);
    let end_column = if end_line == line {
        column + matched.chars().count() as u32
    } else {
        text[last_nl..end].chars().count() as u32
    };
    let line_end = text[line_start..]
        .find('\n')
        .map(|idx| line_start + idx)
        .unwrap_or(text.len());
    let preview = text[line_start..line_end].trim_end_matches('\r').to_string();
    Some(LocatedMatch {
        line,
        column,
        end_line,
        end_column,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_case_and_word() {
        let text = "Alpha alpha alphabet\nbeta\n";
        let mut request = SearchRequest::free_text("alpha");
        let all = locate_matches(text, "alpha", &request);
        assert!(all.len() >= 2);

        request.case_sensitive = true;
        let cased = locate_matches(text, "alpha", &request);
        assert_eq!(cased.len(), 2);

        request.whole_word = true;
        let words = locate_matches(text, "alpha", &request);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].line, 0);
    }

    #[test]
    fn regex_digit_match() {
        let text = "v1 and v22\n";
        let request = SearchRequest::free_text(r"v\d+").with_regex(true);
        let hits = locate_matches(text, r"v\d+", &request);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn replace_literal_matches_case_insensitive() {
        let text = "Alpha alpha beta";
        let request = SearchRequest::free_text("alpha");
        let (replaced, count) = replace_matches(text, "alpha", "gamma", &request);
        assert_eq!(count, 2);
        assert_eq!(replaced, "gamma gamma beta");
    }

    #[test]
    fn replace_respects_whole_word_and_case_sensitive() {
        let text = "cat catalog Cat\n";
        let mut request = SearchRequest::free_text("cat").with_whole_word(true);
        let (replaced, count) = replace_matches(text, "cat", "dog", &request);
        assert_eq!(count, 2);
        assert_eq!(replaced, "dog catalog dog\n");

        request.case_sensitive = true;
        let (replaced, count) = replace_matches(text, "cat", "dog", &request);
        assert_eq!(count, 1);
        assert_eq!(replaced, "dog catalog Cat\n");
    }

    #[test]
    fn replace_regex_supports_capture_groups() {
        let text = "v1 and v22\n";
        let request = SearchRequest::free_text(r"v(\d+)").with_regex(true);
        let (replaced, count) = replace_matches(text, r"v(\d+)", "ver$1", &request);
        assert_eq!(count, 2);
        assert_eq!(replaced, "ver1 and ver22\n");
    }

    #[test]
    fn replace_no_match_returns_original() {
        let request = SearchRequest::free_text("missing");
        let (replaced, count) = replace_matches("hello world", "missing", "x", &request);
        assert_eq!(count, 0);
        assert_eq!(replaced, "hello world");
    }
}
