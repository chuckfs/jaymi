//! Model identity and metadata (vendor-neutral).

use serde::{Deserialize, Serialize};

/// Default tokens reserved for the completion when a model window is known.
pub const DEFAULT_RESERVED_COMPLETION_TOKENS: u64 = 1_024;

/// Stable identifier for a reasoning model.
///
/// `provider` is a logical backend id chosen by Jaymi registration (not a
/// wire-protocol name). `name` is the model label within that backend.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelIdentifier {
    /// Logical reasoning-backend id (registration key).
    pub provider: String,
    /// Model name within the backend.
    pub name: String,
    /// Optional revision / variant tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

impl ModelIdentifier {
    /// Create an identifier without a revision.
    pub fn new(provider: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            name: name.into(),
            revision: None,
        }
    }

    /// Attach a revision tag.
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    /// Compact display form: `provider/name` or `provider/name@revision`.
    pub fn display(&self) -> String {
        match &self.revision {
            Some(revision) => format!("{}/{}@{revision}", self.provider, self.name),
            None => format!("{}/{}", self.provider, self.name),
        }
    }
}

impl std::fmt::Display for ModelIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

/// Model context / generation limits exposed by a [`crate::ReasoningProvider`].
///
/// PromptBuilder derives [`crate::PromptBudget`] from these limits automatically.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelLimits {
    /// Full context window in tokens when known (prompt + completion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    /// Provider-reported max output / completion tokens when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

impl ModelLimits {
    /// Limits with a known context window.
    pub fn new(context_tokens: u64) -> Self {
        Self {
            context_tokens: Some(context_tokens),
            max_output_tokens: None,
        }
    }

    /// Unknown / unbounded limits (PromptBuilder falls back to defaults).
    pub fn unknown() -> Self {
        Self::default()
    }

    /// Attach max output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Tokens to reserve for completion (provider max_output, else default).
    pub fn reserved_completion_tokens(&self) -> u64 {
        self.max_output_tokens
            .unwrap_or(DEFAULT_RESERVED_COMPLETION_TOKENS)
            .max(1)
    }

    /// Prompt token budget: `context − reserved_completion` when the window is known.
    pub fn prompt_token_budget(&self, reserved_completion: u64) -> Option<u64> {
        self.context_tokens
            .map(|window| window.saturating_sub(reserved_completion).max(1))
    }
}

/// Vendor-neutral per-model capability flags (Settings / diagnostics).
///
/// Providers populate these during discovery. The UI never invents them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelCapabilityFlags {
    /// Can produce chat / completion replies.
    pub completion: bool,
    /// Supports extended thinking / reasoning traces.
    pub thinking: bool,
    /// Supports tool / function calling.
    pub tools: bool,
    /// Accepts image / multimodal inputs.
    pub vision: bool,
    /// Embedding / vector model (not chat).
    pub embeddings: bool,
}

impl ModelCapabilityFlags {
    /// Chat completion only (typical default for local LLMs).
    pub fn completion_only() -> Self {
        Self {
            completion: true,
            ..Self::default()
        }
    }

    /// Embedding models.
    pub fn embeddings_only() -> Self {
        Self {
            embeddings: true,
            ..Self::default()
        }
    }

    /// Short labels for Settings chips (stable order).
    pub fn labels(self) -> Vec<&'static str> {
        let mut labels = Vec::new();
        if self.completion {
            labels.push("Completion");
        }
        if self.thinking {
            labels.push("Thinking");
        }
        if self.tools {
            labels.push("Tools");
        }
        if self.vision {
            labels.push("Vision");
        }
        if self.embeddings {
            labels.push("Embeddings");
        }
        labels
    }
}

/// Descriptive metadata for a discoverable model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningModelInfo {
    /// Stable model identity.
    pub id: ModelIdentifier,
    /// Human-readable display name.
    pub display_name: String,
    /// Optional family label (e.g. chat, code, embed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Context window size in tokens when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    /// Max output / completion tokens when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Parameter count label when known (e.g. `7B`, `3.2B`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_count: Option<String>,
    /// Quantization / weight format when known (e.g. `Q4_K_M`, `fp16`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    /// Whether the model supports streaming token delivery.
    pub supports_streaming: bool,
    /// Whether the model is suitable for local / offline use.
    pub local: bool,
    /// Capability flags for Settings / selection UX.
    #[serde(default)]
    pub capabilities: ModelCapabilityFlags,
    /// Free-form notes for diagnostics (never wire payloads).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl ReasoningModelInfo {
    /// Minimal model metadata.
    pub fn new(id: ModelIdentifier, display_name: impl Into<String>) -> Self {
        Self {
            id,
            display_name: display_name.into(),
            family: None,
            context_tokens: None,
            max_output_tokens: None,
            parameter_count: None,
            quantization: None,
            supports_streaming: false,
            local: true,
            capabilities: ModelCapabilityFlags::completion_only(),
            notes: Vec::new(),
        }
    }

    /// Attach a context window size.
    pub fn with_context_tokens(mut self, context_tokens: u64) -> Self {
        self.context_tokens = Some(context_tokens);
        self
    }

    /// Attach max output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Attach parameter count label.
    pub fn with_parameter_count(mut self, parameter_count: impl Into<String>) -> Self {
        self.parameter_count = Some(parameter_count.into());
        self
    }

    /// Attach quantization label.
    pub fn with_quantization(mut self, quantization: impl Into<String>) -> Self {
        self.quantization = Some(quantization.into());
        self
    }

    /// Attach capability flags.
    pub fn with_capabilities(mut self, capabilities: ModelCapabilityFlags) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Provider-independent limits view for PromptBuilder.
    pub fn limits(&self) -> ModelLimits {
        ModelLimits {
            context_tokens: self.context_tokens,
            max_output_tokens: self.max_output_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats_with_and_without_revision() {
        let base = ModelIdentifier::new("backend", "chat");
        assert_eq!(base.display(), "backend/chat");
        let rev = base.clone().with_revision("1");
        assert_eq!(rev.display(), "backend/chat@1");
    }

    #[test]
    fn limits_compute_prompt_budget() {
        let limits = ModelLimits::new(8_192).with_max_output_tokens(2_048);
        assert_eq!(limits.prompt_token_budget(2_048), Some(6_144));
        assert_eq!(limits.reserved_completion_tokens(), 2_048);
    }
}
