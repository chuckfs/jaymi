//! Generation parameters (sampling / limits).

use serde::{Deserialize, Serialize};

/// Tunables for a reasoning generation.
///
/// Fields are optional so providers may apply their own defaults. Values are
/// semantic, not tied to any specific backend API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationParameters {
    /// Sampling temperature when supported (`None` = provider default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Nucleus sampling `top_p` when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Maximum tokens to generate when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sequences that should stop generation when emitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    /// Soft time budget in milliseconds when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Prefer deterministic / low-variance decoding when true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic: Option<bool>,
    /// Seed for reproducible sampling when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl Default for GenerationParameters {
    fn default() -> Self {
        Self {
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            stop_sequences: Vec::new(),
            timeout_ms: None,
            deterministic: None,
            seed: None,
        }
    }
}

impl GenerationParameters {
    /// Empty parameters (all provider defaults).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set max output tokens.
    pub fn with_max_output_tokens(mut self, max: u32) -> Self {
        self.max_output_tokens = Some(max);
        self
    }

    /// Prefer deterministic decoding.
    pub fn deterministic(mut self) -> Self {
        self.deterministic = Some(true);
        self
    }

    /// Soft time budget in milliseconds.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_set_optional_fields() {
        let params = GenerationParameters::new()
            .with_temperature(0.2)
            .with_max_output_tokens(256)
            .deterministic();
        assert_eq!(params.temperature, Some(0.2));
        assert_eq!(params.max_output_tokens, Some(256));
        assert_eq!(params.deterministic, Some(true));
    }
}
