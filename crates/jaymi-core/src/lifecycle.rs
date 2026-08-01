//! Common lifecycle interface shared by every Jaymi subsystem.

use crate::health::HealthReport;
use crate::result::JaymiResult;

/// Runtime lifecycle for a Jaymi subsystem.
///
/// Subsystems participate in the deterministic boot and shutdown sequence by
/// implementing this trait. Business logic does not belong here — only
/// lifecycle management.
pub trait Lifecycle: Send + Sync {
    /// Stable subsystem name used in health reports and dependency lists.
    fn name(&self) -> &'static str;

    /// Semantic version of this subsystem implementation.
    fn version(&self) -> &'static str;

    /// Names of subsystems that must be healthy before this one initializes.
    fn dependencies(&self) -> &[&'static str];

    /// Bring the subsystem into an initialized state.
    fn initialize(&mut self) -> JaymiResult<()>;

    /// Report whether the subsystem is initialized and healthy.
    fn health_check(&self) -> HealthReport;

    /// Release resources and leave the subsystem inert.
    fn shutdown(&mut self) -> JaymiResult<()>;
}
