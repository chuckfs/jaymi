//! Cancellation tokens for cooperative stream / generation abort.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Shared cancellation flag for a reasoning request.
///
/// Provider-independent: backends poll [`Self::is_cancelled`] and stop work
/// without knowing how the flag was set (UI, timeout, new user message, …).
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a fresh, not-cancelled token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cooperative cancellation.
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    /// True when cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }

    /// Snapshot for serialization / diagnostics (not a live handle).
    pub fn flag(&self) -> CancellationFlag {
        CancellationFlag {
            cancelled: self.is_cancelled(),
        }
    }
}

/// Serializable cancellation snapshot (not shared across processes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CancellationFlag {
    /// Whether cancellation was requested at snapshot time.
    pub cancelled: bool,
}

impl CancellationFlag {
    /// Snapshot that is not cancelled.
    pub fn active() -> Self {
        Self { cancelled: false }
    }

    /// Snapshot that is cancelled.
    pub fn cancelled() -> Self {
        Self { cancelled: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_starts_active_and_cancels() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert!(!token.flag().cancelled);
        token.cancel();
        assert!(token.is_cancelled());
        assert!(token.flag().cancelled);
    }

    #[test]
    fn clones_share_cancellation_state() {
        let token = CancellationToken::new();
        let clone = token.clone();
        clone.cancel();
        assert!(token.is_cancelled());
    }
}
