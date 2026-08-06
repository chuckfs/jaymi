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
// Permission checks are independent of Action Policy evaluation; the Planner
// sequences Policy → Permission at request time. Grants are in-memory today
// (no database peer until persisted permissions land).
const DEPENDENCIES: &[&str] = &["configuration", "logging"];

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
///
/// The Planner maps these directly onto execution gates:
/// - [`Allowed`](Self::Allowed) → execute (when policies also allow)
/// - [`RequiresApproval`](Self::RequiresApproval) → Review Card → await → resume
/// - [`Denied`](Self::Denied) → explain why → do not execute
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Action may proceed without conversational review.
    Allowed,
    /// Action must not proceed.
    Denied,
    /// User approval is required before proceeding (Review Card).
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
    /// True when the Planner may execute the tool without review.
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

    /// Evaluate a permission request into Allowed / RequiresApproval / Denied.
    ///
    /// Local filesystem reads are Allowed. Local writes, deletes, and terminal
    /// execution RequireApproval (Planner shows a Review Card). Internet and
    /// unconfigured categories are Denied until an explicit grant exists.
    ///
    /// Action Policies may further escalate or deny; they never grant what this
    /// engine denies. The Planner never lets a Tool execute itself.
    pub fn check(&self, request: &PermissionRequest) -> JaymiResult<PermissionCheckResult> {
        self.ensure_initialized()?;

        let resource = request
            .resource
            .as_deref()
            .unwrap_or("the requested resource");

        let (decision, explanation) = match (request.category, request.action) {
            (PermissionCategory::Filesystem, PermissionAction::Read) => (
                PermissionDecision::Allowed,
                format!(
                    "Read access to local files is granted for '{resource}' ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Filesystem, PermissionAction::Write) => (
                PermissionDecision::RequiresApproval,
                format!(
                    "Writing to '{resource}' requires your approval before Jaymi can modify local files ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Filesystem, PermissionAction::Delete) => (
                PermissionDecision::RequiresApproval,
                format!(
                    "Deleting '{resource}' requires your approval before Jaymi can remove local data ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Filesystem, _) => (
                PermissionDecision::Denied,
                format!(
                    "Filesystem action is not granted for '{resource}' ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Terminal, PermissionAction::Execute) => (
                PermissionDecision::RequiresApproval,
                format!(
                    "Running a terminal command requires your approval ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Terminal, _) => (
                PermissionDecision::Denied,
                format!(
                    "Terminal action is not granted ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Internet, _) => (
                PermissionDecision::Denied,
                format!(
                    "Internet access is not granted. Jaymi cannot use the network for '{resource}' until you grant this permission ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::Communication, _) => (
                PermissionDecision::Denied,
                format!(
                    "Communication access is not granted ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::System, _) => (
                PermissionDecision::Denied,
                format!(
                    "System access is not granted ({})",
                    request.explanation
                ),
            ),
            (PermissionCategory::AiProviders, _) => (
                PermissionDecision::Denied,
                format!(
                    "AI provider access is not granted ({})",
                    request.explanation
                ),
            ),
        };

        Ok(PermissionCheckResult {
            decision,
            explanation,
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
                "local_filesystem_read".to_string(),
                "allowed".to_string(),
            ),
            (
                "local_filesystem_write_delete".to_string(),
                "requires_approval".to_string(),
            ),
            (
                "local_terminal".to_string(),
                "requires_approval".to_string(),
            ),
            (
                "internet".to_string(),
                "denied".to_string(),
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
        assert!(result.explanation.contains("granted"));
    }

    #[test]
    fn write_requires_approval() {
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
        assert_eq!(result.decision, PermissionDecision::RequiresApproval);
        assert!(!result.allows_execution());
        assert!(result.explanation.contains("approval"));
    }

    #[test]
    fn delete_requires_approval() {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        let result = engine
            .check(&PermissionRequest {
                category: PermissionCategory::Filesystem,
                action: PermissionAction::Delete,
                scope: PermissionScope::Once,
                explanation: "Delete a path".into(),
                resource: Some("/tmp/a.txt".into()),
            })
            .unwrap();
        assert_eq!(result.decision, PermissionDecision::RequiresApproval);
        assert!(result.explanation.contains("approval"));
    }

    #[test]
    fn terminal_execute_requires_approval() {
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
        assert_eq!(result.decision, PermissionDecision::RequiresApproval);
        assert!(!result.allows_execution());
        assert!(result.explanation.contains("approval"));
    }

    #[test]
    fn denies_internet_actions_with_explanation() {
        let mut engine = PermissionEngine::new();
        engine.initialize().unwrap();
        let result = engine
            .check(&PermissionRequest {
                category: PermissionCategory::Internet,
                action: PermissionAction::Network,
                scope: PermissionScope::Once,
                explanation: "Call an API".into(),
                resource: Some("https://example.com".into()),
            })
            .unwrap();
        assert_eq!(result.decision, PermissionDecision::Denied);
        assert!(!result.allows_execution());
        assert!(result.explanation.contains("not granted"));
        assert!(result.explanation.contains("https://example.com"));
    }
}
