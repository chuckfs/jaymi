//! Canonical read-only project intelligence observation (Sprint B2.4).
//!
//! [`ProjectSnapshot`] is the immutable representation of a software project as
//! an artifact (metadata, languages, frameworks, toolchain, dependency summary,
//! cargo/npm metadata, repository metadata, workspace layout).
//!
//! It is observational only:
//!
//! * executes no tools
//! * performs no reasoning
//! * owns no policy
//! * never builds a [`crate::ContextBundle`]
//! * never talks to an LLM
//!
//! ## Ownership
//!
//! | Role | Owner |
//! |------|--------|
//! | Orchestration (when to assemble) | Planner (via Application host prep) |
//! | Ambient refresh / FS observation | Application `ContextMaintenance` |
//! | Observation contract | [`ProjectSnapshot`] |
//! | Consumption | Context providers (`ProjectProvider`) — session only |
//! | Project identity | Project Engine |
//! | Request path | Providers **must not** scan the filesystem |
//!
//! Distinct from [`crate::WorkspaceSnapshot`] (live Coding chrome) and
//! [`crate::EditorSnapshot`] (buffer / LSP observation).

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::workspace_snapshot::{
    observe_toolchain, BuildSystemKind, PackageManagerKind, ToolchainObservation,
};

/// Project identity metadata observed from Project Engine (+ root).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ProjectMetadata {
    /// Project id when session-open.
    pub project_id: Option<String>,
    /// Display name.
    pub name: Option<String>,
    /// Optional description.
    pub description: Option<String>,
    /// Canonical root directory.
    pub root_directory: Option<String>,
    /// Project type label (`code`, `general`, …).
    pub project_type: Option<String>,
}

/// Capped dependency-graph summary (not a full lockfile dump).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DependencyGraphSummary {
    /// Top-level dependency names (capped).
    pub top_level: Vec<String>,
    /// Approximate direct dependency count when known.
    pub direct_count: usize,
    /// Approximate transitive / lockfile count when known.
    pub lockfile_count: Option<usize>,
    /// Workspace / member package names when a monorepo is detected.
    pub workspace_members: Vec<String>,
}

/// Cargo.toml observational metadata (shallow parse).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CargoProjectMeta {
    /// `[package].name`.
    pub package_name: Option<String>,
    /// `[package].edition`.
    pub edition: Option<String>,
    /// Whether `[workspace]` is present.
    pub is_workspace: bool,
    /// Workspace members (capped).
    pub members: Vec<String>,
    /// Direct dependency names from `[dependencies]` (capped).
    pub dependencies: Vec<String>,
}

/// package.json observational metadata (shallow parse).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct NpmProjectMeta {
    /// `name`.
    pub package_name: Option<String>,
    /// `private` flag when present.
    pub private: Option<bool>,
    /// Workspace package globs / names (capped).
    pub workspaces: Vec<String>,
    /// Script keys (capped).
    pub scripts: Vec<String>,
    /// Direct dependency names (deps + devDeps, capped).
    pub dependencies: Vec<String>,
}

/// Repository observation (marker / lightweight reads only).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct RepositoryMetadata {
    /// True when `.git` exists under the root.
    pub is_git_repository: bool,
    /// Absolute git root when known (often the project root).
    pub git_root: Option<String>,
    /// HEAD branch name when readable from `.git/HEAD`.
    pub head_branch: Option<String>,
    /// First remote URL from `.git/config` when cheaply available.
    pub remote_url: Option<String>,
}

/// Top-level workspace layout summary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct WorkspaceLayoutSummary {
    /// Top-level directory names (capped, sorted).
    pub top_level_dirs: Vec<String>,
    /// Top-level file names that look like project markers (capped).
    pub marker_files: Vec<String>,
    /// Detected monorepo / multi-package shape label.
    pub shape: Option<String>,
}

/// Read-only project intelligence snapshot.
#[derive(Debug, Clone, Eq)]
pub struct ProjectSnapshot {
    /// Project identity metadata.
    pub metadata: ProjectMetadata,
    /// Observed languages (e.g. `rust`, `typescript`).
    pub languages: Vec<String>,
    /// Observed frameworks (heuristic / marker-based).
    pub frameworks: Vec<String>,
    /// Package manager at the root.
    pub package_manager: Option<PackageManagerKind>,
    /// Build system at the root.
    pub build_system: Option<BuildSystemKind>,
    /// Capped dependency graph summary.
    pub dependency_summary: DependencyGraphSummary,
    /// Cargo metadata when present.
    pub cargo: Option<CargoProjectMeta>,
    /// npm / package.json metadata when present.
    pub npm: Option<NpmProjectMeta>,
    /// Repository metadata.
    pub repository: RepositoryMetadata,
    /// Workspace layout summary.
    pub workspace_layout: WorkspaceLayoutSummary,
    /// Unix seconds when captured (ignored by Eq/Hash).
    pub timestamp: i64,
}

impl Default for ProjectSnapshot {
    fn default() -> Self {
        Self {
            metadata: ProjectMetadata::default(),
            languages: Vec::new(),
            frameworks: Vec::new(),
            package_manager: None,
            build_system: None,
            dependency_summary: DependencyGraphSummary::default(),
            cargo: None,
            npm: None,
            repository: RepositoryMetadata::default(),
            workspace_layout: WorkspaceLayoutSummary::default(),
            timestamp: 0,
        }
    }
}

impl PartialEq for ProjectSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.metadata == other.metadata
            && self.languages == other.languages
            && self.frameworks == other.frameworks
            && self.package_manager == other.package_manager
            && self.build_system == other.build_system
            && self.dependency_summary == other.dependency_summary
            && self.cargo == other.cargo
            && self.npm == other.npm
            && self.repository == other.repository
            && self.workspace_layout == other.workspace_layout
    }
}

impl Hash for ProjectSnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.metadata.hash(state);
        self.languages.hash(state);
        self.frameworks.hash(state);
        self.package_manager.hash(state);
        self.build_system.hash(state);
        self.dependency_summary.hash(state);
        self.cargo.hash(state);
        self.npm.hash(state);
        self.repository.hash(state);
        self.workspace_layout.hash(state);
    }
}

/// Host-supplied parts for building a [`ProjectSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectSnapshotObservation {
    /// Project metadata.
    pub metadata: ProjectMetadata,
    /// Languages.
    pub languages: Vec<String>,
    /// Frameworks.
    pub frameworks: Vec<String>,
    /// Package manager.
    pub package_manager: Option<PackageManagerKind>,
    /// Build system.
    pub build_system: Option<BuildSystemKind>,
    /// Dependency summary.
    pub dependency_summary: DependencyGraphSummary,
    /// Cargo metadata.
    pub cargo: Option<CargoProjectMeta>,
    /// npm metadata.
    pub npm: Option<NpmProjectMeta>,
    /// Repository metadata.
    pub repository: RepositoryMetadata,
    /// Layout summary.
    pub workspace_layout: WorkspaceLayoutSummary,
    /// Optional capture time.
    pub timestamp: Option<i64>,
}

/// Intelligence subset contributed into a [`crate::ContextBundle`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectIntelligenceSection {
    /// Languages.
    pub languages: Vec<String>,
    /// Frameworks.
    pub frameworks: Vec<String>,
    /// Package manager id.
    pub package_manager: Option<String>,
    /// Build system id.
    pub build_system: Option<String>,
    /// Dependency summary.
    pub dependency_summary: DependencyGraphSummary,
    /// Cargo package name when known.
    pub cargo_package: Option<String>,
    /// npm package name when known.
    pub npm_package: Option<String>,
    /// Repository branch when known.
    pub repository_branch: Option<String>,
    /// Layout shape label.
    pub layout_shape: Option<String>,
    /// Top-level dirs (capped).
    pub top_level_dirs: Vec<String>,
}

impl ProjectSnapshot {
    /// Empty observational snapshot.
    pub fn empty() -> Self {
        Self {
            timestamp: now_unix_secs(),
            ..Self::default()
        }
    }

    /// Build from host-observed parts (no tools / reasoning / assemble).
    pub fn from_observation(parts: ProjectSnapshotObservation) -> Self {
        Self {
            metadata: parts.metadata,
            languages: parts.languages,
            frameworks: parts.frameworks,
            package_manager: parts.package_manager,
            build_system: parts.build_system,
            dependency_summary: parts.dependency_summary,
            cargo: parts.cargo,
            npm: parts.npm,
            repository: parts.repository,
            workspace_layout: parts.workspace_layout,
            timestamp: parts.timestamp.unwrap_or_else(now_unix_secs),
        }
    }

    /// True when a project identity or root was observed.
    pub fn has_project(&self) -> bool {
        self.metadata.project_id.is_some()
            || self.metadata.root_directory.is_some()
            || self.metadata.name.is_some()
    }

    /// True when any intelligence field beyond identity is populated.
    pub fn has_intelligence(&self) -> bool {
        !self.languages.is_empty()
            || !self.frameworks.is_empty()
            || self.package_manager.is_some()
            || self.build_system.is_some()
            || !self.dependency_summary.top_level.is_empty()
            || self.cargo.is_some()
            || self.npm.is_some()
            || self.repository.is_git_repository
            || !self.workspace_layout.top_level_dirs.is_empty()
    }

    /// Intelligence subset for ContextBundle contribution.
    pub fn intelligence_section(&self) -> ProjectIntelligenceSection {
        ProjectIntelligenceSection {
            languages: self.languages.clone(),
            frameworks: self.frameworks.clone(),
            package_manager: self
                .package_manager
                .as_ref()
                .map(|kind| kind.as_str().to_string()),
            build_system: self
                .build_system
                .as_ref()
                .map(|kind| kind.as_str().to_string()),
            dependency_summary: self.dependency_summary.clone(),
            cargo_package: self
                .cargo
                .as_ref()
                .and_then(|cargo| cargo.package_name.clone()),
            npm_package: self.npm.as_ref().and_then(|npm| npm.package_name.clone()),
            repository_branch: self.repository.head_branch.clone(),
            layout_shape: self.workspace_layout.shape.clone(),
            top_level_dirs: self.workspace_layout.top_level_dirs.clone(),
        }
    }
}

/// Host facts for ambient project observation (identity from Project Engine).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectSnapshotHostFacts {
    /// Open project id.
    pub project_id: Option<String>,
    /// Display name.
    pub name: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Root directory.
    pub root_directory: Option<String>,
    /// Project type label.
    pub project_type: Option<String>,
}

/// Observe project intelligence at `root` (ambient / maintenance only).
///
/// Marker-file presence + shallow text parses. Never runs package managers,
/// never opens a tool route, never mutates the workspace. **Must not** be
/// called from Context providers during assemble.
pub fn observe_project_intelligence(
    root: &Path,
    facts: &ProjectSnapshotHostFacts,
) -> ProjectSnapshot {
    let metadata = ProjectMetadata {
        project_id: facts.project_id.clone(),
        name: facts.name.clone(),
        description: facts.description.clone(),
        root_directory: facts
            .root_directory
            .clone()
            .or_else(|| Some(root.display().to_string())),
        project_type: facts.project_type.clone(),
    };

    if !root.is_dir() {
        return ProjectSnapshot::from_observation(ProjectSnapshotObservation {
            metadata,
            ..ProjectSnapshotObservation::default()
        });
    }

    let toolchain = observe_toolchain(root);
    let cargo = parse_cargo_meta(root);
    let npm = parse_npm_meta(root);
    let repository = observe_repository(root);
    let workspace_layout = observe_layout(root, &toolchain, cargo.as_ref(), npm.as_ref());
    let (languages, frameworks) = detect_languages_and_frameworks(root, &toolchain, cargo.as_ref(), npm.as_ref());
    let dependency_summary = build_dependency_summary(cargo.as_ref(), npm.as_ref());

    ProjectSnapshot::from_observation(ProjectSnapshotObservation {
        metadata,
        languages,
        frameworks,
        package_manager: toolchain.package_manager,
        build_system: toolchain.build_system,
        dependency_summary,
        cargo,
        npm,
        repository,
        workspace_layout,
        timestamp: None,
    })
}

fn build_dependency_summary(
    cargo: Option<&CargoProjectMeta>,
    npm: Option<&NpmProjectMeta>,
) -> DependencyGraphSummary {
    let mut top_level = Vec::new();
    let mut workspace_members = Vec::new();
    if let Some(cargo) = cargo {
        top_level.extend(cargo.dependencies.iter().cloned());
        workspace_members.extend(cargo.members.iter().cloned());
    }
    if let Some(npm) = npm {
        for dep in &npm.dependencies {
            if !top_level.iter().any(|existing| existing == dep) {
                top_level.push(dep.clone());
            }
        }
        for member in &npm.workspaces {
            if !workspace_members.iter().any(|existing| existing == member) {
                workspace_members.push(member.clone());
            }
        }
    }
    top_level.truncate(32);
    workspace_members.truncate(24);
    let direct_count = top_level.len();
    DependencyGraphSummary {
        top_level,
        direct_count,
        lockfile_count: None,
        workspace_members,
    }
}

fn detect_languages_and_frameworks(
    root: &Path,
    toolchain: &ToolchainObservation,
    cargo: Option<&CargoProjectMeta>,
    npm: Option<&NpmProjectMeta>,
) -> (Vec<String>, Vec<String>) {
    let mut languages = Vec::new();
    let mut frameworks = Vec::new();

    let push_lang = |langs: &mut Vec<String>, label: &str| {
        if !langs.iter().any(|existing| existing == label) {
            langs.push(label.to_string());
        }
    };
    let push_fw = |fws: &mut Vec<String>, label: &str| {
        if !fws.iter().any(|existing| existing == label) {
            fws.push(label.to_string());
        }
    };

    if matches!(toolchain.package_manager, Some(PackageManagerKind::Cargo)) || cargo.is_some() {
        push_lang(&mut languages, "rust");
    }
    if matches!(
        toolchain.package_manager,
        Some(
            PackageManagerKind::Npm
                | PackageManagerKind::Pnpm
                | PackageManagerKind::Yarn
        )
    ) || npm.is_some()
    {
        push_lang(&mut languages, "javascript");
        if root.join("tsconfig.json").is_file()
            || root.join("tsconfig.base.json").is_file()
        {
            push_lang(&mut languages, "typescript");
        }
    }
    if matches!(toolchain.package_manager, Some(PackageManagerKind::GoMod)) {
        push_lang(&mut languages, "go");
    }
    if matches!(
        toolchain.package_manager,
        Some(PackageManagerKind::Pip | PackageManagerKind::Poetry)
    ) || root.join("pyproject.toml").is_file()
    {
        push_lang(&mut languages, "python");
    }
    if root.join("Package.swift").is_file() {
        push_lang(&mut languages, "swift");
    }

    // Framework heuristics from markers / dependency names.
    let dep_names: Vec<&str> = cargo
        .map(|c| {
            c.dependencies
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        })
        .into_iter()
        .chain(
            npm.map(|n| {
                n.dependencies
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            })
            .into_iter(),
        )
        .flatten()
        .collect();

    for name in &dep_names {
        let lower = name.to_ascii_lowercase();
        if lower.contains("tauri") {
            push_fw(&mut frameworks, "tauri");
        }
        if lower.contains("axum") {
            push_fw(&mut frameworks, "axum");
        }
        if lower.contains("actix") {
            push_fw(&mut frameworks, "actix");
        }
        if lower == "next" || lower.starts_with("next/") {
            push_fw(&mut frameworks, "next");
        }
        if lower == "react" || lower.starts_with("react-") {
            push_fw(&mut frameworks, "react");
        }
        if lower.contains("django") {
            push_fw(&mut frameworks, "django");
        }
        if lower.contains("fastapi") {
            push_fw(&mut frameworks, "fastapi");
        }
    }
    if root.join("src-tauri").is_dir() {
        push_fw(&mut frameworks, "tauri");
    }
    if root.join("next.config.js").is_file()
        || root.join("next.config.mjs").is_file()
        || root.join("next.config.ts").is_file()
    {
        push_fw(&mut frameworks, "next");
    }

    languages.truncate(12);
    frameworks.truncate(12);
    (languages, frameworks)
}

fn observe_layout(
    root: &Path,
    toolchain: &ToolchainObservation,
    cargo: Option<&CargoProjectMeta>,
    npm: Option<&NpmProjectMeta>,
) -> WorkspaceLayoutSummary {
    const MAX_DIRS: usize = 24;
    const MAX_MARKERS: usize = 16;
    let skip = [
        ".git",
        "node_modules",
        "target",
        ".jaymi",
        "dist",
        "build",
        ".next",
        ".venv",
    ];

    let mut top_level_dirs = Vec::new();
    let mut marker_files = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        let mut collected = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if skip.iter().any(|s| *s == name.as_str()) || name.starts_with('.') {
                // Still record important markers that start with '.' when files.
                if entry.path().is_file()
                    && matches!(
                        name.as_str(),
                        ".gitignore" | ".nvmrc" | ".node-version" | ".python-version"
                    )
                {
                    marker_files.push(name);
                }
                continue;
            }
            collected.push((name, entry.path().is_dir()));
        }
        collected.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        for (name, is_dir) in collected {
            if is_dir {
                if top_level_dirs.len() < MAX_DIRS {
                    top_level_dirs.push(name);
                }
            } else if marker_files.len() < MAX_MARKERS {
                let lower = name.to_lowercase();
                if lower.ends_with(".toml")
                    || lower.ends_with(".json")
                    || lower.ends_with(".lock")
                    || lower.ends_with(".md")
                    || lower.starts_with("makefile")
                    || lower == "dockerfile"
                    || lower.ends_with(".yaml")
                    || lower.ends_with(".yml")
                {
                    marker_files.push(name);
                }
            }
        }
    }

    let shape = if cargo.map(|c| c.is_workspace).unwrap_or(false)
        || npm.map(|n| !n.workspaces.is_empty()).unwrap_or(false)
    {
        Some("monorepo".into())
    } else if matches!(
        toolchain.package_manager,
        Some(PackageManagerKind::Cargo)
    ) {
        Some("cargo_package".into())
    } else if matches!(
        toolchain.package_manager,
        Some(
            PackageManagerKind::Npm | PackageManagerKind::Pnpm | PackageManagerKind::Yarn
        )
    ) {
        Some("node_package".into())
    } else if !top_level_dirs.is_empty() {
        Some("directory".into())
    } else {
        None
    };

    WorkspaceLayoutSummary {
        top_level_dirs,
        marker_files,
        shape,
    }
}

fn observe_repository(root: &Path) -> RepositoryMetadata {
    let git_dir = root.join(".git");
    if !git_dir.exists() {
        return RepositoryMetadata::default();
    }
    let git_root = Some(root.display().to_string());
    let head_branch = fs::read_to_string(git_dir.join("HEAD"))
        .ok()
        .and_then(|contents| {
            let trimmed = contents.trim();
            trimmed
                .strip_prefix("ref: refs/heads/")
                .map(|branch| branch.to_string())
        });
    let remote_url = fs::read_to_string(git_dir.join("config")).ok().and_then(|config| {
        let mut in_origin = false;
        for line in config.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_origin = line.contains("remote \"origin\"");
                continue;
            }
            if in_origin {
                if let Some(url) = line
                    .strip_prefix("url =")
                    .or_else(|| line.strip_prefix("url="))
                {
                    return Some(url.trim().to_string());
                }
            }
        }
        None
    });
    RepositoryMetadata {
        is_git_repository: true,
        git_root,
        head_branch,
        remote_url,
    }
}

fn parse_cargo_meta(root: &Path) -> Option<CargoProjectMeta> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(path).ok()?;
    let mut meta = CargoProjectMeta::default();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            if section == "workspace" {
                meta.is_workspace = true;
            }
            continue;
        }
        if section == "package" {
            if let Some(value) = toml_string_value(line, "name") {
                meta.package_name = Some(value);
            } else if let Some(value) = toml_string_value(line, "edition") {
                meta.edition = Some(value);
            }
        } else if section == "dependencies" {
            if let Some(name) = line.split('=').next().map(str::trim) {
                if !name.is_empty() && meta.dependencies.len() < 32 {
                    meta.dependencies.push(name.to_string());
                }
            }
        } else if section == "workspace.members" || (section == "workspace" && line.starts_with("members")) {
            // members = ["a", "b"] on one line
            if let Some(start) = line.find('[') {
                if let Some(end) = line.rfind(']') {
                    for part in line[start + 1..end].split(',') {
                        let member = part.trim().trim_matches('"').trim_matches('\'');
                        if !member.is_empty() && meta.members.len() < 24 {
                            meta.members.push(member.to_string());
                        }
                    }
                }
            }
        }
    }
    // Workspace member array items on following lines: "crates/foo",
    if meta.is_workspace && meta.members.is_empty() {
        let mut in_members = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with("members") && line.contains('[') {
                in_members = true;
            }
            if in_members {
                if line.starts_with(']') {
                    break;
                }
                let member = line.trim_matches(',').trim().trim_matches('"').trim_matches('\'');
                if !member.is_empty()
                    && member != "members"
                    && !member.starts_with('[')
                    && meta.members.len() < 24
                {
                    meta.members.push(member.to_string());
                }
            }
        }
    }
    Some(meta)
}

fn parse_npm_meta(root: &Path) -> Option<NpmProjectMeta> {
    let path = root.join("package.json");
    let text = fs::read_to_string(path).ok()?;
    let mut meta = NpmProjectMeta {
        package_name: json_string_field(&text, "name"),
        private: json_bool_field(&text, "private"),
        ..NpmProjectMeta::default()
    };
    meta.scripts = json_object_keys(&text, "scripts");
    meta.dependencies = {
        let mut deps = json_object_keys(&text, "dependencies");
        for dep in json_object_keys(&text, "devDependencies") {
            if !deps.iter().any(|existing| existing == &dep) {
                deps.push(dep);
            }
        }
        deps.truncate(32);
        deps
    };
    meta.workspaces = json_string_array_field(&text, "workspaces");
    if meta.workspaces.is_empty() {
        // workspaces: { "packages": ["packages/*"] }
        meta.workspaces = json_object_string_array(&text, "workspaces", "packages");
    }
    Some(meta)
}

fn toml_string_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim();
    let prefix = format!("{key} ");
    let prefix_eq = format!("{key}=");
    let rest = if let Some(rest) = trimmed.strip_prefix(&prefix) {
        rest.trim().strip_prefix('=')?.trim()
    } else if let Some(rest) = trimmed.strip_prefix(&prefix_eq) {
        rest.trim()
    } else {
        return None;
    };
    Some(rest.trim_matches('"').trim_matches('\'').to_string())
}

fn json_string_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let idx = text.find(&pattern)?;
    let after = &text[idx + pattern.len()..];
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let body = &after[1..];
    let end = body.find('"')?;
    Some(body[..end].to_string())
}

fn json_bool_field(text: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{key}\"");
    let idx = text.find(&pattern)?;
    let after = &text[idx + pattern.len()..];
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn json_object_keys(text: &str, key: &str) -> Vec<String> {
    let pattern = format!("\"{key}\"");
    let Some(idx) = text.find(&pattern) else {
        return Vec::new();
    };
    let after = &text[idx + pattern.len()..];
    let Some(after) = after.trim_start().strip_prefix(':') else {
        return Vec::new();
    };
    let after = after.trim_start();
    let Some(rest) = after.strip_prefix('{') else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for segment in rest.split(',') {
        if keys.len() >= 32 {
            break;
        }
        if let Some(name) = segment.split(':').next() {
            let name = name.trim().trim_matches('"');
            if !name.is_empty() && !name.contains('{') && !name.contains('}') {
                keys.push(name.to_string());
            }
        }
        if segment.contains('}') {
            break;
        }
    }
    keys
}

fn json_string_array_field(text: &str, key: &str) -> Vec<String> {
    let pattern = format!("\"{key}\"");
    let Some(idx) = text.find(&pattern) else {
        return Vec::new();
    };
    let after = &text[idx + pattern.len()..];
    let Some(after) = after.trim_start().strip_prefix(':') else {
        return Vec::new();
    };
    let after = after.trim_start();
    let Some(start) = after.find('[') else {
        return Vec::new();
    };
    let Some(end) = after[start..].find(']') else {
        return Vec::new();
    };
    let body = &after[start + 1..start + end];
    body.split(',')
        .filter_map(|part| {
            let value = part.trim().trim_matches('"').trim_matches('\'');
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
        .take(24)
        .collect()
}

fn json_object_string_array(text: &str, object_key: &str, array_key: &str) -> Vec<String> {
    let pattern = format!("\"{object_key}\"");
    let Some(idx) = text.find(&pattern) else {
        return Vec::new();
    };
    let slice = &text[idx..];
    json_string_array_field(slice, array_key)
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-proj-snap-{}-{}",
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
    fn empty_snapshot_has_no_project() {
        let snap = ProjectSnapshot::empty();
        assert!(!snap.has_project());
        assert!(!snap.has_intelligence());
    }

    #[test]
    fn observe_cargo_project_intelligence() {
        let root = temp_dir("cargo");
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo"
edition = "2021"

[dependencies]
serde = "1"
axum = "0.7"
"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        let snap = observe_project_intelligence(
            &root,
            &ProjectSnapshotHostFacts {
                project_id: Some("project:demo".into()),
                name: Some("Demo".into()),
                project_type: Some("code".into()),
                root_directory: Some(root.display().to_string()),
                ..ProjectSnapshotHostFacts::default()
            },
        );
        assert!(snap.has_project());
        assert!(snap.has_intelligence());
        assert_eq!(snap.package_manager, Some(PackageManagerKind::Cargo));
        assert!(snap.languages.iter().any(|lang| lang == "rust"));
        assert!(snap.frameworks.iter().any(|fw| fw == "axum"));
        assert_eq!(
            snap.cargo.as_ref().and_then(|c| c.package_name.as_deref()),
            Some("demo")
        );
        assert!(snap
            .dependency_summary
            .top_level
            .iter()
            .any(|dep| dep == "serde"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn observe_npm_project_intelligence() {
        let root = temp_dir("npm");
        fs::write(
            root.join("package.json"),
            r#"{
  "name": "web",
  "private": true,
  "scripts": { "dev": "next dev", "build": "next build" },
  "dependencies": { "next": "14.0.0", "react": "18.0.0" }
}"#,
        )
        .unwrap();
        fs::write(root.join("tsconfig.json"), "{}").unwrap();
        let snap = observe_project_intelligence(&root, &ProjectSnapshotHostFacts::default());
        assert_eq!(snap.package_manager, Some(PackageManagerKind::Npm));
        assert!(snap.languages.iter().any(|lang| lang == "typescript"));
        assert!(snap.frameworks.iter().any(|fw| fw == "next"));
        assert_eq!(
            snap.npm.as_ref().and_then(|n| n.package_name.as_deref()),
            Some("web")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn snapshot_ignores_timestamp_for_equality() {
        let mut a = ProjectSnapshot::from_observation(ProjectSnapshotObservation {
            metadata: ProjectMetadata {
                project_id: Some("p1".into()),
                ..ProjectMetadata::default()
            },
            languages: vec!["rust".into()],
            timestamp: Some(1),
            ..ProjectSnapshotObservation::default()
        });
        let mut b = a.clone();
        b.timestamp = 99;
        assert_eq!(a, b);
        a.languages.push("go".into());
        assert_ne!(a, b);
    }
}
