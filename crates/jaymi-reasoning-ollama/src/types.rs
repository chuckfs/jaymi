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
