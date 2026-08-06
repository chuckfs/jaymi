//! Wire types for Ollama responses (kept inside this crate).

use serde::{Deserialize, Serialize};

/// Chat message sent to `/api/chat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role (`system` / `user` / `assistant` / `tool`).
    pub role: String,
    /// Message content.
    pub content: String,
}

impl ChatMessage {
    /// User message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    /// Assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }

    /// System message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
}

/// One model entry from `/api/tags`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OllamaModelTag {
    /// Model name (e.g. `llama3.2:latest`).
    pub name: String,
    /// Optional digest / model key.
    #[serde(default)]
    pub model: Option<String>,
    /// Optional size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// Optional family from details.
    #[serde(default)]
    pub details: Option<OllamaModelDetails>,
}

/// Nested details on a tag entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OllamaModelDetails {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub parameter_size: Option<String>,
    /// Quantization label from Ollama (e.g. `Q4_K_M`).
    #[serde(default)]
    pub quantization_level: Option<String>,
    /// Weight format when advertised (e.g. `gguf`).
    #[serde(default)]
    pub format: Option<String>,
}

/// Response from `/api/show` (subset used for Settings metadata).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OllamaShowResponse {
    #[serde(default)]
    pub details: Option<OllamaModelDetails>,
    /// Model capability strings (e.g. `completion`, `tools`, `vision`).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Optional model_info map (context length often lives here).
    #[serde(default)]
    pub model_info: serde_json::Map<String, serde_json::Value>,
}

impl OllamaShowResponse {
    /// Best-effort context length from model_info keys.
    pub fn context_length(&self) -> Option<u64> {
        for (key, value) in &self.model_info {
            let lower = key.to_ascii_lowercase();
            if lower.ends_with(".context_length") || lower == "context_length" {
                if let Some(n) = value.as_u64() {
                    return Some(n);
                }
                if let Some(n) = value.as_i64() {
                    return Some(n.max(0) as u64);
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TagsResponse {
    #[serde(default)]
    pub models: Vec<OllamaModelTag>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VersionResponse {
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PsResponse {
    #[serde(default)]
    pub models: Vec<PsModel>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PsModel {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamEvent {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub message: Option<ChatStreamMessage>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub done_reason: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub prompt_eval_count: Option<u64>,
    #[serde(default)]
    pub eval_count: Option<u64>,
    #[serde(default)]
    pub total_duration: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamMessage {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
}
