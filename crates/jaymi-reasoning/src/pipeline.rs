//! Lightweight conversational pipeline stage timings (diagnostics only).
//!
//! Instrumentation is observational — it must not change generation behavior.
//! Timings surface only in Developer Diagnostics, never in the conversation UI.

use serde::{Deserialize, Serialize};

/// One measured pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineStageTiming {
    /// Stable stage id (e.g. `context_assembly`, `prompt_builder`).
    pub stage: String,
    /// Elapsed milliseconds for this stage.
    pub duration_ms: u64,
    /// Optional detail (provider id, cache hit, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl PipelineStageTiming {
    /// Construct a stage timing row.
    pub fn new(stage: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            stage: stage.into(),
            duration_ms,
            detail: None,
        }
    }

    /// Attach a short detail string.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Aggregated timings for one conversational generation.
///
/// Stages are durations (ms spent), not wall-clock timestamps. `total_ms` is
/// request-received → terminal when the Application records it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PipelineTiming {
    /// Ordered stage rows for diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<PipelineStageTiming>,
    /// Time to first token (ms from provider stream start), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    /// Generation duration (first token → terminal), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_generation_ms: Option<u64>,
    /// End-to-end wall clock (request received → terminal), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<u64>,
}

impl PipelineTiming {
    /// Empty timing bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a stage (skips zero-cost noise only when explicitly empty detail and 0).
    pub fn push(&mut self, stage: PipelineStageTiming) {
        self.stages.push(stage);
    }

    /// Record or replace a top-level stage by id.
    pub fn set_stage(&mut self, stage: impl Into<String>, duration_ms: u64) {
        let stage = stage.into();
        if let Some(existing) = self.stages.iter_mut().find(|row| row.stage == stage) {
            existing.duration_ms = duration_ms;
        } else {
            self.stages.push(PipelineStageTiming::new(stage, duration_ms));
        }
    }

    /// Record a context-provider contribute timing.
    pub fn push_provider(&mut self, provider_id: impl Into<String>, duration_ms: u64) {
        self.stages.push(
            PipelineStageTiming::new("context_provider", duration_ms)
                .with_detail(provider_id.into()),
        );
    }

    /// Merge another bag.
    ///
    /// Provider contribute rows always append. Named stages keep the earliest
    /// planner/context values and prefer later engine/transport values.
    pub fn merge(&mut self, other: PipelineTiming) {
        for stage in other.stages {
            if stage.stage == "context_provider" {
                self.stages.push(stage);
                continue;
            }
            if let Some(existing) = self.stages.iter_mut().find(|row| row.stage == stage.stage) {
                match stage.stage.as_str() {
                    "prompt_builder" | "reasoning_engine" | "provider_transport" => {
                        *existing = stage;
                    }
                    _ => {
                        // Keep the earlier request/planner/context timing.
                    }
                }
            } else {
                self.stages.push(stage);
            }
        }
        if other.ttft_ms.is_some() {
            self.ttft_ms = other.ttft_ms;
        }
        if other.total_generation_ms.is_some() {
            self.total_generation_ms = other.total_generation_ms;
        }
        if other.total_ms.is_some() {
            self.total_ms = other.total_ms;
        }
    }

    /// True when any timing is present.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
            && self.ttft_ms.is_none()
            && self.total_generation_ms.is_none()
            && self.total_ms.is_none()
    }

    /// Human-readable lines for Developer Diagnostics.
    pub fn labeled_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for stage in &self.stages {
            let label = match stage.stage.as_str() {
                "request_received" => "Request Received".to_string(),
                "planner" => "Planner".to_string(),
                "context_assembly" => "Context Assembly".to_string(),
                "context_provider" => format!(
                    "Context Provider ({})",
                    stage.detail.as_deref().unwrap_or("?")
                ),
                "prompt_builder" => "PromptBuilder".to_string(),
                "reasoning_engine" => "ReasoningEngine".to_string(),
                "provider_transport" => "Provider Transport".to_string(),
                other => other.replace('_', " "),
            };
            let value = if let Some(detail) = &stage.detail {
                if stage.stage == "context_provider" {
                    format!("{} ms", stage.duration_ms)
                } else {
                    format!("{} ms ({detail})", stage.duration_ms)
                }
            } else {
                format!("{} ms", stage.duration_ms)
            };
            rows.push((label, value));
        }
        if let Some(ttft) = self.ttft_ms {
            rows.push(("Time To First Token".into(), format!("{ttft} ms")));
        }
        if let Some(gen) = self.total_generation_ms {
            rows.push(("Total Generation".into(), format!("{gen} ms")));
        }
        if let Some(total) = self.total_ms {
            rows.push(("Total (Request → Done)".into(), format!("{total} ms")));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labeled_rows_include_providers_and_totals() {
        let mut timing = PipelineTiming::new();
        timing.set_stage("planner", 2);
        timing.push_provider("memory", 5);
        timing.set_stage("prompt_builder", 3);
        timing.ttft_ms = Some(40);
        timing.total_generation_ms = Some(120);
        timing.total_ms = Some(180);
        let rows = timing.labeled_rows();
        assert!(rows.iter().any(|(label, _)| label == "Planner"));
        assert!(rows
            .iter()
            .any(|(label, _)| label.contains("Context Provider (memory)")));
        assert!(rows
            .iter()
            .any(|(label, _)| label == "Time To First Token"));
        assert!(rows.iter().any(|(label, _)| label == "Total Generation"));
    }
}
