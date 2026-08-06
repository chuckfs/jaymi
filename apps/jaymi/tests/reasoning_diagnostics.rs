//! Conversational Reasoning diagnostics (Sprint B1.10).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::{Application, OperationalStatus};
use jaymi_reasoning::{
    CancelReason, ModelIdentifier, ReasoningDiagnosticsInput, ReasoningDiagnosticsReport,
    ReasoningHealth, ReasoningMetrics, StreamingLifecycle,
};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("jaymi-b110-{label}-{nanos}"));
    std::fs::create_dir_all(&path).expect("temp dir");
    path
}

#[test]
fn diagnostics_expose_every_conversational_reasoning_field() {
    let app = Application::boot_with_data_dir(temp_dir("fields")).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let report = snapshot
        .reasoning_inspector
        .as_ref()
        .expect("reasoning inspector");

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
            "missing {required}"
        );
    }

    let row = snapshot.subsystem("Reasoning Status").expect("row");
    assert_ne!(row.status, OperationalStatus::Stub);
    assert!(row.detail.contains("health="));
    assert!(row.detail.contains("state="));

    let dashboard = snapshot.render_dashboard();
    assert!(dashboard.contains("Conversational Reasoning"));
    assert!(dashboard.contains("Reasoning Provider"));
    assert!(dashboard.contains("Conversation Runtime State"));
}

#[test]
fn provider_unavailable_is_reported_on_health_and_status() {
    let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
        health: Some(ReasoningHealth::Unavailable {
            reason: "ollama unreachable".into(),
        }),
        provider_id: Some("ollama".into()),
        conversation_runtime_state: Some("idle".into()),
        ..ReasoningDiagnosticsInput::default()
    });
    assert_eq!(report.reasoning_health, "unavailable");
    assert!(report.provider_status.contains("unavailable"));
    assert_eq!(report.cancellation, "none");
    assert_eq!(report.streaming, "idle");

    let app = Application::boot_with_data_dir(temp_dir("unavailable")).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let live = snapshot.reasoning_inspector.expect("inspector");
    // Local Ollama may or may not be running; either way health is explicit.
    assert!(
        live.reasoning_health == "ready"
            || live.reasoning_health == "degraded"
            || live.reasoning_health == "unavailable"
    );
    assert!(!live.provider_status.is_empty());
}

#[test]
fn streaming_and_cancellation_appear_in_report() {
    let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
        health: Some(ReasoningHealth::Ready),
        provider_id: Some("ollama".into()),
        metrics: Some(
            ReasoningMetrics::timed(18)
                .with_tokens(Some(11), Some(4))
                .with_model(ModelIdentifier::new("ollama", "llama3.2"))
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
    assert_eq!(report.prompt_tokens, Some(11));
    assert_eq!(report.completion_tokens, Some(4));
    assert!(report.partial);
    assert!(report.render().contains("Cancellation"));
}

#[test]
fn prompt_inspection_surfaces_budget_and_sections() {
    use jaymi_reasoning::{
        PromptBudgetUsage, PromptDiagnostics, PromptSectionContribution, PromptSectionId,
    };

    let prompt = PromptDiagnostics {
        prompt_size_characters: 200,
        prompt_size_tokens: 50,
        final_token_estimate: 50,
        conversation_turns: 0,
        budget: PromptBudgetUsage {
            used_characters: 200,
            estimated_tokens: 50,
            max_characters: None,
            max_tokens: Some(7_168),
            remaining_characters: None,
            remaining_tokens: Some(7_118),
            reserved_completion_tokens: 1_024,
            context_window_tokens: Some(8_192),
            context_efficiency_bps: Some(69),
            truncated: false,
        },
        sections: vec![PromptSectionContribution {
            id: PromptSectionId::UserRequest,
            characters: 200,
            estimated_tokens: 50,
            included: true,
            truncated: false,
            disposition: jaymi_reasoning::PromptSectionDisposition::Included,
            note: None,
            source_llm_sections: vec!["user_request".into()],
        }],
        llm_coverage: Vec::new(),
        truncated: false,
        truncation_notes: Vec::new(),
        template_id: None,
        formatter_id: None,
        adapter_id: None,
    };

    let report = ReasoningDiagnosticsReport::assemble(ReasoningDiagnosticsInput {
        health: Some(ReasoningHealth::Ready),
        prompt: Some(prompt),
        conversation_runtime_state: Some("reasoning".into()),
        reasoning_used: true,
        ..ReasoningDiagnosticsInput::default()
    });
    assert_eq!(report.prompt_tokens, Some(50));
    assert_eq!(report.context_size, Some(8_192));
    assert!(report.prompt_budget_label().contains("used=50"));
    assert!(report.prompt_sections_label().contains("user_request:"));
    assert!(report.render().contains("user_request"));
}

#[test]
fn health_reporting_matches_subsystem_row() {
    let app = Application::boot_with_data_dir(temp_dir("health")).expect("boot");
    let snapshot = app.diagnostics().expect("diagnostics");
    let report = snapshot
        .reasoning_inspector
        .as_ref()
        .expect("inspector");
    let row = snapshot.subsystem("Reasoning Status").expect("row");
    match report.reasoning_health.as_str() {
        "ready" | "degraded" => {
            assert_eq!(row.status, OperationalStatus::Operational);
        }
        "unavailable" => {
            assert_eq!(row.status, OperationalStatus::Disabled);
        }
        other => panic!("unexpected health {other}"),
    }
}
