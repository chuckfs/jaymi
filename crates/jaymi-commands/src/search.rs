//! Lightweight fuzzy filtering for the Command Palette.

use crate::descriptor::CommandDescriptor;

/// Score how well `query` matches a command (higher is better; 0 = no match).
pub fn command_score(command: &CommandDescriptor, query: &str) -> u32 {
    let query = query.trim();
    if query.is_empty() {
        return 1;
    }
    let needle = query.to_ascii_lowercase();
    let mut best = 0_u32;

    best = best.max(score_text(&command.title, &needle));
    best = best.max(score_text(&command.id, &needle));
    best = best.max(score_text(command.category.label(), &needle) / 2);
    for keyword in &command.keywords {
        best = best.max(score_text(keyword, &needle));
    }
    best
}

/// Filter and rank commands for `query` (empty query → all, title-sorted by caller).
pub fn filter_commands(commands: &[CommandDescriptor], query: &str) -> Vec<CommandDescriptor> {
    let query = query.trim();
    if query.is_empty() {
        return commands.to_vec();
    }
    let mut scored: Vec<_> = commands
        .iter()
        .filter_map(|command| {
            let score = command_score(command, query);
            (score > 0).then_some((score, command.clone()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.cmp(&b.1.title)));
    scored.into_iter().map(|(_, command)| command).collect()
}

fn score_text(haystack: &str, needle: &str) -> u32 {
    let hay = haystack.to_ascii_lowercase();
    if hay == needle {
        return 1000;
    }
    if hay.starts_with(needle) {
        return 800;
    }
    if hay.contains(needle) {
        return 600;
    }
    // Subsequence match (e.g. "tgl" → "Toggle").
    if subsequence_match(&hay, needle) {
        return 400;
    }
    // Token match: every needle word appears somewhere.
    let tokens: Vec<_> = needle.split_whitespace().filter(|t| !t.is_empty()).collect();
    if !tokens.is_empty() && tokens.iter().all(|token| hay.contains(token)) {
        return 500;
    }
    0
}

fn subsequence_match(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    for needle_ch in needle.chars() {
        loop {
            match chars.next() {
                Some(ch) if ch == needle_ch => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builtin_descriptors, ids};

    #[test]
    fn empty_query_returns_all() {
        let all = builtin_descriptors();
        assert_eq!(filter_commands(&all, "").len(), all.len());
    }

    #[test]
    fn save_ranks_above_unrelated() {
        let all = builtin_descriptors();
        let hits = filter_commands(&all, "save");
        assert_eq!(hits[0].id, ids::SAVE);
    }
}
