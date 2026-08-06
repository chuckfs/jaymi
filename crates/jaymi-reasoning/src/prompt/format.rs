//! Provider-independent prompt formatting.

use super::types::PromptSection;

/// Formats ordered [`PromptSection`]s into a single prompt string.
///
/// Implementations must be deterministic: identical sections → identical text.
pub trait PromptFormatter: Send + Sync {
    /// Stable formatter id for diagnostics.
    fn id(&self) -> &str;

    /// Format sections into the final prompt body.
    fn format(&self, sections: &[PromptSection]) -> String;
}

/// Default plain-text formatter (`## Heading` + body, blank line between sections).
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainTextFormatter;

impl PromptFormatter for PlainTextFormatter {
    fn id(&self) -> &str {
        "plain_text"
    }

    fn format(&self, sections: &[PromptSection]) -> String {
        let mut out = String::new();
        for (index, section) in sections.iter().enumerate() {
            if index > 0 {
                out.push_str("\n\n");
            }
            out.push_str("## ");
            out.push_str(&section.heading);
            out.push('\n');
            out.push_str(section.body.trim_end());
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }
}
