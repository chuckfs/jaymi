//! Scopes at which permissions may be granted.

/// Permission grant duration / applicability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionScope {
    /// Valid for one action only.
    Once,
    /// Valid until the current conversation ends.
    Conversation,
    /// Valid only within the active project.
    Project,
    /// Persistent until revoked.
    Global,
}
