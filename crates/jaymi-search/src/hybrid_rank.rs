//! Hybrid ranking — fuse independent retrieval signals into one normalized score.
//!
//! Search strategies remain independent. This module only combines their
//! signal contributions into a single relevance score for ordering.
//!
//! Signals:
//! - filename relevance
//! - title relevance
//! - metadata relevance
//! - full-text relevance
//! - semantic relevance
//! - recency

/// Public `SearchHit.score` scale: normalized relevance ∈ `[0, SCORE_SCALE]`.
pub const SCORE_SCALE: u32 = 10_000;

/// Per-channel raw maxima used for min-max normalization.
pub const MAX_FILENAME: u32 = 100;
pub const MAX_TITLE: u32 = 135;
pub const MAX_METADATA: u32 = 200;
pub const MAX_FULL_TEXT: u32 = 225;
pub const MAX_SEMANTIC: u32 = 100;
pub const MAX_RECENCY: u32 = 100;

/// Fixed fusion weights (sum = 1.0).
pub const W_FILENAME: f64 = 0.20;
pub const W_TITLE: f64 = 0.15;
pub const W_METADATA: f64 = 0.10;
pub const W_FULL_TEXT: f64 = 0.25;
pub const W_SEMANTIC: f64 = 0.20;
pub const W_RECENCY: f64 = 0.10;

/// Half-life for recency decay (~30 days), in seconds.
const RECENCY_HALF_LIFE_SECS: f64 = 30.0 * 24.0 * 3600.0;

/// Independent ranking signals before fusion.
///
/// Each channel is populated by its own retrieval strategy; fusion never
/// requires strategies to know about each other.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RankSignals {
    /// Filename / path match strength.
    pub filename: u32,
    /// Document title / heading-title match strength.
    pub title: u32,
    /// Structured / inventory metadata match strength.
    pub metadata: u32,
    /// Full-text body match strength (phrase / frequency).
    pub full_text: u32,
    /// Semantic / embedding similarity strength.
    pub semantic: u32,
    /// Recency boost from modification time.
    pub recency: u32,
}

impl RankSignals {
    /// Merge two independent signal sets by taking the per-channel maximum.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            filename: self.filename.max(other.filename),
            title: self.title.max(other.title),
            metadata: self.metadata.max(other.metadata),
            full_text: self.full_text.max(other.full_text),
            semantic: self.semantic.max(other.semantic),
            recency: self.recency.max(other.recency),
        }
    }

    /// True when no channel contributed.
    pub fn is_empty(&self) -> bool {
        self.filename == 0
            && self.title == 0
            && self.metadata == 0
            && self.full_text == 0
            && self.semantic == 0
            && self.recency == 0
    }
}

/// Normalize a raw channel score into `[0.0, 1.0]`.
pub fn normalize_channel(raw: u32, max: u32) -> f64 {
    if max == 0 || raw == 0 {
        return 0.0;
    }
    ((raw as f64) / (max as f64)).min(1.0)
}

/// Fuse independent signals into a normalized relevance score on `[0, SCORE_SCALE]`.
pub fn fuse_relevance(signals: &RankSignals) -> u32 {
    let relevance = W_FILENAME * normalize_channel(signals.filename, MAX_FILENAME)
        + W_TITLE * normalize_channel(signals.title, MAX_TITLE)
        + W_METADATA * normalize_channel(signals.metadata, MAX_METADATA)
        + W_FULL_TEXT * normalize_channel(signals.full_text, MAX_FULL_TEXT)
        + W_SEMANTIC * normalize_channel(signals.semantic, MAX_SEMANTIC)
        + W_RECENCY * normalize_channel(signals.recency, MAX_RECENCY);
    (relevance * f64::from(SCORE_SCALE)).round() as u32
}

/// Map cosine similarity `[-1, 1]` onto the semantic raw channel `[0, MAX_SEMANTIC]`.
pub fn semantic_signal_from_similarity(similarity: f32) -> u32 {
    if similarity <= 0.0 {
        return 0;
    }
    let scaled = (similarity.clamp(0.0, 1.0) * MAX_SEMANTIC as f32).round();
    scaled as u32
}

/// Deterministic recency signal from a modification timestamp.
///
/// `reference_unix` is typically "now". Fresher files score higher; missing
/// timestamps contribute nothing.
pub fn recency_score(modified: Option<i64>, reference_unix: i64) -> u32 {
    let Some(ts) = modified.filter(|value| *value > 0) else {
        return 0;
    };
    let age = (reference_unix - ts).max(0) as f64;
    let score = 100.0 * RECENCY_HALF_LIFE_SECS / (RECENCY_HALF_LIFE_SECS + age);
    score.round().clamp(0.0, 100.0) as u32
}

/// Wall-clock unix seconds for recency (search-time reference).
pub fn ranking_now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuse_is_deterministic_and_bounded() {
        let signals = RankSignals {
            filename: 100,
            title: 110,
            metadata: 90,
            full_text: 200,
            semantic: 80,
            recency: 100,
        };
        let score = fuse_relevance(&signals);
        assert_eq!(score, fuse_relevance(&signals));
        assert!(score > 0);
        assert!(score <= SCORE_SCALE);

        let empty = fuse_relevance(&RankSignals::default());
        assert_eq!(empty, 0);
    }

    #[test]
    fn multi_signal_outranks_single_channel() {
        let filename_only = RankSignals {
            filename: 100,
            ..RankSignals::default()
        };
        let filename_and_text = RankSignals {
            filename: 100,
            full_text: 120,
            ..RankSignals::default()
        };
        assert!(fuse_relevance(&filename_and_text) > fuse_relevance(&filename_only));
    }

    #[test]
    fn semantic_boosts_fused_score() {
        let lexical = RankSignals {
            full_text: 40,
            ..RankSignals::default()
        };
        let hybrid = RankSignals {
            full_text: 40,
            semantic: 90,
            ..RankSignals::default()
        };
        assert!(fuse_relevance(&hybrid) > fuse_relevance(&lexical));
    }

    #[test]
    fn merge_keeps_independent_channel_maxima() {
        let left = RankSignals {
            filename: 80,
            full_text: 40,
            ..RankSignals::default()
        };
        let right = RankSignals {
            filename: 50,
            full_text: 120,
            title: 90,
            semantic: 70,
            ..RankSignals::default()
        };
        assert_eq!(
            left.merge(&right),
            RankSignals {
                filename: 80,
                title: 90,
                metadata: 0,
                full_text: 120,
                semantic: 70,
                recency: 0,
            }
        );
    }

    #[test]
    fn recency_prefers_fresher_timestamps() {
        let now = 1_700_000_000i64;
        let fresh = recency_score(Some(now), now);
        let week_old = recency_score(Some(now - 7 * 24 * 3600), now);
        let year_old = recency_score(Some(now - 365 * 24 * 3600), now);
        assert_eq!(fresh, 100);
        assert!(fresh > week_old);
        assert!(week_old > year_old);
        assert_eq!(recency_score(None, now), 0);
    }

    #[test]
    fn channel_normalization_caps_at_one() {
        assert_eq!(normalize_channel(0, 100), 0.0);
        assert_eq!(normalize_channel(50, 100), 0.5);
        assert_eq!(normalize_channel(200, 100), 1.0);
    }
}
