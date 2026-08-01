//! Logical provider categories.

/// Provider grouping used for discovery and policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderCategory {
    Local,
    Ai,
    Internet,
    Import,
    Automation,
}
