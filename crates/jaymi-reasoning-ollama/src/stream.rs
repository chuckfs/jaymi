//! Streaming handle for Ollama `/api/chat` NDJSON.

use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jaymi_reasoning::{
    CancellationToken, FinishReason, ModelIdentifier, ReasoningError, ReasoningMetrics,
    ReasoningResult, ReasoningStream, StreamingChunk,
};

use crate::diagnostics::StreamingStatus;
use crate::types::ChatStreamEvent;

/// Shared diagnostics mutex.
pub type SharedDiagnostics = Arc<Mutex<crate::diagnostics::OllamaDiagnostics>>;

/// Pull-based NDJSON stream adapter.
pub struct OllamaReasoningStream {
    reader: Option<Box<dyn BufRead + Send>>,
    cancellation: CancellationToken,
    cancelled: bool,
    finished: bool,
    index: u64,
    model: ModelIdentifier,
    started: Instant,
    accumulated: String,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    diagnostics: SharedDiagnostics,
}

impl OllamaReasoningStream {
    /// Create a stream over an NDJSON body.
    pub fn new(
        reader: Box<dyn BufRead + Send>,
        cancellation: CancellationToken,
        model: ModelIdentifier,
        diagnostics: SharedDiagnostics,
    ) -> Self {
        {
            let mut state = diagnostics.lock().expect("diagnostics");
            state.streaming_status = StreamingStatus::Thinking;
            state.detail = Some("thinking".into());
        }
        Self {
            reader: Some(reader),
            cancellation,
            cancelled: false,
            finished: false,
            index: 0,
            model,
            started: Instant::now(),
            accumulated: String::new(),
            input_tokens: None,
            output_tokens: None,
            diagnostics,
        }
    }

    fn mark_status(&self, status: StreamingStatus, detail: impl Into<String>) {
        let mut state = self.diagnostics.lock().expect("diagnostics");
        state.streaming_status = status;
        state.detail = Some(detail.into());
        state.latency_ms = Some(self.started.elapsed().as_millis() as u64);
        if !self.accumulated.is_empty() {
            // Keep loaded_model as the model we just used when stream ends.
        }
        state.loaded_model = Some(self.model.name.clone());
    }

    fn parse_line(&mut self, line: &str) -> ReasoningResult<Option<StreamingChunk>> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }
        let event: ChatStreamEvent = serde_json::from_str(line).map_err(|err| {
            ReasoningError::StreamFailed {
                reason: format!("malformed stream event: {err}"),
            }
        })?;
        if let Some(error) = event.error {
            self.finished = true;
            self.mark_status(StreamingStatus::Failed, error.clone());
            return Err(ReasoningError::GenerationFailed { reason: error });
        }
        if let Some(message) = &event.message {
            if let Some(thinking) = &message.thinking {
                if !thinking.is_empty() {
                    let chunk = StreamingChunk::thought(self.index, thinking.clone());
                    self.index += 1;
                    return Ok(Some(chunk));
                }
            }
            if let Some(content) = &message.content {
                if !content.is_empty() {
                    self.accumulated.push_str(content);
                    {
                        let mut state = self.diagnostics.lock().expect("diagnostics");
                        if matches!(
                            state.streaming_status,
                            StreamingStatus::Thinking | StreamingStatus::Idle
                        ) {
                            state.streaming_status = StreamingStatus::Streaming;
                            state.detail = Some("streaming".into());
                        }
                    }
                    let chunk = StreamingChunk::token(self.index, content.clone());
                    self.index += 1;
                    return Ok(Some(chunk));
                }
            }
        }
        if event.done {
            self.finished = true;
            if let Some(count) = event.prompt_eval_count {
                self.input_tokens = Some(count);
            }
            if let Some(count) = event.eval_count {
                self.output_tokens = Some(count);
            }
            let latency_ms = self.started.elapsed().as_millis() as u64;
            let mut metrics = ReasoningMetrics::timed(latency_ms)
                .with_tokens(self.input_tokens, self.output_tokens)
                .with_model(self.model.clone());
            if let Some(total_ns) = event.total_duration {
                metrics.provider_latency_ms = Some(total_ns / 1_000_000);
            }
            let finish = match event.done_reason.as_deref() {
                Some("length") => FinishReason::Length,
                Some("stop") | None => FinishReason::Completed,
                _ => FinishReason::Completed,
            };
            self.mark_status(StreamingStatus::Completed, "completed");
            let mut chunk = StreamingChunk::completed(self.index, metrics);
            chunk.finish_reason = Some(finish);
            self.index += 1;
            return Ok(Some(chunk));
        }
        Ok(None)
    }
}

impl ReasoningStream for OllamaReasoningStream {
    fn next_chunk(&mut self) -> ReasoningResult<Option<StreamingChunk>> {
        if self.finished {
            return Ok(None);
        }
        if self.cancelled || self.cancellation.is_cancelled() {
            self.finished = true;
            self.reader = None;
            self.mark_status(StreamingStatus::Cancelled, "cancelled");
            let chunk = StreamingChunk::cancelled(self.index);
            self.index += 1;
            return Ok(Some(chunk));
        }
        let mut reader = match self.reader.take() {
            Some(reader) => reader,
            None => {
                self.finished = true;
                return Ok(None);
            }
        };

        loop {
            if self.cancelled || self.cancellation.is_cancelled() {
                self.finished = true;
                self.mark_status(StreamingStatus::Cancelled, "cancelled");
                let chunk = StreamingChunk::cancelled(self.index);
                self.index += 1;
                return Ok(Some(chunk));
            }
            let mut line = String::new();
            let read = match reader.read_line(&mut line) {
                Ok(n) => n,
                Err(err) => {
                    self.mark_status(StreamingStatus::Failed, err.to_string());
                    return Err(ReasoningError::StreamFailed {
                        reason: format!("failed reading stream: {err}"),
                    });
                }
            };
            if read == 0 {
                self.finished = true;
                if self.accumulated.is_empty() {
                    self.mark_status(StreamingStatus::Failed, "empty stream");
                    return Err(ReasoningError::StreamFailed {
                        reason: "stream ended without a completion event".into(),
                    });
                }
                let latency_ms = self.started.elapsed().as_millis() as u64;
                let metrics = ReasoningMetrics::timed(latency_ms)
                    .with_tokens(self.input_tokens, self.output_tokens)
                    .with_model(self.model.clone());
                self.mark_status(StreamingStatus::Completed, "completed_without_done");
                return Ok(Some(StreamingChunk::completed(self.index, metrics)));
            }
            match self.parse_line(&line) {
                Ok(Some(chunk)) => {
                    if !chunk.is_terminal() {
                        self.reader = Some(reader);
                    }
                    return Ok(Some(chunk));
                }
                Ok(None) => {}
                Err(err) => return Err(err),
            }
        }
    }

    fn cancel(&mut self) {
        self.cancelled = true;
        self.cancellation.cancel();
        self.reader = None;
    }
}
