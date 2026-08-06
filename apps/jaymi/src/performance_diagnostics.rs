//! Performance dashboard for Developer Diagnostics.
//!
//! Aggregates observational timings and sizes from the last conversational turn
//! and Context History. Never influences Planner routing, ConversationState,
//! prompt construction, or generation behavior. Surfaces only under Developer
//! Diagnostics / Coding Diagnostics — never in the conversation transcript.

use jaymi_context::{ContextHistoryEntry, ContextInspectorReport};
use jaymi_reasoning::ReasoningDiagnosticsReport;

/// One pipeline / timing row on the Performance dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceTimelineRow {
    /// Human-readable stage label.
    pub label: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Optional detail (provider id, cache status, …).
    pub detail: Option<String>,
    /// Whether this row is a context-provider contribute timing.
    pub is_context_provider: bool,
}

/// Read-only Performance dashboard assembled from existing diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PerformanceDashboard {
    /// Model used for the last turn (`provider/name` preference order).
    pub model_used: Option<String>,
    /// Time to first token (ms), when known.
    pub ttft_ms: Option<u64>,
    /// End-to-end response time (request → done), when known.
    pub total_response_ms: Option<u64>,
    /// Generation duration (first token → terminal), when known.
    pub total_generation_ms: Option<u64>,
    /// Reasoning provider transport / latency rows.
    pub provider_timings: Vec<(String, String)>,
    /// Per-context-provider contribute timings.
    pub context_provider_timings: Vec<(String, String)>,
    /// Context assemble cache hits across retained history.
    pub cache_hits: usize,
    /// Context assemble cache misses across retained history.
    pub cache_misses: usize,
    /// Last assemble cache status (`hit` / `miss`).
    pub last_cache_status: Option<String>,
    /// Assembled (pre-seal) prompt size in characters.
    pub prompt_size_characters: Option<usize>,
    /// Delivered chat-message prompt size in characters.
    pub delivered_prompt_size_characters: Option<usize>,
    /// Ordered pipeline timeline (stages + aggregates).
    pub timeline: Vec<PerformanceTimelineRow>,
}

impl PerformanceDashboard {
    /// Assemble from live Developer Diagnostics snapshot fields.
    ///
    /// Pure observation — copies labels and numbers only.
    pub fn from_sources(
        reasoning: Option<&ReasoningDiagnosticsReport>,
        context: Option<&ContextInspectorReport>,
        history: &[ContextHistoryEntry],
    ) -> Self {
        let mut dashboard = Self::default();

        let (cache_hits, cache_misses) = history.iter().fold((0usize, 0usize), |(hits, misses), entry| {
            if entry.cache_hit {
                (hits + 1, misses)
            } else {
                (hits, misses + 1)
            }
        });
        dashboard.cache_hits = cache_hits;
        dashboard.cache_misses = cache_misses;

        if let Some(inspector) = context {
            dashboard.last_cache_status = Some(inspector.cache_status().to_string());
            // Prefer inspector timings when history is empty (still show last assemble).
            if history.is_empty() {
                if inspector.cache_hit {
                    dashboard.cache_hits = 1;
                } else {
                    dashboard.cache_misses = 1;
                }
            }
            for (id, detail) in inspector.provider_timing_rows() {
                dashboard
                    .context_provider_timings
                    .push((format!("Context Provider ({id})"), detail));
            }
        }

        let Some(report) = reasoning else {
            return dashboard;
        };

        dashboard.model_used = report
            .actual_model
            .clone()
            .or_else(|| report.provider_model.clone())
            .or_else(|| report.current_model.clone())
            .or_else(|| report.configured_model.clone());

        dashboard.prompt_size_characters = report
            .assembled_prompt_size_characters
            .or(report.prompt_size_characters);
        dashboard.delivered_prompt_size_characters = report.prompt_size_characters;

        if let Some(timing) = &report.pipeline_timing {
            dashboard.ttft_ms = timing.ttft_ms;
            dashboard.total_response_ms = timing.total_ms.or(report.latency_ms);
            dashboard.total_generation_ms = timing.total_generation_ms;

            for stage in &timing.stages {
                let is_context_provider = stage.stage == "context_provider";
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
                if stage.stage == "provider_transport" {
                    let value = if let Some(detail) = &stage.detail {
                        format!("{} ms ({detail})", stage.duration_ms)
                    } else {
                        format!("{} ms", stage.duration_ms)
                    };
                    dashboard
                        .provider_timings
                        .push(("Provider Transport".into(), value));
                }
                if is_context_provider {
                    let value = format!("{} ms", stage.duration_ms);
                    // Prefer pipeline contribute rows when present (more precise).
                    if !dashboard
                        .context_provider_timings
                        .iter()
                        .any(|(existing, _)| existing == &label)
                    {
                        dashboard
                            .context_provider_timings
                            .push((label.clone(), value));
                    } else if let Some((_, existing)) = dashboard
                        .context_provider_timings
                        .iter_mut()
                        .find(|(existing, _)| existing == &label)
                    {
                        // Prefer pipeline ms when inspector detail is verbose.
                        if !existing.ends_with(" ms") || existing.contains('·') {
                            *existing = value;
                        }
                    }
                }
                dashboard.timeline.push(PerformanceTimelineRow {
                    label,
                    duration_ms: stage.duration_ms,
                    detail: stage.detail.clone(),
                    is_context_provider,
                });
            }
            if let Some(ttft) = timing.ttft_ms {
                dashboard.timeline.push(PerformanceTimelineRow {
                    label: "Time To First Token".into(),
                    duration_ms: ttft,
                    detail: None,
                    is_context_provider: false,
                });
            }
            if let Some(gen) = timing.total_generation_ms {
                dashboard.timeline.push(PerformanceTimelineRow {
                    label: "Total Generation".into(),
                    duration_ms: gen,
                    detail: None,
                    is_context_provider: false,
                });
            }
            if let Some(total) = timing.total_ms {
                dashboard.timeline.push(PerformanceTimelineRow {
                    label: "Total (Request → Done)".into(),
                    duration_ms: total,
                    detail: None,
                    is_context_provider: false,
                });
            }
        } else {
            dashboard.ttft_ms = None;
            dashboard.total_response_ms = report.latency_ms;
        }

        if let Some(provider_ms) = report.provider_latency_ms {
            dashboard.provider_timings.push((
                "Provider Latency".into(),
                format!("{provider_ms} ms"),
            ));
        }
        if let Some(wall) = report.latency_ms {
            if dashboard.total_response_ms.is_none() {
                dashboard.total_response_ms = Some(wall);
            }
            dashboard
                .provider_timings
                .push(("Wall Latency".into(), format!("{wall} ms")));
        }

        dashboard
    }

    /// True when any performance signal is present.
    pub fn has_content(&self) -> bool {
        self.model_used.is_some()
            || self.ttft_ms.is_some()
            || self.total_response_ms.is_some()
            || !self.timeline.is_empty()
            || !self.provider_timings.is_empty()
            || !self.context_provider_timings.is_empty()
            || self.cache_hits + self.cache_misses > 0
            || self.prompt_size_characters.is_some()
            || self.delivered_prompt_size_characters.is_some()
    }

    /// Compact one-line summary for Coding Diagnostics.
    pub fn summary_line(&self) -> String {
        format!(
            "model={} · ttft={} · total={} · cache={}/{} · prompt={} · delivered={}",
            opt_str(self.model_used.as_deref()),
            opt_ms(self.ttft_ms),
            opt_ms(self.total_response_ms),
            self.cache_hits,
            self.cache_misses,
            opt_chars(self.prompt_size_characters),
            opt_chars(self.delivered_prompt_size_characters),
        )
    }

    /// Headline metric rows for grids / text render.
    pub fn metric_rows(&self) -> Vec<(String, String)> {
        vec![
            (
                "Model Used".into(),
                opt_str(self.model_used.as_deref()),
            ),
            ("TTFT".into(), opt_ms(self.ttft_ms)),
            (
                "Total Response Time".into(),
                opt_ms(self.total_response_ms),
            ),
            (
                "Total Generation".into(),
                opt_ms(self.total_generation_ms),
            ),
            (
                "Cache Hits / Misses".into(),
                format!(
                    "{} / {}{}",
                    self.cache_hits,
                    self.cache_misses,
                    self.last_cache_status
                        .as_ref()
                        .map(|status| format!(" (last={status})"))
                        .unwrap_or_default()
                ),
            ),
            (
                "Prompt Size".into(),
                opt_chars(self.prompt_size_characters),
            ),
            (
                "Delivered Prompt Size".into(),
                opt_chars(self.delivered_prompt_size_characters),
            ),
        ]
    }

    /// Lines for Coding Diagnostics / headless render.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.summary_line()];
        for (label, value) in self.metric_rows() {
            lines.push(format!("{label}={value}"));
        }
        if !self.provider_timings.is_empty() {
            lines.push("Provider timings:".into());
            for (label, value) in &self.provider_timings {
                lines.push(format!("  {label}={value}"));
            }
        }
        if !self.context_provider_timings.is_empty() {
            lines.push("Context provider timings:".into());
            for (label, value) in &self.context_provider_timings {
                lines.push(format!("  {label}={value}"));
            }
        }
        if !self.timeline.is_empty() {
            lines.push("Pipeline timeline:".into());
            for row in &self.timeline {
                let detail = row
                    .detail
                    .as_ref()
                    .filter(|_| !row.is_context_provider)
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default();
                lines.push(format!(
                    "  {}={} ms{detail}",
                    row.label, row.duration_ms
                ));
            }
        }
        lines
    }

    /// Plain-text section for `DiagnosticsSnapshot::render_dashboard`.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Performance".to_string());
        lines.push(
            "Observational only — never shown in conversation mode.".to_string(),
        );
        lines.extend(self.lines());
        lines.join("\n")
    }
}

fn opt_str(value: Option<&str>) -> String {
    value.unwrap_or("-").to_string()
}

fn opt_ms(value: Option<u64>) -> String {
    value
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| "-".into())
}

fn opt_chars(value: Option<usize>) -> String {
    value
        .map(|n| format!("{n} chars"))
        .unwrap_or_else(|| "-".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_reasoning::{
        PipelineTiming, ReasoningDiagnosticsInput, ReasoningDiagnosticsReport, ReasoningHealth,
    };

    #[test]
    fn dashboard_surfaces_required_performance_fields() {
        let mut timing = PipelineTiming::new();
        timing.set_stage("planner", 2);
        timing.push_provider("memory", 5);
        timing.set_stage("provider_transport", 80);
        timing.ttft_ms = Some(40);
        timing.total_generation_ms = Some(120);
        timing.total_ms = Some(200);

        let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
            health: Some(ReasoningHealth::Ready),
            reasoning_used: true,
            pipeline_timing: Some(timing),
            configured_model: Some("ollama/llama3.2".into()),
            provider_model: Some("ollama/llama3.2".into()),
            prompt: Some(jaymi_reasoning::PromptDiagnostics {
                prompt_size_characters: 100,
                prompt_size_tokens: 25,
                assembled_prompt_size_characters: Some(110),
                assembled_prompt_size_tokens: Some(28),
                final_token_estimate: 25,
                conversation_turns: 0,
                budget: jaymi_reasoning::PromptBudgetUsage {
                    used_characters: 100,
                    estimated_tokens: 25,
                    max_characters: None,
                    max_tokens: None,
                    remaining_characters: None,
                    remaining_tokens: None,
                    reserved_completion_tokens: 0,
                    context_window_tokens: None,
                    context_efficiency_bps: None,
                    truncated: false,
                },
                sections: vec![],
                llm_coverage: vec![],
                truncated: false,
                truncation_notes: vec![],
                template_id: None,
                formatter_id: None,
                adapter_id: None,
                build_duration_ms: None,
            }),
            ..ReasoningDiagnosticsInput::default()
        });

        let dashboard = PerformanceDashboard::from_sources(Some(&report), None, &[]);
        assert!(dashboard.has_content());
        assert_eq!(dashboard.model_used.as_deref(), Some("ollama/llama3.2"));
        assert_eq!(dashboard.ttft_ms, Some(40));
        assert_eq!(dashboard.total_response_ms, Some(200));
        assert_eq!(dashboard.prompt_size_characters, Some(110));
        assert_eq!(dashboard.delivered_prompt_size_characters, Some(100));
        assert!(dashboard
            .timeline
            .iter()
            .any(|row| row.label == "Time To First Token"));
        assert!(dashboard
            .context_provider_timings
            .iter()
            .any(|(label, _)| label.contains("memory")));
        assert!(dashboard
            .provider_timings
            .iter()
            .any(|(label, _)| label == "Provider Transport"));
        let rendered = dashboard.render();
        assert!(rendered.contains("Performance"));
        assert!(rendered.contains("Observational only"));
    }
}
