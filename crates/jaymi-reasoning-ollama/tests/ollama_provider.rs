//! Sprint B1.3 / B1.13.1 — OllamaReasoningProvider tests (mock transport).

use std::sync::Arc;

use jaymi_context::{
    ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
    UserRequestMetadataSection,
};
use jaymi_reasoning::{
    CancellationToken, ModelIdentifier, PromptBuilder, ReasoningEngine, ReasoningEngineConfig,
    ReasoningError, ReasoningHealth, ReasoningProvider, ReasoningRequest, StreamingChunkKind,
};
use jaymi_reasoning_ollama::{
    MockOllamaTransport, OllamaReasoningProvider, StreamingStatus, OLLAMA_PROVIDER_ID,
};

fn sample_context() -> LlmContext {
    let bundle = ContextBundleBuilder::new()
        .user_request(UserRequestMetadataSection {
            content_preview: "hi".into(),
            ..UserRequestMetadataSection::default()
        })
        .planner_metadata(PlannerMetadataSection {
            assemble_generation: 1,
            sources: vec![ContextSource::UserRequest],
            notes: vec![],
            budget: None,
            policy: None,
                        environmental: None,
            })
        .build();
    LlmContext::from_bundle(&bundle)
}

fn request_with_prompt(goal: &str) -> ReasoningRequest {
    let request = ReasoningRequest::new(goal, sample_context());
    let prompt = PromptBuilder::new()
        .with_system_instructions("Constitutional system line.")
        .build_from_request(&request);
    request.with_prompt(prompt)
}

fn mock_with_model(name: &str) -> (Arc<MockOllamaTransport>, OllamaReasoningProvider) {
    let transport = Arc::new(MockOllamaTransport::connected("0.5.1"));
    transport.set_tags_json(format!(
        r#"{{"models":[{{"name":"{name}","size":1,"details":{{"family":"llama"}}}}]}}"#
    ));
    transport.set_ps_json(format!(r#"{{"models":[{{"name":"{name}"}}]}}"#));
    transport.set_chat_response(format!(
        r#"{{"model":"{name}","message":{{"role":"assistant","content":"pong"}},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":1}}"#
    ));
    transport.set_chat_stream_lines(vec![
        format!(
            r#"{{"model":"{name}","message":{{"role":"assistant","content":"hel"}},"done":false}}"#
        ),
        format!(
            r#"{{"model":"{name}","message":{{"role":"assistant","content":"lo"}},"done":false}}"#
        ),
        format!(
            r#"{{"model":"{name}","message":{{"role":"assistant","content":""}},"done":true,"done_reason":"stop","prompt_eval_count":2,"eval_count":2}}"#
        ),
    ]);
    let provider =
        OllamaReasoningProvider::with_transport(transport.clone(), Some(name.into()));
    (transport, provider)
}

#[test]
fn health_check_reports_ready_when_connected() {
    let (_transport, provider) = mock_with_model("llama3.2");
    assert_eq!(provider.health(), ReasoningHealth::Ready);
    let diag = provider.diagnostics();
    assert!(diag.connected);
    assert_eq!(diag.provider_version.as_deref(), Some("0.5.1"));
    assert_eq!(diag.installed_models, vec!["llama3.2".to_string()]);
    assert_eq!(diag.loaded_model.as_deref(), Some("llama3.2"));
    assert!(diag.latency_ms.is_some());
}

#[test]
fn provider_unavailable_surfaces_health_and_errors() {
    let transport = Arc::new(MockOllamaTransport::unavailable());
    let provider = OllamaReasoningProvider::with_transport(transport, None);
    match provider.health() {
        ReasoningHealth::Unavailable { reason } => {
            assert!(reason.contains("unreachable") || reason.contains("offline"));
        }
        other => panic!("expected unavailable, got {other:?}"),
    }
    let diag = provider.diagnostics_cached();
    assert!(!diag.connected);
    let err = provider.complete(request_with_prompt("hi")).unwrap_err();
    assert!(matches!(
        err,
        ReasoningError::Unavailable { .. }
            | ReasoningError::ModelNotFound { .. }
            | ReasoningError::StreamFailed { .. }
    ));
}

#[test]
fn list_models_returns_metadata() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let models = provider.list_models().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id.provider, OLLAMA_PROVIDER_ID);
    assert_eq!(models[0].id.name, "llama3.2");
    assert!(models[0].supports_streaming);
    assert!(models[0].local);
    assert_eq!(models[0].family.as_deref(), Some("llama"));
    assert_eq!(models[0].context_tokens, Some(131_072));
    let limits = provider.model_limits(Some(&models[0].id)).unwrap();
    assert_eq!(limits.context_tokens, Some(131_072));
}

#[test]
fn complete_returns_assistant_content() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let response = provider.complete(request_with_prompt("ping")).unwrap();
    assert_eq!(response.content, "pong");
    assert_eq!(
        response.model.as_ref().map(|m| m.display()),
        Some("ollama/llama3.2".into())
    );
    assert_eq!(response.metrics.input_tokens, Some(3));
}

#[test]
fn complete_sends_prompt_builder_output_to_model() {
    let (transport, provider) = mock_with_model("llama3.2");
    let request = request_with_prompt("ping");
    let expected = request.require_prompt().unwrap().clone();
    let _ = provider.complete(request).unwrap();
    let body = transport.last_chat_body().expect("chat body recorded");
    let messages = body
        .get("messages")
        .and_then(|value| value.as_array())
        .expect("messages array");
    let blob: String = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(blob.contains("Constitutional system line."));
    assert!(blob.contains("ping"));
    assert!(expected.text.contains("Constitutional system line."));
}

#[test]
fn provider_inspection_matches_delivered_prompt_diagnostics() {
    let (transport, provider) = mock_with_model("llama3.2");
    let request = request_with_prompt("provider-inspect");
    let expected_chars = request
        .require_prompt()
        .unwrap()
        .diagnostics
        .prompt_size_characters;
    let expected_tokens = request
        .require_prompt()
        .unwrap()
        .diagnostics
        .final_token_estimate;
    let _ = provider.complete(request).unwrap();
    let body = transport.last_chat_body().expect("chat body");
    let messages = body
        .get("messages")
        .and_then(|value| value.as_array())
        .expect("messages");
    let wire_chars: usize = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(|value| value.as_str()))
        .map(|content| content.chars().count())
        .sum();
    assert_eq!(wire_chars, expected_chars);
    assert!(expected_tokens > 0);
}

#[test]
fn streaming_inspection_matches_attached_prompt_diagnostics() {
    let (transport, provider) = mock_with_model("llama3.2");
    let engine = ReasoningEngine::with_provider(Arc::new(provider))
        .with_config(ReasoningEngineConfig::minimal());
    let request = ReasoningRequest::new("stream-inspect", sample_context()).with_history(vec![
        jaymi_reasoning::ConversationTurn::user("prior"),
    ]);
    let stream = engine.stream(request).unwrap();
    let diagnostics = stream.prompt_diagnostics().clone();
    assert_eq!(diagnostics.conversation_turns, 1);
    assert_eq!(
        diagnostics.prompt_size_characters,
        stream.prompt_characters()
    );
    let _ = stream.collect().unwrap();
    let body = transport.last_stream_body().expect("stream body");
    let messages = body
        .get("messages")
        .and_then(|value| value.as_array())
        .expect("messages");
    let wire_chars: usize = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(|value| value.as_str()))
        .map(|content| content.chars().count())
        .sum();
    assert_eq!(wire_chars, diagnostics.prompt_size_characters);
    assert_eq!(
        diagnostics.final_token_estimate,
        diagnostics.prompt_size_tokens
    );
}

#[test]
fn rejects_request_without_assembled_prompt() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let err = provider
        .complete(ReasoningRequest::new("hi", sample_context()))
        .unwrap_err();
    assert!(matches!(err, ReasoningError::InvalidRequest { .. }));
}

#[test]
fn model_unavailable_maps_to_model_not_found() {
    let (transport, provider) = mock_with_model("llama3.2");
    transport.fail_chat(404, "model \"missing\" not found");
    let request = request_with_prompt("x")
        .with_model(ModelIdentifier::new(OLLAMA_PROVIDER_ID, "missing"));
    let err = provider.complete(request).unwrap_err();
    assert!(matches!(err, ReasoningError::ModelNotFound { .. }));
}

#[test]
fn streaming_emits_tokens_then_completed() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let mut stream = provider.stream(request_with_prompt("hi")).unwrap();
    let first = stream.next_chunk().unwrap().unwrap();
    assert_eq!(first.kind, StreamingChunkKind::Token);
    assert_eq!(first.text.as_deref(), Some("hel"));
    let second = stream.next_chunk().unwrap().unwrap();
    assert_eq!(second.text.as_deref(), Some("lo"));
    let done = stream.next_chunk().unwrap().unwrap();
    assert_eq!(done.kind, StreamingChunkKind::Completed);
    assert!(done.is_terminal());
    assert_eq!(
        provider.diagnostics_cached().streaming_status,
        StreamingStatus::Completed
    );
}

#[test]
fn streaming_path_sends_prompt_builder_output() {
    let (transport, provider) = mock_with_model("llama3.2");
    let engine = ReasoningEngine::with_provider(Arc::new(provider))
        .with_config(ReasoningEngineConfig::minimal());
    let stream = engine
        .stream(ReasoningRequest::new("stream-goal", sample_context()))
        .unwrap();
    let _ = stream.collect().unwrap();
    let body = transport.last_stream_body().expect("stream body recorded");
    let messages = body
        .get("messages")
        .and_then(|value| value.as_array())
        .expect("messages");
    let blob: String = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(blob.contains("stream-goal"));
}

#[test]
fn blocking_path_attaches_prompt_via_engine() {
    let (transport, provider) = mock_with_model("llama3.2");
    let engine = ReasoningEngine::with_provider(Arc::new(provider))
        .with_config(ReasoningEngineConfig::minimal());
    let _ = engine
        .complete(ReasoningRequest::new("block-goal", sample_context()))
        .unwrap();
    let body = transport.last_chat_body().expect("chat body");
    let blob = body.to_string();
    assert!(blob.contains("block-goal"));
}

#[test]
fn stream_cancel_yields_cancelled_terminal() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let mut stream = provider.stream(request_with_prompt("hi")).unwrap();
    let _ = stream.next_chunk().unwrap().unwrap();
    stream.cancel();
    let chunk = stream.next_chunk().unwrap().unwrap();
    assert_eq!(chunk.kind, StreamingChunkKind::Cancelled);
    assert_eq!(
        provider.diagnostics_cached().streaming_status,
        StreamingStatus::Cancelled
    );
}

#[test]
fn request_cancellation_token_aborts_complete() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let token = CancellationToken::new();
    token.cancel();
    let err = provider
        .complete(request_with_prompt("hi").with_cancellation(token))
        .unwrap_err();
    assert_eq!(err, ReasoningError::Cancelled);
}

#[test]
fn malformed_stream_event_fails() {
    let transport = Arc::new(MockOllamaTransport::connected("0.5.1"));
    transport.set_tags_json(r#"{"models":[{"name":"m"}]}"#);
    transport.set_ps_json(r#"{"models":[]}"#);
    transport.set_chat_stream_lines(vec!["{not-json".into()]);
    let provider = OllamaReasoningProvider::with_transport(transport, Some("m".into()));
    let mut stream = provider.stream(request_with_prompt("hi")).unwrap();
    let err = stream.next_chunk().unwrap_err();
    assert!(matches!(err, ReasoningError::StreamFailed { .. }));
}

#[test]
fn diagnostics_summary_includes_required_fields() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let _ = provider.complete(request_with_prompt("ping"));
    let line = provider.diagnostics().summary_line();
    assert!(line.contains("connected=true"));
    assert!(line.contains("version=0.5.1"));
    assert!(line.contains("models=1"));
    assert!(line.contains("streaming="));
}

#[test]
fn capabilities_require_assembled_prompt() {
    let (_transport, provider) = mock_with_model("llama3.2");
    let caps = provider.capabilities();
    assert!(caps.assembled_prompt);
    assert!(caps.structured_context);
}
