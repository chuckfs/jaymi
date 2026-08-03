//! Permission Engine for Jaymi.
//!
//! Fifth subsystem in the deterministic boot sequence.
//! Permissions determine whether Jaymi may perform an action.

#![forbid(unsafe_code)]

pub mod categories;
pub mod scope;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

pub use categories::{PermissionAction, PermissionCategory};
pub use scope::PermissionScope;

const NAME: &str = "permission_engine";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database", "policy_engine"];

/// A request for authorization before a protected action.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// Category of the requested action.
    pub category: PermissionCategory,
    /// Specific action class within the category.
    pub action: PermissionAction,
    /// Scope requested for the grant.
    pub scope: PermissionScope,
    /// Plain-language explanation of the action.
    pub explanation: String,
    /// Optional resource path or identifier.
    pub resource: Option<String>,
}

/// Possible outcomes of a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Action may proceed.
    Allowed,
    /// Action must not proceed.
    Denied,
    /// User approval is required before proceeding.
    RequiresApproval,
}

impl PermissionDecision {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::RequiresApproval => "requires_approval",
        }
    }

    /// Whether execution may proceed without an approval UI.
    pub fn allows_execution(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Structured permission check result returned to the Planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionCheckResult {
    /// Final decision.
    pub decision: PermissionDecision,
    /// Explanation associated with the check.
    pub explanation: String,
    /// Category that was evaluated.
    pub category: PermissionCategory,
    /// Action that was evaluated.
    pub action: PermissionAction,
    /// Optional resource involved.
    pub resource: Option<String>,
}

impl PermissionCheckResult {
    /// True when the Planner may execute the tool.
    pub fn allows_execution(&self) -> bool {
        self.decision.allows_execution()
    }
}

/// Permission Engine lifecycle.
#[derive(Debug, Default)]
pub struct PermissionEngine {
    initialized: bool,
}

impl PermissionEngine {
    /// Create an uninitialized permission engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true after initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Slice 0.4+: local filesystem read/write and terminal execute are allowed.
    /// Network and other actions remain denied / approval-gated.
    pub fn check(&self, request: &PermissionRequest) -> JaymiResult<PermissionCheckResult> {
        self.ensure_initialized()?;

        let decision = match (request.category, request.action) {
            (PermissionCategory::Filesystem, PermissionAction::Read)
            | (PermissionCategory::Filesystem, PermissionAction::Write) => {
                PermissionDecision::Allowed
            }
            (PermissionCategory::Filesystem, _) => PermissionDecision::Denied,
            (PermissionCategory::Terminal, PermissionAction::Execute) => {
                PermissionDecision::Allowed
            }
            (PermissionCategory::Terminal, _) => PermissionDecision::Denied,
            (PermissionCategory::Internet, _) => PermissionDecision::Denied,
            (PermissionCategory::Communication, _) => PermissionDecision::RequiresApproval,
            (PermissionCategory::System, _) => PermissionDecision::RequiresApproval,
            (PermissionCategory::AiProviders, _) => PermissionDecision::RequiresApproval,
        };

        Ok(PermissionCheckResult {
            decision,
            explanation: request.explanation.clone(),
            category: request.category,
            action: request.action,
            resource: request.resource.clone(),
        })
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("permission engine is not initialized"))
        }
    }
}

impl Lifecycle for PermissionEngine {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            (
                "local_filesystem".to_string(),
                "read_write_auto_allowed".to_string(),
            ),
            (
                "local_terminal".to_string(),
                "execute_auto_allowed".to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_request() -> PermissionRequest {
        PermissionRequest {
            category: PermissionCategory::Filesystem,
            action: PermissionAction::Read,
            scope: PermissionScope::Once,
            explanation: "List a directory".into(),
            resource: Some("/tmp".into()),
        }
    }

    #[test]
    fn lifecycle_health_requires_initialize() {
        let mut engine = PermissionEngine::new();
        assert!(!engine.health_check().healthy);
        engine.initialize().unwrap();
        assert!(engine.health_check().healthy);
    }

    #[test]
    fn allows_filesystem_read() {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        let result = engine.check(&read_request()).unwrap();
        assert_eq!(result.decision, PermissionDecision::Allowed);
        assert!(result.allows_execution());
    }

    #[test]
    fn allows_filesystem_write() {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        let result = engine
            .check(&PermissionRequest {
                category: PermissionCategory::Filesystem,
                action: PermissionAction::Write,
                scope: PermissionScope::Once,
                explanation: "Write a file".into(),
                resource: Some("/tmp/a.txt".into()),
            })
            .unwrap();
        assert_eq!(result.decision, PermissionDecision::Allowed);
        assert!(result.allows_execution());
    }

    #[test]
    fn allows_terminal_execute() {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        let result = engine
            .check(&PermissionRequest {
                category: PermissionCategory::Terminal,
                action: PermissionAction::Execute,
                scope: PermissionScope::Once,
                explanation: "Run a shell command".into(),
                resource: Some("pwd".into()),
            })
            .unwrap();
        assert_eq!(result.decision, PermissionDecision::Allowed);
        assert!(result.allows_execution());
    }

    #[test]
    fn denies_internet_actions() {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        let result = engine
            .check(&PermissionRequest {
                category: PermissionCategory::Internet,
                action: PermissionAction::Network,
                scope: PermissionScope::Once,
                explanation: "Call an API".into(),
                resource: None,
            })
            .unwrap();
        assert_eq!(result.decision, PermissionDecision::Denied);
    }
}
