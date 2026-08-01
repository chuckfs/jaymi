//! Projects — first-class workspaces in Jaymi.
//!
//! A project is more than a folder. It is a persistent environment that
//! combines code, documents, conversations, memories, tasks, decisions, and
//! context into a single unit of work.

#![forbid(unsafe_code)]

pub mod structure;

use jaymi_core::{EntityId, JaymiResult};
use structure::JaymiProjectLayout;

/// Project identity and workspace skeleton.
#[derive(Debug, Clone)]
pub struct Project {
    pub id: EntityId,
    pub root: std::path::PathBuf,
}

impl Project {
    /// Initialize the hidden `.jaymi` directory for a project root.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn initialize(root: impl Into<std::path::PathBuf>) -> JaymiResult<Self> {
        let root = root.into();
        let _layout = JaymiProjectLayout::for_root(&root);
        Ok(Self {
            id: EntityId::new("project-placeholder"),
            root,
        })
    }

    /// Restore project context when opened.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn open(&self) -> JaymiResult<()> {
        Ok(())
    }
}

/// Project lifecycle stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectLifecycle {
    Create,
    Initialize,
    Index,
    Work,
    Update,
    Archive,
    Restore,
}
