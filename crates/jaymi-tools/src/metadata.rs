//! Tool metadata used by the Planner for selection.
//!
//! The Planner should never contain provider-specific rules. Every Tool
//! describes itself well enough that decisions emerge from metadata.

use jaymi_capabilities::Capability;

/// Complete metadata surface for a Tool.
#[derive(Debug, Clone)]
pub struct ToolMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub provider: String,
    pub capabilities: Vec<Capability>,
    pub execution_mode: ExecutionMode,
    pub estimated_runtime: EstimatedRuntime,
    pub resource_cost: ResourceCost,
    pub memory_usage: MemoryUsage,
    pub gpu_requirements: GpuRequirements,
    pub privacy: PrivacyMode,
    pub internet: InternetRequirement,
    pub reliability: Reliability,
    pub result_type: ResultType,
}

/// How the Tool behaves during execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Synchronous,
    Asynchronous,
    Streaming,
}

/// Approximate execution time class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimatedRuntime {
    Instant,
    Fast,
    Medium,
    Slow,
}

/// Relative computational expense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCost {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

/// Estimated memory requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryUsage {
    Tiny,
    Small,
    Moderate,
    Large,
    Extreme,
}

/// GPU needs for execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuRequirements {
    None,
    Optional,
    Recommended,
    Required,
}

/// Where execution occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyMode {
    LocalOnly,
    CloudOnly,
    LocalPreferred,
    CloudOptional,
    Hybrid,
}

/// Internet dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternetRequirement {
    Never,
    Optional,
    Required,
}

/// Expected reliability tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    Experimental,
    Stable,
    Production,
}

/// Type of data returned by the Tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultType {
    Text,
    Image,
    StructuredData,
    File,
    Stream,
    Diff,
    SearchResults,
}
