//! Reasoning Engine — orchestration over PromptBuilder + ReasoningProvider.
//!
//! Pipeline:
//!
//! ```text
//! LlmContext → PromptBuilder → ReasoningRequest(prompt) → ReasoningProvider
//!   → StreamingResponse → ReasoningResponse
//! ```
//!
//! The engine owns timeouts, cancellation, metrics, provider selection, retry,
//! stream lifecycle, and attaching PromptBuilder output onto the request before
//! every provider call. It does **not** own prompt construction logic
//! (delegates to [`PromptBuilder`]), context assembly, Planner routing, or
//! tool execution.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::cancellation::CancellationToken;
use crate::error::{ReasoningError, ReasoningResult};
use crate::lifecycle::{CancelReason, StreamingLifecycle};
use crate::metrics::ReasoningMetrics;
use crate::model::ReasoningModelInfo;
use crate::prompt::{Prompt, PromptBudget, PromptBuilder};
use crate::provider::{
    ReasoningCapabilities, ReasoningHealth, ReasoningProvider, ReasoningStream,
};
use crate::request::ReasoningRequest;
use crate::response::{FinishReason, ReasoningResponse};
use crate::stream::{StreamingChunk, StreamingChunkKind};
use crate::types::ConversationTurn;

/// Default complete-call timeout when neither request nor config specifies one.
pub const DEFAULT_REASONING_TIMEOUT_MS: u64 = 120_000;

/// Orchestration configuration for [`ReasoningEngine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningEngineConfig {
    /// Default timeout for complete / stream collection (`None` = no engine timeout).
    pub default_timeout_ms: Option<u64>,
    /// Extra attempts after the first failure (`0` = no retry).
    pub max_retries: u32,
    /// Fixed backoff between retries.
    pub retry_backoff_ms: u64,
    /// Preferred provider id when the request does not name one.
    pub preferred_provider_id: Option<String>,
}

impl Default for ReasoningEngineConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: Some(DEFAULT_REASONING_TIMEOUT_MS),
            max_retries: 1,
            retry_backoff_ms: 25,
            preferred_provider_id: None,
        }
    }
}

impl ReasoningEngineConfig {
    /// No retries, no default timeout.
    pub fn minimal() -> Self {
        Self {
            default_timeout_ms: None,
            max_retries: 0,
            retry_backoff_ms: 0,
            preferred_provider_id: None,
        }
    }

    /// Prefer a provider id during selection.
    pub fn with_preferred_provider(mut self, id: impl Into<String>) -> Self {
        self.preferred_provider_id = Some(id.into());
        self
    }

    /// Set retry budget.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set default timeout.
    pub fn with_default_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.default_timeout_ms = Some(timeout_ms);
        self
    }
}

/// Orchestration layer between structured context and reasoning backends.
#[derive(Clone, Default)]
pub struct ReasoningEngine {
    providers: Vec<Arc<dyn ReasoningProvider>>,
    prompt_builder: PromptBuilder,
    config: ReasoningEngineConfig,
}

impl std::fmt::Debug for ReasoningEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReasoningEngine")
            .field(
                "providers",
                &self
                    .providers
                    .iter()
                    .map(|provider| provider.id().to_string())
                    .collect::<Vec<_>>(),
            )
            .field("config", &self.config)
            .field("prompt_builder", &self.prompt_builder)
            .finish()
    }
}

impl ReasoningEngine {
    /// Empty engine (no providers) with default config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Engine with a single provider.
    pub fn with_provider(provider: Arc<dyn ReasoningProvider>) -> Self {
        Self {
            providers: vec![provider],
            prompt_builder: PromptBuilder::new(),
            config: ReasoningEngineConfig::default(),
        }
    }

    /// Replace orchestration config.
    pub fn with_config(mut self, config: ReasoningEngineConfig) -> Self {
        self.config = config;
        self
    }

    /// Replace the prompt builder (construction still delegated).
    pub fn with_prompt_builder(mut self, prompt_builder: PromptBuilder) -> Self {
        self.prompt_builder = prompt_builder;
        self
    }

    /// Register an additional backend.
    pub fn register_provider(&mut self, provider: Arc<dyn ReasoningProvider>) {
        self.providers.push(provider);
    }

    /// Builder-style register.
    pub fn with_additional_provider(mut self, provider: Arc<dyn ReasoningProvider>) -> Self {
        self.register_provider(provider);
        self
    }

    /// Shared prompt builder.
    pub fn prompt_builder(&self) -> &PromptBuilder {
        &self.prompt_builder
    }

    /// Orchestration config.
    pub fn config(&self) -> &ReasoningEngineConfig {
        &self.config
    }

    /// Registered providers (registration order).
    pub fn providers(&self) -> &[Arc<dyn ReasoningProvider>] {
        &self.providers
    }

    /// Derive a [`PromptBudget`] from the selected model's limits + request reservation.
    ///
    /// Uses `GenerationParameters.max_output_tokens` when set; otherwise the
    /// provider's max-output or [`DEFAULT_RESERVED_COMPLETION_TOKENS`].
    pub fn prompt_budget_for_request(&self, request: &ReasoningRequest) -> PromptBudget {
        let reserved = request
            .parameters
            .max_output_tokens
            .map(|tokens| tokens as u64)
            .unwrap_or(0);
        match self.select_provider(request) {
            Ok(provider) => {
                let limits = provider
                    .model_limits(request.model.as_ref())
                    .unwrap_or_default();
                let reserved = if reserved == 0 {
                    limits.reserved_completion_tokens()
                } else {
                    reserved
                };
                PromptBudget::from_model_limits(&limits, reserved)
            }
            Err(_) => {
                let mut budget = self.prompt_builder.budget().clone();
                if reserved > 0 {
                    budget = budget.with_reserved_completion(reserved);
                }
                budget
            }
        }
    }

    /// Assemble a prompt, adapting the budget to the selected model automatically.
    pub fn build_prompt(&self, request: &ReasoningRequest) -> Prompt {
        let budget = self.prompt_budget_for_request(request);
        self.prompt_builder
            .clone()
            .with_budget(budget)
            .build_from_request(request)
    }

    /// Assemble a prompt with an explicit budget (tests / overrides).
    pub fn build_prompt_with_budget(
        &self,
        request: &ReasoningRequest,
        budget: PromptBudget,
    ) -> Prompt {
        self.prompt_builder
            .clone()
            .with_budget(budget)
            .build_from_request(request)
    }

    /// Whether any registered provider is currently usable.
    pub fn is_implemented(&self) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.health().is_usable())
    }

    /// Honest status for diagnostics.
    pub fn status_label(&self) -> &'static str {
        if self.providers.is_empty() {
            return "stub";
        }
        match self.primary_provider() {
            Some(provider) => match provider.health() {
                ReasoningHealth::Ready => "ready",
                ReasoningHealth::Degraded { .. } => "degraded",
                ReasoningHealth::Unavailable { .. } => "unavailable",
            },
            None => "unavailable",
        }
    }

    /// Primary / preferred provider id when any is registered.
    pub fn provider_id(&self) -> Option<&str> {
        self.primary_provider().map(|provider| provider.id())
    }

    /// Capabilities of the primary provider, or empty.
    pub fn capabilities(&self) -> ReasoningCapabilities {
        self.primary_provider()
            .map(|provider| provider.capabilities())
            .unwrap_or_default()
    }

    /// Health of the primary provider, or unavailable.
    pub fn health(&self) -> ReasoningHealth {
        match self.primary_provider() {
            Some(provider) => provider.health(),
            None => ReasoningHealth::Unavailable {
                reason: "no reasoning provider wired".into(),
            },
        }
    }

    /// List models from the selected (or primary) usable provider.
    pub fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
        let provider = self
            .select_provider_for_list()
            .ok_or(ReasoningError::NotImplemented)?;
        provider.list_models()
    }

    /// Select a provider for this request.
    pub fn select_provider(
        &self,
        request: &ReasoningRequest,
    ) -> ReasoningResult<Arc<dyn ReasoningProvider>> {
        if self.providers.is_empty() {
            return Err(ReasoningError::NotImplemented);
        }

        if let Some(model) = &request.model {
            if let Some(provider) = self.find_by_id(&model.provider) {
                return if provider.health().is_usable() {
                    Ok(Arc::clone(provider))
                } else {
                    Err(ReasoningError::Unavailable {
                        reason: format!(
                            "provider `{}` is not usable ({})",
                            provider.id(),
                            provider.health().as_str()
                        ),
                    })
                };
            }
            return Err(ReasoningError::Unavailable {
                reason: format!("no provider registered with id `{}`", model.provider),
            });
        }

        if let Some(preferred) = &self.config.preferred_provider_id {
            if let Some(provider) = self.find_by_id(preferred) {
                if provider.health().is_usable() {
                    return Ok(Arc::clone(provider));
                }
            }
        }

        if let Some(provider) = self
            .providers
            .iter()
            .find(|provider| matches!(provider.health(), ReasoningHealth::Ready))
        {
            return Ok(Arc::clone(provider));
        }

        if let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.health().is_usable())
        {
            return Ok(Arc::clone(provider));
        }

        Err(ReasoningError::Unavailable {
            reason: "no usable reasoning provider".into(),
        })
    }

    /// Complete a request: build prompt → select → retry/timeout → response.
    pub fn complete(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
        self.invoke(request, InvokeMode::Complete)
    }

    /// Begin a managed stream: build prompt → attach → select → [`StreamingResponse`].
    pub fn stream(&self, request: ReasoningRequest) -> ReasoningResult<StreamingResponse> {
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        let engine_started = Instant::now();
        let (prompt, request) = self.attach_prompt(request);
        let select_started = Instant::now();
        let provider = self.select_provider(&request)?;
        let select_ms = select_started.elapsed().as_millis() as u64;
        if !provider.capabilities().stream {
            return Err(ReasoningError::Unavailable {
                reason: format!("provider `{}` does not support streaming", provider.id()),
            });
        }
        let timeout = effective_timeout(&request, &self.config);
        let deadline = timeout.map(|ms| Instant::now() + Duration::from_millis(ms));
        let cancellation = request.cancellation.clone();
        // Transport clock starts at the provider call (includes open + generation).
        let started = Instant::now();
        let inner = provider.stream(request).map_err(|err| {
            if matches!(err, ReasoningError::Cancelled) {
                err
            } else if cancellation.is_cancelled() {
                ReasoningError::Cancelled
            } else {
                err
            }
        })?;
        let mut pipeline_seed = crate::pipeline::PipelineTiming::new();
        if let Some(ms) = prompt.diagnostics.build_duration_ms {
            pipeline_seed.set_stage("prompt_builder", ms);
        }
        let engine_ms = engine_started
            .elapsed()
            .as_millis()
            .saturating_sub(prompt.diagnostics.build_duration_ms.unwrap_or(0) as u128)
            as u64;
        // ReasoningEngine stage = attach/select overhead excluding PromptBuilder work.
        pipeline_seed.set_stage("reasoning_engine", engine_ms.max(select_ms));
        Ok(StreamingResponse::spawn_forwarding(
            inner,
            cancellation,
            deadline,
            timeout,
            started,
            provider.id().to_string(),
            prompt.size_characters(),
            prompt.diagnostics.clone(),
            pipeline_seed,
        ))
    }

    /// Build PromptBuilder output and attach it onto the request.
    fn attach_prompt(&self, mut request: ReasoningRequest) -> (Prompt, ReasoningRequest) {
        let prompt = self.build_prompt(&request);
        request.prompt = Some(prompt.clone());
        (prompt, request)
    }

    /// Begin a managed stream with start-time retry on transient disconnect / failure.
    pub fn stream_with_retry(
        &self,
        request: ReasoningRequest,
    ) -> ReasoningResult<StreamingResponse> {
        let max_attempts = self.config.max_retries.saturating_add(1);
        let mut attempts = 0u32;
        let mut last_error = None;
        while attempts < max_attempts {
            attempts += 1;
            if request.is_cancelled() {
                return Err(ReasoningError::Cancelled);
            }
            match self.stream(clone_request_for_attempt(&request)) {
                Ok(mut streaming) => {
                    streaming.attempts = attempts;
                    return Ok(streaming);
                }
                Err(err) => {
                    if matches!(err, ReasoningError::Cancelled) || request.is_cancelled() {
                        return Err(ReasoningError::Cancelled);
                    }
                    if !is_retryable(&err) || attempts >= max_attempts {
                        return Err(err);
                    }
                    last_error = Some(err);
                    if self.config.retry_backoff_ms > 0 {
                        thread::sleep(Duration::from_millis(self.config.retry_backoff_ms));
                    }
                }
            }
        }
        Err(last_error.unwrap_or(ReasoningError::StreamFailed {
            reason: "stream retry exhausted".into(),
        }))
    }

    /// Full pipeline collect: stream (or complete fallback) → [`ReasoningResponse`].
    pub fn reason(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
        match self.stream_with_retry(request.clone()) {
            Ok(streaming) => streaming.collect(),
            Err(ReasoningError::Unavailable { reason })
                if reason.contains("does not support streaming") =>
            {
                self.complete(request)
            }
            Err(err) => Err(err),
        }
    }

    fn invoke(
        &self,
        request: ReasoningRequest,
        mode: InvokeMode,
    ) -> ReasoningResult<ReasoningResponse> {
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        let engine_started = Instant::now();
        let (prompt, request) = self.attach_prompt(request);
        let select_started = Instant::now();
        let provider = self.select_provider(&request)?;
        let select_ms = select_started.elapsed().as_millis() as u64;
        let provider_id = provider.id().to_string();
        let timeout = effective_timeout(&request, &self.config);
        let max_attempts = self.config.max_retries.saturating_add(1);
        let started = Instant::now();
        let mut attempts = 0u32;
        let mut last_error = None;

        while attempts < max_attempts {
            attempts += 1;
            if request.is_cancelled() {
                return Err(ReasoningError::Cancelled);
            }
            if let Some(limit_ms) = timeout {
                if started.elapsed() >= Duration::from_millis(limit_ms) {
                    return Err(ReasoningError::TimedOut {
                        limit_ms: Some(limit_ms),
                    });
                }
            }

            let attempt_request = clone_request_for_attempt(&request);
            let result = match mode {
                InvokeMode::Complete => complete_with_timeout(
                    Arc::clone(&provider),
                    attempt_request,
                    remaining_timeout(started, timeout),
                ),
            };

            match result {
                Ok(mut response) => {
                    let latency_ms = started.elapsed().as_millis() as u64;
                    enrich_response(
                        &mut response,
                        &provider_id,
                        attempts,
                        latency_ms,
                        prompt.size_characters(),
                    );
                    let mut pipeline = crate::pipeline::PipelineTiming::new();
                    if let Some(ms) = prompt.diagnostics.build_duration_ms {
                        pipeline.set_stage("prompt_builder", ms);
                    }
                    let engine_ms = engine_started
                        .elapsed()
                        .as_millis()
                        .saturating_sub(
                            prompt.diagnostics.build_duration_ms.unwrap_or(0) as u128,
                        )
                        .saturating_sub(latency_ms as u128) as u64;
                    pipeline.set_stage("reasoning_engine", engine_ms.max(select_ms));
                    pipeline.set_stage("provider_transport", latency_ms);
                    pipeline.total_generation_ms = Some(latency_ms);
                    response.metrics.pipeline = Some(pipeline);
                    return Ok(response);
                }
                Err(err) => {
                    // Prefer TimedOut over Cancelled when the engine aborted for time.
                    if matches!(err, ReasoningError::TimedOut { .. }) {
                        return Err(err);
                    }
                    if matches!(err, ReasoningError::Cancelled) || request.is_cancelled() {
                        return Err(ReasoningError::Cancelled);
                    }
                    if !is_retryable(&err) || attempts >= max_attempts {
                        return Err(err);
                    }
                    last_error = Some(err);
                    if self.config.retry_backoff_ms > 0 {
                        thread::sleep(Duration::from_millis(self.config.retry_backoff_ms));
                    }
                }
            }
        }

        Err(last_error.unwrap_or(ReasoningError::GenerationFailed {
            reason: "retry loop exhausted".into(),
        }))
    }

    fn primary_provider(&self) -> Option<&Arc<dyn ReasoningProvider>> {
        if let Some(preferred) = &self.config.preferred_provider_id {
            if let Some(provider) = self.find_by_id(preferred) {
                return Some(provider);
            }
        }
        self.providers
            .iter()
            .find(|provider| provider.health().is_usable())
            .or_else(|| self.providers.first())
    }

    fn select_provider_for_list(&self) -> Option<&Arc<dyn ReasoningProvider>> {
        self.providers
            .iter()
            .find(|provider| provider.health().is_usable())
            .or_else(|| self.providers.first())
    }

    fn find_by_id(&self, id: &str) -> Option<&Arc<dyn ReasoningProvider>> {
        self.providers.iter().find(|provider| provider.id() == id)
    }
}

#[derive(Clone, Copy)]
enum InvokeMode {
    Complete,
}

/// Managed stream lifecycle owned by the Reasoning Engine.
///
/// Provider I/O runs on a **background worker**. [`Self::try_next_chunk`] never
/// blocks the caller — tokens are forwarded as soon as the worker receives them.
/// Diagnostics / metrics keep collecting on the worker and are attached on
/// terminal chunks only (never gate visible tokens).
pub struct StreamingResponse {
    /// Background provider stream (shared so cancel can interrupt).
    provider_stream: Arc<Mutex<Option<Box<dyn ReasoningStream>>>>,
    /// Chunks from the background reader (never blocks the UI pump).
    inbox: Receiver<StreamInboxMessage>,
    cancellation: CancellationToken,
    deadline: Option<Instant>,
    timeout_ms: Option<u64>,
    started: Instant,
    accumulated: String,
    provider_id: String,
    prompt_characters: usize,
    prompt_diagnostics: crate::prompt::PromptDiagnostics,
    attempts: u32,
    last_metrics: Option<ReasoningMetrics>,
    finish_reason: Option<FinishReason>,
    model: Option<crate::model::ModelIdentifier>,
    finished: bool,
    terminal_emitted: bool,
    lifecycle: StreamingLifecycle,
    first_token_at: Option<Instant>,
    token_count: u64,
    cancel_reason: Option<CancelReason>,
    /// PromptBuilder + provider-select timings captured at stream open.
    pipeline_seed: crate::pipeline::PipelineTiming,
}

/// Background → consumer inbox message.
enum StreamInboxMessage {
    Chunk(StreamingChunk),
    Ended,
    Error(ReasoningError),
}

/// Non-blocking poll result for [`StreamingResponse::try_next_chunk`].
#[derive(Debug)]
pub enum ChunkPoll {
    /// No provider chunk yet — call again on the next UI frame.
    Pending,
    /// A chunk is ready to forward (Token / Thought never wait on metrics).
    Ready(StreamingChunk),
    /// Provider stream exhausted without a terminal chunk.
    Closed,
}

impl StreamingResponse {
    /// Provider selected for this stream.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Pipeline timings captured at stream open (prompt / engine).
    pub fn pipeline_seed(&self) -> &crate::pipeline::PipelineTiming {
        &self.pipeline_seed
    }

    /// Prompt size recorded at stream start (delivered characters).
    pub fn prompt_characters(&self) -> usize {
        self.prompt_characters
    }

    /// Prompt diagnostics for the Prompt attached at stream start (delivered).
    pub fn prompt_diagnostics(&self) -> &crate::prompt::PromptDiagnostics {
        &self.prompt_diagnostics
    }

    /// Elapsed wall time since stream start.
    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Accumulated visible token text so far.
    pub fn accumulated_text(&self) -> &str {
        &self.accumulated
    }

    /// Current streaming lifecycle.
    pub fn lifecycle(&self) -> StreamingLifecycle {
        self.lifecycle
    }

    /// Cancel reason when cancelled.
    pub fn cancel_reason(&self) -> Option<CancelReason> {
        self.cancel_reason
    }

    /// Attempt count (includes start retries).
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    fn spawn_forwarding(
        inner: Box<dyn ReasoningStream>,
        cancellation: CancellationToken,
        deadline: Option<Instant>,
        timeout_ms: Option<u64>,
        started: Instant,
        provider_id: String,
        prompt_characters: usize,
        prompt_diagnostics: crate::prompt::PromptDiagnostics,
        pipeline_seed: crate::pipeline::PipelineTiming,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let provider_stream = Arc::new(Mutex::new(Some(inner)));
        let worker_stream = Arc::clone(&provider_stream);
        let worker_cancel = cancellation.clone();
        let _ = thread::Builder::new()
            .name("jaymi-reasoning-stream".into())
            .spawn(move || {
                loop {
                    if worker_cancel.is_cancelled() {
                        if let Ok(mut guard) = worker_stream.lock() {
                            if let Some(mut stream) = guard.take() {
                                stream.cancel();
                            }
                        }
                        let _ = tx.send(StreamInboxMessage::Chunk(StreamingChunk::cancelled(0)));
                        break;
                    }
                    let result = {
                        let mut guard = match worker_stream.lock() {
                            Ok(guard) => guard,
                            Err(_) => break,
                        };
                        let Some(stream) = guard.as_mut() else {
                            break;
                        };
                        stream.next_chunk()
                    };
                    match result {
                        Ok(Some(chunk)) => {
                            let terminal = chunk.is_terminal();
                            if tx.send(StreamInboxMessage::Chunk(chunk)).is_err() {
                                break;
                            }
                            if terminal {
                                if let Ok(mut guard) = worker_stream.lock() {
                                    *guard = None;
                                }
                                break;
                            }
                        }
                        Ok(None) => {
                            let _ = tx.send(StreamInboxMessage::Ended);
                            if let Ok(mut guard) = worker_stream.lock() {
                                *guard = None;
                            }
                            break;
                        }
                        Err(error) => {
                            let _ = tx.send(StreamInboxMessage::Error(error));
                            if let Ok(mut guard) = worker_stream.lock() {
                                *guard = None;
                            }
                            break;
                        }
                    }
                }
            });

        Self {
            provider_stream,
            inbox: rx,
            cancellation,
            deadline,
            timeout_ms,
            started,
            accumulated: String::new(),
            provider_id,
            prompt_characters,
            prompt_diagnostics,
            attempts: 1,
            last_metrics: None,
            finish_reason: None,
            model: None,
            finished: false,
            terminal_emitted: false,
            lifecycle: StreamingLifecycle::Thinking,
            first_token_at: None,
            token_count: 0,
            cancel_reason: None,
            pipeline_seed,
        }
    }

    /// Non-blocking poll — never waits on provider I/O, diagnostics, or metrics.
    ///
    /// Token / Thought chunks are forwarded immediately. Metrics enrichment
    /// happens only on terminal chunks (developer diagnostics continue in the
    /// background worker).
    pub fn try_next_chunk(&mut self) -> ReasoningResult<ChunkPoll> {
        if self.finished {
            return Ok(ChunkPoll::Closed);
        }
        if self.cancellation.is_cancelled() {
            return Ok(ChunkPoll::Ready(
                self.emit_cancelled(self.cancel_reason.unwrap_or(CancelReason::User)),
            ));
        }
        if let Some(deadline) = self.deadline {
            if Instant::now() >= deadline {
                self.request_provider_cancel();
                self.cancel_reason = Some(CancelReason::Timeout);
                self.lifecycle = StreamingLifecycle::Failed;
                self.finished = true;
                return Err(ReasoningError::TimedOut {
                    limit_ms: self.timeout_ms,
                });
            }
        }

        match self.inbox.try_recv() {
            Ok(StreamInboxMessage::Chunk(chunk)) => Ok(ChunkPoll::Ready(self.observe_chunk(chunk))),
            Ok(StreamInboxMessage::Ended) => {
                if !self.accumulated.is_empty() {
                    self.lifecycle = StreamingLifecycle::Completed;
                    self.finished = true;
                    self.terminal_emitted = true;
                    self.finish_reason = Some(FinishReason::Completed);
                    return Ok(ChunkPoll::Ready(StreamingChunk::completed(
                        self.token_count,
                        self.build_metrics(false),
                    )));
                }
                self.finished = true;
                Ok(ChunkPoll::Closed)
            }
            Ok(StreamInboxMessage::Error(ReasoningError::Cancelled)) => Ok(ChunkPoll::Ready(
                self.emit_cancelled(self.cancel_reason.unwrap_or(CancelReason::User)),
            )),
            Ok(StreamInboxMessage::Error(err)) => {
                let reason = if matches!(
                    err,
                    ReasoningError::StreamFailed { .. } | ReasoningError::Unavailable { .. }
                ) {
                    CancelReason::ProviderDisconnect
                } else {
                    CancelReason::Error
                };
                self.cancel_reason = Some(reason);
                self.lifecycle = StreamingLifecycle::Failed;
                self.finished = true;
                Err(err)
            }
            Err(TryRecvError::Empty) => Ok(ChunkPoll::Pending),
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                Ok(ChunkPoll::Closed)
            }
        }
    }

    /// Pull the next chunk, blocking until the background worker delivers one.
    ///
    /// Prefer [`Self::try_next_chunk`] on the UI / pumpable path so conversation
    /// tokens are never gated on provider read latency of later chunks.
    pub fn next_chunk(&mut self) -> ReasoningResult<Option<StreamingChunk>> {
        loop {
            match self.try_next_chunk()? {
                ChunkPoll::Pending => {
                    // Blocking delivery (collect / observer) waits briefly for
                    // the next provider chunk without spinning the CPU.
                    thread::sleep(Duration::from_millis(1));
                }
                ChunkPoll::Ready(chunk) => return Ok(Some(chunk)),
                ChunkPoll::Closed => return Ok(None),
            }
        }
    }

    /// Request cooperative cancellation.
    pub fn cancel(&mut self) {
        self.cancel_with_reason(CancelReason::User);
    }

    /// Request cooperative cancellation with an explicit reason.
    pub fn cancel_with_reason(&mut self, reason: CancelReason) {
        self.cancel_reason = Some(reason);
        self.cancellation.cancel();
        self.request_provider_cancel();
    }

    fn request_provider_cancel(&self) {
        if let Ok(mut guard) = self.provider_stream.lock() {
            if let Some(stream) = guard.as_mut() {
                stream.cancel();
            }
        }
    }

    /// Drain remaining chunks into a [`ReasoningResponse`].
    pub fn collect(mut self) -> ReasoningResult<ReasoningResponse> {
        while !self.finished {
            match self.next_chunk()? {
                Some(chunk) if chunk.is_terminal() => break,
                Some(_) => {}
                None => break,
            }
        }
        if self.cancellation.is_cancelled() && !self.terminal_emitted {
            let _ = self.emit_cancelled(self.cancel_reason.unwrap_or(CancelReason::User));
        }
        Ok(self.into_response())
    }

    /// Build a response from current accumulated state (partial or complete).
    pub fn into_response(self) -> ReasoningResponse {
        let partial = matches!(
            self.lifecycle,
            StreamingLifecycle::Cancelled | StreamingLifecycle::Failed
        );
        let metrics = self.build_metrics(partial);
        let content = self.accumulated.clone();
        let finish = self.finish_reason.unwrap_or(match self.lifecycle {
            StreamingLifecycle::Cancelled => FinishReason::Cancelled,
            StreamingLifecycle::Failed => FinishReason::Error,
            _ => FinishReason::Completed,
        });
        let mut response = ReasoningResponse {
            assistant_turn: ConversationTurn::assistant(content.clone()),
            content,
            finish_reason: finish,
            model: self.model.clone(),
            metrics,
            notes: vec![
                format!("prompt_chars={}", self.prompt_characters),
                format!("provider={}", self.provider_id),
                format!("lifecycle={}", self.lifecycle.as_str()),
            ],
        };
        if let Some(model) = self.model {
            response = response.with_model(model);
        }
        response
    }

    fn observe_chunk(&mut self, chunk: StreamingChunk) -> StreamingChunk {
        if let Some(text) = &chunk.text {
            if matches!(chunk.kind, StreamingChunkKind::Token) {
                self.accumulated.push_str(text);
                self.token_count = self.token_count.saturating_add(1);
                if self.first_token_at.is_none() {
                    self.first_token_at = Some(Instant::now());
                }
                if !self.lifecycle.is_terminal() {
                    self.lifecycle = StreamingLifecycle::Streaming;
                }
            } else if matches!(chunk.kind, StreamingChunkKind::Thought)
                && matches!(self.lifecycle, StreamingLifecycle::Thinking | StreamingLifecycle::Idle)
            {
                self.lifecycle = StreamingLifecycle::Thinking;
            }
        }
        if let Some(metrics) = &chunk.metrics {
            self.last_metrics = Some(metrics.clone());
            if let Some(model) = &metrics.model {
                self.model = Some(model.clone());
            }
        }
        if let Some(reason) = chunk.finish_reason {
            self.finish_reason = Some(reason);
        }
        if chunk.is_terminal() {
            self.lifecycle = match chunk.kind {
                StreamingChunkKind::Cancelled => StreamingLifecycle::Cancelled,
                StreamingChunkKind::Failed => StreamingLifecycle::Failed,
                _ => StreamingLifecycle::Completed,
            };
            self.terminal_emitted = true;
            self.finished = true;
            // Enrich terminal metrics before handing the chunk out — tokens already
            // forwarded without waiting for this enrichment.
            let mut enriched = chunk;
            enriched.metrics = Some(self.build_metrics(matches!(
                self.lifecycle,
                StreamingLifecycle::Cancelled | StreamingLifecycle::Failed
            )));
            return enriched;
        }
        // Token / Thought: never attach metrics; forward immediately.
        chunk
    }

    fn build_metrics(&self, partial: bool) -> ReasoningMetrics {
        let latency_ms = self.elapsed_ms();
        let mut metrics = self
            .last_metrics
            .clone()
            .unwrap_or_else(|| ReasoningMetrics::timed(latency_ms));
        metrics.latency_ms = latency_ms;
        metrics.provider_id = Some(self.provider_id.clone());
        metrics.attempts = self.attempts.max(1);
        metrics.partial = partial;
        let mut pipeline = self.pipeline_seed.clone();
        pipeline.set_stage("provider_transport", latency_ms);
        if let Some(first) = self.first_token_at {
            let ttft = first.duration_since(self.started).as_millis() as u64;
            metrics.ttft_ms = Some(ttft);
            // Keep provider_latency_ms as provider-reported duration when present;
            // otherwise mirror TTFT for backward-compatible diagnostics.
            if metrics.provider_latency_ms.is_none() {
                metrics.provider_latency_ms = Some(ttft);
            }
            let generation_ms = first.elapsed().as_millis() as u64;
            metrics.generation_duration_ms = Some(generation_ms);
            pipeline.ttft_ms = Some(ttft);
            pipeline.total_generation_ms = Some(generation_ms);
            if generation_ms > 0 && self.token_count > 0 {
                let tps = (self.token_count as f64) / (generation_ms as f64 / 1000.0);
                metrics = metrics.with_tokens_per_sec(tps);
            }
            if metrics.output_tokens.is_none() {
                metrics.output_tokens = Some(self.token_count);
                if let Some(input) = metrics.input_tokens {
                    metrics.total_tokens = Some(input.saturating_add(self.token_count));
                } else {
                    metrics.total_tokens = Some(self.token_count);
                }
            }
        }
        metrics.pipeline = Some(pipeline);
        if self.cancellation.is_cancelled()
            || matches!(self.finish_reason, Some(FinishReason::Cancelled))
            || matches!(self.lifecycle, StreamingLifecycle::Cancelled)
        {
            metrics.cancelled = true;
            metrics.cancel_reason = self.cancel_reason.or(Some(CancelReason::User));
        } else if matches!(self.lifecycle, StreamingLifecycle::Failed) {
            metrics.cancel_reason = self.cancel_reason.or(Some(CancelReason::Error));
            metrics.partial = true;
        }
        metrics
    }

    fn emit_cancelled(&mut self, reason: CancelReason) -> StreamingChunk {
        self.cancel_reason = Some(reason);
        self.lifecycle = StreamingLifecycle::Cancelled;
        self.finished = true;
        self.terminal_emitted = true;
        self.finish_reason = Some(FinishReason::Cancelled);
        StreamingChunk::cancelled_with_reason(0, self.build_metrics(true))
    }
}

fn effective_timeout(
    request: &ReasoningRequest,
    config: &ReasoningEngineConfig,
) -> Option<u64> {
    request
        .parameters
        .timeout_ms
        .or(config.default_timeout_ms)
}

fn remaining_timeout(started: Instant, timeout: Option<u64>) -> Option<Duration> {
    let limit_ms = timeout?;
    let elapsed = started.elapsed();
    let limit = Duration::from_millis(limit_ms);
    if elapsed >= limit {
        Some(Duration::from_millis(0))
    } else {
        Some(limit - elapsed)
    }
}

fn complete_with_timeout(
    provider: Arc<dyn ReasoningProvider>,
    request: ReasoningRequest,
    timeout: Option<Duration>,
) -> ReasoningResult<ReasoningResponse> {
    let Some(limit) = timeout else {
        return provider.complete(request);
    };
    if limit.is_zero() {
        request.cancellation.cancel();
        return Err(ReasoningError::TimedOut { limit_ms: Some(0) });
    }

    let cancellation = request.cancellation.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(provider.complete(request));
    });

    match rx.recv_timeout(limit) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            cancellation.cancel();
            Err(ReasoningError::TimedOut {
                limit_ms: Some(limit.as_millis() as u64),
            })
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(ReasoningError::GenerationFailed {
            reason: "provider worker disconnected".into(),
        }),
    }
}

fn clone_request_for_attempt(request: &ReasoningRequest) -> ReasoningRequest {
    ReasoningRequest {
        goal: request.goal.clone(),
        history: request.history.clone(),
        context: request.context.clone(),
        prompt: request.prompt.clone(),
        parameters: request.parameters.clone(),
        model: request.model.clone(),
        request_id: request.request_id.clone(),
        cancellation: request.cancellation.clone(),
    }
}

fn enrich_response(
    response: &mut ReasoningResponse,
    provider_id: &str,
    attempts: u32,
    latency_ms: u64,
    prompt_characters: usize,
) {
    response.metrics.latency_ms = latency_ms;
    response.metrics.provider_id = Some(provider_id.to_string());
    response.metrics.attempts = attempts;
    response.notes.push(format!("prompt_chars={prompt_characters}"));
    response.notes.push(format!("provider={provider_id}"));
    if attempts > 1 {
        response.notes.push(format!("attempts={attempts}"));
    }
}

fn is_retryable(error: &ReasoningError) -> bool {
    matches!(
        error,
        ReasoningError::Unavailable { .. }
            | ReasoningError::GenerationFailed { .. }
            | ReasoningError::StreamFailed { .. }
    )
}
