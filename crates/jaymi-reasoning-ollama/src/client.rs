//! Thin Ollama API client over [`crate::OllamaTransport`].

use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

use crate::transport::{OllamaTransport, TransportError, DEFAULT_OLLAMA_BASE_URL};
use crate::types::{
    ChatMessage, OllamaModelTag, PsResponse, TagsResponse, VersionResponse,
};

/// Client configuration.
#[derive(Debug, Clone)]
pub struct OllamaClientConfig {
    /// Base URL (default `http://127.0.0.1:11434`).
    pub base_url: String,
    /// Default model name when the request does not specify one.
    pub default_model: Option<String>,
}

impl Default for OllamaClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_OLLAMA_BASE_URL.into(),
            default_model: None,
        }
    }
}

/// Ollama HTTP client (provider-facing helpers).
#[derive(Clone)]
pub struct OllamaClient {
    transport: Arc<dyn OllamaTransport>,
    config: OllamaClientConfig,
}

impl OllamaClient {
    /// Live client against the default local endpoint.
    pub fn local() -> Self {
        Self::with_transport(
            Arc::new(crate::transport::HttpOllamaTransport::default()),
            OllamaClientConfig::default(),
        )
    }

    /// Client with an injected transport (tests / custom endpoints).
    pub fn with_transport(
        transport: Arc<dyn OllamaTransport>,
        config: OllamaClientConfig,
    ) -> Self {
        Self { transport, config }
    }

    /// Base URL / default model configuration.
    pub fn config(&self) -> &OllamaClientConfig {
        &self.config
    }

    /// Shared transport.
    pub fn transport(&self) -> Arc<dyn OllamaTransport> {
        Arc::clone(&self.transport)
    }

    /// Probe `/api/version`. Returns `(version, latency_ms)`.
    pub fn version(&self) -> Result<(String, u64), TransportError> {
        let started = Instant::now();
        let body = self.transport.get_text("/api/version")?;
        let latency_ms = started.elapsed().as_millis() as u64;
        let parsed: VersionResponse = serde_json::from_str(&body).map_err(|err| {
            TransportError::Io(format!("malformed /api/version: {err}"))
        })?;
        Ok((parsed.version, latency_ms))
    }

    /// List installed models via `/api/tags`.
    pub fn list_tags(&self) -> Result<Vec<OllamaModelTag>, TransportError> {
        let body = self.transport.get_text("/api/tags")?;
        let parsed: TagsResponse = serde_json::from_str(&body)
            .map_err(|err| TransportError::Io(format!("malformed /api/tags: {err}")))?;
        Ok(parsed.models)
    }

    /// First loaded model name from `/api/ps`, when any.
    pub fn loaded_model(&self) -> Result<Option<String>, TransportError> {
        let body = match self.transport.get_text("/api/ps") {
            Ok(body) => body,
            Err(TransportError::HttpStatus { status: 404, .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let parsed: PsResponse = serde_json::from_str(&body)
            .map_err(|err| TransportError::Io(format!("malformed /api/ps: {err}")))?;
        Ok(parsed.models.into_iter().next().map(|model| model.name))
    }

    /// Non-streaming chat completion.
    pub fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: Value,
    ) -> Result<crate::types::ChatStreamEvent, TransportError> {
        let body = json!({
            "model": model,
            "messages": messages,
            "stream": false,
            "options": options,
        });
        let text = self.transport.post_json("/api/chat", &body)?;
        serde_json::from_str(&text)
            .map_err(|err| TransportError::Io(format!("malformed /api/chat: {err}")))
    }

    /// Begin a streaming chat (NDJSON reader).
    pub fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: Value,
    ) -> Result<Box<dyn std::io::BufRead + Send>, TransportError> {
        let body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "options": options,
        });
        self.transport.post_json_stream("/api/chat", &body)
    }
}
