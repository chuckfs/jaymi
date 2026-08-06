//! Context History — recent ContextBundles retained for inspection.
//!
//! Debugging and future reasoning transparency only. Recording or reading
//! history never changes Planner / provider / tool execution.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::bundle::ContextBundle;
use crate::inspector::{measure_bundle_size, ContextInspectorReport};

/// Default number of recent assembles retained.
pub const DEFAULT_HISTORY_CAPACITY: usize = 32;

/// One recorded ContextBundle assemble for inspection / transparency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHistoryEntry {
    /// Unix timestamp in milliseconds when the assemble finished.
    pub timestamp_unix_ms: u64,
    /// Assemble generation counter for this entry.
    pub assemble_generation: u64,
    /// Request content preview / text recorded for this assemble.
    pub request: String,
    /// Derived request kind label (`chat`, `file_read`, …).
    pub request_kind: String,
    /// Provider ids that contributed to the bundle.
    pub providers_used: Vec<String>,
    /// Assembled bundle size in characters.
    pub bundle_size_characters: usize,
    /// Estimated tokens for the bundle size.
    pub bundle_size_estimated_tokens: usize,
    /// Wall-clock assemble duration in milliseconds.
    pub duration_ms: u64,
    /// True when this assemble was served from the ContextBundle cache.
    pub cache_hit: bool,
    /// Immutable ContextBundle snapshot retained for inspection.
    pub bundle: ContextBundle,
}

impl ContextHistoryEntry {
    /// Build an entry from a finished assemble + inspector snapshot.
    pub fn from_assemble(
        bundle: ContextBundle,
        inspection: &ContextInspectorReport,
        duration_ms: u64,
        chars_per_token: usize,
    ) -> Self {
        let (bundle_size_characters, bundle_size_estimated_tokens) =
            measure_bundle_size(&bundle, chars_per_token);
        let providers_used: Vec<String> = inspection
            .contributed()
            .into_iter()
            .map(|provider| provider.id.clone())
            .collect();
        Self {
            timestamp_unix_ms: unix_now_ms(),
            assemble_generation: bundle.assemble_generation(),
            request: inspection.request_preview.clone(),
            request_kind: inspection.request_kind.clone(),
            providers_used,
            bundle_size_characters,
            bundle_size_estimated_tokens,
            duration_ms,
            cache_hit: inspection.cache_hit,
            bundle,
        }
    }

    /// Compact one-line summary for diagnostics lists.
    pub fn summary(&self) -> String {
        format!(
            "gen={} · {}ms · {} chars (≈{} tok) · providers=[{}] · cache_hit={} · {}",
            self.assemble_generation,
            self.duration_ms,
            self.bundle_size_characters,
            self.bundle_size_estimated_tokens,
            self.providers_used.join(","),
            self.cache_hit,
            truncate(&self.request, 64)
        )
    }

    /// Plain-text render for CLI / headless diagnostics.
    pub fn render(&self) -> String {
        format!(
            "ts={} · kind={} · {}",
            self.timestamp_unix_ms,
            self.request_kind,
            self.summary()
        )
    }
}

/// Ring buffer of recent Context History entries (newest at the back).
#[derive(Debug, Default)]
pub struct ContextHistory {
    capacity: usize,
    entries: VecDeque<ContextHistoryEntry>,
}

impl ContextHistory {
    /// Create a history buffer with the given capacity (minimum 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    /// Maximum retained entries.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of retained entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no entries are retained.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a finished assemble (evicts oldest when over capacity).
    pub fn push(&mut self, entry: ContextHistoryEntry) {
        while self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Entries newest-first.
    pub fn entries(&self) -> Vec<ContextHistoryEntry> {
        self.entries.iter().rev().cloned().collect()
    }

    /// Most recent entry, when any.
    pub fn latest(&self) -> Option<&ContextHistoryEntry> {
        self.entries.back()
    }

    /// Clear all retained entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Plain-text render of recent history (newest first).
    pub fn render(&self, limit: usize) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Context History ({} / {} retained)",
            self.len(),
            self.capacity
        ));
        if self.is_empty() {
            lines.push("  (empty)".into());
            return lines.join("\n");
        }
        for (index, entry) in self.entries().into_iter().take(limit).enumerate() {
            lines.push(format!("  [{index}] {}", entry.render()));
        }
        lines.join("\n")
    }
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBundle;

    fn sample_entry(generation: u64, request: &str) -> ContextHistoryEntry {
        ContextHistoryEntry {
            timestamp_unix_ms: 1_000 + generation,
            assemble_generation: generation,
            request: request.into(),
            request_kind: "chat".into(),
            providers_used: vec!["memory".into()],
            bundle_size_characters: 100,
            bundle_size_estimated_tokens: 25,
            duration_ms: 3,
            cache_hit: false,
            bundle: ContextBundle::default(),
        }
    }

    #[test]
    fn history_retains_newest_and_evicts_oldest() {
        let mut history = ContextHistory::with_capacity(2);
        history.push(sample_entry(1, "a"));
        history.push(sample_entry(2, "b"));
        history.push(sample_entry(3, "c"));
        let entries = history.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].assemble_generation, 3);
        assert_eq!(entries[1].assemble_generation, 2);
        assert_eq!(history.latest().unwrap().request, "c");
    }

    #[test]
    fn render_includes_core_fields() {
        let entry = sample_entry(7, "hello history");
        let rendered = entry.render();
        assert!(rendered.contains("gen=7"));
        assert!(rendered.contains("providers=[memory]"));
        assert!(rendered.contains("hello history"));
        assert!(rendered.contains("3ms"));
    }
}
