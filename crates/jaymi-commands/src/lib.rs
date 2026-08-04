//! Command registry — plugin-ready catalog of invokable commands.
//!
//! Architecture mirrors Tool / Provider / Capability registries:
//! metadata lives here; execution is owned by the Application / UI layer
//! (or future plugins) that register handlers against command ids.
//!
//! The Command Palette must never hardcode its list — it queries this registry.

#![forbid(unsafe_code)]

mod descriptor;
mod registry;
mod search;

pub use descriptor::{CommandCategory, CommandDescriptor, CommandSource};
pub use registry::CommandRegistry;
pub use search::{command_score, filter_commands};

/// Built-in command ids shipped with Jaymi (stable for plugins to depend on).
pub mod ids {
    /// Open a file in the Coding editor.
    pub const OPEN_FILE: &str = "jaymi.workbench.openFile";
    /// Open a folder as a project.
    pub const OPEN_FOLDER: &str = "jaymi.workbench.openFolder";
    /// Search project files by name / text.
    pub const SEARCH_FILES: &str = "jaymi.workbench.searchFiles";
    /// Quick Open — fuzzy filename jump (⌘P).
    pub const QUICK_OPEN: &str = "jaymi.workbench.quickOpen";
    /// Find in Files — project content search + replace panel (⌘⇧F).
    pub const FIND_IN_FILES: &str = "jaymi.workbench.findInFiles";
    /// Show or hide the Project Explorer.
    pub const TOGGLE_EXPLORER: &str = "jaymi.workbench.toggleExplorer";
    /// Show or hide the Terminal panel.
    pub const TOGGLE_TERMINAL: &str = "jaymi.workbench.toggleTerminal";
    /// Open a new terminal tab.
    pub const NEW_TERMINAL: &str = "jaymi.terminal.new";
    /// Close the active terminal tab.
    pub const KILL_TERMINAL: &str = "jaymi.terminal.kill";
    /// Rename the active terminal tab.
    pub const RENAME_TERMINAL: &str = "jaymi.terminal.rename";
    /// Show or hide the Git panel.
    pub const TOGGLE_GIT: &str = "jaymi.workbench.toggleGit";
    /// Show or hide the Problems panel.
    pub const TOGGLE_PROBLEMS: &str = "jaymi.workbench.toggleProblems";
    /// Show or hide the Search (Find in Files) panel.
    pub const TOGGLE_SEARCH: &str = "jaymi.workbench.toggleSearch";
    /// Show or hide the Diagnostics panel.
    pub const TOGGLE_DIAGNOSTICS: &str = "jaymi.workbench.toggleDiagnostics";
    /// Show or hide the Output panel.
    pub const TOGGLE_OUTPUT: &str = "jaymi.workbench.toggleOutput";
    /// Toggle the entire bottom dock (collapse / reopen last page).
    pub const TOGGLE_PANEL: &str = "jaymi.workbench.togglePanel";
    /// Create a new file in the explorer.
    pub const CREATE_FILE: &str = "jaymi.explorer.createFile";
    /// Create a new folder in the explorer.
    pub const CREATE_FOLDER: &str = "jaymi.explorer.createFolder";
    /// Save the active editor buffer.
    pub const SAVE: &str = "jaymi.editor.save";
    /// Close the active editor tab.
    pub const CLOSE_EDITOR: &str = "jaymi.editor.close";
    /// Close the expanded workspace (conversation remains).
    pub const CLOSE_WORKSPACE: &str = "jaymi.workbench.closeWorkspace";
    /// Open / expand the Coding workspace.
    pub const OPEN_CODING: &str = "jaymi.workspace.openCoding";
    /// Open / expand the Research workspace.
    pub const OPEN_RESEARCH: &str = "jaymi.workspace.openResearch";
    /// Open / expand the Creation (Creative) workspace.
    pub const OPEN_CREATIVE: &str = "jaymi.workspace.openCreative";
    /// Index the active project root.
    pub const INDEX_PROJECT: &str = "jaymi.project.index";
    /// Search project knowledge.
    pub const SEARCH_KNOWLEDGE: &str = "jaymi.search.knowledge";
    /// Search memory.
    pub const SEARCH_MEMORY: &str = "jaymi.search.memory";
    /// Run the Planner with a free-text request.
    pub const RUN_PLANNER: &str = "jaymi.planner.run";
}

/// Descriptors for the built-in command set.
pub fn builtin_descriptors() -> Vec<CommandDescriptor> {
    use ids::*;
    vec![
        CommandDescriptor::builtin(OPEN_FILE, "Open File", CommandCategory::File)
            .with_keywords(["open", "file", "edit"]),
        CommandDescriptor::builtin(OPEN_FOLDER, "Open Project", CommandCategory::File)
            .with_keywords(["open", "folder", "project", "directory"]),
        CommandDescriptor::builtin(SEARCH_FILES, "Search Files", CommandCategory::Search)
            .with_keywords(["find", "file", "fuzzy", "quick open"])
            .with_argument_prompt("Search files"),
        CommandDescriptor::builtin(QUICK_OPEN, "Quick Open", CommandCategory::Search)
            .with_keywords(["quick open", "go to file", "fuzzy", "file"])
            .with_keybinding("⌘P"),
        CommandDescriptor::builtin(FIND_IN_FILES, "Find in Files", CommandCategory::Search)
            .with_keywords(["find", "replace", "grep", "search", "project"])
            .with_keybinding("⌘⇧F"),
        CommandDescriptor::builtin(TOGGLE_EXPLORER, "Toggle Explorer", CommandCategory::View)
            .with_keywords(["explorer", "sidebar", "files"]),
        CommandDescriptor::builtin(TOGGLE_TERMINAL, "Toggle Terminal", CommandCategory::View)
            .with_keywords(["terminal", "shell", "console"]),
        CommandDescriptor::builtin(NEW_TERMINAL, "New Terminal", CommandCategory::View)
            .with_keywords(["terminal", "shell", "new", "tab", "create"]),
        CommandDescriptor::builtin(KILL_TERMINAL, "Kill Terminal", CommandCategory::View)
            .with_keywords(["terminal", "shell", "kill", "close"]),
        CommandDescriptor::builtin(RENAME_TERMINAL, "Rename Terminal", CommandCategory::View)
            .with_keywords(["terminal", "shell", "rename", "title"])
            .with_argument_prompt("New terminal title"),
        CommandDescriptor::builtin(TOGGLE_GIT, "Toggle Git", CommandCategory::View)
            .with_keywords(["git", "source control", "scm"]),
        CommandDescriptor::builtin(TOGGLE_PROBLEMS, "Toggle Problems", CommandCategory::View)
            .with_keywords(["problems", "diagnostics", "errors"]),
        CommandDescriptor::builtin(TOGGLE_SEARCH, "Toggle Search", CommandCategory::View)
            .with_keywords(["search", "find", "files", "panel"]),
        CommandDescriptor::builtin(
            TOGGLE_DIAGNOSTICS,
            "Toggle Diagnostics",
            CommandCategory::View,
        )
        .with_keywords(["diagnostics", "status", "workspace", "panel"]),
        CommandDescriptor::builtin(TOGGLE_OUTPUT, "Toggle Output", CommandCategory::View)
            .with_keywords(["output", "build", "logs", "panel"]),
        CommandDescriptor::builtin(TOGGLE_PANEL, "Toggle Panel", CommandCategory::View)
            .with_keywords(["panel", "dock", "bottom", "terminal"]),
        CommandDescriptor::builtin(CREATE_FILE, "Create File", CommandCategory::File)
            .with_keywords(["new", "file", "create"]),
        CommandDescriptor::builtin(CREATE_FOLDER, "Create Folder", CommandCategory::File)
            .with_keywords(["new", "folder", "directory", "mkdir"]),
        CommandDescriptor::builtin(SAVE, "Save", CommandCategory::File)
            .with_keywords(["save", "write", "disk"])
            .with_keybinding("⌘S"),
        CommandDescriptor::builtin(CLOSE_EDITOR, "Close Editor", CommandCategory::Editor)
            .with_keywords(["close", "tab", "editor"]),
        CommandDescriptor::builtin(CLOSE_WORKSPACE, "Close Workspace", CommandCategory::View)
            .with_keywords(["close", "workspace", "panel"]),
        CommandDescriptor::builtin(OPEN_CODING, "Open Coding", CommandCategory::Workspace)
            .with_keywords(["coding", "code", "ide", "project"]),
        CommandDescriptor::builtin(
            OPEN_RESEARCH,
            "Open Research Workspace (soon)",
            CommandCategory::Workspace,
        )
        .with_keywords(["research", "notes", "sources"]),
        CommandDescriptor::builtin(
            OPEN_CREATIVE,
            "Open Creation Workspace (soon)",
            CommandCategory::Workspace,
        )
        .with_keywords(["creation", "creative", "canvas", "images"]),
        CommandDescriptor::builtin(INDEX_PROJECT, "Index Project", CommandCategory::Project)
            .with_keywords(["index", "scan", "project"]),
        CommandDescriptor::builtin(
            SEARCH_KNOWLEDGE,
            "Search Knowledge",
            CommandCategory::Search,
        )
        .with_keywords(["knowledge", "project", "search"])
        .with_argument_prompt("Search knowledge"),
        CommandDescriptor::builtin(SEARCH_MEMORY, "Search Memory", CommandCategory::Search)
            .with_keywords(["memory", "recall", "search"])
            .with_argument_prompt("Search memory"),
        CommandDescriptor::builtin(RUN_PLANNER, "Run Planner", CommandCategory::Planner)
            .with_keywords(["planner", "run", "ask", "prompt"])
            .with_argument_prompt("Ask Jaymi"),
    ]
}
