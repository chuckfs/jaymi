//! User-facing request types that enter the Planner.

/// A request originating from the conversation interface.
///
/// Every interaction begins with understanding intent. The Planner receives
/// this request and coordinates the rest of the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRequest {
    /// Natural-language content provided by the user.
    pub content: String,
}

impl UserRequest {
    /// Create a new user request.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }
}
