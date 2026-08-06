//! Ollama local ReasoningProvider for Jaymi.
//!
//! Implements [`jaymi_reasoning::ReasoningProvider`] against Ollama's local
//! HTTP API. Does **not** assemble prompts, call tools, plan, execute, or
//! touch memory / context assembly — those stay elsewhere.
//!
//! **Sprint B1.3**

#![forbid(unsafe_code)]

mod client;
mod diagnostics;
mod messages;
mod provider;
mod stream;
mod transport;
mod types;

pub use client::{OllamaClient, OllamaClientConfig};
pub use diagnostics::{OllamaDiagnostics, StreamingStatus};
pub use provider::{OllamaReasoningProvider, OLLAMA_PROVIDER_ID};
pub use transport::{
    HttpOllamaTransport, MockOllamaTransport, OllamaTransport, TransportError, DEFAULT_OLLAMA_BASE_URL,
};
pub use types::{ChatMessage, OllamaModelTag};
