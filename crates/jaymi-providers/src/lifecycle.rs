//! Provider lifecycle stages.

/// Lifecycle state machine for installable providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLifecycle {
    Discover,
    Register,
    Initialize,
    HealthCheck,
    Ready,
    ExecuteRequests,
    Shutdown,
}
