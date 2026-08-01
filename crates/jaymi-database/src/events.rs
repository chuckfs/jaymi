//! Internal event log for auditing, debugging, and future automation.

use jaymi_core::EntityId;

/// A recorded system event.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: EntityId,
    pub kind: EventKind,
}

/// Event kinds retained by the knowledge store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    ConversationCreated,
    MemoryPromoted,
    ProviderInstalled,
    ProjectOpened,
    ToolExecuted,
    PermissionGranted,
}
