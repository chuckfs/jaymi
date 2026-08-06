//! Provider diagnostics snapshot.

use serde::{Deserialize, Serialize};

/// Streaming lifecycle for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StreamingStatus {
    /// No active stream.
    #[default]
    Idle,
    /// Waiting for the first visible token (may include thoughts).
    Thinking,
    /// Tokens currently streaming.
    Streaming,
    /// Last stream completed successfully.
    Completed,
    /// Last stream was cancelled.
    Cancelled,
    /// Last stream failed.
    Failed,
}

impl StreamingStatus {
    /// Stable label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Streaming => "streaming",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Diagnostics exposed by [`super::OllamaReasoningProvider`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OllamaDiagnostics {
    /// True when the last health probe reached Ollama.
    pub connected: bool,
    /// Ollama server version when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    /// Installed model names from `/api/tags`.
    pub installed_models: Vec<String>,
    /// Currently loaded model from `/api/ps` (first entry) when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_model: Option<String>,
    /// Latency of the last successful health or generation probe (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Current / last streaming status.
    pub streaming_status: StreamingStatus,
    /// Optional detail for logs / diagnostics panels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Default for OllamaDiagnostics {
    fn default() -> Self {
        Self {
            connected: false,
            provider_version: None,
            installed_models: Vec::new(),
            loaded_model: None,
            latency_ms: None,
            streaming_status: StreamingStatus::Idle,
            detail: None,
        }
    }
}

impl OllamaDiagnostics {
    /// Compact single-line summary for subsystem diagnostics.
    pub fn summary_line(&self) -> String {
        format!(
            "connected={} · version={} · models={} · loaded={} · latency_ms={} · streaming={}",
            self.connected,
            self.provider_version.as_deref().unwrap_or("-"),
            self.installed_models.len(),
            self.loaded_model.as_deref().unwrap_or("-"),
            self.latency_ms
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| "-".into()),
            self.streaming_status.as_str(),
        )
    }
}
