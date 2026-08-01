//! Tool framework and Tool Orchestrator for Jaymi.
//!
//! Tools are executable building blocks. They do not make decisions, reason,
//! or remember. The Planner decides; the Tool performs.

#![forbid(unsafe_code)]

pub mod categories;
pub mod metadata;
pub mod orchestrator;
pub mod tool;

pub use metadata::ToolMetadata;
pub use orchestrator::ToolOrchestrator;
pub use tool::{Tool, ToolInput, ToolOutput};
