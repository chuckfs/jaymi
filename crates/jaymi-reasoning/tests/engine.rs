//! Sprint B1.4 — ReasoningEngine orchestration tests.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use jaymi_context::{
    ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
    UserRequestMetadataSection,
};
use jaymi_reasoning::{
    CancellationToken, FinishReason, GenerationParameters, ModelIdentifier, ReasoningCapabilities,
    ReasoningEngine, ReasoningEngineConfig, ReasoningError, ReasoningHealth, ReasoningMetrics,
    ReasoningModelInfo, ReasoningProvider, ReasoningRequest, ReasoningResponse, ReasoningResult,
    ReasoningStream, StreamingChunk, StreamingChunkKind,
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
                        environmental: None,
            })
        .build();
    LlmContext::from_bundle(&bundle)
}

#[derive(Clone)]
struct ScriptedProvider {
    id: String,
    health: Arc<Mutex<ReasoningHealth>>,
    complete_calls: Arc<AtomicU32>,
    stream_calls: Arc<AtomicU32>,
    fail_times: Arc<AtomicU32>,
    fail_remaining: Arc<AtomicU32>,
    delay_ms: u64,
    stream_tokens: Vec<String>,
    last_prompt_text: Arc<Mutex<Option<String>>>,
}

impl ScriptedProvider {
    fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            health: Arc::new(Mutex::new(ReasoningHealth::Ready)),
            complete_calls: Arc::new(AtomicU32::new(0)),
            stream_calls: Arc::new(AtomicU32::new(0)),
            fail_times: Arc::new(AtomicU32::new(0)),
            fail_remaining: Arc::new(AtomicU32::new(0)),
            delay_ms: 0,
            stream_tokens: vec!["ok".into()],
            last_prompt_text: Arc::new(Mutex::new(None)),
        }
    }

    fn with_failures(mut self, n: u32) -> Self {
        self.fail_remaining = Arc::new(AtomicU32::new(n));
        self
    }

    fn with_delay_ms(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    fn with_health(self, health: ReasoningHealth) -> Self {
        *self.health.lock().expect("lock") = health;
        self
    }

    fn with_tokens(mut self, tokens: Vec<&str>) -> Self {
        self.stream_tokens = tokens.into_iter().map(str::to_string).collect();
        self
    }
}

struct ScriptedStream {
    tokens: Vec<String>,
    index: usize,
    cancelled: bool,
    provider_id: String,
}

impl ReasoningStream for ScriptedStream {
    fn next_chunk(&mut self) -> ReasoningResult<Option<StreamingChunk>> {
        if self.cancelled {
            return Ok(Some(StreamingChunk::cancelled(self.index as u64)));
        }
        if self.index < self.tokens.len() {
            let text = self.tokens[self.index].clone();
            let chunk = StreamingChunk::token(self.index as u64, text);
            self.index += 1;
            return Ok(Some(chunk));
        }
        if self.index == self.tokens.len() {
            let chunk = StreamingChunk::completed(
                self.index as u64,
                ReasoningMetrics::timed(1)
                    .with_provider_id(self.provider_id.clone())
                    .with_tokens(Some(1), Some(self.tokens.len() as u64)),
            );
            self.index += 1;
            return Ok(Some(chunk));
        }
        Ok(None)
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }
}

impl ReasoningProvider for ScriptedProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ReasoningCapabilities {
        ReasoningCapabilities::full()
    }

    fn health(&self) -> ReasoningHealth {
        self.health.lock().expect("lock").clone()
    }

    fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
        Ok(vec![ReasoningModelInfo::new(
            ModelIdentifier::new(&self.id, "default"),
            "default",
        )
        .with_context_tokens(8_192)
        .with_max_output_tokens(1_024)])
    }

    fn complete(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        *self.last_prompt_text.lock().expect("lock") =
            request.prompt.as_ref().map(|prompt| prompt.text.clone());
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        if self.delay_ms > 0 {
            thread::sleep(Duration::from_millis(self.delay_ms));
        }
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        if self.fail_remaining.load(Ordering::SeqCst) > 0 {
            self.fail_remaining.fetch_sub(1, Ordering::SeqCst);
            self.fail_times.fetch_add(1, Ordering::SeqCst);
            return Err(ReasoningError::GenerationFailed {
                reason: "transient".into(),
            });
        }
        Ok(ReasoningResponse::completed(format!("{}:{}", self.id, request.goal))
            .with_metrics(ReasoningMetrics::timed(1).with_tokens(Some(2), Some(2))))
    }

    fn stream(&self, request: ReasoningRequest) -> ReasoningResult<Box<dyn ReasoningStream>> {
        self.stream_calls.fetch_add(1, Ordering::SeqCst);
        *self.last_prompt_text.lock().expect("lock") =
            request.prompt.as_ref().map(|prompt| prompt.text.clone());
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        if self.fail_remaining.load(Ordering::SeqCst) > 0 {
            self.fail_remaining.fetch_sub(1, Ordering::SeqCst);
            return Err(ReasoningError::StreamFailed {
                reason: "transient stream".into(),
            });
        }
        Ok(Box::new(ScriptedStream {
            tokens: self.stream_tokens.clone(),
            index: 0,
            cancelled: false,
            provider_id: self.id.clone(),
        }))
    }
}

#[test]
fn provider_selection_prefers_request_model_provider() {
    let a = Arc::new(ScriptedProvider::new("alpha"));
    let b = Arc::new(ScriptedProvider::new("beta"));
    let engine = ReasoningEngine::new()
        .with_additional_provider(a)
        .with_additional_provider(b)
        .with_config(ReasoningEngineConfig::minimal().with_preferred_provider("alpha"));

    let request = ReasoningRequest::new("hi", sample_context())
        .with_model(ModelIdentifier::new("beta", "m"));
    let selected = engine.select_provider(&request).unwrap();
    assert_eq!(selected.id(), "beta");

    let response = engine.complete(request).unwrap();
    assert_eq!(response.content, "beta:hi");
    assert_eq!(response.metrics.provider_id.as_deref(), Some("beta"));
}

#[test]
fn provider_selection_uses_preferred_then_ready() {
    let down = Arc::new(ScriptedProvider::new("down").with_health(ReasoningHealth::Unavailable {
        reason: "offline".into(),
    }));
    let ready = Arc::new(ScriptedProvider::new("ready"));
    let engine = ReasoningEngine::new()
        .with_additional_provider(down)
        .with_additional_provider(ready.clone())
        .with_config(ReasoningEngineConfig::minimal().with_preferred_provider("down"));

    // Preferred is down → fall through to first Ready.
    let selected = engine
        .select_provider(&ReasoningRequest::new("x", sample_context()))
        .unwrap();
    assert_eq!(selected.id(), "ready");
}

#[test]
fn streaming_collects_into_reasoning_response() {
    let provider = Arc::new(ScriptedProvider::new("echo").with_tokens(vec!["hel", "lo"]));
    let engine = ReasoningEngine::with_provider(provider)
        .with_config(ReasoningEngineConfig::minimal());
    let mut stream = engine
        .stream(ReasoningRequest::new("hi", sample_context()))
        .unwrap();
    assert_eq!(stream.provider_id(), "echo");
    let first = stream.next_chunk().unwrap().unwrap();
    assert_eq!(first.kind, StreamingChunkKind::Token);
    let response = stream.collect().unwrap();
    assert_eq!(response.content, "hello");
    assert_eq!(response.finish_reason, FinishReason::Completed);
    assert_eq!(response.metrics.provider_id.as_deref(), Some("echo"));
    assert_eq!(response.metrics.attempts, 1);
    assert!(response.notes.iter().any(|n| n.starts_with("prompt_chars=")));
}

#[test]
fn cancellation_aborts_complete_and_stream() {
    let provider = Arc::new(ScriptedProvider::new("echo"));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());

    let token = CancellationToken::new();
    token.cancel();
    let err = engine
        .complete(ReasoningRequest::new("x", sample_context()).with_cancellation(token))
        .unwrap_err();
    assert_eq!(err, ReasoningError::Cancelled);

    let mut stream = engine
        .stream(ReasoningRequest::new("x", sample_context()))
        .unwrap();
    stream.cancel();
    let chunk = stream.next_chunk().unwrap().unwrap();
    assert_eq!(chunk.kind, StreamingChunkKind::Cancelled);
}

#[test]
fn failures_surface_when_not_retryable_or_exhausted() {
    let provider = Arc::new(ScriptedProvider::new("echo").with_failures(5));
    let engine = ReasoningEngine::with_provider(provider).with_config(
        ReasoningEngineConfig::minimal().with_max_retries(1), // 2 attempts total
    );
    let err = engine
        .complete(ReasoningRequest::new("x", sample_context()))
        .unwrap_err();
    assert!(matches!(err, ReasoningError::GenerationFailed { .. }));
}

#[test]
fn metrics_include_provider_attempts_and_prompt() {
    let provider = Arc::new(ScriptedProvider::new("metrics"));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let response = engine
        .complete(ReasoningRequest::new("goal", sample_context()))
        .unwrap();
    assert_eq!(response.metrics.provider_id.as_deref(), Some("metrics"));
    assert_eq!(response.metrics.attempts, 1);
    assert!(response.metrics.latency_ms < 5_000);
    assert!(response.notes.iter().any(|n| n.contains("prompt_chars=")));
}

#[test]
fn timeouts_abort_slow_complete() {
    let provider = Arc::new(ScriptedProvider::new("slow").with_delay_ms(200));
    let engine = ReasoningEngine::with_provider(provider).with_config(
        ReasoningEngineConfig::minimal().with_default_timeout_ms(30),
    );
    let err = engine
        .complete(ReasoningRequest::new("x", sample_context()))
        .unwrap_err();
    assert!(
        matches!(err, ReasoningError::TimedOut { .. }),
        "expected TimedOut, got {err:?}"
    );
}

#[test]
fn request_timeout_overrides_config() {
    let provider = Arc::new(ScriptedProvider::new("slow").with_delay_ms(150));
    let engine = ReasoningEngine::with_provider(provider).with_config(
        ReasoningEngineConfig::minimal().with_default_timeout_ms(5_000),
    );
    let request = ReasoningRequest::new("x", sample_context())
        .with_parameters(GenerationParameters::new().with_timeout_ms(40));
    let err = engine.complete(request).unwrap_err();
    assert!(matches!(err, ReasoningError::TimedOut { .. }));
}

#[test]
fn retry_succeeds_after_transient_failure() {
    let provider = Arc::new(ScriptedProvider::new("flaky").with_failures(1));
    let engine = ReasoningEngine::with_provider(provider.clone()).with_config(
        ReasoningEngineConfig::minimal()
            .with_max_retries(2)
            .with_default_timeout_ms(5_000),
    );
    let response = engine
        .complete(ReasoningRequest::new("hi", sample_context()))
        .unwrap();
    assert_eq!(response.content, "flaky:hi");
    assert_eq!(response.metrics.attempts, 2);
    assert_eq!(provider.complete_calls.load(Ordering::SeqCst), 2);
    assert!(response.notes.iter().any(|n| n == "attempts=2"));
}

#[test]
fn empty_engine_is_stub() {
    let engine = ReasoningEngine::new();
    assert!(!engine.is_implemented());
    assert_eq!(engine.status_label(), "stub");
    assert!(matches!(
        engine.complete(ReasoningRequest::new("x", sample_context())),
        Err(ReasoningError::NotImplemented)
    ));
}

#[test]
fn reason_uses_streaming_pipeline() {
    let provider = Arc::new(ScriptedProvider::new("pipe").with_tokens(vec!["a", "b"]));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let response = engine
        .reason(ReasoningRequest::new("x", sample_context()))
        .unwrap();
    assert_eq!(response.content, "ab");
}

#[test]
fn build_prompt_adapts_budget_to_model_limits() {
    let provider = Arc::new(ScriptedProvider::new("budget"));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let request = ReasoningRequest::new("goal", sample_context()).with_parameters(
        GenerationParameters::new().with_max_output_tokens(2_048),
    );
    let budget = engine.prompt_budget_for_request(&request);
    assert_eq!(budget.context_window_tokens, Some(8_192));
    assert_eq!(budget.reserved_completion_tokens, 2_048);
    assert_eq!(budget.max_tokens, Some(8_192 - 2_048));
    let prompt = engine.build_prompt(&request);
    assert_eq!(
        prompt.diagnostics.budget.context_window_tokens,
        Some(8_192)
    );
    assert_eq!(prompt.diagnostics.budget.reserved_completion_tokens, 2_048);
}

#[test]
fn blocking_path_delivers_prompt_builder_output_to_provider() {
    let provider = Arc::new(ScriptedProvider::new("echo"));
    let engine =
        ReasoningEngine::with_provider(provider.clone()).with_config(ReasoningEngineConfig::minimal());
    let request = ReasoningRequest::new("deliver-complete", sample_context());
    let expected = engine.build_prompt(&request);
    let _ = engine.complete(request).unwrap();
    let seen = provider
        .last_prompt_text
        .lock()
        .expect("lock")
        .clone()
        .expect("prompt attached");
    assert_eq!(seen, expected.text);
    assert!(seen.contains("deliver-complete"));
}

#[test]
fn streaming_path_delivers_prompt_builder_output_to_provider() {
    let provider = Arc::new(ScriptedProvider::new("echo").with_tokens(vec!["a"]));
    let engine =
        ReasoningEngine::with_provider(provider.clone()).with_config(ReasoningEngineConfig::minimal());
    let request = ReasoningRequest::new("deliver-stream", sample_context());
    let expected = engine.build_prompt(&request);
    let stream = engine.stream(request).unwrap();
    let _ = stream.collect().unwrap();
    let seen = provider
        .last_prompt_text
        .lock()
        .expect("lock")
        .clone()
        .expect("prompt attached");
    assert_eq!(seen, expected.text);
    assert!(seen.contains("deliver-stream"));
}
