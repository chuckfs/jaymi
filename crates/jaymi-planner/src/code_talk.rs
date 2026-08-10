//! Code Talk (Sprint C0.2) — teach Jaymi how to talk about code.
//!
//! Not ChatGPT-style “prompt engineering.” This module owns the **voice and
//! shape** of coding conversation: direct engineering partner language,
//! structured sections, and bracketed next-action chips. Soft-path scaffolds
//! and Reasoning instructions both consume these rules so the model never
//! invents a parallel style.
//!
//! Example shape:
//!
//! ```text
//! script.js
//!
//! Purpose
//! Implements the runtime for a browser-based Pong game.
//!
//! Responsibilities
//! • Canvas rendering
//! • Game loop
//!
//! Interesting observations
//! • Rendering and game logic are tightly coupled.
//!
//! Recommendations
//! • Extract Renderer
//!
//! Next actions
//! [Review]
//! [Refactor]
//! ```

/// Banned hedging / narrator phrases — models and soft scaffolds must avoid these.
pub const BANNED_PHRASES: &[&str] = &[
    "appears to be",
    "appears to",
    "seems to be",
    "seems to",
    "seems like",
    "might be",
    "may be",
    "probably",
    "likely",
    "i think",
    "it looks like",
    "it would appear",
    "based on the available",
    "from what i can tell",
    "the current file appears",
    "the file appears",
    "this file appears",
];

/// Shared voice rules injected into Reasoning instructions.
pub fn voice_rules_block() -> String {
    let banned = BANNED_PHRASES
        .iter()
        .map(|p| format!("`{p}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Code Talk voice (mandatory):\n\
         - Speak as an engineering partner, not a narrator summarizing uncertainty.\n\
         - Lead with the file or symbol name as the title — never \"Coding Understanding\".\n\
         - Purpose is a direct sentence of what the code does (not \"Understand what X is for\").\n\
         - Responsibilities are capability bullets (verbs / subsystems), not metadata labels.\n\
         - Interesting observations call out coupling, size, globals, risks — concrete and blunt.\n\
         - Recommendations are actionable engineering moves (Extract X, Introduce Y).\n\
         - Next actions are short chips in square brackets only, e.g. [Review] [Refactor].\n\
         - Never use hedging: {banned}.\n\
         - Stay observational: use Workspace Intelligence only; do not invent unread file bodies.\n\
         - Do not call tools, edit files, or produce an Execution Plan on this turn."
    )
}

/// Render a next-action chip label (`Review` → `[Review]`).
pub fn next_action_chip(label: &str) -> String {
    let trimmed = label.trim().trim_matches(|c| c == '[' || c == ']');
    format!("[{trimmed}]")
}

/// Normalize a free-form suggestion into a short chip label when possible.
pub fn chip_label_from_suggestion(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.contains("review") {
        return "Review".into();
    }
    if lower.contains("refactor") {
        return "Refactor".into();
    }
    if lower.contains("test") {
        return "Generate Tests".into();
    }
    if lower.contains("architecture") {
        return "Explain Architecture".into();
    }
    if lower.contains("render") {
        return "Explain Rendering".into();
    }
    if lower.contains("dead code") || lower.contains("unused") {
        return "Find Dead Code".into();
    }
    if lower.contains("run") || lower.contains("build") {
        return "Run Build".into();
    }
    if lower.contains("generate") && lower.contains("code") {
        return "Generate Code".into();
    }
    // Keep short labels as-is; truncate long prose.
    let trimmed = raw.trim().trim_matches(|c| c == '[' || c == ']');
    if trimmed.chars().count() <= 28 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(24).collect::<String>() + "…"
    }
}

/// Display title for a path (`/proj/src/script.js` → `script.js`).
pub fn subject_title(path: Option<&str>) -> String {
    path.and_then(|p| {
        std::path::Path::new(p)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
    })
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "Current file".into())
}

/// True when text contains a banned hedging phrase.
pub fn contains_hedging(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    BANNED_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_and_titles() {
        assert_eq!(next_action_chip("Review"), "[Review]");
        assert_eq!(chip_label_from_suggestion("Review my changes."), "Review");
        assert_eq!(subject_title(Some("/game/script.js")), "script.js");
        assert!(contains_hedging("The current file appears to be a game."));
        assert!(!contains_hedging("Implements the Pong runtime."));
    }
}
