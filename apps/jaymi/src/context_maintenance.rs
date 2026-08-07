//! Application-owned background context maintenance.
//!
//! Slow provider updates (git status / GitSnapshot, workspace inventory,
//! diagnostics, file summaries), ambient [`jaymi_context::WorkspaceSnapshot`]
//! refresh (Sprint B2.2), ambient [`jaymi_context::EditorSnapshot`] refresh
//! (Sprint B2.3), ambient [`jaymi_context::ProjectSnapshot`] refresh (Sprint
//! B2.4), ambient [`jaymi_context::GitSnapshot`] refresh (Sprint B2.5), and
//! ambient [`jaymi_context::RuntimeSnapshot`] refresh (Sprint B2.6) run off the
//! conversational path. Workers publish **completed** snapshots;
//! [`crate::Application::prepare_context_session`] merges the latest completed
//! values into [`jaymi_context::ContextSessionInputs`] without waiting.
//!
//! Conversation still assembles exclusively through
//! [`jaymi_context::ContextEngine::assemble_with`] — maintenance never builds a
//! parallel context bundle and never bypasses the Context Engine.
//!
//! ## Ownership
//!
//! | Kind | Owns refresh | Consumes via |
//! |------|--------------|--------------|
//! | Git status / GitSnapshot | Application maintenance (read-only `GitProvider`) | `GitStatusProvider` ← session (`git_snapshot` / `git_status`) |
//! | Workspace inventory | Application maintenance (filesystem walk) | `WorkspaceInventoryProvider` ← session |
//! | Diagnostics | Application maintenance (`ProblemsRegistry`) | `DiagnosticsProvider` ← session |
//! | File summaries | Application maintenance (file head read) | `FileSummariesProvider` ← session |
//! | Workspace snapshot | Application maintenance (host observation only) | `ContextSessionInputs.workspace_snapshot` |
//! | Editor snapshot | Application maintenance (host observation ± read-only `LspProvider`) | Context providers via `editor_snapshot` |
//! | Project snapshot | Application maintenance (marker / shallow FS observation) | `ProjectProvider` via `project_snapshot` |
//! | Runtime snapshot | Application maintenance (Coding terminal + TerminalProvider alive list) | `RuntimeProvider` via `runtime_snapshot` |
//!
//! Mutating Git / path tools still go Planner → Tool → Provider. Maintenance is
//! host-side snapshot refresh only — never Planner tool execute, reasoning, or LLMs.
//! Interactive LSP (rename / goto UI) stays on Application `coding_lsp_*` → Planner.
//! Project intelligence FS observation never runs on the request / assemble path.
//! Runtime observation never re-runs cargo / tests; conversation never waits.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_capabilities::{
    build_explorer_tree, CodingState, ExplorerNode, ExplorerStatus, GitFileEntry, GitStatusState,
    ProblemIssue, ProblemsCollectContext, ProblemsRegistry,
};
use jaymi_context::{
    observe_project_intelligence, observe_runtime_intelligence, BundleDiagnostic,
    DiagnosticsSection, EditorHover, EditorRange, EditorReference, EditorSnapshot, EditorSymbol,
    FileSummariesSection, FileSummaryEntry, GitSnapshot, GitStatusSection, ProjectSnapshot,
    ProjectSnapshotHostFacts, RuntimeSnapshot, RuntimeSnapshotHostFacts,
    WorkspaceInventorySection, WorkspaceSnapshot,
};
use jaymi_core::{LspOperation, LspRequest};
use jaymi_providers::{GitProvider, LspProvider, Provider};

use crate::context_session::{
    capture_editor_snapshot_from_coding, capture_workspace_snapshot_from_coding,
    EditorSnapshotEnrichment, WorkspaceSnapshotHostFacts,
};

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
    /// Canonical Coding [`WorkspaceSnapshot`] (Sprint B2.2 ambient).
    WorkspaceSnapshot,
    /// Canonical [`EditorSnapshot`] (Sprint B2.3 ambient editor intelligence).
    EditorSnapshot,
    /// Canonical [`ProjectSnapshot`] (Sprint B2.4 ambient project intelligence).
    ProjectSnapshot,
    /// Canonical [`RuntimeSnapshot`] (Sprint B2.6 ambient runtime intelligence).
    RuntimeSnapshot,
}

impl MaintenanceKind {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitStatus => "git_status",
            Self::WorkspaceInventory => "workspace_inventory",
            Self::Diagnostics => "diagnostics",
            Self::FileSummaries => "file_summaries",
            Self::WorkspaceSnapshot => "workspace_snapshot",
            Self::EditorSnapshot => "editor_snapshot",
            Self::ProjectSnapshot => "project_snapshot",
            Self::RuntimeSnapshot => "runtime_snapshot",
        }
    }

    fn coalesces(self) -> bool {
        matches!(
            self,
            Self::WorkspaceSnapshot
                | Self::EditorSnapshot
                | Self::ProjectSnapshot
                | Self::RuntimeSnapshot
        )
    }
}

/// Latest completed maintenance snapshots for Context session merge.
#[derive(Debug, Clone, Default)]
pub struct CompletedMaintenanceSnapshots {
    /// Git status section for ContextSessionInputs.
    pub git_status: Option<GitStatusSection>,
    /// Canonical Git intelligence observation (Sprint B2.5).
    pub git_snapshot: Option<GitSnapshot>,
    /// Workspace inventory section for ContextSessionInputs.
    pub workspace_inventory: Option<WorkspaceInventorySection>,
    /// Diagnostics section for ContextSessionInputs (preferred over live Coding when set).
    pub diagnostics: Option<DiagnosticsSection>,
    /// File summaries for ContextSessionInputs.
    pub file_summaries: Option<FileSummariesSection>,
    /// Canonical Coding workspace observation (Sprint B2.2).
    pub workspace_snapshot: Option<WorkspaceSnapshot>,
    /// Canonical editor intelligence observation (Sprint B2.3).
    pub editor_snapshot: Option<EditorSnapshot>,
    /// Canonical project intelligence observation (Sprint B2.4).
    pub project_snapshot: Option<ProjectSnapshot>,
    /// Canonical runtime intelligence observation (Sprint B2.6).
    pub runtime_snapshot: Option<RuntimeSnapshot>,
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

/// Host-observed Coding + project facts for ambient WorkspaceSnapshot capture.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceSnapshotCaptureInput {
    /// Active Experience workspace kind id.
    pub workspace_kind: Option<String>,
    /// Cloned CodingState when Coding is open.
    pub coding: Option<CodingState>,
    /// Project / branch facts from Project Engine + git contributions.
    pub facts: WorkspaceSnapshotHostFacts,
}

/// Host-observed Coding facts for ambient EditorSnapshot capture.
#[derive(Debug, Clone, Default)]
pub struct EditorSnapshotCaptureInput {
    /// Cloned CodingState when Coding is open.
    pub coding: Option<CodingState>,
    /// Optional read-only LspProvider for hover / references enrichment.
    ///
    /// Never routes through Planner / language_server tool.
    pub lsp: Option<Arc<LspProvider>>,
    /// Workspace root for LSP requests.
    pub project_root: Option<PathBuf>,
}

/// Host-observed facts for ambient ProjectSnapshot capture.
#[derive(Debug, Clone, Default)]
pub struct ProjectSnapshotCaptureInput {
    /// Absolute project root when known.
    pub project_root: Option<PathBuf>,
    /// Project Engine identity facts (no filesystem scan).
    pub facts: ProjectSnapshotHostFacts,
}

/// Host-observed terminal facts for ambient RuntimeSnapshot capture.
#[derive(Debug, Clone, Default)]
pub struct RuntimeSnapshotCaptureInput {
    /// Coding / TerminalProvider session facts (no cargo re-run).
    pub facts: RuntimeSnapshotHostFacts,
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
    /// Coding observation payload (WorkspaceSnapshot jobs).
    pub workspace_snapshot_input: Option<WorkspaceSnapshotCaptureInput>,
    /// Editor intelligence payload (EditorSnapshot jobs).
    pub editor_snapshot_input: Option<EditorSnapshotCaptureInput>,
    /// Project intelligence payload (ProjectSnapshot jobs).
    pub project_snapshot_input: Option<ProjectSnapshotCaptureInput>,
    /// Runtime intelligence payload (RuntimeSnapshot jobs).
    pub runtime_snapshot_input: Option<RuntimeSnapshotCaptureInput>,
}

/// Application-owned maintenance coordinator.
#[derive(Debug)]
pub struct ContextMaintenance {
    completed: Mutex<CompletedMaintenanceSnapshots>,
    inflight: Mutex<HashSet<MaintenanceKind>>,
    /// Kinds that need another refresh after the current inflight job finishes.
    pending_reschedule: Mutex<HashSet<MaintenanceKind>>,
    ui_tx: Sender<MaintenanceUiUpdate>,
    ui_rx: Mutex<Receiver<MaintenanceUiUpdate>>,
    completed_tx: Sender<CompletedPayload>,
    completed_rx: Mutex<Receiver<CompletedPayload>>,
    jobs_started: AtomicU64,
    jobs_completed: AtomicU64,
    /// True when WorkspaceSnapshot was requested while a job was already inflight.
    workspace_snapshot_coalesced: AtomicBool,
    /// True when EditorSnapshot was requested while a job was already inflight.
    editor_snapshot_coalesced: AtomicBool,
    /// True when ProjectSnapshot was requested while a job was already inflight.
    project_snapshot_coalesced: AtomicBool,
    /// True when RuntimeSnapshot was requested while a job was already inflight.
    runtime_snapshot_coalesced: AtomicBool,
}

#[derive(Debug, Clone)]
enum CompletedPayload {
    Git {
        snapshot: GitSnapshot,
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
    WorkspaceSnapshot {
        snapshot: WorkspaceSnapshot,
    },
    EditorSnapshot {
        snapshot: EditorSnapshot,
    },
    ProjectSnapshot {
        snapshot: ProjectSnapshot,
    },
    RuntimeSnapshot {
        snapshot: RuntimeSnapshot,
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
            pending_reschedule: Mutex::new(HashSet::new()),
            ui_tx,
            ui_rx: Mutex::new(ui_rx),
            completed_tx,
            completed_rx: Mutex::new(completed_rx),
            jobs_started: AtomicU64::new(0),
            jobs_completed: AtomicU64::new(0),
            workspace_snapshot_coalesced: AtomicBool::new(false),
            editor_snapshot_coalesced: AtomicBool::new(false),
            project_snapshot_coalesced: AtomicBool::new(false),
            runtime_snapshot_coalesced: AtomicBool::new(false),
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

    /// True when a WorkspaceSnapshot refresh was coalesced while a job was inflight.
    pub fn workspace_snapshot_needs_reschedule(&self) -> bool {
        self.workspace_snapshot_coalesced.load(Ordering::Relaxed)
            || self
                .pending_reschedule
                .lock()
                .map(|guard| guard.contains(&MaintenanceKind::WorkspaceSnapshot))
                .unwrap_or(false)
    }

    /// Clear the coalesced WorkspaceSnapshot reschedule flag (caller must schedule).
    pub fn take_workspace_snapshot_reschedule(&self) -> bool {
        self.take_coalesced_reschedule(
            MaintenanceKind::WorkspaceSnapshot,
            &self.workspace_snapshot_coalesced,
        )
    }

    /// Clear the coalesced EditorSnapshot reschedule flag (caller must schedule).
    pub fn take_editor_snapshot_reschedule(&self) -> bool {
        self.take_coalesced_reschedule(
            MaintenanceKind::EditorSnapshot,
            &self.editor_snapshot_coalesced,
        )
    }

    /// Clear the coalesced ProjectSnapshot reschedule flag (caller must schedule).
    pub fn take_project_snapshot_reschedule(&self) -> bool {
        self.take_coalesced_reschedule(
            MaintenanceKind::ProjectSnapshot,
            &self.project_snapshot_coalesced,
        )
    }

    /// Clear the coalesced RuntimeSnapshot reschedule flag (caller must schedule).
    pub fn take_runtime_snapshot_reschedule(&self) -> bool {
        self.take_coalesced_reschedule(
            MaintenanceKind::RuntimeSnapshot,
            &self.runtime_snapshot_coalesced,
        )
    }

    fn take_coalesced_reschedule(
        &self,
        kind: MaintenanceKind,
        flag: &AtomicBool,
    ) -> bool {
        let coalesced = flag.swap(false, Ordering::Relaxed);
        let pending = self
            .pending_reschedule
            .lock()
            .map(|mut guard| guard.remove(&kind))
            .unwrap_or(false);
        coalesced || pending
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
    /// When a full [`GitSnapshot`] is available, prefer [`Self::publish_git_snapshot`].
    pub fn publish_git_section(&self, section: GitStatusSection) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::GitStatus);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.git_status = Some(section);
        }
    }

    /// Publish a completed [`GitSnapshot`] (+ derived section) without a background job.
    ///
    /// Observational store only — never assembles a ContextBundle.
    pub fn publish_git_snapshot(&self, snapshot: GitSnapshot) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::GitStatus);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.git_status = Some(snapshot.status_section());
            store.git_snapshot = Some(snapshot);
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

    /// Publish a completed [`WorkspaceSnapshot`] without a background job (bootstrap / close).
    ///
    /// Observational store only — never assembles a ContextBundle.
    pub fn publish_workspace_snapshot(&self, snapshot: WorkspaceSnapshot) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::WorkspaceSnapshot);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.workspace_snapshot = Some(snapshot);
        }
    }

    /// Publish a completed [`EditorSnapshot`] without a background job (bootstrap / close).
    ///
    /// Observational store only — never assembles a ContextBundle.
    pub fn publish_editor_snapshot(&self, snapshot: EditorSnapshot) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::EditorSnapshot);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.editor_snapshot = Some(snapshot);
        }
    }

    /// Publish a completed [`ProjectSnapshot`] without a background job (bootstrap / close).
    ///
    /// Observational store only — never assembles a ContextBundle.
    pub fn publish_project_snapshot(&self, snapshot: ProjectSnapshot) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::ProjectSnapshot);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.project_snapshot = Some(snapshot);
        }
    }

    /// Publish a completed [`RuntimeSnapshot`] without a background job (bootstrap / close).
    ///
    /// Observational store only — never assembles a ContextBundle.
    pub fn publish_runtime_snapshot(&self, snapshot: RuntimeSnapshot) {
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&MaintenanceKind::RuntimeSnapshot);
        }
        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            store.runtime_snapshot = Some(snapshot);
        }
    }

    /// Schedule a background refresh. Returns `false` when already in flight.
    ///
    /// Never blocks the caller on I/O. WorkspaceSnapshot / EditorSnapshot /
    /// ProjectSnapshot / RuntimeSnapshot requests that arrive while inflight are
    /// coalesced for a later Application reschedule.
    pub fn schedule(&self, request: MaintenanceJobRequest) -> bool {
        {
            let Ok(mut inflight) = self.inflight.lock() else {
                return false;
            };
            if !inflight.insert(request.kind) {
                if request.kind.coalesces() {
                    match request.kind {
                        MaintenanceKind::WorkspaceSnapshot => {
                            self.workspace_snapshot_coalesced
                                .store(true, Ordering::Relaxed);
                        }
                        MaintenanceKind::EditorSnapshot => {
                            self.editor_snapshot_coalesced
                                .store(true, Ordering::Relaxed);
                        }
                        MaintenanceKind::ProjectSnapshot => {
                            self.project_snapshot_coalesced
                                .store(true, Ordering::Relaxed);
                        }
                        MaintenanceKind::RuntimeSnapshot => {
                            self.runtime_snapshot_coalesced
                                .store(true, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                    if let Ok(mut pending) = self.pending_reschedule.lock() {
                        pending.insert(request.kind);
                    }
                }
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
                    MaintenanceKind::WorkspaceSnapshot => run_workspace_snapshot(
                        request.workspace_snapshot_input.unwrap_or_default(),
                    ),
                    MaintenanceKind::EditorSnapshot => {
                        run_editor_snapshot(request.editor_snapshot_input.unwrap_or_default())
                    }
                    MaintenanceKind::ProjectSnapshot => {
                        run_project_snapshot(request.project_snapshot_input.unwrap_or_default())
                    }
                    MaintenanceKind::RuntimeSnapshot => {
                        run_runtime_snapshot(request.runtime_snapshot_input.unwrap_or_default())
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
        let workspace_snapshot_input = request.workspace_snapshot_input.clone();
        let editor_snapshot_input = request.editor_snapshot_input.clone();
        let project_snapshot_input = request.project_snapshot_input.clone();
        let runtime_snapshot_input = request.runtime_snapshot_input.clone();

        for kind in [
            MaintenanceKind::WorkspaceInventory,
            MaintenanceKind::GitStatus,
            MaintenanceKind::Diagnostics,
            MaintenanceKind::FileSummaries,
            MaintenanceKind::WorkspaceSnapshot,
            MaintenanceKind::EditorSnapshot,
            MaintenanceKind::ProjectSnapshot,
            MaintenanceKind::RuntimeSnapshot,
        ] {
            let _ = self.schedule(MaintenanceJobRequest {
                kind,
                project_root: root.clone(),
                open_file_paths: open_files.clone(),
                problems_context: problems_context.clone(),
                problems_registry: problems_registry.clone(),
                workspace_snapshot_input: workspace_snapshot_input.clone(),
                editor_snapshot_input: editor_snapshot_input.clone(),
                project_snapshot_input: project_snapshot_input.clone(),
                runtime_snapshot_input: runtime_snapshot_input.clone(),
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
            CompletedPayload::WorkspaceSnapshot { .. } => MaintenanceKind::WorkspaceSnapshot,
            CompletedPayload::EditorSnapshot { .. } => MaintenanceKind::EditorSnapshot,
            CompletedPayload::ProjectSnapshot { .. } => MaintenanceKind::ProjectSnapshot,
            CompletedPayload::RuntimeSnapshot { .. } => MaintenanceKind::RuntimeSnapshot,
        };
        if let Ok(mut inflight) = self.inflight.lock() {
            inflight.remove(&kind);
        }
        self.jobs_completed.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut store) = self.completed.lock() {
            store.generation = store.generation.saturating_add(1);
            match &payload {
                CompletedPayload::Git {
                    snapshot, section, ..
                } => {
                    store.git_status = Some(section.clone());
                    store.git_snapshot = Some(snapshot.clone());
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
                CompletedPayload::WorkspaceSnapshot { snapshot } => {
                    store.workspace_snapshot = Some(snapshot.clone());
                }
                CompletedPayload::EditorSnapshot { snapshot } => {
                    store.editor_snapshot = Some(snapshot.clone());
                }
                CompletedPayload::ProjectSnapshot { snapshot } => {
                    store.project_snapshot = Some(snapshot.clone());
                }
                CompletedPayload::RuntimeSnapshot { snapshot } => {
                    store.runtime_snapshot = Some(snapshot.clone());
                }
            }
        }

        match payload {
            CompletedPayload::Git { ui, .. } => {
                let _ = self.ui_tx.send(MaintenanceUiUpdate::Git(ui));
            }
            CompletedPayload::Inventory {
                ui: Some((root, nodes, status)),
                ..
            } => {
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
            CompletedPayload::FileSummaries { .. }
            | CompletedPayload::WorkspaceSnapshot { .. }
            | CompletedPayload::EditorSnapshot { .. }
            | CompletedPayload::ProjectSnapshot { .. }
            | CompletedPayload::RuntimeSnapshot { .. } => {}
        }
    }
}

impl Default for ContextMaintenance {
    fn default() -> Self {
        Self::new()
    }
}

fn run_workspace_snapshot(input: WorkspaceSnapshotCaptureInput) -> CompletedPayload {
    let snapshot = capture_workspace_snapshot_from_coding(
        input.workspace_kind,
        input.coding.as_ref(),
        &input.facts,
    );
    CompletedPayload::WorkspaceSnapshot { snapshot }
}

fn run_editor_snapshot(input: EditorSnapshotCaptureInput) -> CompletedPayload {
    let enrichment = enrich_editor_from_lsp(
        input.lsp.as_ref(),
        input.coding.as_ref(),
        input.project_root.as_deref(),
    );
    let snapshot = capture_editor_snapshot_from_coding(input.coding.as_ref(), enrichment);
    CompletedPayload::EditorSnapshot { snapshot }
}

fn run_project_snapshot(input: ProjectSnapshotCaptureInput) -> CompletedPayload {
    let snapshot = match input.project_root.as_deref() {
        Some(root) => observe_project_intelligence(root, &input.facts),
        None => {
            let mut snap = ProjectSnapshot::empty();
            snap.metadata.project_id = input.facts.project_id;
            snap.metadata.name = input.facts.name;
            snap.metadata.description = input.facts.description;
            snap.metadata.root_directory = input.facts.root_directory;
            snap.metadata.project_type = input.facts.project_type;
            snap
        }
    };
    CompletedPayload::ProjectSnapshot { snapshot }
}

fn run_runtime_snapshot(input: RuntimeSnapshotCaptureInput) -> CompletedPayload {
    let snapshot = observe_runtime_intelligence(&input.facts);
    CompletedPayload::RuntimeSnapshot { snapshot }
}

/// Best-effort read-only LSP enrichment — never Planner / tool / Reasoning.
fn enrich_editor_from_lsp(
    lsp: Option<&Arc<LspProvider>>,
    coding: Option<&CodingState>,
    project_root: Option<&Path>,
) -> EditorSnapshotEnrichment {
    let mut enrichment = EditorSnapshotEnrichment::default();
    let (Some(lsp), Some(coding), Some(root)) = (lsp, coding, project_root) else {
        return enrichment;
    };
    let Some(session) = coding.editors.active_session() else {
        return enrichment;
    };
    let path = PathBuf::from(&session.path);
    let line = session.view.cursor.line;
    let character = session.view.cursor.column;
    let language = session
        .path
        .rsplit('.')
        .next()
        .map(|ext| match ext {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            _ => "plaintext",
        })
        .unwrap_or("plaintext")
        .to_string();

    let base = LspRequest {
        workspace_root: root.to_path_buf(),
        operation: LspOperation::Hover,
        path: Some(path.clone()),
        content: Some(session.content.clone()),
        language: Some(language),
        version: Some(1),
        line: Some(line),
        character: Some(character),
        new_name: None,
    };

    if let Ok(result) = lsp.execute(&base) {
        if let Some(hover) = result.hover {
            enrichment.hover = Some(EditorHover {
                contents: hover.contents.clone(),
                range: hover.range.map(|range| EditorRange {
                    start_line: range.start.line,
                    start_column: range.start.character,
                    end_line: range.end.line,
                    end_column: range.end.character,
                }),
            });
            // Best-effort symbol name from the first non-empty hover line.
            if let Some(name) = hover
                .contents
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with("```"))
                .map(|line| line.trim_matches('`').to_string())
            {
                if !name.is_empty() {
                    enrichment.symbol = Some(EditorSymbol {
                        name,
                        kind: None,
                        detail: None,
                        range: enrichment.hover.as_ref().and_then(|h| h.range),
                    });
                }
            }
        }
    }

    let mut refs_request = base;
    refs_request.operation = LspOperation::References;
    if let Ok(result) = lsp.execute(&refs_request) {
        enrichment.references = result
            .references
            .into_iter()
            .take(32)
            .map(|location| EditorReference {
                path: location.path,
                range: EditorRange {
                    start_line: location.range.start.line,
                    start_column: location.range.start.character,
                    end_line: location.range.end.line,
                    end_column: location.range.end.character,
                },
            })
            .collect();
    }

    enrichment
}

fn run_git_status(project_root: Option<&Path>) -> CompletedPayload {
    let Some(root) = project_root else {
        let snap = jaymi_context::GitSnapshot::from_observation(
            jaymi_context::GitSnapshotObservation {
                summary: "No open project".into(),
                ..Default::default()
            },
        );
        let section = snap.status_section();
        let ui = GitStatusState {
            is_repository: false,
            summary: "No open project".into(),
            last_error: Some("open a project to use Git".into()),
            ..GitStatusState::default()
        };
        return CompletedPayload::Git {
            snapshot: snap,
            section,
            ui,
        };
    };

    let mut provider = GitProvider::new();
    let snapshot = match (|| -> jaymi_core::JaymiResult<_> {
        provider.initialize()?;
        provider.status(root)
    })() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let message = error.message().to_string();
            let snap = jaymi_context::GitSnapshot::from_observation(
                jaymi_context::GitSnapshotObservation {
                    summary: "unavailable".into(),
                    ..Default::default()
                },
            );
            let section = snap.status_section();
            let ui = GitStatusState {
                is_repository: false,
                summary: "unavailable".into(),
                last_error: Some(message),
                ..GitStatusState::default()
            };
            return CompletedPayload::Git {
                snapshot: snap,
                section,
                ui,
            };
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
    let to_path_entries = |items: &[jaymi_core::GitPathStatus]| -> Vec<jaymi_context::GitPathEntry> {
        items
            .iter()
            .take(64)
            .map(|item| jaymi_context::GitPathEntry {
                path: item.path.clone(),
                status: item.status.clone(),
            })
            .collect()
    };

    // Dirty = modified + deleted worktree paths (not already staged-only deletes).
    let mut dirty = to_path_entries(&snapshot.modified);
    for entry in to_path_entries(&snapshot.deleted) {
        if !dirty.iter().any(|existing| existing.path == entry.path)
            && !snapshot
                .staged
                .iter()
                .any(|staged| staged.path == entry.path)
        {
            dirty.push(entry);
        }
    }

    let git_snapshot = jaymi_context::GitSnapshot::from_observation(
        jaymi_context::GitSnapshotObservation {
            is_repository: snapshot.is_repository,
            repo_root: Some(snapshot.repo_root.to_string_lossy().into_owned()),
            branch: snapshot.branch.clone(),
            head_sha: snapshot.head_sha.clone(),
            head_short: snapshot.head_short.clone(),
            summary: snapshot.summary.clone(),
            dirty,
            staged: to_path_entries(&snapshot.staged),
            untracked: to_path_entries(&snapshot.untracked),
            conflicts: to_path_entries(&snapshot.conflicts),
            recent_commits: snapshot.recent_commits.clone(),
            timestamp: None,
        },
    );
    let section = git_snapshot.status_section();

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
    ui.conflicts = to_entries(&snapshot.conflicts);
    ui.head_sha = snapshot.head_sha;
    ui.head_short = snapshot.head_short;

    CompletedPayload::Git {
        snapshot: git_snapshot,
        section,
        ui,
    }
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
    if let Some(snapshot) = &completed.git_snapshot {
        inputs.git_snapshot = Some(snapshot.clone());
        // Prefer the derived section from the richer snapshot when present.
        inputs.git_status = snapshot.status_section();
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
    if let Some(snapshot) = &completed.workspace_snapshot {
        inputs.workspace_snapshot = Some(snapshot.clone());
    }
    if let Some(snapshot) = &completed.editor_snapshot {
        // Live CodingState (fill_editor_sections) is fresher for open file /
        // selection / tabs than a completed ambient capture that may predate
        // the latest Monaco IPC. Prefer live chrome fields when present; keep
        // completed snapshot for language intelligence (symbol, hover, refs).
        let prefer_live = inputs.current_file.path.is_some() || !inputs.open_files.files.is_empty();
        let live_file = inputs.current_file.clone();
        let live_selection = inputs.current_selection.clone();
        let live_open = inputs.open_files.clone();

        let mut merged = snapshot.clone();
        if prefer_live {
            merged.active_file = live_file.clone();
            merged.selection = live_selection.clone();
            merged.open_editors = live_open.clone();
            if live_selection.path.is_some() {
                merged.cursor = Some(jaymi_context::CursorPosition {
                    line: live_selection.start_line,
                    column: live_selection.start_column,
                });
            }
        }
        inputs.editor_snapshot = Some(merged);

        if prefer_live {
            inputs.current_file = live_file;
            inputs.current_selection = live_selection;
            inputs.open_files = live_open;
        } else {
            inputs.current_file = snapshot.active_file.clone();
            inputs.current_selection = snapshot.selection.clone();
            inputs.open_files = snapshot.open_editors.clone();
        }
        if !snapshot.diagnostics.is_empty() {
            inputs.diagnostics = DiagnosticsSection {
                diagnostics: snapshot.diagnostics.clone(),
            };
        }
    }
    if let Some(snapshot) = &completed.project_snapshot {
        inputs.project_snapshot = Some(snapshot.clone());
    }
    if let Some(snapshot) = &completed.runtime_snapshot {
        inputs.runtime_snapshot = Some(snapshot.clone());
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
            workspace_snapshot_input: None,
            editor_snapshot_input: None,
            project_snapshot_input: None,
            runtime_snapshot_input: None,
        }));
        assert!(!maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::WorkspaceInventory,
            project_root: Some(dir.clone()),
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
            workspace_snapshot_input: None,
            editor_snapshot_input: None,
            project_snapshot_input: None,
            runtime_snapshot_input: None,
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

    #[test]
    fn workspace_snapshot_job_publishes_completed_observation() {
        let maintenance = ContextMaintenance::new();
        let dir = std::env::temp_dir().join(format!(
            "jaymi-ws-snap-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

        assert!(maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::WorkspaceSnapshot,
            project_root: Some(dir.clone()),
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
            workspace_snapshot_input: Some(WorkspaceSnapshotCaptureInput {
                workspace_kind: Some("coding".into()),
                coding: None,
                facts: WorkspaceSnapshotHostFacts {
                    project_id: Some("project:x".into()),
                    project_name: Some("X".into()),
                    project_root: Some(dir.display().to_string()),
                    active_branch: Some("main".into()),
                },
            }),
            editor_snapshot_input: None,
            project_snapshot_input: None,
            runtime_snapshot_input: None,
        }));

        let started = std::time::Instant::now();
        while maintenance.latest_completed().workspace_snapshot.is_none()
            && started.elapsed() < std::time::Duration::from_secs(2)
        {
            let _ = maintenance.pump();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        let snap = maintenance
            .latest_completed()
            .workspace_snapshot
            .expect("workspace snapshot completed");
        assert_eq!(snap.workspace_kind.as_deref(), Some("coding"));
        assert_eq!(snap.active_branch.as_deref(), Some("main"));
        assert!(snap.package_manager.is_some());

        let mut inputs = jaymi_context::ContextSessionInputs::default();
        inputs.workspace_snapshot = Some(jaymi_context::WorkspaceSnapshot::empty());
        merge_completed_into_session(&mut inputs, &maintenance.latest_completed());
        assert_eq!(
            inputs
                .workspace_snapshot
                .as_ref()
                .and_then(|s| s.active_branch.as_deref()),
            Some("main")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn project_snapshot_job_publishes_completed_observation() {
        let maintenance = ContextMaintenance::new();
        let dir = std::env::temp_dir().join(format!(
            "jaymi-proj-snap-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();

        assert!(maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::ProjectSnapshot,
            project_root: Some(dir.clone()),
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
            workspace_snapshot_input: None,
            editor_snapshot_input: None,
            project_snapshot_input: Some(ProjectSnapshotCaptureInput {
                project_root: Some(dir.clone()),
                facts: ProjectSnapshotHostFacts {
                    project_id: Some("project:demo".into()),
                    name: Some("Demo".into()),
                    root_directory: Some(dir.display().to_string()),
                    project_type: Some("code".into()),
                    ..ProjectSnapshotHostFacts::default()
                },
            }),
            runtime_snapshot_input: None,
        }));

        let started = std::time::Instant::now();
        while maintenance.latest_completed().project_snapshot.is_none()
            && started.elapsed() < std::time::Duration::from_secs(2)
        {
            let _ = maintenance.pump();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        let snap = maintenance
            .latest_completed()
            .project_snapshot
            .expect("project snapshot completed");
        assert_eq!(snap.metadata.name.as_deref(), Some("Demo"));
        assert!(snap.has_intelligence());
        assert!(snap
            .languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("rust")));

        let mut inputs = jaymi_context::ContextSessionInputs::default();
        merge_completed_into_session(&mut inputs, &maintenance.latest_completed());
        assert!(inputs.project_snapshot.is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn git_snapshot_job_publishes_completed_observation() {
        let maintenance = ContextMaintenance::new();
        let dir = std::env::temp_dir().join(format!(
            "jaymi-git-snap-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let init = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(init.status.success());
        let _ = std::process::Command::new("git")
            .args(["-c", "user.name=Jaymi", "-c", "user.email=jaymi@local", "commit", "--allow-empty", "-m", "init"])
            .current_dir(&dir)
            .output();
        fs::write(dir.join("dirty.txt"), "x\n").unwrap();

        assert!(maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::GitStatus,
            project_root: Some(dir.clone()),
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
            workspace_snapshot_input: None,
            editor_snapshot_input: None,
            project_snapshot_input: None,
            runtime_snapshot_input: None,
        }));

        let started = std::time::Instant::now();
        while maintenance.latest_completed().git_snapshot.is_none()
            && started.elapsed() < std::time::Duration::from_secs(3)
        {
            let _ = maintenance.pump();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        let snap = maintenance
            .latest_completed()
            .git_snapshot
            .expect("git snapshot completed");
        assert!(snap.is_repository);
        assert!(snap.has_intelligence());

        let mut inputs = jaymi_context::ContextSessionInputs::default();
        merge_completed_into_session(&mut inputs, &maintenance.latest_completed());
        assert!(inputs.git_snapshot.is_some());
        assert!(inputs.git_status.is_repository);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn runtime_snapshot_job_publishes_completed_observation() {
        let maintenance = ContextMaintenance::new();
        assert!(maintenance.schedule(MaintenanceJobRequest {
            kind: MaintenanceKind::RuntimeSnapshot,
            project_root: None,
            open_file_paths: Vec::new(),
            problems_context: None,
            problems_registry: None,
            workspace_snapshot_input: None,
            editor_snapshot_input: None,
            project_snapshot_input: None,
            runtime_snapshot_input: Some(RuntimeSnapshotCaptureInput {
                facts: RuntimeSnapshotHostFacts {
                    active_session_id: Some("term-1".into()),
                    sessions: vec![jaymi_context::RuntimeTerminalSessionFact {
                        id: "term-1".into(),
                        title: "Terminal".into(),
                        cwd: Some("/tmp/demo".into()),
                        last_command: Some("cargo check".into()),
                        output: "error[E0425]: cannot find value `x`\nerror: could not compile\n".into(),
                        history: vec!["cargo check".into()],
                        alive: true,
                    }],
                },
            }),
        }));

        let started = std::time::Instant::now();
        while maintenance.latest_completed().runtime_snapshot.is_none()
            && started.elapsed() < std::time::Duration::from_secs(2)
        {
            let _ = maintenance.pump();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        let snap = maintenance
            .latest_completed()
            .runtime_snapshot
            .expect("runtime snapshot completed");
        assert!(snap.has_intelligence());
        assert!(snap.latest_cargo_check.is_some());
        assert!(!snap.recent_failures.is_empty());

        let mut inputs = jaymi_context::ContextSessionInputs::default();
        merge_completed_into_session(&mut inputs, &maintenance.latest_completed());
        assert!(inputs.runtime_snapshot.is_some());
    }
}
