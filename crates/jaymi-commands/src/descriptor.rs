//! Command metadata descriptors.

/// High-level grouping shown in the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    /// File / folder operations.
    File,
    /// Editor tab operations.
    Editor,
    /// View / panel toggles.
    View,
    /// Workspace expansion.
    Workspace,
    /// Project lifecycle.
    Project,
    /// Search surfaces.
    Search,
    /// Planner / conversation.
    Planner,
    /// Extension / plugin commands.
    Extension,
}

impl CommandCategory {
    /// Short label for the palette.
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Editor => "Editor",
            Self::View => "View",
            Self::Workspace => "Workspace",
            Self::Project => "Project",
            Self::Search => "Search",
            Self::Planner => "Planner",
            Self::Extension => "Extension",
        }
    }
}

/// Who registered the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandSource {
    /// Shipped with Jaymi.
    Builtin,
    /// Registered by a plugin / extension (future).
    Plugin,
}

/// Immutable command catalog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescriptor {
    /// Stable id (`jaymi.workbench.save`, `ext.foo.bar`, …).
    pub id: String,
    /// Human-readable title shown in the palette.
    pub title: String,
    /// Optional category for grouping / secondary text.
    pub category: CommandCategory,
    /// Extra tokens matched by fuzzy search.
    pub keywords: Vec<String>,
    /// Optional displayed keybinding hint (not binding itself).
    pub keybinding: Option<String>,
    /// When set, the palette prompts for an argument before execute.
    pub argument_prompt: Option<String>,
    /// Registration source.
    pub source: CommandSource,
}

impl CommandDescriptor {
    /// Create a built-in command descriptor.
    pub fn builtin(
        id: impl Into<String>,
        title: impl Into<String>,
        category: CommandCategory,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            category,
            keywords: Vec::new(),
            keybinding: None,
            argument_prompt: None,
            source: CommandSource::Builtin,
        }
    }

    /// Create a plugin-registered command descriptor.
    pub fn plugin(
        id: impl Into<String>,
        title: impl Into<String>,
        category: CommandCategory,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            category,
            keywords: Vec::new(),
            keybinding: None,
            argument_prompt: None,
            source: CommandSource::Plugin,
        }
    }

    /// Attach search keywords.
    pub fn with_keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    /// Attach a keybinding hint string.
    pub fn with_keybinding(mut self, keybinding: impl Into<String>) -> Self {
        self.keybinding = Some(keybinding.into());
        self
    }

    /// Require a free-text argument from the palette before execute.
    pub fn with_argument_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.argument_prompt = Some(prompt.into());
        self
    }
}
