//! Explorer interaction events (UI → Application).

/// Events emitted by the Project Explorer component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerEvent {
    /// Open the project picker (no project loaded).
    OpenProject,
    /// Toggle expand/collapse for a directory.
    ToggleExpand(String),
    /// Single-click selection (does not open files).
    Select {
        /// Absolute path.
        path: String,
        /// Whether the path is a directory.
        is_dir: bool,
    },
    /// Double-click open (files only; folders still select + expand).
    Open(String),
    /// Start inline new-file draft under `parent`.
    BeginNewFile {
        /// Absolute parent directory.
        parent: String,
    },
    /// Start inline new-folder draft under `parent`.
    BeginNewFolder {
        /// Absolute parent directory.
        parent: String,
    },
    /// Start inline rename draft.
    BeginRename {
        /// Absolute path being renamed.
        path: String,
        /// Current basename.
        name: String,
    },
    /// Update the pending draft name.
    PendingNameChanged(String),
    /// Confirm the pending create/rename.
    ConfirmPending,
    /// Cancel the pending create/rename.
    CancelPending,
    /// Delete a path (file or empty/non-empty folder).
    Delete(String),
    /// Reveal a path in the OS file manager (Finder on macOS).
    Reveal(String),
}
