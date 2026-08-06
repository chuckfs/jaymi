//! Sprint B1.6 — ConversationStream lifecycle, cancel, retry, reconnect, large responses.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use jaymi_context::{
    ContextBundleBuilder, ContextSource, LlmContext, PlannerMetadataSection,
    UserRequestMetadataSection,
};
use jaymi_reasoning::{
    CancelReason, ConversationStream, ConversationStreamEvent, FinishReason, ModelIdentifier,
    ReasoningCapabilities, ReasoningEngine, ReasoningEngineConfig, ReasoningError, ReasoningHealth,
    ReasoningMetrics, ReasoningModelInfo, ReasoningProvider, ReasoningRequest, ReasoningResponse,
    ReasoningResult, ReasoningStream, StreamingChunk, StreamingLifecycle,
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

#[derive(Clone)]
struct ScriptedProvider {
    id: String,
    health: Arc<Mutex<ReasoningHealth>>,
    stream_calls: Arc<AtomicU32>,
    fail_remaining: Arc<AtomicU32>,
    disconnect_after: Arc<Mutex<Option<usize>>>,
    stream_tokens: Arc<Mutex<Vec<String>>>,
    thoughts: Arc<Mutex<Vec<String>>>,
}

impl ScriptedProvider {
    fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            health: Arc::new(Mutex::new(ReasoningHealth::Ready)),
            stream_calls: Arc::new(AtomicU32::new(0)),
            fail_remaining: Arc::new(AtomicU32::new(0)),
            disconnect_after: Arc::new(Mutex::new(None)),
            stream_tokens: Arc::new(Mutex::new(vec!["ok".into()])),
            thoughts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_tokens(self, tokens: Vec<&str>) -> Self {
        *self.stream_tokens.lock().expect("lock") =
            tokens.into_iter().map(str::to_string).collect();
        self
    }

    fn with_thoughts(self, thoughts: Vec<&str>) -> Self {
        *self.thoughts.lock().expect("lock") =
            thoughts.into_iter().map(str::to_string).collect();
        self
    }

    fn with_start_failures(self, n: u32) -> Self {
        self.fail_remaining.store(n, Ordering::SeqCst);
        self
    }

    fn with_disconnect_after(self, n: usize) -> Self {
        *self.disconnect_after.lock().expect("lock") = Some(n);
        self
    }
}

struct ScriptedStream {
    tokens: Vec<String>,
    thoughts: Vec<String>,
    thought_index: usize,
    index: usize,
    cancelled: bool,
    provider_id: String,
    disconnect_after: Option<usize>,
}

impl ReasoningStream for ScriptedStream {
    fn next_chunk(&mut self) -> ReasoningResult<Option<StreamingChunk>> {
        if self.cancelled {
            return Ok(Some(StreamingChunk::cancelled(self.index as u64)));
        }
        if self.thought_index < self.thoughts.len() {
            let text = self.thoughts[self.thought_index].clone();
            self.thought_index += 1;
            let chunk = StreamingChunk::thought(self.index as u64, text);
            self.index += 1;
            return Ok(Some(chunk));
        }
        if let Some(limit) = self.disconnect_after {
            if self.index >= limit {
                return Err(ReasoningError::StreamFailed {
                    reason: "provider disconnect".into(),
                });
            }
        }
        let token_index = self.index.saturating_sub(self.thoughts.len());
        if token_index < self.tokens.len() {
            let text = self.tokens[token_index].clone();
            let chunk = StreamingChunk::token(self.index as u64, text);
            self.index += 1;
            return Ok(Some(chunk));
        }
        if token_index == self.tokens.len() {
            let chunk = StreamingChunk::completed(
                self.index as u64,
                ReasoningMetrics::timed(5)
                    .with_provider_id(self.provider_id.clone())
                    .with_tokens(Some(2), Some(self.tokens.len() as u64)),
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
        )])
    }

    fn complete(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
        Ok(ReasoningResponse::completed(format!(
            "{}:{}",
            self.id, request.goal
        )))
    }

    fn stream(&self, request: ReasoningRequest) -> ReasoningResult<Box<dyn ReasoningStream>> {
        self.stream_calls.fetch_add(1, Ordering::SeqCst);
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        if self.fail_remaining.load(Ordering::SeqCst) > 0 {
            self.fail_remaining.fetch_sub(1, Ordering::SeqCst);
            return Err(ReasoningError::StreamFailed {
                reason: "transient disconnect".into(),
            });
        }
        Ok(Box::new(ScriptedStream {
            tokens: self.stream_tokens.lock().expect("lock").clone(),
            thoughts: self.thoughts.lock().expect("lock").clone(),
            thought_index: 0,
            index: 0,
            cancelled: false,
            provider_id: self.id.clone(),
            disconnect_after: *self.disconnect_after.lock().expect("lock"),
        }))
    }
}

#[test]
fn streaming_lifecycle_thinking_then_streaming_then_completed() {
    let provider = Arc::new(
        ScriptedProvider::new("echo")
            .with_thoughts(vec!["plan"])
            .with_tokens(vec!["hel", "lo"]),
    );
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let mut stream =
        ConversationStream::start(engine, ReasoningRequest::new("hi", sample_context())).unwrap();

    let mut events = Vec::new();
    while let Some(event) = stream.pump().unwrap() {
        events.push(event);
        if events.last().map(ConversationStreamEvent::is_terminal).unwrap_or(false) {
            break;
        }
    }

    let lifecycles: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ConversationStreamEvent::Lifecycle(lifecycle) => Some(*lifecycle),
            _ => None,
        })
        .collect();
    assert!(lifecycles.contains(&StreamingLifecycle::Thinking));
    assert!(lifecycles.contains(&StreamingLifecycle::Streaming));
    assert!(matches!(
        events.last(),
        Some(ConversationStreamEvent::Completed(_))
    ));
    assert_eq!(stream.accumulated_text(), "hello");
}

#[test]
fn cancellation_preserves_partial_and_reason() {
    let provider = Arc::new(ScriptedProvider::new("echo").with_tokens(vec!["a", "b", "c", "d"]));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let mut stream =
        ConversationStream::start(engine, ReasoningRequest::new("hi", sample_context())).unwrap();

    // Advance into streaming tokens.
    loop {
        let event = stream.pump().unwrap().expect("event");
        if matches!(event, ConversationStreamEvent::Token(_)) {
            break;
        }
    }
    stream.cancel_with_reason(CancelReason::User);
    let mut saw_cancelled = false;
    while let Some(event) = stream.pump().unwrap() {
        if let ConversationStreamEvent::Cancelled {
            partial,
            reason,
            metrics,
        } = event
        {
            assert_eq!(reason, CancelReason::User);
            assert!(!partial.is_empty());
            assert!(metrics.cancelled);
            assert_eq!(metrics.cancel_reason, Some(CancelReason::User));
            assert!(metrics.partial);
            saw_cancelled = true;
            break;
        }
    }
    assert!(saw_cancelled);
}

#[test]
fn reconnect_retry_after_provider_disconnect() {
    let provider = Arc::new(
        ScriptedProvider::new("flaky")
            .with_tokens(vec!["one", "two"])
            .with_disconnect_after(1),
    );
    let engine = ReasoningEngine::with_provider(provider.clone()).with_config(
        ReasoningEngineConfig::minimal().with_max_retries(0),
    );
    let mut stream =
        ConversationStream::start(engine, ReasoningRequest::new("hi", sample_context())).unwrap();

    let mut failed = false;
    while let Some(event) = stream.pump().unwrap() {
        if let ConversationStreamEvent::Failed {
            partial, error, ..
        } = event
        {
            assert!(!partial.is_empty() || matches!(error, ReasoningError::StreamFailed { .. }));
            assert!(matches!(error, ReasoningError::StreamFailed { .. }));
            failed = true;
            break;
        }
        if event.is_terminal() {
            break;
        }
    }
    assert!(failed);

    // Heal disconnect for reconnect attempt.
    *provider.disconnect_after.lock().expect("lock") = None;
    stream.retry(true).unwrap();
    let response = stream.collect().unwrap();
    assert!(response.content.contains("one"));
    assert!(provider.stream_calls.load(Ordering::SeqCst) >= 2);
}

#[test]
fn error_handling_surfaces_failed_event() {
    let provider = Arc::new(ScriptedProvider::new("down").with_start_failures(3));
    let engine = ReasoningEngine::with_provider(provider).with_config(
        ReasoningEngineConfig::minimal().with_max_retries(0),
    );
    let err = ConversationStream::start(engine, ReasoningRequest::new("hi", sample_context()));
    assert!(matches!(err, Err(ReasoningError::StreamFailed { .. })));
}

#[test]
fn retry_recovers_from_start_failures() {
    let provider = Arc::new(
        ScriptedProvider::new("flaky")
            .with_tokens(vec!["ok"])
            .with_start_failures(1),
    );
    let engine = ReasoningEngine::with_provider(provider.clone()).with_config(
        ReasoningEngineConfig::minimal().with_max_retries(2),
    );
    let response = ConversationStream::start(
        engine,
        ReasoningRequest::new("hi", sample_context()),
    )
    .unwrap()
    .collect()
    .unwrap();
    assert_eq!(response.content, "ok");
    assert!(provider.stream_calls.load(Ordering::SeqCst) >= 2);
    assert!(response.metrics.attempts >= 2);
}

#[test]
fn large_responses_accumulate_all_tokens() {
    let tokens: Vec<String> = (0..500).map(|i| format!("t{i} ")).collect();
    let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let provider = Arc::new(ScriptedProvider::new("big").with_tokens(token_refs));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let mut observed = String::new();
    let response = ConversationStream::start(
        engine,
        ReasoningRequest::new("large", sample_context()),
    )
    .unwrap()
    .run_with_observer(|event| {
        if let ConversationStreamEvent::Token(token) = event {
            observed.push_str(&token);
        }
    })
    .unwrap();
    assert_eq!(response.finish_reason, FinishReason::Completed);
    assert_eq!(response.content, observed);
    assert!(response.content.len() > 1_000);
    assert!(response.metrics.latency_ms < 5_000);
    // Diagnostics should expose generation timing fields after streaming tokens.
    assert!(response.metrics.provider_latency_ms.is_some() || response.metrics.output_tokens.is_some());
}

#[test]
fn diagnostics_include_cancel_reason_and_rates() {
    let provider = Arc::new(ScriptedProvider::new("echo").with_tokens(vec!["a", "b", "c"]));
    let engine =
        ReasoningEngine::with_provider(provider).with_config(ReasoningEngineConfig::minimal());
    let mut stream =
        ConversationStream::start(engine, ReasoningRequest::new("hi", sample_context())).unwrap();
    // Consume thinking + first token so generation timing can be measured.
    for _ in 0..4 {
        let _ = stream.pump().unwrap();
    }
    thread::sleep(Duration::from_millis(5));
    stream.cancel();
    let response = loop {
        match stream.pump().unwrap() {
            Some(ConversationStreamEvent::Cancelled { metrics, .. }) => {
                break metrics;
            }
            Some(ConversationStreamEvent::Completed(response)) => {
                break response.metrics;
            }
            Some(_) => {}
            None => panic!("expected terminal"),
        }
    };
    assert_eq!(response.cancel_reason, Some(CancelReason::User));
    assert!(response.partial);
    let diagnostics = ConversationStream::start(
        ReasoningEngine::with_provider(Arc::new(ScriptedProvider::new("x").with_tokens(vec!["z"])))
            .with_config(ReasoningEngineConfig::minimal()),
        ReasoningRequest::new("z", sample_context()),
    )
    .unwrap()
    .collect()
    .unwrap();
    assert!(diagnostics.metrics.latency_ms < 5_000);
}
