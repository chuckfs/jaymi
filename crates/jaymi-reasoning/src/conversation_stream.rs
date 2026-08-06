//! Conversation streaming session — lifecycle, incremental events, retry.

use crate::engine::{ReasoningEngine, StreamingResponse};
use crate::error::{ReasoningError, ReasoningResult};
use crate::lifecycle::{CancelReason, StreamingLifecycle};
use crate::metrics::ReasoningMetrics;
use crate::request::ReasoningRequest;
use crate::response::{FinishReason, ReasoningResponse};
use crate::stream::{StreamingChunk, StreamingChunkKind};

/// Incremental event emitted while pumping a conversation stream.
#[derive(Debug, Clone)]
pub enum ConversationStreamEvent {
    /// Lifecycle transition (Idle → Thinking → Streaming → terminal).
    Lifecycle(StreamingLifecycle),
    /// Intermediate thought / scratch text (not final answer).
    Thought(String),
    /// Visible token appended to the assistant reply.
    Token(String),
    /// Successful completion with full response.
    Completed(ReasoningResponse),
    /// Cancelled; `partial` may hold text streamed so far.
    Cancelled {
        /// Text streamed before cancel.
        partial: String,
        /// Why cancellation happened.
        reason: CancelReason,
        /// Diagnostics at cancel time.
        metrics: ReasoningMetrics,
    },
    /// Failed; `partial` may hold text streamed before the failure.
    Failed {
        /// Text streamed before failure.
        partial: String,
        /// Failure detail.
        error: ReasoningError,
        /// Diagnostics at failure time.
        metrics: ReasoningMetrics,
    },
}

impl ConversationStreamEvent {
    /// True when this event ends the stream.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed(_) | Self::Cancelled { .. } | Self::Failed { .. }
        )
    }
}

/// Diagnostics snapshot for an in-flight or finished conversation stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationStreamDiagnostics {
    /// Current lifecycle.
    pub lifecycle: StreamingLifecycle,
    /// Wall-clock latency so far / total.
    pub latency_ms: u64,
    /// Time-to-first-token or provider-reported latency.
    pub provider_latency_ms: Option<u64>,
    /// First token → now / end.
    pub generation_duration_ms: Option<u64>,
    /// Approximate tokens/sec × 1000.
    pub tokens_per_sec_milli: Option<u64>,
    /// Cancel reason when cancelled / disconnect-failed.
    pub cancel_reason: Option<CancelReason>,
    /// Selected provider.
    pub provider_id: Option<String>,
    /// Attempt count including reconnects.
    pub attempts: u32,
    /// True when content is partial.
    pub partial: bool,
}

impl ConversationStreamDiagnostics {
    /// Tokens/sec when known.
    pub fn tokens_per_sec(&self) -> Option<f64> {
        self.tokens_per_sec_milli
            .map(|milli| milli as f64 / 1000.0)
    }

    /// Lift into [`ReasoningMetrics`] for terminal Failed events.
    pub fn into_metrics(self) -> ReasoningMetrics {
        let mut metrics = ReasoningMetrics::timed(self.latency_ms)
            .with_attempts(self.attempts)
            .with_partial(self.partial);
        if let Some(provider_id) = self.provider_id {
            metrics = metrics.with_provider_id(provider_id);
        }
        if let Some(provider_latency_ms) = self.provider_latency_ms {
            metrics = metrics.with_provider_latency_ms(provider_latency_ms);
        }
        if let Some(generation_duration_ms) = self.generation_duration_ms {
            metrics = metrics.with_generation_duration_ms(generation_duration_ms);
        }
        if let Some(milli) = self.tokens_per_sec_milli {
            metrics.tokens_per_sec_milli = Some(milli);
        }
        if let Some(reason) = self.cancel_reason {
            metrics = metrics.with_cancel_reason(reason);
        }
        metrics
    }
}

/// Planner/Experience-facing streaming conversation session.
///
/// Owns lifecycle Idle → Thinking → Streaming → terminal, incremental text,
/// cancel / retry / reconnect, and partial completion.
pub struct ConversationStream {
    engine: ReasoningEngine,
    request: ReasoningRequest,
    inner: Option<StreamingResponse>,
    lifecycle: StreamingLifecycle,
    last_emitted_lifecycle: StreamingLifecycle,
    accumulated: String,
    finished: bool,
    attempts: u32,
    last_error: Option<ReasoningError>,
    cancel_reason: Option<CancelReason>,
    last_metrics: ReasoningMetrics,
    pending: Option<StreamingChunk>,
    complete_fallback: Option<ReasoningResponse>,
    /// Diagnostics for the Prompt actually attached for delivery (B1.13.5).
    prompt_diagnostics: Option<crate::prompt::PromptDiagnostics>,
}

impl ConversationStream {
    /// Start a conversation stream (lifecycle → Thinking).
    pub fn start(engine: ReasoningEngine, request: ReasoningRequest) -> ReasoningResult<Self> {
        let mut session = Self {
            engine,
            request,
            inner: None,
            lifecycle: StreamingLifecycle::Idle,
            last_emitted_lifecycle: StreamingLifecycle::Idle,
            accumulated: String::new(),
            finished: false,
            attempts: 0,
            last_error: None,
            cancel_reason: None,
            last_metrics: ReasoningMetrics::default(),
            pending: None,
            complete_fallback: None,
            prompt_diagnostics: None,
        };
        session.begin_attempt()?;
        Ok(session)
    }

    /// Diagnostics for the Prompt attached and delivered to the provider.
    pub fn prompt_diagnostics(&self) -> Option<&crate::prompt::PromptDiagnostics> {
        self.prompt_diagnostics.as_ref().or_else(|| {
            self.inner
                .as_ref()
                .map(|stream| stream.prompt_diagnostics())
        })
    }

    /// Current lifecycle.
    pub fn lifecycle(&self) -> StreamingLifecycle {
        self.lifecycle
    }

    /// Text accumulated so far (partial or complete).
    pub fn accumulated_text(&self) -> &str {
        &self.accumulated
    }

    /// True when a terminal event has been produced.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Diagnostics snapshot.
    pub fn diagnostics(&self) -> ConversationStreamDiagnostics {
        let mut diagnostics = ConversationStreamDiagnostics {
            lifecycle: self.lifecycle,
            latency_ms: self
                .inner
                .as_ref()
                .map(StreamingResponse::elapsed_ms)
                .unwrap_or(self.last_metrics.latency_ms),
            provider_latency_ms: self.last_metrics.provider_latency_ms,
            generation_duration_ms: self.last_metrics.generation_duration_ms,
            tokens_per_sec_milli: self.last_metrics.tokens_per_sec_milli,
            cancel_reason: self.cancel_reason.or(self.last_metrics.cancel_reason),
            provider_id: self
                .inner
                .as_ref()
                .map(|s| s.provider_id().to_string())
                .or_else(|| self.last_metrics.provider_id.clone()),
            attempts: self.attempts.max(self.last_metrics.attempts),
            partial: self.last_metrics.partial
                || matches!(
                    self.lifecycle,
                    StreamingLifecycle::Cancelled | StreamingLifecycle::Failed
                ),
        };
        if let Some(inner) = &self.inner {
            diagnostics.lifecycle = inner.lifecycle();
        }
        diagnostics
    }

    /// Cooperative cancel.
    pub fn cancel(&mut self) {
        self.cancel_with_reason(CancelReason::User);
    }

    /// Cooperative cancel with reason.
    pub fn cancel_with_reason(&mut self, reason: CancelReason) {
        self.cancel_reason = Some(reason);
        if let Some(inner) = self.inner.as_mut() {
            inner.cancel_with_reason(reason);
        } else {
            self.request.cancellation.cancel();
            self.lifecycle = StreamingLifecycle::Cancelled;
        }
    }

    /// Retry / reconnect after failure or cancel.
    ///
    /// Starts a fresh stream attempt for the same request. When `keep_partial`
    /// is true, prior streamed text is kept as a prefix of the new reply.
    pub fn retry(&mut self, keep_partial: bool) -> ReasoningResult<()> {
        if self.lifecycle.is_active() {
            self.cancel_with_reason(CancelReason::Engine);
            while !self.finished {
                match self.pump() {
                    Ok(Some(event)) if event.is_terminal() => break,
                    Ok(Some(_)) | Ok(None) => {}
                    Err(_) => break,
                }
            }
        }
        self.finished = false;
        self.last_error = None;
        self.cancel_reason = None;
        self.complete_fallback = None;
        self.pending = None;
        if !keep_partial {
            self.accumulated.clear();
        }
        self.begin_attempt()
    }

    /// Pull the next conversation event.
    pub fn pump(&mut self) -> ReasoningResult<Option<ConversationStreamEvent>> {
        if self.finished && self.pending.is_none() && self.complete_fallback.is_none() {
            return Ok(None);
        }

        if let Some(response) = self.complete_fallback.take() {
            self.finished = true;
            self.lifecycle = StreamingLifecycle::Completed;
            if self.last_emitted_lifecycle != StreamingLifecycle::Thinking
                && self.last_emitted_lifecycle != StreamingLifecycle::Completed
            {
                // Ensure Thinking was visible for complete-fallback path.
            }
            if self.last_emitted_lifecycle != StreamingLifecycle::Completed {
                // Emit lifecycle Completed via the Completed event only.
            }
            return Ok(Some(ConversationStreamEvent::Completed(response)));
        }

        if self.lifecycle != self.last_emitted_lifecycle {
            let lifecycle = self.lifecycle;
            self.last_emitted_lifecycle = lifecycle;
            return Ok(Some(ConversationStreamEvent::Lifecycle(lifecycle)));
        }

        if let Some(chunk) = self.pending.take() {
            return Ok(Some(self.event_from_chunk(chunk)));
        }

        let chunk_result = {
            let Some(inner) = self.inner.as_mut() else {
                self.finished = true;
                return Ok(None);
            };
            inner.next_chunk()
        };

        match chunk_result {
            Ok(Some(chunk)) => {
                if let Some(inner) = self.inner.as_ref() {
                    self.lifecycle = inner.lifecycle();
                    self.accumulated = inner.accumulated_text().to_string();
                }
                if let Some(metrics) = &chunk.metrics {
                    self.last_metrics = metrics.clone();
                }

                if self.lifecycle != self.last_emitted_lifecycle {
                    self.pending = Some(chunk);
                    let lifecycle = self.lifecycle;
                    self.last_emitted_lifecycle = lifecycle;
                    return Ok(Some(ConversationStreamEvent::Lifecycle(lifecycle)));
                }
                Ok(Some(self.event_from_chunk(chunk)))
            }
            Ok(None) => {
                if let Some(inner) = self.inner.as_ref() {
                    self.accumulated = inner.accumulated_text().to_string();
                }
                self.lifecycle = StreamingLifecycle::Completed;
                self.finished = true;
                let response = self.take_inner_response();
                Ok(Some(ConversationStreamEvent::Completed(response)))
            }
            Err(ReasoningError::TimedOut { limit_ms }) => {
                if let Some(inner) = self.inner.as_ref() {
                    self.accumulated = inner.accumulated_text().to_string();
                }
                self.cancel_reason = Some(CancelReason::Timeout);
                self.lifecycle = StreamingLifecycle::Failed;
                self.finished = true;
                let metrics = self.snapshot_metrics(true);
                self.inner = None;
                let error = ReasoningError::TimedOut { limit_ms };
                self.last_error = Some(error.clone());
                self.last_metrics = metrics.clone();
                Ok(Some(ConversationStreamEvent::Failed {
                    partial: self.accumulated.clone(),
                    error,
                    metrics,
                }))
            }
            Err(err) => {
                if let Some(inner) = self.inner.as_ref() {
                    self.accumulated = inner.accumulated_text().to_string();
                }
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
                let metrics = self.snapshot_metrics(true);
                self.inner = None;
                self.last_error = Some(err.clone());
                self.last_metrics = metrics.clone();
                Ok(Some(ConversationStreamEvent::Failed {
                    partial: self.accumulated.clone(),
                    error: err,
                    metrics,
                }))
            }
        }
    }

    /// Drain until terminal, invoking `on_event` for every event (incremental updates).
    pub fn run_with_observer<F>(mut self, mut on_event: F) -> ReasoningResult<ReasoningResponse>
    where
        F: FnMut(ConversationStreamEvent),
    {
        let mut final_response = None;
        loop {
            match self.pump()? {
                Some(event) => {
                    let terminal = event.is_terminal();
                    match &event {
                        ConversationStreamEvent::Completed(response) => {
                            final_response = Some(response.clone());
                        }
                        ConversationStreamEvent::Cancelled {
                            partial,
                            reason,
                            metrics,
                        } => {
                            final_response = Some(
                                ReasoningResponse::completed(partial.clone())
                                    .with_finish_reason(FinishReason::Cancelled)
                                    .with_metrics(metrics.clone().with_cancel_reason(*reason)),
                            );
                        }
                        ConversationStreamEvent::Failed {
                            partial, metrics, ..
                        } => {
                            final_response = Some(
                                ReasoningResponse::completed(partial.clone())
                                    .with_finish_reason(FinishReason::Error)
                                    .with_metrics(metrics.clone().with_partial(true)),
                            );
                        }
                        _ => {}
                    }
                    on_event(event);
                    if terminal {
                        break;
                    }
                }
                None => break,
            }
        }
        if let Some(response) = final_response {
            return Ok(response);
        }
        Ok(ReasoningResponse::completed(self.accumulated)
            .with_metrics(self.last_metrics)
            .with_finish_reason(FinishReason::Completed))
    }

    /// Drain until terminal without an observer.
    pub fn collect(self) -> ReasoningResult<ReasoningResponse> {
        self.run_with_observer(|_| {})
    }

    fn begin_attempt(&mut self) -> ReasoningResult<()> {
        self.attempts = self.attempts.saturating_add(1);
        self.lifecycle = StreamingLifecycle::Thinking;
        self.last_emitted_lifecycle = StreamingLifecycle::Idle;
        self.finished = false;
        self.pending = None;
        match self.engine.stream_with_retry(self.request.clone()) {
            Ok(stream) => {
                self.prompt_diagnostics = Some(stream.prompt_diagnostics().clone());
                self.attempts = self.attempts.max(stream.attempts());
                self.inner = Some(stream);
                Ok(())
            }
            Err(ReasoningError::Unavailable { reason })
                if reason.contains("does not support streaming") =>
            {
                // Same attach path complete uses — seal diagnostics from that Prompt.
                let prompt = self.engine.build_prompt(&self.request);
                self.prompt_diagnostics = Some(prompt.diagnostics.clone());
                let response = self.engine.complete(self.request.clone())?;
                self.accumulated = response.content.clone();
                self.lifecycle = StreamingLifecycle::Completed;
                self.last_metrics = response.metrics.clone();
                self.inner = None;
                self.complete_fallback = Some(response);
                Ok(())
            }
            Err(err) => {
                self.lifecycle = StreamingLifecycle::Failed;
                self.finished = true;
                self.last_error = Some(err.clone());
                Err(err)
            }
        }
    }

    fn event_from_chunk(&mut self, chunk: StreamingChunk) -> ConversationStreamEvent {
        match chunk.kind {
            StreamingChunkKind::Thought => {
                ConversationStreamEvent::Thought(chunk.text.unwrap_or_default())
            }
            StreamingChunkKind::Token => {
                ConversationStreamEvent::Token(chunk.text.unwrap_or_default())
            }
            StreamingChunkKind::Completed => {
                self.finished = true;
                let mut response = self.take_inner_response();
                if let Some(reason) = chunk.finish_reason {
                    response = response.with_finish_reason(reason);
                }
                if let Some(metrics) = chunk.metrics {
                    self.last_metrics = metrics.clone();
                    response = response.with_metrics(metrics);
                }
                ConversationStreamEvent::Completed(response)
            }
            StreamingChunkKind::Cancelled => {
                self.finished = true;
                if let Some(inner) = self.inner.as_ref() {
                    self.accumulated = inner.accumulated_text().to_string();
                }
                self.inner = None;
                let reason = self
                    .cancel_reason
                    .or_else(|| chunk.metrics.as_ref().and_then(|m| m.cancel_reason))
                    .unwrap_or(CancelReason::User);
                let metrics = chunk
                    .metrics
                    .unwrap_or_else(|| self.snapshot_metrics(true))
                    .with_cancel_reason(reason);
                self.last_metrics = metrics.clone();
                ConversationStreamEvent::Cancelled {
                    partial: self.accumulated.clone(),
                    reason,
                    metrics,
                }
            }
            StreamingChunkKind::Failed => {
                self.finished = true;
                if let Some(inner) = self.inner.as_ref() {
                    self.accumulated = inner.accumulated_text().to_string();
                }
                self.inner = None;
                let metrics = chunk
                    .metrics
                    .unwrap_or_else(|| self.snapshot_metrics(true))
                    .with_partial(true);
                self.last_metrics = metrics.clone();
                let error = self
                    .last_error
                    .clone()
                    .unwrap_or(ReasoningError::StreamFailed {
                        reason: "stream failed".into(),
                    });
                ConversationStreamEvent::Failed {
                    partial: self.accumulated.clone(),
                    error,
                    metrics,
                }
            }
        }
    }

    fn take_inner_response(&mut self) -> ReasoningResponse {
        if let Some(inner) = self.inner.take() {
            let response = inner.into_response();
            self.last_metrics = response.metrics.clone();
            self.accumulated = response.content.clone();
            response
        } else {
            ReasoningResponse::completed(self.accumulated.clone()).with_metrics(self.last_metrics.clone())
        }
    }

    fn snapshot_metrics(&self, partial: bool) -> ReasoningMetrics {
        let mut metrics = self.last_metrics.clone();
        metrics.partial = partial;
        metrics.attempts = self.attempts.max(metrics.attempts).max(1);
        if let Some(reason) = self.cancel_reason {
            metrics.cancel_reason = Some(reason);
            if matches!(reason, CancelReason::User | CancelReason::Timeout | CancelReason::Engine) {
                metrics.cancelled = true;
            }
        }
        if let Some(inner) = &self.inner {
            metrics.latency_ms = inner.elapsed_ms();
            metrics.provider_id = Some(inner.provider_id().to_string());
        }
        metrics
    }
}
