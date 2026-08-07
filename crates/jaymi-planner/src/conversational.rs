//! Shared conversational generation helpers (Sprint B1.13.8).
//!
//! Streaming (pumpable) and blocking (observer collect) are **two delivery
//! modes** over one generation pipeline (`ConversationStream`). This module
//! holds the shared mapping / assemble pieces without collapsing the modes.
//!
//! ```text
//! Shared: assemble + build_reasoning_request + ConversationStream + terminal map
//! Blocking: run_with_observer (one-shot collect)
//! Pumpable: start → pump → complete_conversation_stream
//! ```

use jaymi_context::{AssembleHints, ContextBundle};
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_reasoning::{
    CancelReason, ConversationStreamEvent, ReasoningMetrics, ReasoningResponse, StreamingLifecycle,
};

use crate::complexity::ComplexityAssessment;
use crate::conversation_state::ConversationState;
use crate::decision::Intent;
use crate::PlannerResponse;

/// Outcome of a terminal conversational generation (shared by both delivery modes).
#[derive(Debug, Clone)]
pub struct ConversationalTerminal {
    /// User-visible reply text.
    pub content: String,
    /// Generation sub-lifecycle at terminal.
    pub lifecycle: StreamingLifecycle,
    /// Provider metrics when known.
    pub metrics: Option<ReasoningMetrics>,
    /// Provider id when known.
    pub provider_id: Option<String>,
}

impl ConversationalTerminal {
    /// Map generation lifecycle onto Planner conversation runtime state.
    pub fn conversation_state(&self) -> ConversationState {
        conversation_state_for_lifecycle(self.lifecycle)
    }
}

/// Map a finished [`ReasoningResponse`] onto a streaming lifecycle.
pub fn lifecycle_from_reasoning_response(response: &ReasoningResponse) -> StreamingLifecycle {
    if response.metrics.cancelled {
        StreamingLifecycle::Cancelled
    } else if response.metrics.partial
        || matches!(response.finish_reason, jaymi_reasoning::FinishReason::Error)
    {
        StreamingLifecycle::Failed
    } else {
        StreamingLifecycle::Completed
    }
}

/// Map generation lifecycle onto Planner conversation runtime state.
pub fn conversation_state_for_lifecycle(lifecycle: StreamingLifecycle) -> ConversationState {
    match lifecycle {
        StreamingLifecycle::Cancelled => ConversationState::Cancelled,
        StreamingLifecycle::Failed => ConversationState::Failed,
        _ => ConversationState::Completed,
    }
}

/// Normalize empty terminal content into stable user-facing copy.
pub fn normalize_terminal_content(content: String, lifecycle: StreamingLifecycle) -> String {
    if !content.trim().is_empty() {
        return content;
    }
    match lifecycle {
        StreamingLifecycle::Failed => {
            "I couldn't finish reasoning about that (generation failed)".to_string()
        }
        StreamingLifecycle::Cancelled => "Generation cancelled.".to_string(),
        _ => content,
    }
}

/// Lift a terminal [`ConversationStreamEvent`] into a shared outcome.
pub fn conversational_terminal_from_event(
    event: ConversationStreamEvent,
) -> JaymiResult<ConversationalTerminal> {
    match event {
        ConversationStreamEvent::Completed(response) => {
            Ok(conversational_terminal_from_response(response))
        }
        ConversationStreamEvent::Cancelled {
            partial,
            reason,
            metrics,
        } => {
            let content = if partial.trim().is_empty() {
                format!("Generation cancelled ({})", reason.as_str())
            } else {
                partial
            };
            Ok(ConversationalTerminal {
                content,
                lifecycle: StreamingLifecycle::Cancelled,
                metrics: Some(metrics),
                provider_id: None,
            })
        }
        ConversationStreamEvent::Failed {
            partial,
            error,
            metrics,
        } => {
            let content = if partial.trim().is_empty() {
                format!(
                    "I couldn't finish reasoning about that ({})",
                    error.message()
                )
            } else {
                partial
            };
            Ok(ConversationalTerminal {
                content,
                lifecycle: StreamingLifecycle::Failed,
                metrics: Some(metrics),
                provider_id: None,
            })
        }
        other => Err(JaymiError::new(format!(
            "conversational terminal expects a terminal event, got {:?}",
            std::mem::discriminant(&other)
        ))),
    }
}

/// Lift a collected [`ReasoningResponse`] into a shared outcome.
pub fn conversational_terminal_from_response(
    response: ReasoningResponse,
) -> ConversationalTerminal {
    let lifecycle = lifecycle_from_reasoning_response(&response);
    let content = if response.content.trim().is_empty()
        && matches!(lifecycle, StreamingLifecycle::Failed)
    {
        format!(
            "I couldn't finish reasoning about that ({})",
            response
                .notes
                .first()
                .cloned()
                .unwrap_or_else(|| "generation failed".into())
        )
    } else if response.content.trim().is_empty()
        && matches!(lifecycle, StreamingLifecycle::Cancelled)
    {
        "Generation cancelled.".to_string()
    } else {
        response.content
    };
    ConversationalTerminal {
        content,
        lifecycle,
        provider_id: response.metrics.provider_id.clone(),
        metrics: Some(response.metrics),
    }
}

/// Soft copy when no reasoning backend is wired (blocking path only).
pub fn no_backend_soft_content() -> &'static str {
    "I'd like to help with that in conversation, but no reasoning backend is available right now. You can still list directories, read files, search, or open a project."
}

/// Soft PlannerResponse when no reasoning backend is wired (blocking / observer).
///
/// Pumpable `start_conversation_stream` returns a hard error instead — the host
/// then bridges to the observer path. That split is intentional (see docs).
pub fn no_backend_soft_response() -> PlannerResponse {
    PlannerResponse {
        content: no_backend_soft_content().to_string(),
        reasoning_used: false,
        stream_lifecycle: Some(StreamingLifecycle::Idle),
        ..PlannerResponse::default()
    }
}

/// Build the conversational fields of a [`PlannerResponse`] from a terminal outcome.
pub fn planner_response_from_terminal(
    terminal: ConversationalTerminal,
    prompt_diagnostics: Option<jaymi_reasoning::PromptDiagnostics>,
    configured_model: Option<jaymi_reasoning::ModelIdentifier>,
    provider_model: Option<jaymi_reasoning::ModelIdentifier>,
    model_fallback: bool,
) -> PlannerResponse {
    let content = normalize_terminal_content(terminal.content, terminal.lifecycle);
    let mut pipeline_timing = terminal
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.pipeline.clone())
        .unwrap_or_default();
    if let Some(prompt) = prompt_diagnostics.as_ref() {
        if let Some(ms) = prompt.build_duration_ms {
            if !pipeline_timing
                .stages
                .iter()
                .any(|stage| stage.stage == "prompt_builder")
            {
                pipeline_timing.set_stage("prompt_builder", ms);
            }
        }
    }
    if let Some(metrics) = terminal.metrics.as_ref() {
        if pipeline_timing.ttft_ms.is_none() {
            pipeline_timing.ttft_ms = metrics.ttft_ms;
        }
        if pipeline_timing.total_generation_ms.is_none() {
            pipeline_timing.total_generation_ms = metrics.generation_duration_ms;
        }
    }
    PlannerResponse {
        content,
        reasoning_used: true,
        reasoning_provider_id: terminal.provider_id,
        stream_lifecycle: Some(terminal.lifecycle),
        reasoning_metrics: terminal.metrics,
        prompt_diagnostics,
        configured_model,
        provider_model,
        model_fallback,
        pipeline_timing: if pipeline_timing.is_empty() {
            None
        } else {
            Some(pipeline_timing)
        },
        ..PlannerResponse::default()
    }
}

/// Failed stream-start soft response (blocking observer path).
pub fn stream_start_failed_response(
    message: &str,
    configured_model: Option<jaymi_reasoning::ModelIdentifier>,
    provider_model: Option<jaymi_reasoning::ModelIdentifier>,
    model_fallback: bool,
) -> PlannerResponse {
    PlannerResponse {
        content: format!("I couldn't finish reasoning about that ({message})"),
        reasoning_used: true,
        stream_lifecycle: Some(StreamingLifecycle::Failed),
        configured_model,
        provider_model,
        model_fallback,
        ..PlannerResponse::default()
    }
}

/// Observer collect hard-fail response.
pub fn stream_collect_failed_response(
    message: &str,
    prompt_diagnostics: Option<jaymi_reasoning::PromptDiagnostics>,
    configured_model: Option<jaymi_reasoning::ModelIdentifier>,
    provider_model: Option<jaymi_reasoning::ModelIdentifier>,
    model_fallback: bool,
) -> PlannerResponse {
    PlannerResponse {
        content: format!("I couldn't finish reasoning about that ({message})"),
        reasoning_used: true,
        stream_lifecycle: Some(StreamingLifecycle::Failed),
        reasoning_metrics: Some(
            ReasoningMetrics::default()
                .with_partial(true)
                .with_cancel_reason(CancelReason::Error),
        ),
        prompt_diagnostics,
        configured_model,
        provider_model,
        model_fallback,
        ..PlannerResponse::default()
    }
}

/// Assemble hints for a conversational / unknown request.
///
/// Complexity and environmental resolution are Planner-authored and never
/// change Intent or capability ids.
pub fn conversational_assemble_hints(
    intent: &Intent,
    capability_ids: Vec<String>,
    complexity: Option<&ComplexityAssessment>,
) -> AssembleHints {
    let mut hints = AssembleHints {
        intent: intent.id(),
        capability_ids,
        complexity: None,
        environmental: None,
        understanding: None,
        review: None,
        coding_plan: None,
    };
    if let Some(assessment) = complexity {
        hints.complexity = Some(assessment.class_id().to_string());
    }
    hints
}

/// Bundle returned after conversational Context assemble prelude.
pub struct ConversationalAssemble {
    /// Resolved intent.
    pub intent: Intent,
    /// Capability list (empty ⇒ conversational).
    pub capability_ids: Vec<String>,
    /// Assembled context for the request.
    pub context: ContextBundle,
    /// Deterministic conversational complexity (Planner-owned).
    pub complexity: ComplexityAssessment,
    /// Environmental resolution (Planner-owned; unused when no deixis).
    pub environmental: crate::environmental::EnvironmentalResolution,
    /// Coding Understanding assessment (Sprint C1.1; unused when not understanding).
    pub understanding: Option<crate::coding_understanding::UnderstandingAssessment>,
    /// Coding Review assessment (Sprint C1.3; unused when not reviewing).
    pub review: Option<crate::coding_review::ReviewAssessment>,
    /// Coding Plan assessment (Sprint C1.4; unused when not generation-planning).
    pub coding_plan: Option<crate::coding_plan::CodingPlanAssessment>,
    /// Lightweight stage timings for Planner + Context (diagnostics only).
    pub pipeline: jaymi_reasoning::PipelineTiming,
}

/// Collect context-assembly + per-provider contribute timings from the last inspection.
pub fn pipeline_from_context_inspection(
    inspection: &jaymi_context::ContextInspectorReport,
) -> jaymi_reasoning::PipelineTiming {
    let mut timing = jaymi_reasoning::PipelineTiming::new();
    let detail = if inspection.cache_hit {
        "cache_hit"
    } else {
        "cache_miss"
    };
    timing.push(
        jaymi_reasoning::PipelineStageTiming::new("context_assembly", inspection.duration_ms)
            .with_detail(detail),
    );
    for provider in &inspection.providers {
        if let Some(ms) = provider.duration_ms {
            timing.push_provider(&provider.id, ms);
        }
    }
    timing
}

/// True when the intent requires tool-backed capability dispatch (not chat).
pub fn is_tool_backed(capability_ids: &[String]) -> bool {
    !capability_ids.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_reasoning::{FinishReason, ReasoningMetrics};

    #[test]
    fn lifecycle_mapping_matches_metrics() {
        let mut response = ReasoningResponse::completed("ok");
        assert_eq!(
            lifecycle_from_reasoning_response(&response),
            StreamingLifecycle::Completed
        );
        response.metrics.cancelled = true;
        assert_eq!(
            lifecycle_from_reasoning_response(&response),
            StreamingLifecycle::Cancelled
        );
        response.metrics.cancelled = false;
        response.metrics.partial = true;
        assert_eq!(
            lifecycle_from_reasoning_response(&response),
            StreamingLifecycle::Failed
        );
        response.metrics.partial = false;
        response = response.with_finish_reason(FinishReason::Error);
        assert_eq!(
            lifecycle_from_reasoning_response(&response),
            StreamingLifecycle::Failed
        );
    }

    #[test]
    fn terminal_from_event_and_response_agree_on_completed() {
        let response = ReasoningResponse::completed("hello").with_metrics(
            ReasoningMetrics::timed(3).with_provider_id("mock"),
        );
        let from_response = conversational_terminal_from_response(response.clone());
        let from_event =
            conversational_terminal_from_event(ConversationStreamEvent::Completed(response))
                .unwrap();
        assert_eq!(from_response.content, from_event.content);
        assert_eq!(from_response.lifecycle, from_event.lifecycle);
        assert_eq!(from_response.provider_id, from_event.provider_id);
        assert_eq!(from_response.conversation_state(), ConversationState::Completed);
    }

    #[test]
    fn normalize_fills_empty_cancel_and_fail() {
        assert!(normalize_terminal_content(String::new(), StreamingLifecycle::Cancelled)
            .contains("cancelled"));
        assert!(normalize_terminal_content(String::new(), StreamingLifecycle::Failed)
            .contains("couldn't finish"));
        assert_eq!(
            normalize_terminal_content("kept".into(), StreamingLifecycle::Completed),
            "kept"
        );
    }
}
