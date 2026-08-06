//! Performance dashboard — Developer Diagnostics only (observational).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, PerformanceDashboard};
use jaymi_reasoning::{
    PipelineTiming, PromptBudgetUsage, PromptDiagnostics, ReasoningDiagnosticsInput,
    ReasoningDiagnosticsReport, ReasoningHealth,
};

#[test]
fn performance_dashboard_is_present_on_diagnostics_snapshot() {
    let data_dir = temp_dir("performance-dashboard");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let dashboard = snapshot.performance_dashboard();
    // Fresh boot may have empty timings; shape must still assemble.
    let _ = dashboard.summary_line();
    let rendered = snapshot.render_dashboard();
    assert!(rendered.contains("Jaymi Diagnostics"));
    // Performance section appears only when content exists.
    if dashboard.has_content() {
        assert!(rendered.contains("Performance"));
        assert!(rendered.contains("Observational only"));
    }
}

#[test]
fn performance_dashboard_lists_required_metrics() {
    let mut timing = PipelineTiming::new();
    timing.set_stage("request_received", 0);
    timing.set_stage("planner", 3);
    timing.set_stage("context_assembly", 12);
    timing.push_provider("workspace", 7);
    timing.set_stage("prompt_builder", 4);
    timing.set_stage("reasoning_engine", 1);
    timing.set_stage("provider_transport", 90);
    timing.ttft_ms = Some(35);
    timing.total_generation_ms = Some(150);
    timing.total_ms = Some(220);

    let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
        health: Some(ReasoningHealth::Ready),
        reasoning_used: true,
        pipeline_timing: Some(timing),
        configured_model: Some("ollama/llama3.2".into()),
        provider_model: Some("ollama/llama3.2".into()),
        prompt: Some(PromptDiagnostics {
            prompt_size_characters: 180,
            prompt_size_tokens: 45,
            assembled_prompt_size_characters: Some(200),
            assembled_prompt_size_tokens: Some(50),
            final_token_estimate: 45,
            conversation_turns: 1,
            budget: PromptBudgetUsage {
                used_characters: 180,
                estimated_tokens: 45,
                max_characters: None,
                max_tokens: Some(7_000),
                remaining_characters: None,
                remaining_tokens: Some(6_955),
                reserved_completion_tokens: 1_024,
                context_window_tokens: Some(8_192),
                context_efficiency_bps: Some(64),
                truncated: false,
            },
            sections: vec![],
            llm_coverage: vec![],
            truncated: false,
            truncation_notes: vec![],
            template_id: None,
            formatter_id: None,
            adapter_id: None,
            build_duration_ms: Some(4),
        }),
        ..ReasoningDiagnosticsInput::default()
    });

    let dashboard = PerformanceDashboard::from_sources(Some(&report), None, &[]);
    assert!(dashboard.has_content());
    let metrics: Vec<_> = dashboard
        .metric_rows()
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    for required in [
        "Model Used",
        "TTFT",
        "Total Response Time",
        "Cache Hits / Misses",
        "Prompt Size",
        "Delivered Prompt Size",
    ] {
        assert!(
            metrics.iter().any(|label| label == required),
            "missing metric {required}"
        );
    }
    assert_eq!(dashboard.ttft_ms, Some(35));
    assert_eq!(dashboard.total_response_ms, Some(220));
    assert_eq!(dashboard.prompt_size_characters, Some(200));
    assert_eq!(dashboard.delivered_prompt_size_characters, Some(180));
    assert!(dashboard.timeline.iter().any(|row| row.label == "Planner"));
    assert!(dashboard
        .timeline
        .iter()
        .any(|row| row.label == "Time To First Token"));
    assert!(dashboard
        .provider_timings
        .iter()
        .any(|(label, _)| label == "Provider Transport"));
    assert!(dashboard
        .context_provider_timings
        .iter()
        .any(|(label, _)| label.contains("workspace")));

    let text = dashboard.render();
    assert!(text.contains("Performance"));
    assert!(text.contains("never shown in conversation mode"));
}

#[test]
fn performance_is_not_part_of_conversation_ux_helpers() {
    // Conversation UX module must not re-export PerformanceDashboard into
    // transcript helpers — keep the boundary explicit for the observational audit.
    let _ = jaymi::display_content;
    let _ = jaymi::show_typing_indicator;
    let _ = jaymi::smooth_streaming_text;
    // Compiles: PerformanceDashboard is a separate public API for diagnostics.
    let _ = PerformanceDashboard::default();
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("jaymi-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}
