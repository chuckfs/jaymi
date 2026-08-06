//! Tool metadata used by the Planner for selection.
//!
//! The Planner should never contain provider-specific rules. Every Tool
//! describes itself well enough that decisions emerge from metadata.

use jaymi_capabilities::Capability;

use crate::tool::ToolInput;

/// Risk classification declared by every Tool.
///
/// The Planner derives review requirements from this classification — not from
/// hardcoded permission approval rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolRisk {
    /// Read-only operations (indexes, inventory, pure retrieval).
    Safe,
    /// Window / workspace management: open files, switch projects, browse trees.
    Workspace,
    /// Edits local user data (writes, renames, non-destructive mutations).
    Modify,
    /// Deletes or permanently changes data (delete, discard, unconstrained shell).
    Destructive,
    /// Internet, APIs, email, cloud providers.
    External,
}

impl ToolRisk {
    /// Stable snake_case label for diagnostics and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Workspace => "workspace",
            Self::Modify => "modify",
            Self::Destructive => "destructive",
            Self::External => "external",
        }
    }

    /// True when conversational review must gate execution.
    pub fn requires_review(self) -> bool {
        matches!(self, Self::Modify | Self::Destructive | Self::External)
    }

    /// Escalate risk for a specific invocation (never de-escalates the base).
    ///
    /// Examples: `manage_path` delete → Destructive; git discard → Destructive;
    /// network-required tools → External.
    pub fn effective_for(self, input: &ToolInput, internet: InternetRequirement) -> Self {
        let mut risk = self;
        if matches!(input.command.as_deref(), Some("delete")) {
            risk = risk.escalate(Self::Destructive);
        }
        if let Some(op) = input.git_operation {
            if op.is_destructive() {
                risk = risk.escalate(Self::Destructive);
            } else if op.is_mutating() {
                risk = risk.escalate(Self::Modify);
            }
        }
        if input
            .lsp
            .as_ref()
            .is_some_and(|request| request.operation.is_mutating())
        {
            risk = risk.escalate(Self::Modify);
        }
        if matches!(internet, InternetRequirement::Required) {
            risk = risk.escalate(Self::External);
        }
        risk
    }

    fn escalate(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Safe => 0,
            Self::Workspace => 1,
            Self::Modify => 2,
            Self::Destructive => 3,
            Self::External => 4,
        }
    }
}

impl std::fmt::Display for ToolRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

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
    /// Declared risk classification (review derives from this).
    pub risk: ToolRisk,
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

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_core::GitOperation;

    #[test]
    fn review_required_for_modify_destructive_external() {
        assert!(!ToolRisk::Safe.requires_review());
        assert!(!ToolRisk::Workspace.requires_review());
        assert!(ToolRisk::Modify.requires_review());
        assert!(ToolRisk::Destructive.requires_review());
        assert!(ToolRisk::External.requires_review());
    }

    #[test]
    fn effective_risk_escalates_delete_and_discard() {
        let delete = ToolInput::manage_path("delete", "/tmp/a", None::<String>);
        assert_eq!(
            ToolRisk::Modify.effective_for(&delete, InternetRequirement::Never),
            ToolRisk::Destructive
        );
        let mut discard = ToolInput::default();
        discard.git_operation = Some(GitOperation::Discard);
        assert_eq!(
            ToolRisk::Workspace.effective_for(&discard, InternetRequirement::Never),
            ToolRisk::Destructive
        );
        let read = ToolInput::read_file("/tmp/a");
        assert_eq!(
            ToolRisk::Workspace.effective_for(&read, InternetRequirement::Required),
            ToolRisk::External
        );
    }
}
