//! Core error type shared across Jaymi crates.

/// Top-level error for architectural boundaries.
///
/// Concrete error variants will be introduced as subsystems are implemented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JaymiError {
    message: String,
}

impl JaymiError {
    /// Create a new architectural error placeholder.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for JaymiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for JaymiError {}
