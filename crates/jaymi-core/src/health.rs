//! Health reporting types for Jaymi subsystems.

/// Snapshot of a subsystem's runtime health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// Subsystem name.
    pub name: String,
    /// Whether [`crate::lifecycle::Lifecycle::initialize`] completed successfully.
    pub initialized: bool,
    /// Whether the subsystem is currently healthy.
    pub healthy: bool,
    /// Subsystem version string.
    pub version: String,
    /// Declared dependency names.
    pub dependencies: Vec<String>,
    /// Optional subsystem-specific health details (key/value).
    pub details: Vec<(String, String)>,
}

impl HealthReport {
    /// Build a health report from lifecycle metadata.
    pub fn new(
        name: impl Into<String>,
        initialized: bool,
        healthy: bool,
        version: impl Into<String>,
        dependencies: &[&'static str],
    ) -> Self {
        Self {
            name: name.into(),
            initialized,
            healthy,
            version: version.into(),
            dependencies: dependencies.iter().map(|dep| (*dep).to_string()).collect(),
            details: Vec::new(),
        }
    }

    /// Attach subsystem-specific health details.
    pub fn with_details(mut self, details: Vec<(String, String)>) -> Self {
        self.details = details;
        self
    }

    /// Convenience constructor for a healthy, initialized subsystem.
    pub fn healthy(
        name: impl Into<String>,
        version: impl Into<String>,
        dependencies: &[&'static str],
    ) -> Self {
        Self::new(name, true, true, version, dependencies)
    }

    /// Convenience constructor for an uninitialized subsystem.
    pub fn uninitialized(
        name: impl Into<String>,
        version: impl Into<String>,
        dependencies: &[&'static str],
    ) -> Self {
        Self::new(name, false, false, version, dependencies)
    }
}
