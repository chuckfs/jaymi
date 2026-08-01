//! Application-level runtime state.

/// High-level lifecycle state of the Jaymi process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Boot sequence is in progress.
    Starting,
    /// All required subsystems initialized successfully.
    Ready,
    /// Ordered shutdown is in progress.
    ShuttingDown,
    /// Boot or runtime failed; contains a human-readable reason.
    Error {
        /// Explanation of the failure.
        message: String,
    },
}

impl AppState {
    /// Returns true when the application reached a healthy ready state.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Returns true when the application is in an error state.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Display label suitable for diagnostics UI.
    pub fn label(&self) -> &str {
        match self {
            Self::Starting => "Starting",
            Self::Ready => "Ready",
            Self::ShuttingDown => "Shutting Down",
            Self::Error { .. } => "Error",
        }
    }
}

impl std::fmt::Display for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error { message } => write!(f, "Error: {message}"),
            other => write!(f, "{}", other.label()),
        }
    }
}
