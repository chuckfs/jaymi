//! [`OllamaReasoningProvider`] — first concrete ReasoningProvider.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use jaymi_reasoning::{
    ModelCapabilityFlags, ModelIdentifier, ReasoningCapabilities, ReasoningError, ReasoningHealth,
    ReasoningModelInfo, ReasoningProvider, ReasoningRequest, ReasoningResponse, ReasoningResult,
    ReasoningStream,
};
use serde_json::{json, Value};

use crate::client::{OllamaClient, OllamaClientConfig};
use crate::diagnostics::{OllamaDiagnostics, StreamingStatus};
use crate::messages::messages_from_request;
use crate::stream::{OllamaReasoningStream, SharedDiagnostics};
use crate::transport::{
    HttpOllamaTransport, OllamaTransport, TransportError, DEFAULT_OLLAMA_BASE_URL,
};

/// Stable registration id for the Ollama reasoning backend.
pub const OLLAMA_PROVIDER_ID: &str = "ollama";

/// Reasoning backend backed by a local Ollama server.
pub struct OllamaReasoningProvider {
    client: OllamaClient,
    diagnostics: SharedDiagnostics,
}

impl OllamaReasoningProvider {
    /// Live provider against the default local Ollama endpoint.
    pub fn local() -> Self {
        Self::with_client(OllamaClient::local())
    }

    /// Live provider for a custom base URL.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        let config = OllamaClientConfig {
            base_url: base_url.clone(),
            default_model: None,
        };
        let transport: Arc<dyn OllamaTransport> =
            Arc::new(HttpOllamaTransport::new(base_url));
        Self::with_client(OllamaClient::with_transport(transport, config))
    }

    /// Provider with an injected client (tests / custom transport).
    pub fn with_client(client: OllamaClient) -> Self {
        Self {
            client,
            diagnostics: Arc::new(Mutex::new(OllamaDiagnostics::default())),
        }
    }

    /// Convenience: mock / custom transport + optional default model.
    pub fn with_transport(
        transport: Arc<dyn OllamaTransport>,
        default_model: Option<String>,
    ) -> Self {
        let config = OllamaClientConfig {
            base_url: DEFAULT_OLLAMA_BASE_URL.into(),
            default_model,
        };
        Self::with_client(OllamaClient::with_transport(transport, config))
    }

    /// Snapshot diagnostics (connected, version, models, latency, streaming).
    pub fn diagnostics(&self) -> OllamaDiagnostics {
        self.refresh_health_cache();
        self.diagnostics.lock().expect("diagnostics").clone()
    }

    /// Last cached diagnostics without re-probing.
    pub fn diagnostics_cached(&self) -> OllamaDiagnostics {
        self.diagnostics.lock().expect("diagnostics").clone()
    }

    /// Set a default model name used when the request omits one.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        let mut config = self.client.config().clone();
        config.default_model = Some(model.into());
        self.client = OllamaClient::with_transport(self.client.transport(), config);
        self
    }

    fn refresh_health_cache(&self) {
        let _ = self.probe_health();
    }

    fn probe_health(&self) -> ReasoningHealth {
        match self.client.version() {
            Ok((version, latency_ms)) => {
                let models = self
                    .client
                    .list_tags()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tag| tag.name)
                    .collect::<Vec<_>>();
                let loaded = self.client.loaded_model().ok().flatten();
                {
                    let mut state = self.diagnostics.lock().expect("diagnostics");
                    state.connected = true;
                    state.provider_version = Some(version);
                    state.installed_models = models;
                    state.loaded_model = loaded.or(state.loaded_model.clone());
                    state.latency_ms = Some(latency_ms);
                    if state.detail.is_none() {
                        state.detail = Some("ready".into());
                    }
                }
                ReasoningHealth::Ready
            }
            Err(err) => {
                {
                    let mut state = self.diagnostics.lock().expect("diagnostics");
                    state.connected = false;
                    state.detail = Some(humanize_unreachable(&err));
                }
                ReasoningHealth::Unavailable {
                    reason: humanize_unreachable(&err),
                }
            }
        }
    }

    fn resolve_model(&self, request: &ReasoningRequest) -> ReasoningResult<ModelIdentifier> {
        if let Some(model) = &request.model {
            return Ok(model.clone());
        }
        if let Some(name) = &self.client.config().default_model {
            return Ok(ModelIdentifier::new(OLLAMA_PROVIDER_ID, name.clone()));
        }
        let tags = self.client.list_tags().map_err(map_transport)?;
        let name = tags
            .first()
            .map(|tag| tag.name.clone())
            .ok_or_else(|| ReasoningError::Unavailable {
                reason: "no models installed in ollama".into(),
            })?;
        Ok(ModelIdentifier::new(OLLAMA_PROVIDER_ID, name))
    }

    fn ensure_model_available(&self, model: &ModelIdentifier) -> ReasoningResult<()> {
        let tags = self.client.list_tags().map_err(map_transport)?;
        let found = tags.iter().any(|tag| {
            tag.name == model.name
                || tag
                    .model
                    .as_deref()
                    .is_some_and(|alias| alias == model.name)
        });
        if found {
            Ok(())
        } else {
            Err(ReasoningError::ModelNotFound {
                model: model.name.clone(),
            })
        }
    }

    fn generation_options(request: &ReasoningRequest) -> Value {
        let mut options = serde_json::Map::new();
        if let Some(temperature) = request.parameters.temperature {
            options.insert("temperature".into(), json!(temperature));
        }
        if let Some(top_p) = request.parameters.top_p {
            options.insert("top_p".into(), json!(top_p));
        }
        if let Some(seed) = request.parameters.seed {
            options.insert("seed".into(), json!(seed));
        }
        if let Some(max) = request.parameters.max_output_tokens {
            options.insert("num_predict".into(), json!(max));
        }
        Value::Object(options)
    }

    fn model_info_from_tag(&self, tag: crate::types::OllamaModelTag) -> ReasoningModelInfo {
        let id = ModelIdentifier::new(OLLAMA_PROVIDER_ID, tag.name.clone());
        let mut info = ReasoningModelInfo::new(id, tag.name.clone());
        info.supports_streaming = true;
        info.local = true;
        let details = tag.details.as_ref();
        info.family = details.and_then(|details| details.family.clone());
        let parameter_size = details.and_then(|details| details.parameter_size.clone());
        info.parameter_count = parameter_size.clone();
        info.quantization = details
            .and_then(|details| details.quantization_level.clone())
            .or_else(|| infer_quantization_from_name(&tag.name));
        let parameter_size_ref = parameter_size.as_deref();
        if let Some(window) =
            infer_context_tokens(&tag.name, info.family.as_deref(), parameter_size_ref)
        {
            info.context_tokens = Some(window);
        }
        if let Some(size) = tag.size {
            info.notes.push(format!("size_bytes={size}"));
        }
        if let Some(format) = details.and_then(|details| details.format.as_deref()) {
            info.notes.push(format!("format={format}"));
        }
        info.capabilities = infer_capabilities_from_name(&tag.name, info.family.as_deref());
        // `/api/show` enrichment is available via `OllamaClient::show_model` for
        // targeted probes; list_models stays tags-based so registry refresh
        // cannot stall on per-model show round-trips.
        info
    }
}

impl ReasoningProvider for OllamaReasoningProvider {
    fn id(&self) -> &str {
        OLLAMA_PROVIDER_ID
    }

    fn display_name(&self) -> &str {
        "Ollama"
    }

    fn capabilities(&self) -> ReasoningCapabilities {
        ReasoningCapabilities {
            complete: true,
            stream: true,
            cancellation: true,
            list_models: true,
            health: true,
            multi_turn: true,
            // Context reaches the model via PromptBuilder → request.prompt.
            structured_context: true,
            assembled_prompt: true,
        }
    }

    fn health(&self) -> ReasoningHealth {
        self.probe_health()
    }

    fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
        let tags = self.client.list_tags().map_err(map_transport)?;
        Ok(tags
            .into_iter()
            .map(|tag| self.model_info_from_tag(tag))
            .collect())
    }

    fn complete(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        let model = self.resolve_model(&request)?;
        self.ensure_model_available(&model)?;
        let messages = messages_from_request(&request)?;
        if messages.is_empty() {
            return Err(ReasoningError::InvalidRequest {
                reason: "assembled prompt produced no chat messages".into(),
            });
        }
        let options = Self::generation_options(&request);
        let started = Instant::now();
        {
            let mut state = self.diagnostics.lock().expect("diagnostics");
            state.streaming_status = StreamingStatus::Idle;
            state.detail = Some("complete".into());
        }
        let event = self
            .client
            .chat(&model.name, &messages, options)
            .map_err(|err| map_chat_error(err, &model.name))?;
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        if let Some(error) = event.error {
            return Err(ReasoningError::GenerationFailed { reason: error });
        }
        let content = event
            .message
            .as_ref()
            .and_then(|message| message.content.clone())
            .unwrap_or_default();
        let latency_ms = started.elapsed().as_millis() as u64;
        let metrics = jaymi_reasoning::ReasoningMetrics::timed(latency_ms)
            .with_tokens(event.prompt_eval_count, event.eval_count)
            .with_model(model.clone());
        {
            let mut state = self.diagnostics.lock().expect("diagnostics");
            state.connected = true;
            state.latency_ms = Some(latency_ms);
            state.loaded_model = Some(model.name.clone());
            state.streaming_status = StreamingStatus::Completed;
            state.detail = Some("complete_ok".into());
        }
        let finish = match event.done_reason.as_deref() {
            Some("length") => jaymi_reasoning::FinishReason::Length,
            _ => jaymi_reasoning::FinishReason::Completed,
        };
        Ok(ReasoningResponse::completed(content)
            .with_model(model)
            .with_metrics(metrics)
            .with_finish_reason(finish))
    }

    fn stream(&self, request: ReasoningRequest) -> ReasoningResult<Box<dyn ReasoningStream>> {
        if request.is_cancelled() {
            return Err(ReasoningError::Cancelled);
        }
        let model = self.resolve_model(&request)?;
        self.ensure_model_available(&model)?;
        let messages = messages_from_request(&request)?;
        if messages.is_empty() {
            return Err(ReasoningError::InvalidRequest {
                reason: "assembled prompt produced no chat messages".into(),
            });
        }
        let options = Self::generation_options(&request);
        let reader = self
            .client
            .chat_stream(&model.name, &messages, options)
            .map_err(|err| map_chat_error(err, &model.name))?;
        Ok(Box::new(OllamaReasoningStream::new(
            reader,
            request.cancellation.clone(),
            model,
            Arc::clone(&self.diagnostics),
        )))
    }
}

fn humanize_unreachable(err: &TransportError) -> String {
    let raw = err.to_string();
    let lower = raw.to_ascii_lowercase();
    if lower.contains("connection refused")
        || lower.contains("couldn't connect")
        || lower.contains("could not connect")
        || lower.contains("failed to connect")
    {
        "Ollama isn’t running. Start Ollama to use local models (default http://127.0.0.1:11434)."
            .into()
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "Ollama did not respond in time. Check that the app is running and try again.".into()
    } else {
        format!("Can’t reach Ollama ({raw})")
    }
}

fn is_embedding_name(name: &str, family: Option<&str>) -> bool {
    let hay = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        family.unwrap_or("").to_ascii_lowercase()
    );
    hay.contains("embed") || hay.contains("nomic-embed") || hay.contains("bge-")
}

fn infer_capabilities_from_name(name: &str, family: Option<&str>) -> ModelCapabilityFlags {
    if is_embedding_name(name, family) {
        return ModelCapabilityFlags::embeddings_only();
    }
    let mut flags = ModelCapabilityFlags::completion_only();
    let hay = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        family.unwrap_or("").to_ascii_lowercase()
    );
    if hay.contains("vision") || hay.contains("llava") || hay.contains("minicpm-v") {
        flags.vision = true;
    }
    if hay.contains("think") || hay.contains("reason") || hay.contains("r1") {
        flags.thinking = true;
    }
    flags
}

#[allow(dead_code)] // Used when /api/show enrichment is enabled; covered by unit tests.
fn capabilities_from_show(capabilities: &[String]) -> ModelCapabilityFlags {
    let mut flags = ModelCapabilityFlags::default();
    for cap in capabilities {
        match cap.trim().to_ascii_lowercase().as_str() {
            "completion" | "chat" | "generate" => flags.completion = true,
            "thinking" | "reasoning" => flags.thinking = true,
            "tools" | "tool" | "function_call" | "function_calling" => flags.tools = true,
            "vision" | "image" | "multimodal" => flags.vision = true,
            "embedding" | "embeddings" => flags.embeddings = true,
            _ => {}
        }
    }
    if flags.embeddings && !flags.completion && !flags.vision && !flags.tools && !flags.thinking {
        return flags;
    }
    if !flags.embeddings && !flags.completion && (flags.thinking || flags.tools || flags.vision) {
        flags.completion = true;
    }
    flags
}

#[cfg(test)]
mod capability_mapping_tests {
    use super::*;

    #[test]
    fn show_capabilities_map_to_flags() {
        let flags = capabilities_from_show(&[
            "completion".into(),
            "tools".into(),
            "vision".into(),
        ]);
        assert!(flags.completion);
        assert!(flags.tools);
        assert!(flags.vision);
        assert!(!flags.embeddings);
    }

    #[test]
    fn embedding_name_inferred() {
        let flags = infer_capabilities_from_name("nomic-embed-text", Some("bert"));
        assert!(flags.embeddings);
        assert!(!flags.completion);
    }
}

fn infer_quantization_from_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    // Common Ollama tag suffixes: `:q4_k_m`, `:q8_0`, `:fp16`, …
    for (needle, label) in [
        ("q8_0", "Q8_0"),
        ("q8_1", "Q8_1"),
        ("q6_k", "Q6_K"),
        ("q5_k_m", "Q5_K_M"),
        ("q5_k_s", "Q5_K_S"),
        ("q5_0", "Q5_0"),
        ("q5_1", "Q5_1"),
        ("q4_k_m", "Q4_K_M"),
        ("q4_k_s", "Q4_K_S"),
        ("q4_0", "Q4_0"),
        ("q4_1", "Q4_1"),
        ("q3_k_m", "Q3_K_M"),
        ("q3_k_s", "Q3_K_S"),
        ("q3_k_l", "Q3_K_L"),
        ("q2_k", "Q2_K"),
        ("fp16", "FP16"),
        ("f16", "F16"),
        ("fp32", "FP32"),
        ("f32", "F32"),
    ] {
        if lower.contains(needle) {
            return Some(label.into());
        }
    }
    None
}

fn infer_context_tokens(
    name: &str,
    family: Option<&str>,
    parameter_size: Option<&str>,
) -> Option<u64> {
    let haystack = format!(
        "{} {} {}",
        name.to_ascii_lowercase(),
        family.unwrap_or("").to_ascii_lowercase(),
        parameter_size.unwrap_or("").to_ascii_lowercase()
    );
    // Explicit window tags in the model name.
    if haystack.contains("1m") || haystack.contains("1000k") {
        return Some(1_048_576);
    }
    if haystack.contains("256k") {
        return Some(262_144);
    }
    if haystack.contains("128k") || haystack.contains("longcontext") || haystack.contains("long-context")
    {
        return Some(131_072);
    }
    if haystack.contains("64k") {
        return Some(65_536);
    }
    if haystack.contains("32k") {
        return Some(32_768);
    }
    if haystack.contains("16k") {
        return Some(16_384);
    }
    if haystack.contains("8k") {
        return Some(8_192);
    }
    // Known families / generations (conservative when ambiguous).
    if haystack.contains("llama3.1")
        || haystack.contains("llama3.2")
        || haystack.contains("llama3.3")
        || haystack.contains("llama4")
    {
        return Some(131_072);
    }
    if haystack.contains("llama3") || haystack.contains("gemma2") || haystack.contains("gemma3") {
        return Some(8_192);
    }
    if haystack.contains("mistral") || haystack.contains("mixtral") || haystack.contains("qwen") {
        return Some(32_768);
    }
    if haystack.contains("phi") || haystack.contains("tinyllama") {
        return Some(4_096);
    }
    // Local Ollama default when the window is not advertised.
    Some(8_192)
}

fn map_transport(err: TransportError) -> ReasoningError {
    match err {
        TransportError::Unavailable(reason) => ReasoningError::Unavailable { reason },
        TransportError::HttpStatus { status, body } => ReasoningError::Unavailable {
            reason: format!("http {status}: {body}"),
        },
        TransportError::Io(reason) => ReasoningError::StreamFailed { reason },
    }
}

fn map_chat_error(err: TransportError, model: &str) -> ReasoningError {
    match err {
        TransportError::Unavailable(reason) => ReasoningError::Unavailable { reason },
        TransportError::HttpStatus { status, body } => {
            let lower = body.to_ascii_lowercase();
            if status == 404
                || lower.contains("not found")
                || (lower.contains("model") && lower.contains("not found"))
            {
                ReasoningError::ModelNotFound {
                    model: model.to_string(),
                }
            } else {
                ReasoningError::GenerationFailed {
                    reason: format!("http {status}: {body}"),
                }
            }
        }
        TransportError::Io(reason) => {
            if reason.contains("malformed") {
                ReasoningError::StreamFailed { reason }
            } else {
                ReasoningError::GenerationFailed { reason }
            }
        }
    }
}
