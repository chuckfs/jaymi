//! Policy scopes, from broadest to most specific.

/// Level at which a policy applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicyScope {
    Global,
    Conversation,
    Project,
    Task,
}
