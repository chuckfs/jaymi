//! First-class relationships between knowledge entities.

use jaymi_core::EntityId;

/// Directed relationship between two entities.
#[derive(Debug, Clone)]
pub struct Relationship {
    pub from: EntityId,
    pub to: EntityId,
    pub kind: RelationshipKind,
}

/// Known relationship kinds described by the architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipKind {
    ConversationBelongsToProject,
    MemoryBelongsToConversation,
    FileBelongsToProject,
    ArtifactBelongsToConversation,
    TaskBelongsToProject,
    ProviderSupportsCapability,
}
