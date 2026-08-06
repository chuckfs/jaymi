//! Application-owned background context maintenance.
//!
//! Slow provider updates (git status, workspace inventory, diagnostics, file
//! summaries) run off the conversational path. Workers publish **completed**
//! snapshots; [`crate::Application::prepare_context_session`] merges the latest
//! completed values into [`jaymi_context::ContextSessionInputs`] without waiting.
//!
//! Conversation still assembles exclusively through
//! [`jaymi_context::ContextEngine::assemble_with`] — maintenance never builds a
//! parallel context bundle and never bypasses the Context Engine.
//!
//! ## Ownership
//!
//! | Kind | Owns refresh | Consumes via |
//! |------|--------------|--------------|
//! | Git status | Application maintenance (read-only `GitProvider`) | `GitStatusProvider` ← session |
//! | Workspace inventory | Application maintenance (filesystem walk) | `WorkspaceInventoryProvider` ← session |
//! | Diagnostics | Application maintenance (`ProblemsRegistry`) | `DiagnosticsProvider` ← session |
//! | File summaries | Application maintenance (file head read) | `FileSummariesProvider` ← session |
//!
//! Mutating Git / path tools still go Planner → Tool → Provider. Maintenance is
//! host-side snapshot refresh only.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_capabilities::{
    build_explorer_tree, ExplorerNode, ExplorerStatus, GitFileEntry, GitStatusState, ProblemIssue,
    ProblemsCollectContext, ProblemsRegistry,
};
use jaymi_context::{
    BundleDiagnostic, DiagnosticsSection, FileSummariesSection, FileSummaryEntry, GitStatusSection,
    WorkspaceInventorySection,
};
use jaymi_providers::{GitProvider, Provider};

/// Which slow snapshot to refresh in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaintenanceKind {
    /// Git status / branch / dirty counts.
    GitStatus,
    /// Workspace file/directory inventory (+ explorer tree for UI).
    WorkspaceInventory,
    /// Aggregated Problems / diagnostics.
    Diagnostics,
    /// Lightweight open-file head summaries.
    FileSummaries,
}

impl MaintenanceKind {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitStatus => "git_status",
            Self::WorkspaceInventory => "workspace_inventory",
            Self::Diagnostics => "diagnostics",
            Self::FileSummaries => "file_summaries",
        }
    }
}

/// Latest completed maintenance snapshots for Context session merge.
#[derive(Debug, Clone, Default)]
pub struct CompletedMaintenanceSnapshots {
    /// Git status section for ContextSessionInputs.
    pub git_status: Option<GitStatusSection>,
    /// Workspace inventory section for ContextSessionInputs.
    pub workspace_inventory: Option<WorkspaceInventorySection>,
    /// Diagnostics section for ContextSessionInputs (preferred over live Coding when set).
    pub diagnostics: Option<DiagnosticsSection>,
    /// File summaries for ContextSessionInputs.
    pub file_summaries: Option<FileSummariesSection>,
    /// Monotonic generation bumped when any completed snapshot lands.
    pub generation: u64,
}

/// UI-side apply payload produced when a background job completes.
#[derive(Debug, Clone)]
pub enum MaintenanceUiUpdate {
    /// Refresh Coding Git panel state.
    Git(GitStatusState),
    /// Refresh Project Explorer tree.
    Explorer {
        root: String,
        nodes: Vec<ExplorerNode>,
        status: ExplorerStatus,
    },
    /// Refresh Problems panel issues.
    Problems(Vec<ProblemIssue>),
}

/// Inputs captured on the main thread before spawning a background job.
#[derive(Debug, Clone)]
pub struct MaintenanceJobRequest {
    /// Which snapshot to refresh.
    pub kind: MaintenanceKind,
    /// Absolute project root, when known.
    pub project_root: Option<PathBuf>,
    /// Open editor paths for file summaries.
    pub open_file_paths: Vec<String>,
    /// Problems collect context (diagnostics jobs only).
    pub problems_context: Option<ProblemsCollectContext>,
    /// Problems registry (diagnostics jobs only).
    pub problems_registry: Option<Arc<ProblemsRegistry>>,
}

/// Application-owned maintenance coordinator.
#[derive(Debug)]
pub struct ContextMaintenance {
    completed: Mutex<CompletedMaintenanceSnapshots>,
    inflight: Mutex<HashSet<MaintenanceKind>>,
    ui_tx: Sender<MaintenanceUiUpdate>,
    ui_rx: Mutex<Receiver<MaintenanceUiUpdate>>,
    completed_tx: Sender<CompletedPayload>,
    completed_rx: Mutex<Receiver<CompletedPayload>>,
    jobs_started: AtomicU64,
    jobs_completed: AtomicU64,
}

#[derive(Debug, Clone)]
enum CompletedPayload {
    Git {
        section: GitStatusSection,
        ui: GitStatusState,
    },
    Inventory {
        section: WorkspaceInventorySection,
        ui: Option<(String, Vec<ExplorerNode>, ExplorerStatus)>,
    },
    Diagnostics {
        section: DiagnosticsSection,
        ui: Vec<ProblemIssue>,
    },
    FileSummaries {
        section: FileSummariesSection,
    },
}

impl ContextMaintenance {
    /// Create an empty maintenance coordinator.
    pub fn new() -> Self {
        let (ui_tx, ui_rx) = mpsc::channel();
        let (completed_tx, completed_rx) = mpsc::channel();
        Self {
            completed: Mutex::new(CompletedMaintenanceSnapshots::default()),
            inflight: Mutex::new(HashSet::new()),
            ui_tx,
            ui_rx: Mutex::new(ui_rx),
            completed_tx,
            completed_rx: Mutex::new(completed_rx),
            jobs_started: AtomicU64::new(0),
            jobs_completed: AtomicU64::new(0),
        }
    }

    /// Monotonic completed-snapshot generation.
    pub fn generation(&self) -> u64 {
        self.completed
            .lock()
            .map(|guard| guard.generation)
            .unwrap_or(0)
    }

    /// Jobs started (diagnostics).
    pub fn jobs_started(&self) -> u64 {
        self.jobs_started.load(Ordering::Relaxed)
    }

    /// Jobs completed (diagnostics).
    pub fn jobs_completed(&self) -> u64 {
        self.jobs_completed.load(Ordering::Relaxed)
    }

    /// True when a kind is currently refreshing.
    pub fn is_inflight(&self, kind: MaintenanceKind) -> bool {
        self.inflight
            .lock()
            .map(|guard| guard.contains(&kind))
            .unwrap_or(false)
    }

    /// Clone of the latest completed snapshots (never blocks on workers).
    pub fn latest_completed(&self) -> CompletedMaintenanceSnapshots {
        self.drain_completed_into_store();
        self.completed
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Publish a completed Git context section without a background job (after Planner mutate).
    ///
    /// Does not enqueue a UI update — the caller already applied Coding Git state.
    pub fn publish_git_section(&self, section: GitStatusSection) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::GitStatus);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.git_status = Some(section);
        }
    }

    /// Publish completed diagnostics into the snapshot store (caller may also update UI).
    pub fn publish_diagnostics_section(&self, section: DiagnosticsSection) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::Diagnostics);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.diagnostics = Some(section);
        }
    }

    /// Schedule a background refresh. Returns `false` when already in flight.
    ///
    /// Never blocks the caller on I/O.
    pub fn schedule(&self, request: MaintenanceJobRequest) -> bool {
        {
            let Ok(mut inflight) = self.inflight.lock() else {
                return false;
            };
            if !inflight.insert(request.kind) {
                return false;
            }
        }
        self.jobs_started.fetch_add(1, Ordering::Relaxed);
        let completed_tx = self.completed_tx.clone();
        let kind = request.kind;
        thread::Builder::new()
            .name(format!("jaymi-ctx-maint-{}", kind.as_str()))
            .spawn(move || {
                let payload = match kind {
                    MaintenanceKind::GitStatus => run_git_status(request.project_root.as_deref()),
                    MaintenanceKind::WorkspaceInventory => {
                        run_workspace_inventory(request.project_root.as_deref())
                    }
                    MaintenanceKind::Diagnostics => run_diagnostics(
                        request.problems_registry,
                        request.problems_context,
                    ),
                    MaintenanceKind::FileSummaries => {
                        run_file_summaries(&request.open_file_paths)
                    }
                };
                let _ = completed_tx.send(payload);
            })
            .ok();
        true
    }

    /// Schedule every maintenance kind suitable for a coding project open.
    pub fn schedule_coding_open(&self, request: MaintenanceJobRequest) {
        let root = request.project_root.clone();
        let open_files = request.open_file_paths.clone();
        let problems_context = request.problems_context.clone();
        let problems_registry = request.problems_registry.clone();

        for kind in [
            MaintenanceKind::WorkspaceInventory,
            MaintenanceKind::GitStatus,
            MaintenanceKind::Diagnostics,
            MaintenanceKind::FileSummaries,
        ] {
            let _ = self.schedule(MaintenanceJobRequest {
                kind,
                project_root: root.clone(),
                open_file_paths: open_files.clone(),
                problems_context: problems_context.clone(),
                problems_registry: problems_registry.clone(),
            });
        }
    }

    /// Drain completed worker payloads into the snapshot store and return UI updates.
    ///
    /// Non-blocking. Safe to call every UI frame and before prepare_context_session.
    pub fn pump(&self) -> Vec<MaintenanceUiUpdate> {
        self.drain_completed_into_store();
        let Ok(rx) = self.ui_rx.lock() else {
            return Vec::new();
        };
        let mut updates = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(update) => updates.push(update),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        updates
    }

    fn drain_completed_into_store(&self) {
        let Ok(rx) = self.completed_rx.lock() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(payload) => {
                    self.apply_completed(payload);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn apply_completed(&self, payload: CompletedPayload) {
        let kind = match &payload {
            CompletedPayload::Git { .. } => MaintenanceKind::GitStatus,
            CompletedPayload::Inventory { .. } => MaintenanceKind::WorkspaceInventory,
            CompletedPayload::Diagnostics { .. } => MaintenanceKind::Diagnostics,
            CompletedPayload::FileSummaries { .. } => MaintenanceKind::FileSummaries,
        };
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&kind);
        }
        self.jobs_completed.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            match &payload {
                CompletedPayload::Git { section, .. } => {
                    store.git_status = Some(section.clone());
                }
                CompletedPayload::Inventory { section, .. } => {
                    store.workspace_inventory = Some(section.clone());
                }
                CompletedPayload::Diagnostics { section, .. } => {
                    store.diagnostics = Some(section.clone());
                }
                CompletedPayload::FileSummaries { section } => {
                    store.file_summaries = Some(section.clone());
                }
            }
        }

        match payload {
            CompletedPayload::Git { ui, .. } => {
                let _ = self.ui_tx.send(MaintenanceUiUpdate::Git(ui));
            }
            CompletedPayload::Inventory { ui: Some((root, nodes, status)), .. } => {
                let _ = self.ui_tx.send(MaintenanceUiUpdate::Explorer {
                    root,
                    nodes,
                    status,
                });
            }
            CompletedPayload::Inventory { ui: None, .. } => {}
            CompletedPayload::Diagnostics { ui, .. } => {
                let _ = self.ui_tx.send(MaintenanceUiUpdate::Problems(ui));
            }
            CompletedPayload::FileSummaries { .. } => {}
        }
    }
}

impl Default for ContextMaintenance {
    fn default() -> Self {
        Self::new()
    }
}

fn run_git_status(project_root: Option<&Path>) -> CompletedPayload {
    let Some(root) = project_root else {
        let section = GitStatusSection {
            is_repository: false,
            summary: "No open project".into(),
            ..GitStatusSection::default()
        };
        let ui = GitStatusState {
            is_repository: false,
            summary: "No open project".into(),
            last_error: Some("open a project to use Git".into()),
            ..GitStatusState::default()
        };
        return CompletedPayload::Git { section, ui };
    };

    let mut provider = GitProvider::new();
    let snapshot = match (|| -> jaymi_core::JaymiResult<_> {
        provider.initialize()?;
        provider.status(root)
    })() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let message = error.message().to_string();
            let section = GitStatusSection {
                is_repository: false,
                summary: "unavailable".into(),
                ..GitStatusSection::default()
            };
            let ui = GitStatusState {
                is_repository: false,
                summary: "unavailable".into(),
                last_error: Some(message),
                ..GitStatusState::default()
            };
            return CompletedPayload::Git { section, ui };
        }
    };

    let to_entries = |items: &[jaymi_core::GitPathStatus]| -> Vec<GitFileEntry> {
        items
            .iter()
            .map(|item| GitFileEntry {
                path: item.path.clone(),
                status: item.status.clone(),
            })
            .collect()
    };

    let mut sample_paths = Vec::new();
    for path in snapshot
        .modified
        .iter()
        .chain(snapshot.staged.iter())
        .chain(snapshot.untracked.iter())
        .map(|item| item.path.clone())
    {
        if sample_paths.len() >= 8 {
            break;
        }
        if !sample_paths.contains(&path) {
            sample_paths.push(path);
        }
    }

    let section = GitStatusSection {
        is_repository: snapshot.is_repository,
        branch: snapshot.branch.clone(),
        summary: snapshot.summary.clone(),
        modified_count: snapshot.modified.len(),
        staged_count: snapshot.staged.len(),
        untracked_count: snapshot.untracked.len(),
        sample_paths,
    };

    let mut ui = GitStatusState::default();
    ui.apply_snapshot(
        snapshot.is_repository,
        Some(snapshot.repo_root.to_string_lossy().into_owned()),
        snapshot.branch,
        snapshot.summary,
        to_entries(&snapshot.modified),
        to_entries(&snapshot.added),
        to_entries(&snapshot.deleted),
        to_entries(&snapshot.staged),
        to_entries(&snapshot.untracked),
    );

    CompletedPayload::Git { section, ui }
}

fn run_workspace_inventory(project_root: Option<&Path>) -> CompletedPayload {
    let Some(root) = project_root else {
        let section = WorkspaceInventorySection {
            status: "empty".into(),
            ..WorkspaceInventorySection::default()
        };
        return CompletedPayload::Inventory {
            section,
            ui: None,
        };
    };

    let root_display = root.to_string_lossy().into_owned();
    match walk_inventory(root) {
        Ok(walk) => {
            let section = WorkspaceInventorySection {
                root: Some(root_display.clone()),
                file_count: walk.file_count,
                directory_count: walk.directory_count,
                status: "ready".into(),
                sample_paths: walk.sample_paths.clone(),
            };
            let nodes = build_explorer_tree(&root_display, &walk.flat);
            CompletedPayload::Inventory {
                section,
                ui: Some((root_display, nodes, ExplorerStatus::Ready)),
            }
        }
        Err(message) => {
            let section = WorkspaceInventorySection {
                root: Some(root_display.clone()),
                status: "error".into(),
                ..WorkspaceInventorySection::default()
            };
            CompletedPayload::Inventory {
                section,
                ui: Some((root_display, Vec::new(), ExplorerStatus::Error(message))),
            }
        }
    }
}

struct InventoryWalk {
    file_count: usize,
    directory_count: usize,
    sample_paths: Vec<String>,
    flat: Vec<(String, String, bool)>,
}

fn walk_inventory(root: &Path) -> Result<InventoryWalk, String> {
    const MAX_ENTRIES: usize = 4_000;
    const MAX_SAMPLE: usize = 12;
    let skip_names = [
        ".git",
        "node_modules",
        "target",
        ".jaymi",
        "dist",
        "build",
        ".next",
    ];

    let mut walk = InventoryWalk {
        file_count: 0,
        directory_count: 0,
        sample_paths: Vec::new(),
        flat: Vec::new(),
    };

    fn visit(
        dir: &Path,
        root: &Path,
        walk: &mut InventoryWalk,
        skip_names: &[&str],
        max_entries: usize,
        max_sample: usize,
    ) -> Result<(), String> {
        if walk.flat.len() >= max_entries {
            return Ok(());
        }
        let entries = fs::read_dir(dir).map_err(|error| error.to_string())?;
        let mut collected = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if skip_names.iter().any(|skip| *skip == name.as_str()) {
                continue;
            }
            let is_dir = entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false);
            collected.push((path, name, is_dir));
        }
        collected.sort_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then(left.1.to_lowercase().cmp(&right.1.to_lowercase()))
        });
        for (path, name, is_dir) in collected {
            if walk.flat.len() >= max_entries {
                break;
            }
            let display = path.to_string_lossy().into_owned();
            walk.flat.push((display.clone(), name, is_dir));
            if is_dir {
                walk.directory_count = walk.directory_count.saturating_add(1);
                visit(&path, root, walk, skip_names, max_entries, max_sample)?;
            } else {
                walk.file_count = walk.file_count.saturating_add(1);
                if walk.sample_paths.len() < max_sample {
                    if let Ok(relative) = path.strip_prefix(root) {
                        walk.sample_paths
                            .push(relative.to_string_lossy().into_owned());
                    } else {
                        walk.sample_paths.push(display);
                    }
                }
            }
        }
        Ok(())
    }

    visit(root, root, &mut walk, &skip_names, MAX_ENTRIES, MAX_SAMPLE)?;
    Ok(walk)
}

fn run_diagnostics(
    registry: Option<Arc<ProblemsRegistry>>,
    context: Option<ProblemsCollectContext>,
) -> CompletedPayload {
    let (section, ui) = match (registry, context) {
        (Some(registry), Some(context)) => match registry.collect_all(&context) {
            Ok(issues) => {
                let diagnostics = issues
                    .iter()
                    .map(|issue| BundleDiagnostic {
                        path: issue.path.clone(),
                        severity: issue.severity.as_str().to_string(),
                        message: issue.message.clone(),
                        line: issue.line,
                        column: issue.column,
                        source: Some(if issue.source_label.is_empty() {
                            issue.source.clone()
                        } else {
                            issue.source_label.clone()
                        }),
                    })
                    .collect();
                (DiagnosticsSection { diagnostics }, issues)
            }
            Err(_) => (DiagnosticsSection::default(), Vec::new()),
        },
        _ => (DiagnosticsSection::default(), Vec::new()),
    };
    CompletedPayload::Diagnostics { section, ui }
}

fn run_file_summaries(open_file_paths: &[String]) -> CompletedPayload {
    const MAX_FILES: usize = 8;
    const MAX_HEAD_LINES: usize = 12;
    const MAX_SUMMARY_CHARS: usize = 400;

    let mut entries = Vec::new();
    for path in open_file_paths.iter().take(MAX_FILES) {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        let line_count = contents.lines().count() as u32;
        let mut head = String::new();
        for line in contents.lines().take(MAX_HEAD_LINES) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !head.is_empty() {
                head.push(' ');
            }
            head.push_str(trimmed);
            if head.chars().count() >= MAX_SUMMARY_CHARS {
                break;
            }
        }
        if head.chars().count() > MAX_SUMMARY_CHARS {
            head = head.chars().take(MAX_SUMMARY_CHARS).collect();
            head.push('…');
        }
        if head.is_empty() {
            head = format!("({line_count} lines)");
        }
        entries.push(FileSummaryEntry {
            path: path.clone(),
            language: language_guess(path).map(str::to_string),
            line_count: Some(line_count),
            summary: head,
        });
    }
    CompletedPayload::FileSummaries {
        section: FileSummariesSection { entries },
    }
}

fn language_guess(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?;
    Some(match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "md" => "markdown",
        "toml" => "toml",
        "json" => "json",
        "yml" | "yaml" => "yaml",
        "go" => "go",
        "swift" => "swift",
        _ => return None,
    })
}

/// Merge completed maintenance into session inputs (never waits for in-flight jobs).
pub fn merge_completed_into_session(
    inputs: &mut jaymi_context::ContextSessionInputs,
    completed: &CompletedMaintenanceSnapshots,
) {
    if let Some(git) = &completed.git_status {
        inputs.git_status = git.clone();
    }
    if let Some(inventory) = &completed.workspace_inventory {
        inputs.workspace_inventory = inventory.clone();
    }
    if let Some(diagnostics) = &completed.diagnostics {
        inputs.diagnostics = diagnostics.clone();
    }
    if let Some(summaries) = &completed.file_summaries {
        inputs.file_summaries = summaries.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_dedupes_inflight_and_completes() {
        let maintenance = ContextMaintenance::new();
        let dir = std::env::temp_dir().join(format!(
            "jaymi-maint-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("hello.txt"), "hello\nworld\n").unwrap();

        assert!(maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::WorkspaceInventory,
            project_root: Some(dir.clone()),
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
        }));
        assert!(!maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::WorkspaceInventory,
            project_root: Some(dir.clone()),
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
        }));

        let started = std::time::Instant::now();
        while maintenance.jobs_completed() == 0 && started.elapsed() < std::time::Duration::from_secs(2)
        {
            let _ = maintenance.pump();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(maintenance.jobs_completed() >= 1);
        let completed = maintenance.latest_completed();
        assert!(completed.workspace_inventory.is_some());
        assert_eq!(
            completed.workspace_inventory.as_ref().unwrap().status,
            "ready"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn merge_prefers_completed_snapshots() {
        let mut inputs = jaymi_context::ContextSessionInputs::default();
        inputs.diagnostics = DiagnosticsSection {
            diagnostics: vec![BundleDiagnostic {
                path: Some("old.rs".into()),
                severity: "warning".into(),
                message: "stale".into(),
                line: Some(1),
                column: Some(0),
                source: None,
            }],
        };
        let completed = CompletedMaintenanceSnapshots {
            diagnostics: Some(DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    path: Some("new.rs".into()),
                    severity: "error".into(),
                    message: "fresh".into(),
                    line: Some(2),
                    column: Some(1),
                    source: Some("lsp".into()),
                }],
            }),
            git_status: Some(GitStatusSection {
                is_repository: true,
                branch: Some("main".into()),
                summary: "clean".into(),
                ..GitStatusSection::default()
            }),
            ..CompletedMaintenanceSnapshots::default()
        };
        merge_completed_into_session(&mut inputs, &completed);
        assert_eq!(inputs.diagnostics.diagnostics[0].message, "fresh");
        assert_eq!(inputs.git_status.branch.as_deref(), Some("main"));
    }
}
