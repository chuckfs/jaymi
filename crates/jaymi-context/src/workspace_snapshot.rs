//! Canonical observational snapshot of the live Coding workspace (Sprint B2.1).
//!
//! [`WorkspaceSnapshot`] is the single immutable representation of what the
//! Coding environment looks like **right now**. It is observational only:
//!
//! * executes no tools
//! * performs no reasoning
//! * owns no policy
//! * never builds a [`crate::ContextBundle`]
//!
//! ## Ownership
//!
//! | Role | Owner |
//! |------|--------|
//! | Orchestration (when to capture / assemble) | Planner (via Application host prep) |
//! | Observation contract | [`WorkspaceSnapshot`] |
//! | Consumption | Context Engine (`ContextSessionInputs`) |
//! | Live mutable UX state | `CodingState` (capability-engine) |
//! | Project identity | Project Engine |
//! | Git / inventory contributions | Providers + Application maintenance |
//!
//! Distinct from capability-engine `EditorWorkspaceSnapshot`, which persists
//! editor chrome only.

use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::bundle::{
    CurrentFileSection, CurrentSelectionSection, OpenFileEntry, OpenFilesSection,
};

/// Zero-based caret position in the active editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CursorPosition {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based column.
    pub column: u32,
}

/// Active project identity observed from Project Engine (not invented).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ActiveProjectRef {
    /// Project id when a project is session-open.
    pub project_id: Option<String>,
    /// Display name when known.
    pub name: Option<String>,
    /// Canonical root directory when known.
    pub root_directory: Option<String>,
}

/// Observed package manager at the workspace root (marker-file based).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PackageManagerKind {
    /// Rust / Cargo (`Cargo.toml`).
    Cargo,
    /// Node npm (`package-lock.json` or package.json without other locks).
    Npm,
    /// pnpm (`pnpm-lock.yaml`).
    Pnpm,
    /// Yarn (`yarn.lock`).
    Yarn,
    /// Python pip (`requirements.txt`).
    Pip,
    /// Poetry (`poetry.lock` / `[tool.poetry]`).
    Poetry,
    /// Go modules (`go.mod`).
    GoMod,
    /// Unrecognized marker — raw label retained.
    Unknown(String),
}

impl PackageManagerKind {
    /// Stable id for diagnostics / fingerprints.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Pip => "pip",
            Self::Poetry => "poetry",
            Self::GoMod => "go_mod",
            Self::Unknown(label) => label.as_str(),
        }
    }
}

/// Observed build system at the workspace root (marker-file based).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildSystemKind {
    /// Cargo / rustc.
    Cargo,
    /// CMake (`CMakeLists.txt`).
    CMake,
    /// Make (`Makefile` / `makefile`).
    Make,
    /// npm scripts via `package.json`.
    NpmScripts,
    /// Go toolchain.
    Go,
    /// Unrecognized marker — raw label retained.
    Unknown(String),
}

impl BuildSystemKind {
    /// Stable id for diagnostics / fingerprints.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cargo => "cargo",
            Self::CMake => "cmake",
            Self::Make => "make",
            Self::NpmScripts => "npm_scripts",
            Self::Go => "go",
            Self::Unknown(label) => label.as_str(),
        }
    }
}

/// Marker-file observation of package manager + build system.
///
/// Pure filesystem presence checks — not a tool, not a provider execute path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ToolchainObservation {
    /// Detected package manager, when any.
    pub package_manager: Option<PackageManagerKind>,
    /// Detected build system, when any.
    pub build_system: Option<BuildSystemKind>,
}

/// Canonical immutable observation of the live Coding workspace.
///
/// Built by the Application host before Context assemble; consumed by the
/// Context Engine through [`crate::ContextSessionInputs`]. Never executes tools,
/// never reasons, never applies policy, never constructs a ContextBundle.
#[derive(Debug, Clone, Eq)]
pub struct WorkspaceSnapshot {
    /// Active project from Project Engine (observational).
    pub active_project: ActiveProjectRef,
    /// Workspace / project root path when known.
    pub workspace_root: Option<String>,
    /// Active UX workspace kind id (`coding`, …).
    pub workspace_kind: Option<String>,
    /// Current editor file.
    pub current_file: CurrentFileSection,
    /// Open editor tabs.
    pub open_files: OpenFilesSection,
    /// Active selection (caret-as-zero-width until selection IPC).
    pub active_selection: CurrentSelectionSection,
    /// Explicit caret position (mirrors selection start when caret-only).
    pub cursor: Option<CursorPosition>,
    /// Active Git branch when known.
    pub active_branch: Option<String>,
    /// Language id for the current file (denormalized from [`Self::current_file`]).
    pub language: Option<String>,
    /// Observed package manager at the root.
    pub package_manager: Option<PackageManagerKind>,
    /// Observed build system at the root.
    pub build_system: Option<BuildSystemKind>,
    /// Unix seconds when this observation was captured.
    ///
    /// Ignored by [`PartialEq`] / [`Hash`] so repeated captures of the same
    /// environment do not churn Context session fingerprints.
    pub timestamp: i64,
}

impl Default for WorkspaceSnapshot {
    fn default() -> Self {
        Self {
            active_project: ActiveProjectRef::default(),
            workspace_root: None,
            workspace_kind: None,
            current_file: CurrentFileSection::default(),
            open_files: OpenFilesSection::default(),
            active_selection: CurrentSelectionSection::default(),
            cursor: None,
            active_branch: None,
            language: None,
            package_manager: None,
            build_system: None,
            timestamp: 0,
        }
    }
}

impl PartialEq for WorkspaceSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.active_project == other.active_project
            && self.workspace_root == other.workspace_root
            && self.workspace_kind == other.workspace_kind
            && self.current_file == other.current_file
            && self.open_files == other.open_files
            && self.active_selection == other.active_selection
            && self.cursor == other.cursor
            && self.active_branch == other.active_branch
            && self.language == other.language
            && self.package_manager == other.package_manager
            && self.build_system == other.build_system
        // timestamp intentionally excluded
    }
}

impl Hash for WorkspaceSnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.active_project.hash(state);
        self.workspace_root.hash(state);
        self.workspace_kind.hash(state);
        self.current_file.path.hash(state);
        self.current_file.dirty.hash(state);
        self.current_file.language.hash(state);
        for file in &self.open_files.files {
            file.path.hash(state);
            file.dirty.hash(state);
            file.active.hash(state);
        }
        self.active_selection.path.hash(state);
        self.active_selection.start_line.hash(state);
        self.active_selection.start_column.hash(state);
        self.active_selection.end_line.hash(state);
        self.active_selection.end_column.hash(state);
        self.active_selection.text.hash(state);
        self.cursor.hash(state);
        self.active_branch.hash(state);
        self.language.hash(state);
        self.package_manager.hash(state);
        self.build_system.hash(state);
    }
}

/// Host-supplied parts for building a [`WorkspaceSnapshot`].
///
/// The host gathers live state from Project Engine, CodingState, Git
/// maintenance, and optional toolchain observation — then calls
/// [`WorkspaceSnapshot::from_observation`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceSnapshotObservation {
    /// Active project identity.
    pub active_project: ActiveProjectRef,
    /// Workspace root path.
    pub workspace_root: Option<String>,
    /// UX workspace kind id.
    pub workspace_kind: Option<String>,
    /// Current file section.
    pub current_file: CurrentFileSection,
    /// Open files section.
    pub open_files: OpenFilesSection,
    /// Active selection.
    pub active_selection: CurrentSelectionSection,
    /// Cursor position.
    pub cursor: Option<CursorPosition>,
    /// Active branch.
    pub active_branch: Option<String>,
    /// Package manager observation.
    pub package_manager: Option<PackageManagerKind>,
    /// Build system observation.
    pub build_system: Option<BuildSystemKind>,
    /// Optional capture time; defaults to now.
    pub timestamp: Option<i64>,
}

impl WorkspaceSnapshot {
    /// Empty observational snapshot (no Coding / project open).
    pub fn empty() -> Self {
        Self {
            timestamp: now_unix_secs(),
            ..Self::default()
        }
    }

    /// Build an immutable snapshot from host-observed parts.
    ///
    /// Does not execute tools, reason, or assemble a ContextBundle.
    pub fn from_observation(parts: WorkspaceSnapshotObservation) -> Self {
        let language = parts.current_file.language.clone();
        Self {
            active_project: parts.active_project,
            workspace_root: parts.workspace_root,
            workspace_kind: parts.workspace_kind,
            current_file: parts.current_file,
            open_files: parts.open_files,
            active_selection: parts.active_selection,
            cursor: parts.cursor,
            active_branch: parts.active_branch,
            language,
            package_manager: parts.package_manager,
            build_system: parts.build_system,
            timestamp: parts.timestamp.unwrap_or_else(now_unix_secs),
        }
    }

    /// True when Coding-like editor state is present.
    pub fn has_editor_state(&self) -> bool {
        self.current_file.path.is_some() || !self.open_files.files.is_empty()
    }

    /// True when a project identity or root was observed.
    pub fn has_project(&self) -> bool {
        self.active_project.project_id.is_some()
            || self.active_project.root_directory.is_some()
            || self.workspace_root.is_some()
    }
}

/// Observe package manager + build system from root marker files.
///
/// Presence checks only — never runs package/build commands, never opens a
/// tool route, never mutates the workspace.
pub fn observe_toolchain(root: &Path) -> ToolchainObservation {
    if !root.is_dir() {
        return ToolchainObservation::default();
    }

    let cargo = root.join("Cargo.toml").is_file();
    let cmake = root.join("CMakeLists.txt").is_file();
    let makefile = root.join("Makefile").is_file() || root.join("makefile").is_file();
    let go_mod = root.join("go.mod").is_file();
    let package_json = root.join("package.json").is_file();
    let pnpm = root.join("pnpm-lock.yaml").is_file();
    let yarn = root.join("yarn.lock").is_file();
    let npm_lock = root.join("package-lock.json").is_file();
    let poetry = root.join("poetry.lock").is_file() || root.join("pyproject.toml").is_file();
    let pip = root.join("requirements.txt").is_file();

    let package_manager = if cargo {
        Some(PackageManagerKind::Cargo)
    } else if pnpm {
        Some(PackageManagerKind::Pnpm)
    } else if yarn {
        Some(PackageManagerKind::Yarn)
    } else if npm_lock || package_json {
        Some(PackageManagerKind::Npm)
    } else if go_mod {
        Some(PackageManagerKind::GoMod)
    } else if poetry && root.join("poetry.lock").is_file() {
        Some(PackageManagerKind::Poetry)
    } else if pip {
        Some(PackageManagerKind::Pip)
    } else {
        None
    };

    let build_system = if cargo {
        Some(BuildSystemKind::Cargo)
    } else if cmake {
        Some(BuildSystemKind::CMake)
    } else if makefile {
        Some(BuildSystemKind::Make)
    } else if go_mod {
        Some(BuildSystemKind::Go)
    } else if package_json {
        Some(BuildSystemKind::NpmScripts)
    } else {
        None
    };

    ToolchainObservation {
        package_manager,
        build_system,
    }
}

/// Helper: open-file list from path/dirty/active triples (tests / thin hosts).
pub fn open_files_from_entries(entries: Vec<OpenFileEntry>) -> OpenFilesSection {
    OpenFilesSection { files: entries }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-ws-snap-{}-{}",
            label,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn snapshot_ignores_timestamp_for_equality() {
        let a = WorkspaceSnapshot::from_observation(WorkspaceSnapshotObservation {
            workspace_kind: Some("coding".into()),
            current_file: CurrentFileSection {
                path: Some("/p/main.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            },
            timestamp: Some(1),
            ..WorkspaceSnapshotObservation::default()
        });
        let mut b = a.clone();
        b.timestamp = 999;
        assert_eq!(a, b);
        assert_eq!(a.language.as_deref(), Some("rust"));
    }

    #[test]
    fn from_observation_denormalizes_language() {
        let snap = WorkspaceSnapshot::from_observation(WorkspaceSnapshotObservation {
            current_file: CurrentFileSection {
                path: Some("/a.ts".into()),
                dirty: true,
                language: Some("typescript".into()),
            },
            ..WorkspaceSnapshotObservation::default()
        });
        assert_eq!(snap.language.as_deref(), Some("typescript"));
        assert!(snap.has_editor_state());
    }

    #[test]
    fn observe_toolchain_cargo_project() {
        let root = temp_dir("cargo");
        fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let observed = observe_toolchain(&root);
        assert_eq!(observed.package_manager, Some(PackageManagerKind::Cargo));
        assert_eq!(observed.build_system, Some(BuildSystemKind::Cargo));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn observe_toolchain_empty_is_none() {
        let root = temp_dir("empty");
        let observed = observe_toolchain(&root);
        assert!(observed.package_manager.is_none());
        assert!(observed.build_system.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_snapshot_has_no_project() {
        let snap = WorkspaceSnapshot::empty();
        assert!(!snap.has_project());
        assert!(!snap.has_editor_state());
    }
}
