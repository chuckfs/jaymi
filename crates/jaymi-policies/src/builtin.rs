//! Built-in policies that shape Planner behavior.

/// Named built-in policies described by the architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinPolicy {
    /// Default: prefer local execution, models, search, and memory.
    OfflineFirst,
    /// Prioritize quality regardless of execution time.
    HighestQuality,
    /// Prioritize speed and lightweight tools.
    FastestResponse,
    /// Never use cloud resources.
    PrivacyMaximum,
    /// Optimize for efficiency and avoid heavy workloads.
    BatterySaver,
    /// Optimize for software development workflows.
    DeveloperMode,
    /// Optimize for ideation and creative generation.
    CreativeMode,
    /// Optimize for information gathering and citations.
    ResearchMode,
}
