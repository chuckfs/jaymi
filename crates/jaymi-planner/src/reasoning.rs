//! Reasoning Engine — re-exported from `jaymi-reasoning`.
//!
//! The orchestration implementation lives in [`jaymi_reasoning::ReasoningEngine`].
//! This module keeps the Planner's historical import path stable.

pub use jaymi_reasoning::{
    CancelReason, ConversationStream, ConversationStreamDiagnostics, ConversationStreamEvent,
    ReasoningEngine, ReasoningEngineConfig, StreamingLifecycle, StreamingResponse,
    DEFAULT_REASONING_TIMEOUT_MS,
};
