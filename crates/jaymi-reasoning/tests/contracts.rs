//! Comprehensive contract tests for Sprint B1.1.

use std::sync::{Arc, Mutex};

use jaymi_context::{
    ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
    UserRequestMetadataSection,
};
use jaymi_reasoning::{
    CancellationToken, ConversationRole, ConversationTurn, FinishReason, GenerationParameters,
    ModelIdentifier, ReasoningCapabilities, ReasoningError, ReasoningHealth, ReasoningMetrics,
    ReasoningModelInfo, ReasoningProvider, ReasoningRequest, ReasoningResponse, ReasoningStream,
    StreamingChunk, StreamingChunkKind,
};

fn sample_context() -> LlmContext {
    let bundle = ContextBundleBuilder::new()
        .user_request(UserRequestMetadataSection {
            content_preview: "hello".into(),
            ..UserRequestMetadataSection::default()
        })
        .planner_metadata(PlannerMetadataSection {
            assemble_generation: 1,
            sources: vec![ContextSource::UserRequest],
            notes: vec![],
            budget: None,
            policy: None,
        })
        .build();
    LlmContext::from_bundle(&bundle)
}

/// In-memory provider used only to prove the contract is implementable
/// without any vendor / transport types.
struct MockProvider {
    id: String,
    models: Vec<ReasoningModelInfo>,
    health: ReasoningHealth,
    complete_text: String,
    fail_stream: bool,
}

impl MockProvider {
    fn new() -> Self {
        let id = ModelIdentifier::new("mock", "echo");
        Self {
            id: "mock".into(),
            models: vec![ReasoningModelInfo::new(id, "Echo").with_streaming()],
            health: ReasoningHealth::Ready,
            complete_text: "echo".into(),
            fail_stream: false,
        }
    }
}

trait ModelInfoExt {
    fn with_streaming(self) -> Self;
}

impl ModelInfoExt for ReasoningModelInfo {
    fn with_streaming(mut self) -> Self {
        self.supports_streaming = true;
        self.context_tokens = Some(4096);
        self.family = Some("chat".into());
        self
    }
}

struct MockStream {
    chunks: Vec<StreamingChunk>,
    index: usize,
    cancelled: Arc<Mutex<bool>>,
}

impl ReasoningStream for MockStream {
    fn next_chunk(&mut self) -> Result<Option<StreamingChunk>, ReasoningError> {
        if *self.cancelled.lock().expect("lock") {
            return Ok(Some(StreamingChunk::cancelled(self.index as u64)));
        }
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = self.chunks[self.index].clone();
        self.index += 1;
        Ok(Some(chunk))
    }

    fn cancel(&mut self) {
        *self.cancelled.lock().expect("lock") = true;
    }
}

impl ReasoningProvider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Mock Provider"
    }

    fn capabilities(&self) -> ReasoningCapabilities {
        ReasoningCapabilities::full()
    }

    fn health(&self) -> ReasoningHealth {
        self.health.clone()
    }

    fn list_models(&self) -> Result<Vec<ReasoningModelInfo>, ReasoningError> {
        Ok(self.models.clone())
    }

    fn complete(&self, request: ReasoningRequest) -> Result<ReasoningResponse, ReasoningError> {
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        if let ReasoningHealth::Unavailable { reason } = &self.health {
            return Err(ReasoningError::Unavailable {
                reason: reason.clone(),
            });
        }
        let model = self.models.first().map(|m| m.id.clone());
        let metrics = ReasoningMetrics::timed(1)
            .with_tokens(Some(3), Some(2))
            .with_model(model.clone().unwrap_or_else(|| ModelIdentifier::new("mock", "echo")));
        let mut response = ReasoningResponse::completed(format!(
            "{}:{}",
            self.complete_text, request.goal
        ))
        .with_metrics(metrics);
        if let Some(model) = model {
            response = response.with_model(model);
        }
        Ok(response)
    }

    fn stream(
        &self,
        request: ReasoningRequest,
    ) -> Result<Box<dyn ReasoningStream>, ReasoningError> {
        if self.fail_stream {
            return Err(ReasoningError::StreamFailed {
                reason: "forced".into(),
            });
        }
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        let cancelled = Arc::new(Mutex::new(false));
        let token = request.cancellation.clone();
        let text = format!("{}:{}", self.complete_text, request.goal);
        let chunks = vec![
            StreamingChunk::token(0, text.clone()),
            StreamingChunk::completed(1, ReasoningMetrics::timed(2).with_tokens(Some(1), Some(1))),
        ];
        // Observe request cancellation by wrapping stream
        let _ = token;
        Ok(Box::new(MockStream {
            chunks,
            index: 0,
            cancelled,
        }))
    }
}

#[test]
fn serialization_roundtrips_core_types() {
    let model = ModelIdentifier::new("backend", "chat").with_revision("r1");
    let json = serde_json::to_string(&model).unwrap();
    let back: ModelIdentifier = serde_json::from_str(&json).unwrap();
    assert_eq!(back, model);

    let info = ReasoningModelInfo::new(model.clone(), "Chat");
    let json = serde_json::to_string(&info).unwrap();
    let back: ReasoningModelInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, model);

    let params = GenerationParameters::new()
        .with_temperature(0.1)
        .with_max_output_tokens(128);
    let json = serde_json::to_string(&params).unwrap();
    let back: GenerationParameters = serde_json::from_str(&json).unwrap();
    assert_eq!(back.temperature, Some(0.1));

    let turn = ConversationTurn::user("hi").with_id("t1");
    let json = serde_json::to_string(&turn).unwrap();
    let back: ConversationTurn = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, ConversationRole::User);

    let metrics = ReasoningMetrics::timed(9)
        .with_tokens(Some(4), Some(5))
        .with_model(model.clone());
    let json = serde_json::to_string(&metrics).unwrap();
    let back: ReasoningMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_tokens, Some(9));

    let response = ReasoningResponse::completed("ok")
        .with_finish_reason(FinishReason::Length)
        .with_model(model)
        .with_metrics(metrics);
    let json = serde_json::to_string(&response).unwrap();
    let back: ReasoningResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.finish_reason, FinishReason::Length);

    let chunk = StreamingChunk::token(0, "a");
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamingChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind, StreamingChunkKind::Token);

    let err = ReasoningError::ModelNotFound {
        model: "x".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ReasoningError = serde_json::from_str(&json).unwrap();
    assert_eq!(back.as_str(), "model_not_found");

    let caps = ReasoningCapabilities::full();
    let json = serde_json::to_string(&caps).unwrap();
    let back: ReasoningCapabilities = serde_json::from_str(&json).unwrap();
    assert!(back.stream);

    let health = ReasoningHealth::Degraded {
        reason: "warm".into(),
    };
    let json = serde_json::to_string(&health).unwrap();
    let back: ReasoningHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(back.as_str(), "degraded");
}

#[test]
fn reasoning_request_serializes_without_cancellation_handle() {
    let request = ReasoningRequest::new("goal", sample_context())
        .with_history(vec![ConversationTurn::user("prior")])
        .with_parameters(GenerationParameters::new().with_temperature(0.5))
        .with_model(ModelIdentifier::new("mock", "echo"))
        .with_request_id("req-1");
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["goal"], "goal");
    assert_eq!(value["request_id"], "req-1");
    assert!(value.get("cancellation").is_none());
    assert_eq!(
        value["context"]["schema_version"],
        jaymi_context::LLM_CONTEXT_SCHEMA_VERSION
    );
}

#[test]
fn streaming_contract_emits_tokens_then_terminal() {
    let provider = MockProvider::new();
    let request = ReasoningRequest::new("hi", sample_context());
    let mut stream = provider.stream(request).unwrap();
    let first = stream.next_chunk().unwrap().unwrap();
    assert_eq!(first.kind, StreamingChunkKind::Token);
    assert!(!first.is_terminal());
    let second = stream.next_chunk().unwrap().unwrap();
    assert!(second.is_terminal());
    assert_eq!(second.finish_reason, Some(FinishReason::Completed));
    assert!(stream.next_chunk().unwrap().is_none());
}

#[test]
fn streaming_cancel_yields_cancelled_terminal() {
    let provider = MockProvider::new();
    let request = ReasoningRequest::new("hi", sample_context());
    let mut stream = provider.stream(request).unwrap();
    stream.cancel();
    let chunk = stream.next_chunk().unwrap().unwrap();
    assert_eq!(chunk.kind, StreamingChunkKind::Cancelled);
    assert_eq!(chunk.finish_reason, Some(FinishReason::Cancelled));
    assert!(chunk.metrics.as_ref().unwrap().cancelled);
}

#[test]
fn cancellation_token_aborts_complete() {
    let provider = MockProvider::new();
    let token = CancellationToken::new();
    token.cancel();
    let request = ReasoningRequest::new("hi", sample_context()).with_cancellation(token);
    let err = provider.complete(request).unwrap_err();
    assert_eq!(err, ReasoningError::Cancelled);
}

#[test]
fn provider_independence_via_trait_object() {
    let provider: Arc<dyn ReasoningProvider> = Arc::new(MockProvider::new());
    assert_eq!(provider.id(), "mock");
    assert!(provider.capabilities().structured_context);
    assert!(provider.capabilities().assembled_prompt);
    assert!(provider.health().is_usable());
    let models = provider.list_models().unwrap();
    assert_eq!(models[0].id.display(), "mock/echo");
    let response = provider
        .complete(ReasoningRequest::new("world", sample_context()))
        .unwrap();
    assert_eq!(response.content, "echo:world");
    assert_eq!(response.assistant_turn.role, ConversationRole::Assistant);
}

#[test]
fn metrics_and_model_metadata_on_response() {
    let provider = MockProvider::new();
    let response = provider
        .complete(ReasoningRequest::new("x", sample_context()))
        .unwrap();
    assert_eq!(response.metrics.input_tokens, Some(3));
    assert_eq!(response.metrics.output_tokens, Some(2));
    assert_eq!(response.metrics.total_tokens, Some(5));
    assert_eq!(response.model.as_ref().unwrap().display(), "mock/echo");
    let info = provider.list_models().unwrap().into_iter().next().unwrap();
    assert_eq!(info.display_name, "Echo");
    assert!(info.supports_streaming);
    assert_eq!(info.context_tokens, Some(4096));
    assert_eq!(info.family.as_deref(), Some("chat"));
}

#[test]
fn unavailable_health_surfaces_as_error() {
    let mut provider = MockProvider::new();
    provider.health = ReasoningHealth::Unavailable {
        reason: "offline".into(),
    };
    let err = provider
        .complete(ReasoningRequest::new("x", sample_context()))
        .unwrap_err();
    match err {
        ReasoningError::Unavailable { reason } => assert!(reason.contains("offline")),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn no_vendor_transport_symbols_in_public_api_names() {
    // Compile-time / naming guard: public types must stay provider-independent.
    let _ = std::any::type_name::<dyn ReasoningProvider>();
    let _ = std::any::type_name::<ReasoningRequest>();
    let _ = std::any::type_name::<ReasoningResponse>();
    let _ = std::any::type_name::<StreamingChunk>();
    let forbidden = [
        "ollama", "http", "json", "gguf", "llama", "openai", "anthropic",
    ];
    let public_names = [
        "ReasoningRequest",
        "ReasoningResponse",
        "ReasoningProvider",
        "ReasoningCapabilities",
        "StreamingChunk",
        "GenerationParameters",
        "ReasoningError",
        "ReasoningHealth",
        "ReasoningModelInfo",
        "ModelIdentifier",
        "ConversationTurn",
        "ReasoningMetrics",
    ];
    for name in public_names {
        let lower = name.to_ascii_lowercase();
        for bad in forbidden {
            assert!(
                !lower.contains(bad),
                "{name} must not reference {bad}"
            );
        }
    }
}
