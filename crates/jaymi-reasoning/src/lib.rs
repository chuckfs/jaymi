//! Provider-independent Reasoning contracts and orchestration engine.
//!
//! Architectural path:
//!
//! ```text
//! LlmContext → PromptBuilder → ReasoningRequest(prompt) → ReasoningProvider
//!   → StreamingResponse → ReasoningResponse
//! ```
//!
//! [`ReasoningEngine`] owns timeouts, cancellation, metrics, provider selection,
//! retry, stream lifecycle, and attaching [`Prompt`] onto the request. Prompt
//! construction is delegated to [`PromptBuilder`]; providers consume that output
//! for transport only. Context assembly and Planner routing live elsewhere.
//!
//! Conversational product streaming uses [`ConversationStream`]
//! (Idle → Thinking → Streaming → Cancelled / Completed / Failed).
//!
//! **Sprint B1.1:** contracts. **B1.2:** Prompt Builder. **B1.3:** Ollama.
//! **B1.4:** Reasoning Engine orchestration. **B1.5:** conversational Planner path.
//! **B1.6:** streaming conversation lifecycle. **B1.7:** conversation state machine.
//! **B1.8:** model-aware LLM context budgeting. **B1.9:** Model Registry.
//! **B1.10:** Conversational Reasoning diagnostics. **B1.11:** Conversation UX polish.
//! **B1.13.1:** Prompt → Provider handoff. **B1.13.2:** ContextBundle section coverage.
//! **B1.13.3:** Multi-turn history (Experience → ReasoningRequest).

#![forbid(unsafe_code)]

mod cancellation;
mod conversation_stream;
mod diagnostics;
mod engine;
mod error;
mod lifecycle;
mod metrics;
mod model;
mod parameters;
mod pipeline;
mod prompt;
mod provider;
mod registry;
mod request;
mod response;
mod stream;
mod types;

pub use cancellation::{CancellationFlag, CancellationToken};
pub use conversation_stream::{
    ConversationStream, ConversationStreamDiagnostics, ConversationStreamEvent, StreamPumpPoll,
};
pub use diagnostics::{ReasoningDiagnosticsInput, ReasoningDiagnosticsReport};
pub use engine::{
    ChunkPoll, ReasoningEngine, ReasoningEngineConfig, StreamingResponse,
    DEFAULT_REASONING_TIMEOUT_MS,
};
pub use error::{ReasoningError, ReasoningResult};
pub use lifecycle::{CancelReason, StreamingLifecycle};
pub use metrics::ReasoningMetrics;
pub use model::{
    ModelCapabilityFlags, ModelIdentifier, ModelLimits, ReasoningModelInfo,
    DEFAULT_RESERVED_COMPLETION_TOKENS,
};
pub use pipeline::{PipelineStageTiming, PipelineTiming};
pub use registry::{
    ModelRegistry, ModelRegistrySnapshot, ProviderHealthEntry, RegisteredModel,
};
pub use parameters::GenerationParameters;
pub use prompt::{
    DefaultPromptTemplate, ModelPromptAdapter, NullPromptAdapter, PlainTextFormatter, Prompt,
    PromptBudget, PromptBudgetUsage, PromptBuilder, PromptChatMessage, PromptChatRole,
    PromptDiagnostics, PromptFormatter, PromptLlmCoverageEntry, PromptSection,
    PromptSectionContribution, PromptSectionDisposition, PromptSectionId, PromptTemplate,
    DEFAULT_PROMPT_MAX_CHARACTERS, PROMPT_SCHEMA_VERSION,
};
pub use provider::{ReasoningCapabilities, ReasoningHealth, ReasoningProvider, ReasoningStream};
pub use request::ReasoningRequest;
pub use response::{FinishReason, ReasoningResponse};
pub use stream::{StreamingChunk, StreamingChunkKind};
pub use types::{ConversationRole, ConversationTurn};
