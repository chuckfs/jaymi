//! Permission categories that group protected actions.

/// High-level permission categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionCategory {
    Filesystem,
    Terminal,
    Internet,
    Communication,
    System,
    AiProviders,
}

/// Common action classes within a category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionAction {
    Read,
    Write,
    Execute,
    Delete,
    Network,
    Import,
    Export,
}
