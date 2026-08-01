//! Logical tool categories.

/// Tool grouping matching the architecture documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Search,
    Reading,
    Writing,
    Coding,
    Ai,
    Automation,
    Import,
}
