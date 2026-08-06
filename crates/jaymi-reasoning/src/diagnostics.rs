//! Conversational Reasoning diagnostics — full lifecycle visibility.
//!
//! Explains provider, model, tokens, context, latency, streaming, cancellation,
//! health, prompt budget/sections, conversation runtime state, and provider
//! status. No hidden state: every field is explicit on
//! [`ReasoningDiagnosticsReport`].
//!
//! **Sprint B1.10**

use crate::lifecycle::{CancelReason, StreamingLifecycle};
use crate::metrics::ReasoningMetrics;
use crate::model::ModelIdentifier;
use crate::prompt::{PromptBudgetUsage, PromptDiagnostics, PromptSectionContribution};
use crate::provider::{ReasoningCapabilities, ReasoningHealth};
use crate::registry::{ModelRegistrySnapshot, ProviderHealthEntry};

/// Snapshot of conversational reasoning for Developer Diagnostics.
///
/// Assembled from live provider/registry health plus the last reasoning turn
/// (metrics + prompt inspection). Missing values render as `-` — never invented.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct ReasoningDiagnosticsReport {
    /// Logical reasoning provider id (e.g. `ollama`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_provider: Option<String>,
    /// Current / last model display id (`provider/name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    /// Configured model (registry default or preferred) — Sprint B1.13.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_model: Option<String>,
    /// Actual model used for the last turn (provider-reported metrics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_model: Option<String>,
    /// Model id attached onto `ReasoningRequest` for the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    /// Currently loaded model in the backend (e.g. Ollama `/api/ps`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_model: Option<String>,
    /// Prompt / input tokens (provider-reported or prompt estimate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    /// Completion / output tokens when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    /// Model context window size in tokens when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u64>,
    /// Wall-clock latency for the last turn (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Provider / TTFT latency when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_latency_ms: Option<u64>,
    /// Streaming lifecycle label (`idle` / `thinking` / `streaming` / …).
    pub streaming: String,
    /// Cancellation status (`none` or a [`CancelReason`] label).
    pub cancellation: String,
    /// Reasoning backend health (`ready` / `degraded` / `unavailable`).
    pub reasoning_health: String,
    /// Prompt budget usage from the last assembled prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_budget: Option<PromptBudgetUsage>,
    /// Per-section prompt contributions from the last assemble.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_sections: Vec<PromptSectionContribution>,
    /// Delivered prompt character size when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_size_characters: Option<usize>,
    /// Final token estimate for the delivered prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_token_estimate: Option<u64>,
    /// Prior conversation turns included in the delivered prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_turns: Option<u64>,
    /// Truncated / budget-omitted section ids (labels).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncated_sections: Vec<String>,
    /// Excluded section ids (labels).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_sections: Vec<String>,
    /// Planner conversation runtime state label.
    pub conversation_runtime_state: String,
    /// Provider status line (health + capabilities + model count).
    pub provider_status: String,
    /// True when the last turn invoked reasoning.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reasoning_used: bool,
    /// True when last content was partial.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// True when the last prompt was truncated for budget.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prompt_truncated: bool,
    /// Registered reasoning providers at assemble time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderHealthEntry>,
    /// Installed / discovered model count from the registry.
    #[serde(default)]
    pub installed_model_count: usize,
    /// Default model display id from the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

/// Inputs used to assemble a [`ReasoningDiagnosticsReport`].
#[derive(Debug, Clone, Default)]
pub struct ReasoningDiagnosticsInput {
    /// Live provider health.
    pub health: Option<ReasoningHealth>,
    /// Live provider capabilities.
    pub capabilities: Option<ReasoningCapabilities>,
    /// Live / preferred provider id.
    pub provider_id: Option<String>,
    /// Model registry snapshot.
    pub registry: Option<ModelRegistrySnapshot>,
    /// Last-turn reasoning metrics.
    pub metrics: Option<ReasoningMetrics>,
    /// Last-turn prompt diagnostics.
    pub prompt: Option<PromptDiagnostics>,
    /// Streaming lifecycle (live or last).
    pub streaming: Option<StreamingLifecycle>,
    /// Conversation runtime state label (Planner-owned).
    pub conversation_runtime_state: Option<String>,
    /// Whether the last Planner turn used reasoning.
    pub reasoning_used: bool,
    /// Configured model display id (registry default / preferred).
    pub configured_model: Option<String>,
    /// Model attached onto the provider request.
    pub provider_model: Option<String>,
    /// Loaded model reported by the provider backend.
    pub loaded_model: Option<String>,
}

impl ReasoningDiagnosticsReport {
    /// Assemble a full report from live + last-turn inputs.
    pub fn assemble(input: ReasoningDiagnosticsInput) -> Self {
        let health = input.health.unwrap_or(ReasoningHealth::Unavailable {
            reason: "no reasoning provider".into(),
        });
        let capabilities = input.capabilities.unwrap_or_default();
        let registry = input.registry.unwrap_or(ModelRegistrySnapshot {
            models: Vec::new(),
            default_model: None,
            providers: Vec::new(),
        });

        let metrics = input.metrics;
        let prompt = input.prompt;

        let reasoning_provider = metrics
            .as_ref()
            .and_then(|m| m.provider_id.clone())
            .or(input.provider_id.clone())
            .or_else(|| registry.providers.first().map(|p| p.provider_id.clone()));

        let actual_model = metrics
            .as_ref()
            .and_then(|m| m.model.as_ref().map(ModelIdentifier::display));
        let configured_model = input.configured_model.clone().or_else(|| {
            registry
                .default_model
                .as_ref()
                .map(ModelIdentifier::display)
        });
        let provider_model = input.provider_model.clone().or_else(|| actual_model.clone());
        let loaded_model = input.loaded_model.clone();

        let current_model = actual_model
            .clone()
            .or_else(|| provider_model.clone())
            .or_else(|| configured_model.clone())
            .or_else(|| {
                registry
                    .models
                    .iter()
                    .find(|model| model.is_default)
                    .map(|model| model.info.id.display())
            });

        let prompt_tokens = metrics
            .as_ref()
            .and_then(|m| m.input_tokens)
            .or_else(|| prompt.as_ref().map(|p| p.final_token_estimate))
            .or_else(|| prompt.as_ref().map(|p| p.prompt_size_tokens));

        let completion_tokens = metrics.as_ref().and_then(|m| m.output_tokens);

        let context_size = prompt
            .as_ref()
            .and_then(|p| p.budget.context_window_tokens)
            .or_else(|| {
                registry
                    .models
                    .iter()
                    .find(|model| {
                        current_model
                            .as_ref()
                            .map(|id| model.info.id.display() == *id)
                            .unwrap_or(model.is_default)
                    })
                    .and_then(|model| model.context_length())
            })
            .or_else(|| {
                registry
                    .models
                    .iter()
                    .find(|model| model.is_default)
                    .and_then(|model| model.context_length())
            });

        let latency_ms = metrics.as_ref().map(|m| m.latency_ms);
        let provider_latency_ms = metrics.as_ref().and_then(|m| m.provider_latency_ms);

        let streaming = input
            .streaming
            .map(|lifecycle| lifecycle.as_str().to_string())
            .unwrap_or_else(|| "idle".into());

        let cancellation = match metrics.as_ref() {
            Some(m) if m.cancelled => m
                .cancel_reason
                .map(CancelReason::as_str)
                .unwrap_or("cancelled")
                .to_string(),
            _ => "none".into(),
        };

        let prompt_budget = prompt.as_ref().map(|p| p.budget.clone());
        let prompt_sections = prompt
            .as_ref()
            .map(|p| p.sections.clone())
            .unwrap_or_default();
        let prompt_truncated = prompt.as_ref().map(|p| p.truncated).unwrap_or(false);
        let prompt_size_characters = prompt.as_ref().map(|p| p.prompt_size_characters);
        let final_token_estimate = prompt.as_ref().map(|p| p.final_token_estimate);
        let conversation_turns = prompt.as_ref().map(|p| p.conversation_turns);
        let truncated_sections = prompt
            .as_ref()
            .map(|p| {
                p.truncated_sections()
                    .into_iter()
                    .map(|id| id.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let excluded_sections = prompt
            .as_ref()
            .map(|p| {
                p.excluded_sections()
                    .into_iter()
                    .map(|id| id.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default();

        let provider_status = format_provider_status(
            &health,
            &capabilities,
            reasoning_provider.as_deref(),
            &registry.providers,
            registry.models.len(),
        );

        Self {
            reasoning_provider,
            current_model: current_model.clone(),
            configured_model,
            actual_model,
            provider_model,
            loaded_model,
            prompt_tokens,
            completion_tokens,
            context_size,
            latency_ms,
            provider_latency_ms,
            streaming,
            cancellation,
            reasoning_health: health.as_str().to_string(),
            prompt_budget,
            prompt_sections,
            prompt_size_characters,
            final_token_estimate,
            conversation_turns,
            truncated_sections,
            excluded_sections,
            conversation_runtime_state: input
                .conversation_runtime_state
                .unwrap_or_else(|| "idle".into()),
            provider_status,
            reasoning_used: input.reasoning_used,
            partial: metrics.as_ref().map(|m| m.partial).unwrap_or(false),
            prompt_truncated,
            providers: registry.providers,
            installed_model_count: registry.models.len(),
            default_model: registry
                .default_model
                .as_ref()
                .map(ModelIdentifier::display)
                .or(current_model),
        }
    }

    /// Compact one-line summary for the subsystem row.
    pub fn summary_line(&self) -> String {
        format!(
            "provider={} · model={} · health={} · state={} · stream={} · cancel={} · prompt_tok={} · completion_tok={} · context={} · latency_ms={}",
            opt_str(self.reasoning_provider.as_deref()),
            opt_str(self.current_model.as_deref()),
            self.reasoning_health,
            self.conversation_runtime_state,
            self.streaming,
            self.cancellation,
            opt_num(self.prompt_tokens),
            opt_num(self.completion_tokens),
            opt_num(self.context_size),
            opt_num(self.latency_ms),
        )
    }

    /// Every diagnostic value as `(label, value)` — no hidden fields.
    pub fn labeled_values(&self) -> Vec<(String, String)> {
        let mut rows = vec![
            (
                "Reasoning Provider".into(),
                opt_str(self.reasoning_provider.as_deref()),
            ),
            (
                "Current Model".into(),
                opt_str(self.current_model.as_deref()),
            ),
            (
                "Configured Model".into(),
                opt_str(self.configured_model.as_deref()),
            ),
            (
                "Actual Model".into(),
                opt_str(self.actual_model.as_deref()),
            ),
            (
                "Provider Model".into(),
                opt_str(self.provider_model.as_deref()),
            ),
            (
                "Loaded Model".into(),
                opt_str(self.loaded_model.as_deref()),
            ),
            ("Prompt Tokens".into(), opt_num(self.prompt_tokens)),
            (
                "Completion Tokens".into(),
                opt_num(self.completion_tokens),
            ),
            ("Context Size".into(), opt_num(self.context_size)),
            ("Latency".into(), {
                match (self.latency_ms, self.provider_latency_ms) {
                    (Some(wall), Some(provider)) => {
                        format!("{wall} ms (provider {provider} ms)")
                    }
                    (Some(wall), None) => format!("{wall} ms"),
                    (None, Some(provider)) => format!("provider {provider} ms"),
                    (None, None) => "-".into(),
                }
            }),
            ("Streaming".into(), self.streaming.clone()),
            ("Cancellation".into(), self.cancellation.clone()),
            ("Reasoning Health".into(), self.reasoning_health.clone()),
            (
                "Prompt Size".into(),
                self.prompt_size_characters
                    .map(|n| format!("{n} chars"))
                    .unwrap_or_else(|| "-".into()),
            ),
            ("Prompt Budget".into(), self.prompt_budget_label()),
            ("Prompt Sections".into(), self.prompt_sections_label()),
            (
                "Truncated Sections".into(),
                if self.truncated_sections.is_empty() {
                    "none".into()
                } else {
                    self.truncated_sections.join(", ")
                },
            ),
            (
                "Excluded Sections".into(),
                if self.excluded_sections.is_empty() {
                    "none".into()
                } else {
                    self.excluded_sections.join(", ")
                },
            ),
            (
                "Conversation Turns".into(),
                opt_num(self.conversation_turns),
            ),
            (
                "Final Token Estimate".into(),
                opt_num(self.final_token_estimate),
            ),
            (
                "Conversation Runtime State".into(),
                self.conversation_runtime_state.clone(),
            ),
            ("Provider Status".into(), self.provider_status.clone()),
        ];
        if self.reasoning_used {
            rows.push(("Reasoning Used".into(), "yes".into()));
        }
        if self.partial {
            rows.push(("Partial".into(), "yes".into()));
        }
        if self.prompt_truncated {
            rows.push(("Prompt Truncated".into(), "yes".into()));
        }
        rows.push((
            "Installed Models".into(),
            self.installed_model_count.to_string(),
        ));
        rows.push((
            "Default Model".into(),
            opt_str(self.default_model.as_deref()),
        ));
        rows
    }

    /// Prompt budget one-liner.
    pub fn prompt_budget_label(&self) -> String {
        match &self.prompt_budget {
            Some(budget) => {
                let efficiency = budget
                    .context_efficiency()
                    .map(|value| format!("{:.1}%", value * 100.0))
                    .unwrap_or_else(|| "-".into());
                format!(
                    "used={} tok · remaining={} · reserved={} · window={} · efficiency={} · truncated={}",
                    budget.estimated_tokens,
                    opt_num(budget.remaining_tokens),
                    budget.reserved_completion_tokens,
                    opt_num(budget.context_window_tokens),
                    efficiency,
                    budget.truncated,
                )
            }
            None => "-".into(),
        }
    }

    /// Prompt sections one-liner.
    pub fn prompt_sections_label(&self) -> String {
        if self.prompt_sections.is_empty() {
            return "-".into();
        }
        self.prompt_sections
            .iter()
            .map(|section| {
                format!(
                    "{}:{}ch/{}tok/{}",
                    section.id.as_str(),
                    section.characters,
                    section.estimated_tokens,
                    section.disposition.as_str()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Plain-text render for CLI / headless diagnostics.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Conversational Reasoning".to_string());
        lines.push(
            "Lifecycle: Idle → Preparing Context → Reasoning / Streaming → Completed | Cancelled | Failed"
                .to_string(),
        );
        lines.push(self.summary_line());
        lines.push(String::new());
        lines.push(format!("{:<28} {}", "Field", "Value"));
        lines.push("-".repeat(72));
        for (label, value) in self.labeled_values() {
            lines.push(format!("{label:<28} {value}"));
        }
        if !self.prompt_sections.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "{:<16} {:>8} {:>8} {:<8} {}",
                "Section", "Chars", "Tokens", "State", "Note"
            ));
            lines.push("-".repeat(72));
            for section in &self.prompt_sections {
                lines.push(format!(
                    "{:<16} {:>8} {:>8} {:<8} {}",
                    section.id.as_str(),
                    section.characters,
                    section.estimated_tokens,
                    section.disposition.as_str(),
                    section.note.as_deref().unwrap_or("-")
                ));
            }
        }
        lines.join("\n")
    }
}

fn format_provider_status(
    health: &ReasoningHealth,
    capabilities: &ReasoningCapabilities,
    provider_id: Option<&str>,
    providers: &[ProviderHealthEntry],
    model_count: usize,
) -> String {
    let caps = format!(
        "stream={} · cancel={} · list_models={} · multi_turn={} · assembled_prompt={}",
        capabilities.stream,
        capabilities.cancellation,
        capabilities.list_models,
        capabilities.multi_turn,
        capabilities.assembled_prompt
    );
    let detail = match health {
        ReasoningHealth::Ready => "ready".into(),
        ReasoningHealth::Degraded { reason } => format!("degraded ({reason})"),
        ReasoningHealth::Unavailable { reason } => format!("unavailable ({reason})"),
    };
    let id = provider_id.unwrap_or_else(|| {
        providers
            .first()
            .map(|entry| entry.provider_id.as_str())
            .unwrap_or("-")
    });
    format!("id={id} · {detail} · models={model_count} · {caps}")
}

fn opt_str(value: Option<&str>) -> String {
    value.unwrap_or("-").to_string()
}

fn opt_num(value: Option<u64>) -> String {
    value
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelIdentifier;
    use crate::prompt::{PromptBudgetUsage, PromptSectionContribution, PromptSectionId};
    use crate::registry::{ProviderHealthEntry, RegisteredModel};
    use crate::model::ReasoningModelInfo;

    fn sample_prompt() -> PromptDiagnostics {
        PromptDiagnostics {
            prompt_size_characters: 120,
            prompt_size_tokens: 30,
            final_token_estimate: 30,
            conversation_turns: 2,
            budget: PromptBudgetUsage {
                used_characters: 120,
                estimated_tokens: 30,
                max_characters: None,
                max_tokens: Some(7_000),
                remaining_characters: None,
                remaining_tokens: Some(6_970),
                reserved_completion_tokens: 1_024,
                context_window_tokens: Some(8_192),
                context_efficiency_bps: Some(42),
                truncated: false,
            },
            sections: vec![
                PromptSectionContribution {
                    id: PromptSectionId::SystemInstructions,
                    characters: 40,
                    estimated_tokens: 10,
                    included: true,
                    truncated: false,
                    disposition: crate::prompt::PromptSectionDisposition::Included,
                    note: None,
                    source_llm_sections: vec![],
                },
                PromptSectionContribution {
                    id: PromptSectionId::UserRequest,
                    characters: 80,
                    estimated_tokens: 20,
                    included: true,
                    truncated: false,
                    disposition: crate::prompt::PromptSectionDisposition::Included,
                    note: None,
                    source_llm_sections: vec!["user_request".into()],
                },
            ],
            llm_coverage: Vec::new(),
            truncated: false,
            truncation_notes: Vec::new(),
            template_id: None,
            formatter_id: None,
            adapter_id: None,
        }
    }

    #[test]
    fn every_diagnostic_value_is_present() {
        let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
            health: Some(ReasoningHealth::Ready),
            capabilities: Some(ReasoningCapabilities::full()),
            provider_id: Some("ollama".into()),
            registry: Some(ModelRegistrySnapshot {
                models: vec![RegisteredModel {
                    info: ReasoningModelInfo::new(
                        ModelIdentifier::new("ollama", "llama3.2"),
                        "llama3.2",
                    )
                    .with_context_tokens(131_072)
                    .with_parameter_count("3.2B")
                    .with_quantization("Q4_K_M"),
                    provider_id: "ollama".into(),
                    provider_health: ReasoningHealth::Ready,
                    is_default: true,
                    available: true,
                }],
                default_model: Some(ModelIdentifier::new("ollama", "llama3.2")),
                providers: vec![ProviderHealthEntry {
                    provider_id: "ollama".into(),
                    display_name: "Ollama".into(),
                    health: ReasoningHealth::Ready,
                    model_count: 1,
                }],
            }),
            metrics: Some(
                ReasoningMetrics::timed(42)
                    .with_tokens(Some(30), Some(12))
                    .with_provider_id("ollama")
                    .with_model(ModelIdentifier::new("ollama", "llama3.2"))
                    .with_provider_latency_ms(15),
            ),
            prompt: Some(sample_prompt()),
            streaming: Some(StreamingLifecycle::Completed),
            conversation_runtime_state: Some("completed".into()),
            reasoning_used: true,
            configured_model: Some("ollama/llama3.2".into()),
            provider_model: Some("ollama/llama3.2".into()),
            loaded_model: Some("llama3.2".into()),
        });

        let labels: Vec<_> = report
            .labeled_values()
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        for required in [
            "Reasoning Provider",
            "Current Model",
            "Configured Model",
            "Actual Model",
            "Provider Model",
            "Loaded Model",
            "Prompt Tokens",
            "Completion Tokens",
            "Context Size",
            "Latency",
            "Streaming",
            "Cancellation",
            "Reasoning Health",
            "Prompt Size",
            "Prompt Budget",
            "Prompt Sections",
            "Truncated Sections",
            "Excluded Sections",
            "Conversation Turns",
            "Final Token Estimate",
            "Conversation Runtime State",
            "Provider Status",
        ] {
            assert!(
                labels.iter().any(|label| label == required),
                "missing diagnostic label {required}"
            );
        }

        assert_eq!(report.reasoning_provider.as_deref(), Some("ollama"));
        assert_eq!(report.current_model.as_deref(), Some("ollama/llama3.2"));
        assert_eq!(report.configured_model.as_deref(), Some("ollama/llama3.2"));
        assert_eq!(report.actual_model.as_deref(), Some("ollama/llama3.2"));
        assert_eq!(report.provider_model.as_deref(), Some("ollama/llama3.2"));
        assert_eq!(report.loaded_model.as_deref(), Some("llama3.2"));
        assert_eq!(report.prompt_tokens, Some(30));
        assert_eq!(report.completion_tokens, Some(12));
        assert_eq!(report.context_size, Some(8_192));
        assert_eq!(report.latency_ms, Some(42));
        assert_eq!(report.streaming, "completed");
        assert_eq!(report.cancellation, "none");
        assert_eq!(report.reasoning_health, "ready");
        assert!(report.prompt_budget.is_some());
        assert_eq!(report.prompt_sections.len(), 2);
        assert_eq!(report.conversation_runtime_state, "completed");
        assert!(report.provider_status.contains("id=ollama"));

        let rendered = report.render();
        assert!(rendered.contains("Conversational Reasoning"));
        assert!(rendered.contains("Prompt Tokens"));
        assert!(rendered.contains("Configured Model"));
        assert!(rendered.contains("system_instructions"));
    }

    #[test]
    fn unavailable_provider_is_reported_honestly() {
        let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
            health: Some(ReasoningHealth::Unavailable {
                reason: "ollama unreachable".into(),
            }),
            capabilities: Some(ReasoningCapabilities::default()),
            provider_id: Some("ollama".into()),
            conversation_runtime_state: Some("idle".into()),
            ..ReasoningDiagnosticsInput::default()
        });
        assert_eq!(report.reasoning_health, "unavailable");
        assert!(report.provider_status.contains("unavailable"));
        assert_eq!(report.streaming, "idle");
        assert_eq!(report.cancellation, "none");
    }

    #[test]
    fn streaming_and_cancellation_surface() {
        let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
            health: Some(ReasoningHealth::Ready),
            provider_id: Some("ollama".into()),
            metrics: Some(
                ReasoningMetrics::timed(9)
                    .with_partial(true)
                    .with_cancel_reason(CancelReason::User),
            ),
            streaming: Some(StreamingLifecycle::Cancelled),
            conversation_runtime_state: Some("cancelled".into()),
            reasoning_used: true,
            ..ReasoningDiagnosticsInput::default()
        });
        assert_eq!(report.streaming, "cancelled");
        assert_eq!(report.cancellation, "user");
        assert!(report.partial);
    }

    #[test]
    fn prompt_inspection_exposes_budget_and_sections() {
        let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
            health: Some(ReasoningHealth::Ready),
            prompt: Some(sample_prompt()),
            conversation_runtime_state: Some("reasoning".into()),
            ..ReasoningDiagnosticsInput::default()
        });
        assert!(report.prompt_budget_label().contains("used=30"));
        assert!(report.prompt_sections_label().contains("system_instructions:"));
        assert_eq!(report.prompt_tokens, Some(30));
        assert_eq!(report.context_size, Some(8_192));
    }
}
