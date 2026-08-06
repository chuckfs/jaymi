//! Context budgeting — provider-agnostic size limits for [`ContextBundle`] assembly.
//!
//! Providers estimate contribution size; the Context Engine allocates budget to
//! higher-priority providers first. Oversized contributions are fitted by
//! truncating and summarizing while preserving important metadata. Character
//! counts and a configurable chars-per-token ratio keep the system ready for
//! future LLM context windows — no model calls happen here.

use crate::provider::ContextContribution;
use crate::{
    ActiveProjectSection, BundleSearchHit, ConversationSection, CurrentSelectionSection,
    DiagnosticsSection, FileSummariesSection, GitStatusSection, MemoryResultsSection,
    OpenFilesSection, PermissionsSection, SearchResultsSection, WorkspaceInventorySection,
};


/// Default character budget for one assembled request (~8k tokens at 4 chars/token).
pub const DEFAULT_MAX_CHARACTERS: usize = 32_000;

/// Default characters-per-token estimate for LLM-oriented budgeting.
pub const DEFAULT_CHARS_PER_TOKEN: usize = 4;

/// Characters reserved for engine-owned user-request + planner metadata stamps.
pub const ENGINE_RESERVED_CHARACTERS: usize = 512;

/// Provider priority — higher values receive budget before lower ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderPriority(u8);

impl ProviderPriority {
    /// Clamp into `0..=100`.
    pub fn new(value: u8) -> Self {
        Self(value.min(100))
    }

    /// Raw priority value.
    pub fn value(self) -> u8 {
        self.0
    }

    /// Critical metadata (workspace identity).
    pub const CRITICAL: Self = Self(100);
    /// Conversation continuity.
    pub const CONVERSATION: Self = Self(95);
    /// Permission / safety metadata.
    pub const PERMISSION: Self = Self(90);
    /// Active project identity.
    pub const PROJECT: Self = Self(85);
    /// Editor focus state.
    pub const EDITOR: Self = Self(80);
    /// Retrieved memories.
    pub const MEMORY: Self = Self(70);
    /// Search coordination / hits.
    pub const SEARCH: Self = Self(60);
    /// Git status (maintenance snapshot).
    pub const GIT_STATUS: Self = Self(55);
    /// Diagnostics.
    pub const DIAGNOSTICS: Self = Self(50);
    /// File summaries (maintenance snapshot).
    pub const FILE_SUMMARIES: Self = Self(48);
    /// Workspace inventory (maintenance snapshot).
    pub const WORKSPACE_INVENTORY: Self = Self(45);
}

impl std::fmt::Display for ProviderPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Measured or estimated size of a contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetUnits {
    /// Unicode scalar count (approximation of “characters”).
    pub characters: usize,
    /// Estimated tokens using the configured chars-per-token ratio.
    pub estimated_tokens: usize,
}

impl BudgetUnits {
    /// Build from a character count and chars-per-token ratio.
    pub fn from_characters(characters: usize, chars_per_token: usize) -> Self {
        let divisor = chars_per_token.max(1);
        Self {
            characters,
            estimated_tokens: characters.div_ceil(divisor),
        }
    }

    /// Zero-sized.
    pub fn zero() -> Self {
        Self::default()
    }
}

/// Provider estimate of an upcoming contribution (before `contribute`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetEstimate {
    /// Estimated size.
    pub units: BudgetUnits,
    /// True when the contribution can be truncated to fit.
    pub can_truncate: bool,
    /// True when the contribution can be summarized to fit.
    pub can_summarize: bool,
}

impl BudgetEstimate {
    /// Exact / measured estimate that may be truncated and summarized.
    pub fn flexible(units: BudgetUnits) -> Self {
        Self {
            units,
            can_truncate: true,
            can_summarize: true,
        }
    }

    /// Small metadata that should not be truncated away.
    pub fn metadata(units: BudgetUnits) -> Self {
        Self {
            units,
            can_truncate: false,
            can_summarize: false,
        }
    }
}

/// Configurable assemble budget (characters and optional token cap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBudgetConfig {
    /// Maximum characters across provider contributions (excluding reserved stamp).
    pub max_characters: usize,
    /// Optional hard token cap (in addition to characters).
    pub max_tokens: Option<usize>,
    /// Characters-per-token ratio for estimates (LLM-oriented).
    pub chars_per_token: usize,
    /// Characters reserved for engine stamps.
    pub reserved_characters: usize,
}

impl Default for ContextBudgetConfig {
    fn default() -> Self {
        Self {
            max_characters: DEFAULT_MAX_CHARACTERS,
            max_tokens: None,
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
            reserved_characters: ENGINE_RESERVED_CHARACTERS,
        }
    }
}

impl ContextBudgetConfig {
    /// Usable character budget for provider contributions.
    pub fn provider_character_budget(&self) -> usize {
        self.max_characters.saturating_sub(self.reserved_characters)
    }

    /// Convert characters → estimated tokens.
    pub fn tokens_for_chars(&self, characters: usize) -> usize {
        characters.div_ceil(self.chars_per_token.max(1))
    }

    /// Convert tokens → characters (upper bound for fitting).
    pub fn chars_for_tokens(&self, tokens: usize) -> usize {
        tokens.saturating_mul(self.chars_per_token.max(1))
    }

    /// Effective remaining character allowance given usage so far.
    pub fn remaining_characters(&self, used_characters: usize) -> usize {
        let mut remaining = self.provider_character_budget().saturating_sub(used_characters);
        if let Some(max_tokens) = self.max_tokens {
            let used_tokens = self.tokens_for_chars(used_characters);
            let token_room = max_tokens.saturating_sub(used_tokens);
            remaining = remaining.min(self.chars_for_tokens(token_room));
        }
        remaining
    }
}

/// Outcome of fitting one contribution into a remaining budget.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FitOutcome {
    /// True when content was truncated.
    pub truncated: bool,
    /// True when a summary note was produced.
    pub summarized: bool,
    /// Optional summary line for planner metadata / LLM preamble.
    pub summary: Option<String>,
    /// Final measured size after fitting.
    pub final_units: BudgetUnits,
    /// True when the contribution could not fit even as metadata and should be dropped.
    pub drop: bool,
}

/// Count Unicode scalars in a string.
pub fn char_len(value: &str) -> usize {
    value.chars().count()
}

fn opt_str(value: Option<&String>) -> usize {
    value.map(|s| char_len(s)).unwrap_or(0)
}

/// Measure a contribution in a provider-agnostic way.
pub fn measure_contribution(contribution: &ContextContribution, chars_per_token: usize) -> BudgetUnits {
    let mut chars = 0usize;

    if let Some(section) = &contribution.conversation {
        chars += measure_conversation(section);
    }
    if let Some(section) = &contribution.active_project {
        chars += measure_active_project(section);
    }
    if let Some(section) = &contribution.active_workspace {
        chars += opt_str(section.kind_id.as_ref()) + 16;
    }
    if let Some(section) = &contribution.current_file {
        chars += opt_str(section.path.as_ref()) + opt_str(section.language.as_ref()) + 8;
    }
    if let Some(section) = &contribution.current_selection {
        chars += measure_selection(section);
    }
    if let Some(section) = &contribution.open_files {
        chars += measure_open_files(section);
    }
    if let Some(section) = &contribution.search_results {
        chars += measure_search(section);
    }
    if let Some(section) = &contribution.memory_results {
        chars += measure_memory(section);
    }
    if let Some(section) = &contribution.diagnostics {
        chars += measure_diagnostics(section);
    }
    if let Some(section) = &contribution.git_status {
        chars += measure_git_status(section);
    }
    if let Some(section) = &contribution.workspace_inventory {
        chars += measure_workspace_inventory(section);
    }
    if let Some(section) = &contribution.file_summaries {
        chars += measure_file_summaries(section);
    }
    if let Some(section) = &contribution.permissions {
        chars += measure_permissions(section);
    }
    if let Some(section) = &contribution.active_capabilities {
        chars += section
            .capability_ids
            .iter()
            .map(|id| char_len(id) + 1)
            .sum::<usize>()
            + 8;
    }
    chars += contribution.sources.len().saturating_mul(12);

    BudgetUnits::from_characters(chars, chars_per_token)
}

fn measure_conversation(section: &ConversationSection) -> usize {
    opt_str(section.id.as_ref())
        + opt_str(section.title.as_ref())
        + opt_str(section.status.as_ref())
        + opt_str(section.project_id.as_ref())
        + 24
}

fn measure_active_project(section: &ActiveProjectSection) -> usize {
    let meta = opt_str(section.project_id.as_ref())
        + opt_str(section.name.as_ref())
        + opt_str(section.root_directory.as_ref())
        + 32;
    let detail = section
        .detail
        .as_ref()
        .map(|ctx| ctx.entry_count().saturating_mul(160).saturating_add(128))
        .unwrap_or(0);
    meta + detail
}

fn measure_selection(section: &CurrentSelectionSection) -> usize {
    opt_str(section.path.as_ref()) + opt_str(section.text.as_ref()) + 32
}

fn measure_open_files(section: &OpenFilesSection) -> usize {
    section
        .files
        .iter()
        .map(|file| char_len(&file.path) + 8)
        .sum::<usize>()
        + 8
}

fn measure_search_hit(hit: &BundleSearchHit) -> usize {
    char_len(&hit.item_id)
        + char_len(&hit.title)
        + opt_str(hit.path.as_ref())
        + opt_str(hit.match_reason.as_ref())
        + opt_str(hit.preview.as_ref())
        + 24
}

fn measure_search(section: &SearchResultsSection) -> usize {
    let hint = section
        .hint
        .as_ref()
        .map(|hint| opt_str(hint.query_preview.as_ref()) + 32)
        .unwrap_or(0);
    hint + section.hits.iter().map(measure_search_hit).sum::<usize>()
}

fn measure_memory(section: &MemoryResultsSection) -> usize {
    let memories: usize = section
        .memory
        .memories
        .iter()
        .map(|item| {
            char_len(&item.record.summary)
                + char_len(&item.record.content)
                + char_len(&item.why)
                + 48
        })
        .sum();
    let promotions: usize = section
        .promotion_suggestions
        .iter()
        .map(|item| char_len(&item.reason) + 64)
        .sum();
    memories + promotions + 48
}

fn measure_diagnostics(section: &DiagnosticsSection) -> usize {
    section
        .diagnostics
        .iter()
        .map(|diag| {
            opt_str(diag.path.as_ref())
                + char_len(&diag.severity)
                + char_len(&diag.message)
                + opt_str(diag.source.as_ref())
                + 16
        })
        .sum::<usize>()
}

fn measure_git_status(section: &GitStatusSection) -> usize {
    opt_str(section.branch.as_ref())
        + char_len(&section.summary)
        + section
            .sample_paths
            .iter()
            .map(|path| char_len(path) + 1)
            .sum::<usize>()
        + 48
}

fn measure_workspace_inventory(section: &WorkspaceInventorySection) -> usize {
    opt_str(section.root.as_ref())
        + char_len(&section.status)
        + section
            .sample_paths
            .iter()
            .map(|path| char_len(path) + 1)
            .sum::<usize>()
        + 48
}

fn measure_file_summaries(section: &FileSummariesSection) -> usize {
    section
        .entries
        .iter()
        .map(|entry| {
            char_len(&entry.path)
                + opt_str(entry.language.as_ref())
                + char_len(&entry.summary)
                + 24
        })
        .sum::<usize>()
}

fn measure_permissions(section: &PermissionsSection) -> usize {
    section
        .entries
        .iter()
        .map(|entry| {
            char_len(&entry.category)
                + char_len(&entry.action)
                + char_len(&entry.decision)
                + opt_str(entry.resource.as_ref())
                + opt_str(entry.explanation.as_ref())
                + 16
        })
        .sum::<usize>()
}

/// Fit a contribution into `max_characters`, truncating / summarizing as needed.
///
/// Preserves important metadata (ids, titles, paths, decisions) preferentially
/// over bulky payloads (project detail, memory bodies, previews, selection text).
pub fn fit_contribution(
    mut contribution: ContextContribution,
    max_characters: usize,
    chars_per_token: usize,
) -> (ContextContribution, FitOutcome) {
    let initial = measure_contribution(&contribution, chars_per_token);
    if initial.characters <= max_characters {
        return (
            contribution,
            FitOutcome {
                final_units: initial,
                ..FitOutcome::default()
            },
        );
    }

    if max_characters == 0 {
        return (
            ContextContribution::default(),
            FitOutcome {
                truncated: true,
                summarized: true,
                summary: Some("contribution dropped: zero remaining budget".into()),
                final_units: BudgetUnits::zero(),
                drop: true,
            },
        );
    }

    let mut truncated = false;
    let mut summaries = Vec::new();

    // 1) Drop bulky project detail first — keep identity metadata.
    if let Some(project) = contribution.active_project.as_mut() {
        if project.detail.is_some() {
            let entries = project
                .detail
                .as_ref()
                .map(|ctx| ctx.entry_count())
                .unwrap_or(0);
            project.detail = None;
            truncated = true;
            summaries.push(format!(
                "project detail omitted (≈{entries} entries); identity metadata preserved"
            ));
            if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                return finish(contribution, truncated, summaries, chars_per_token);
            }
        }
    }

    // 2) Summarize / truncate memories (keep highest-score first).
    if contribution.memory_results.is_some() {
        let before = contribution
            .memory_results
            .as_ref()
            .map(|section| section.memory.memories.len())
            .unwrap_or(0);
        if before > 0 {
            if let Some(memory) = contribution.memory_results.as_mut() {
                memory
                    .memory
                    .memories
                    .sort_by(|a, b| b.score.cmp(&a.score));
            }
            loop {
                if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                    break;
                }
                let Some(memory) = contribution.memory_results.as_mut() else {
                    break;
                };
                if memory.memory.memories.len() <= 1 {
                    break;
                }
                memory.memory.memories.pop();
                truncated = true;
            }
            if measure_contribution(&contribution, chars_per_token).characters > max_characters {
                if let Some(memory) = contribution.memory_results.as_mut() {
                    for item in &mut memory.memory.memories {
                        if !item.record.content.is_empty() {
                            item.record.content.clear();
                            truncated = true;
                        }
                        if item.why.len() > 80 {
                            item.why = truncate_chars(&item.why, 80);
                            truncated = true;
                        }
                    }
                }
            }
            loop {
                if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                    break;
                }
                let Some(memory) = contribution.memory_results.as_mut() else {
                    break;
                };
                if memory.promotion_suggestions.is_empty() {
                    break;
                }
                memory.promotion_suggestions.pop();
                truncated = true;
            }
            let after = contribution
                .memory_results
                .as_ref()
                .map(|section| section.memory.memories.len())
                .unwrap_or(0);
            if let Some(memory) = contribution.memory_results.as_mut() {
                if after < before || memory.memory.truncated {
                    memory.memory.truncated = true;
                    summaries.push(format!(
                        "memory results fitted: kept {after} of {before} memories"
                    ));
                }
            }
            if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                return finish(contribution, truncated, summaries, chars_per_token);
            }
        }
    }

    // 3) Truncate search hits / previews.
    if contribution.search_results.is_some() {
        let before = contribution
            .search_results
            .as_ref()
            .map(|section| section.hits.len())
            .unwrap_or(0);
        if let Some(search) = contribution.search_results.as_mut() {
            if let Some(hint) = search.hint.as_mut() {
                if let Some(preview) = hint.query_preview.as_mut() {
                    if char_len(preview) > 160 {
                        *preview = truncate_chars(preview, 160);
                        truncated = true;
                        summaries.push("search query preview truncated".into());
                    }
                }
            }
            for hit in &mut search.hits {
                if let Some(preview) = hit.preview.as_mut() {
                    if char_len(preview) > 120 {
                        *preview = truncate_chars(preview, 120);
                        truncated = true;
                    }
                }
            }
        }
        loop {
            if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                break;
            }
            let Some(search) = contribution.search_results.as_mut() else {
                break;
            };
            if search.hits.len() <= 1 {
                break;
            }
            search.hits.pop();
            truncated = true;
        }
        let after = contribution
            .search_results
            .as_ref()
            .map(|section| section.hits.len())
            .unwrap_or(0);
        if after < before {
            summaries.push(format!("search hits fitted: kept {after} of {before}"));
        }
        if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
            return finish(contribution, truncated, summaries, chars_per_token);
        }
    }

    // 4) Diagnostics — keep errors, then warnings; drop the rest.
    if contribution.diagnostics.is_some() {
        let before = contribution
            .diagnostics
            .as_ref()
            .map(|section| section.diagnostics.len())
            .unwrap_or(0);
        if let Some(diagnostics) = contribution.diagnostics.as_mut() {
            diagnostics
                .diagnostics
                .sort_by_key(|diag| severity_rank(&diag.severity));
            for diag in &mut diagnostics.diagnostics {
                if char_len(&diag.message) > 160 {
                    diag.message = truncate_chars(&diag.message, 160);
                    truncated = true;
                }
            }
        }
        loop {
            if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                break;
            }
            let Some(diagnostics) = contribution.diagnostics.as_mut() else {
                break;
            };
            if diagnostics.diagnostics.len() <= 1 {
                break;
            }
            diagnostics.diagnostics.pop();
            truncated = true;
        }
        let after = contribution
            .diagnostics
            .as_ref()
            .map(|section| section.diagnostics.len())
            .unwrap_or(0);
        if after < before {
            summaries.push(format!("diagnostics fitted: kept {after} of {before}"));
        }
        if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
            return finish(contribution, truncated, summaries, chars_per_token);
        }
    }

    // 4b) Maintenance snapshots — trim samples / summaries.
    if contribution.git_status.is_some()
        || contribution.workspace_inventory.is_some()
        || contribution.file_summaries.is_some()
    {
        loop {
            if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                break;
            }
            let mut trimmed = false;
            if let Some(git) = contribution.git_status.as_mut() {
                if git.sample_paths.len() > 4 {
                    git.sample_paths.pop();
                    trimmed = true;
                }
            }
            if !trimmed {
                if let Some(inventory) = contribution.workspace_inventory.as_mut() {
                    if inventory.sample_paths.len() > 4 {
                        inventory.sample_paths.pop();
                        trimmed = true;
                    }
                }
            }
            if !trimmed {
                if let Some(summaries_section) = contribution.file_summaries.as_mut() {
                    for entry in &mut summaries_section.entries {
                        if char_len(&entry.summary) > 200 {
                            entry.summary = truncate_chars(&entry.summary, 200);
                            trimmed = true;
                        }
                    }
                    if !trimmed && summaries_section.entries.len() > 1 {
                        summaries_section.entries.pop();
                        trimmed = true;
                    }
                }
            }
            if !trimmed {
                break;
            }
            truncated = true;
        }
        if truncated {
            summaries.push("maintenance snapshots fitted".into());
        }
        if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
            return finish(contribution, truncated, summaries, chars_per_token);
        }
    }

    // 5) Open files — keep active first.
    if contribution.open_files.is_some() {
        let before = contribution
            .open_files
            .as_ref()
            .map(|section| section.files.len())
            .unwrap_or(0);
        if let Some(open_files) = contribution.open_files.as_mut() {
            open_files
                .files
                .sort_by_key(|file| if file.active { 0 } else { 1 });
        }
        loop {
            if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
                break;
            }
            let Some(open_files) = contribution.open_files.as_mut() else {
                break;
            };
            if open_files.files.len() <= 1 {
                break;
            }
            open_files.files.pop();
            truncated = true;
        }
        let after = contribution
            .open_files
            .as_ref()
            .map(|section| section.files.len())
            .unwrap_or(0);
        if after < before {
            summaries.push(format!("open files fitted: kept {after} of {before}"));
        }
        if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
            return finish(contribution, truncated, summaries, chars_per_token);
        }
    }

    // 6) Selection text truncate / drop.
    if let Some(selection) = contribution.current_selection.as_mut() {
        if let Some(text) = selection.text.as_mut() {
            if char_len(text) > 200 {
                *text = truncate_chars(text, 200);
                truncated = true;
                summaries.push("selection text truncated".into());
            }
        }
    }
    if measure_contribution(&contribution, chars_per_token).characters > max_characters {
        if let Some(selection) = contribution.current_selection.as_mut() {
            if selection.text.is_some() {
                selection.text = None;
                truncated = true;
                summaries.push("selection text omitted to preserve path/range metadata".into());
            }
        }
    }
    if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
        return finish(contribution, truncated, summaries, chars_per_token);
    }

    // 7) Permissions — keep decisions; trim explanations.
    if let Some(permissions) = contribution.permissions.as_mut() {
        for entry in &mut permissions.entries {
            if let Some(explanation) = entry.explanation.as_mut() {
                if char_len(explanation) > 120 {
                    *explanation = truncate_chars(explanation, 120);
                    truncated = true;
                }
            }
        }
    }
    loop {
        if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
            break;
        }
        let Some(permissions) = contribution.permissions.as_mut() else {
            break;
        };
        if permissions.entries.len() <= 1 {
            break;
        }
        permissions.entries.pop();
        truncated = true;
    }
    if measure_contribution(&contribution, chars_per_token).characters <= max_characters {
        return finish(contribution, truncated, summaries, chars_per_token);
    }

    // 8) Last resort: keep only metadata-shaped stubs for heavy sections.
    if let Some(memory) = contribution.memory_results.as_mut() {
        let n = memory.memory.memories.len();
        memory.memory.memories.clear();
        memory.promotion_suggestions.clear();
        memory.memory.truncated = true;
        truncated = true;
        summaries.push(format!(
            "memory bodies omitted; metadata preserved (was {n} memories)"
        ));
    }
    if let Some(search) = contribution.search_results.as_mut() {
        let n = search.hits.len();
        search.hits.clear();
        if let Some(hint) = search.hint.as_mut() {
            if hint.query_preview.as_ref().map(|value| char_len(value)).unwrap_or(0) > 80 {
                hint.query_preview = hint
                    .query_preview
                    .as_ref()
                    .map(|value| truncate_chars(value, 80));
            }
        }
        truncated = true;
        if n > 0 {
            summaries.push(format!("search hits omitted; hint metadata preserved (was {n})"));
        }
    }
    if let Some(diagnostics) = contribution.diagnostics.as_mut() {
        let n = diagnostics.diagnostics.len();
        diagnostics.diagnostics.clear();
        truncated = true;
        if n > 0 {
            summaries.push(format!("diagnostics omitted after budget fit (was {n})"));
        }
    }

    let final_units = measure_contribution(&contribution, chars_per_token);
    if final_units.characters > max_characters {
        // Even metadata does not fit — drop entirely but report.
        return (
            ContextContribution::default(),
            FitOutcome {
                truncated: true,
                summarized: true,
                summary: Some(format!(
                    "contribution dropped: metadata still exceeds budget ({} > {max_characters})",
                    final_units.characters
                )),
                final_units: BudgetUnits::zero(),
                drop: true,
            },
        );
    }

    finish(contribution, truncated, summaries, chars_per_token)
}

fn finish(
    contribution: ContextContribution,
    truncated: bool,
    summaries: Vec<String>,
    chars_per_token: usize,
) -> (ContextContribution, FitOutcome) {
    let final_units = measure_contribution(&contribution, chars_per_token);
    let summary = if summaries.is_empty() {
        None
    } else {
        Some(summaries.join("; "))
    };
    let summarized = summary.is_some();
    (
        contribution,
        FitOutcome {
            truncated,
            summarized,
            summary,
            final_units,
            drop: false,
        },
    )
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

fn severity_rank(severity: &str) -> u8 {
    match severity.to_ascii_lowercase().as_str() {
        "error" => 0,
        "warning" => 1,
        "info" => 2,
        "hint" => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActiveProjectSection, BundleSearchHit, ContextSource, SearchContextHint};

    #[test]
    fn fit_drops_project_detail_before_identity() {
        // Simulate a large detail by using measure path without a real ProjectContext:
        // build a contribution with a long name and ensure identity survives fitting.
        let contribution = ContextContribution {
            sources: vec![ContextSource::ActiveProject],
            active_project: Some(ActiveProjectSection {
                project_id: Some("proj-1".into()),
                name: Some("Jaymi".into()),
                root_directory: Some("/tmp/jaymi".into()),
                detail: None,
            }),
            search_results: Some(SearchResultsSection {
                hint: Some(SearchContextHint {
                    structured_query_pending: true,
                    query_preview: Some("x".repeat(800)),
                    project_indexed_documents: Some(9),
                }),
                hits: (0..50)
                    .map(|i| BundleSearchHit {
                        item_id: format!("id-{i}"),
                        title: format!("title-{i}"),
                        path: Some(format!("/tmp/file-{i}.rs")),
                        score: Some(100 - i),
                        match_reason: Some("free_text".into()),
                        preview: Some("p".repeat(400)),
                        line: Some(1),
                        column: Some(0),
                    })
                    .collect(),
            }),
            ..ContextContribution::default()
        };

        let before = measure_contribution(&contribution, 4).characters;
        assert!(before > 2_000);

        let (fitted, outcome) = fit_contribution(contribution, 1_500, 4);
        assert!(!outcome.drop);
        assert!(outcome.truncated || outcome.summarized);
        assert!(fitted.active_project.as_ref().unwrap().project_id.is_some());
        assert!(fitted.active_project.as_ref().unwrap().name.as_deref() == Some("Jaymi"));
        assert!(measure_contribution(&fitted, 4).characters <= 1_500);
    }

    #[test]
    fn remaining_respects_token_cap() {
        let config = ContextBudgetConfig {
            max_characters: 10_000,
            max_tokens: Some(100),
            chars_per_token: 4,
            reserved_characters: 0,
        };
        // 100 tokens * 4 = 400 chars usable.
        assert_eq!(config.remaining_characters(0), 400);
        assert_eq!(config.remaining_characters(200), 200);
    }
}
