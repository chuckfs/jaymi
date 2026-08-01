//! Tool metadata used by the Planner for selection.
//!
//! The Planner should never contain provider-specific rules. Every Tool
//! describes itself well enough that decisions emerge from metadata.

use jaymi_capabilities::Capability;

/// Complete metadata surface for a Tool.
#[derive(Debug, Clone)]
pub struct ToolMetadata {
    /// Stable tool identifier.
    pub id: String,
    /// Human-readable tool name.
    pub name: String,
    /// Tool version string.
    pub version: String,
    /// Short description of what the tool does.
    pub description: String,
    /// Provider ID that executes this tool.
    pub provider: String,
    /// Capabilities satisfied by this tool.
    pub capabilities: Vec<Capability>,
    /// How the tool behaves during execution.
    pub execution_mode: ExecutionMode,
    /// Approximate execution time class.
    pub estimated_runtime: EstimatedRuntime,
    /// Relative computational expense.
    pub resource_cost: ResourceCost,
    /// Estimated memory requirements.
    pub memory_usage: MemoryUsage,
    /// GPU needs for execution.
    pub gpu_requirements: GpuRequirements,
    /// Where execution occurs.
    pub privacy: PrivacyMode,
    /// Internet dependency.
    pub internet: InternetRequirement,
    /// Expected reliability tier.
    pub reliability: Reliability,
    /// Type of data returned by the tool.
    pub result_type: ResultType,
}

/// How the Tool behaves during execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Blocks until completion.
    Synchronous,
    /// Returns immediately and completes later.
    Asynchronous,
    /// Streams partial results.
    Streaming,
}

/// Approximate execution time class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimatedRuntime {
    /// Under 100 ms.
    Instant,
    /// Under 1 second.
    Fast,
    /// Under 10 seconds.
    Medium,
    /// Over 10 seconds.
    Slow,
}

/// Relative computational expense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCost {
    /// Negligible cost.
    VeryLow,
    /// Lightweight cost.
    Low,
    /// Moderate cost.
    Medium,
    /// Expensive cost.
    High,
    /// Extremely expensive cost.
    VeryHigh,
}

/// Estimated memory requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryUsage {
    /// Tiny memory footprint.
    Tiny,
    /// Small memory footprint.
    Small,
    /// Moderate memory footprint.
    Moderate,
    /// Large memory footprint.
    Large,
    /// Extreme memory footprint.
    Extreme,
}

/// GPU needs for execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuRequirements {
    /// No GPU involvement.
    None,
    /// GPU optional.
    Optional,
    /// GPU recommended.
    Recommended,
    /// GPU required.
    Required,
}

/// Where execution occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyMode {
    /// Executes only on the local device.
    LocalOnly,
    /// Executes only in the cloud.
    CloudOnly,
    /// Prefers local execution.
    LocalPreferred,
    /// Cloud execution is optional.
    CloudOptional,
    /// Mixes local and cloud execution.
    Hybrid,
}

/// Internet dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternetRequirement {
    /// Never uses the network.
    Never,
    /// Network optional.
    Optional,
    /// Network required.
    Required,
}

/// Expected reliability tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Experimental quality.
    Experimental,
    /// Stable quality.
    Stable,
    /// Production quality.
    Production,
}

/// Type of data returned by the Tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultType {
    /// Plain text.
    Text,
    /// Image bytes or path.
    Image,
    /// Structured records.
    StructuredData,
    /// File reference.
    File,
    /// Streaming payload.
    Stream,
    /// Text or binary diff.
    Diff,
    /// Search / listing results.
    SearchResults,
}
