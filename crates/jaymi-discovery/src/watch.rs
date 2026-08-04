//! Filesystem watcher — keeps the knowledge inventory synchronized.
//!
//! Monitors configured discovery roots and triggers incremental scans when
//! files are created, deleted, renamed, or have metadata changes.
//!
//! Does not read file contents, parse documents, OCR, or call models.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::DiscoveryEngine;

const NAME: &str = "filesystem_watcher";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database", "discovery_engine"];
const DEBOUNCE: Duration = Duration::from_millis(150);

/// High-level watcher lifecycle for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WatcherStatus {
    /// Indexing/watching disabled by configuration.
    Disabled,
    /// Enabled but no roots configured.
    Idle,
    /// Actively monitoring roots.
    Watching,
    /// Watcher stopped during shutdown.
    #[default]
    Stopped,
    /// Watcher failed to start or encountered a fatal error.
    Error(String),
}

impl WatcherStatus {
    /// Stable label for diagnostics.
    pub fn label(&self) -> &str {
        match self {
            Self::Disabled => "disabled",
            Self::Idle => "idle",
            Self::Watching => "watching",
            Self::Stopped => "stopped",
            Self::Error(_) => "error",
        }
    }
}

/// Snapshot of watcher state for diagnostics and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherDiagnostics {
    /// Current watcher status.
    pub status: WatcherStatus,
    /// Roots currently monitored.
    pub watched_directories: Vec<PathBuf>,
    /// Pending paths/roots waiting for a debounced scan.
    pub queued_updates: usize,
    /// Human-readable description of the last filesystem event.
    pub last_event: Option<String>,
    /// Unix seconds of the last filesystem event, when any.
    pub last_event_at: Option<i64>,
}

#[derive(Default)]
struct SharedState {
    status: WatcherStatus,
    watched_directories: Vec<PathBuf>,
    dirty_roots: HashSet<PathBuf>,
    last_event: Option<String>,
    last_event_at: Option<i64>,
}

/// Background filesystem watcher bound to the discovery engine.
pub struct FilesystemWatcher {
    initialized: bool,
    discovery: Arc<DiscoveryEngine>,
    state: Arc<Mutex<SharedState>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    /// Keeps the native watcher alive while the worker runs.
    _watcher: Option<RecommendedWatcher>,
}

impl FilesystemWatcher {
    /// Create an uninitialized watcher.
    pub fn new(discovery: Arc<DiscoveryEngine>) -> Self {
        Self {
            initialized: false,
            discovery,
            state: Arc::new(Mutex::new(SharedState {
                status: WatcherStatus::Stopped,
                ..SharedState::default()
            })),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            _watcher: None,
        }
    }

    /// Diagnostics snapshot.
    pub fn diagnostics(&self) -> WatcherDiagnostics {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        WatcherDiagnostics {
            status: state.status.clone(),
            watched_directories: state.watched_directories.clone(),
            queued_updates: state.dirty_roots.len(),
            last_event: state.last_event.clone(),
            last_event_at: state.last_event_at,
        }
    }

    /// Force processing of queued dirty roots (used by tests).
    pub fn process_pending(&self) -> JaymiResult<usize> {
        flush_dirty_roots(&self.discovery, &self.state)
    }

    /// Whether the watcher is actively monitoring.
    pub fn is_watching(&self) -> bool {
        matches!(self.diagnostics().status, WatcherStatus::Watching)
    }

    fn start_watching(&mut self) -> JaymiResult<()> {
        if !self.discovery.indexing_enabled() {
            let mut state = self
                .state
                .lock()
                .map_err(|_| JaymiError::new("filesystem watcher state lock poisoned"))?;
            state.status = WatcherStatus::Disabled;
            state.watched_directories.clear();
            return Ok(());
        }

        let roots: Vec<PathBuf> = self
            .discovery
            .configured_roots()
            .iter()
            .filter_map(|root| crate::normalize_path(root).ok())
            .filter(|root| root.exists())
            .map(|root| canonicalize_best_effort(&root))
            .collect();

        if roots.is_empty() {
            let mut state = self
                .state
                .lock()
                .map_err(|_| JaymiError::new("filesystem watcher state lock poisoned"))?;
            state.status = WatcherStatus::Idle;
            state.watched_directories.clear();
            jaymi_logging::info(
                "watcher",
                "filesystem watcher idle: no existing discovery_roots configured",
            );
            return Ok(());
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |result| {
            let _ = tx.send(result);
        })
        .map_err(|error| {
            JaymiError::new(format!("failed to create filesystem watcher: {error}"))
        })?;

        for root in &roots {
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(|error| {
                    JaymiError::new(format!("failed to watch {}: {error}", root.display()))
                })?;
        }

        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| JaymiError::new("filesystem watcher state lock poisoned"))?;
            state.status = WatcherStatus::Watching;
            state.watched_directories = roots.clone();
            state.dirty_roots.clear();
        }

        self.stop.store(false, Ordering::SeqCst);
        let stop = Arc::clone(&self.stop);
        let state = Arc::clone(&self.state);
        let discovery = Arc::clone(&self.discovery);
        let watched_roots = roots;

        self.worker = Some(thread::spawn(move || {
            let mut last_event_at = Instant::now();
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }

                match rx.recv_timeout(DEBOUNCE) {
                    Ok(Ok(event)) => {
                        handle_notify_event(&event, &watched_roots, &state);
                        last_event_at = Instant::now();
                    }
                    Ok(Err(error)) => {
                        jaymi_logging::warn("watcher", format!("filesystem watch error: {error}"));
                        if let Ok(mut guard) = state.lock() {
                            guard.last_event = Some(format!("error: {error}"));
                            guard.last_event_at = Some(unix_now());
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if last_event_at.elapsed() >= DEBOUNCE {
                            if let Err(error) = flush_dirty_roots(&discovery, &state) {
                                jaymi_logging::warn(
                                    "watcher",
                                    format!(
                                        "failed to apply filesystem updates: {}",
                                        error.message()
                                    ),
                                );
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }

            let _ = flush_dirty_roots(&discovery, &state);
        }));

        self._watcher = Some(watcher);
        jaymi_logging::info(
            "watcher",
            format!(
                "filesystem watcher started roots={}",
                self.diagnostics()
                    .watched_directories
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
        Ok(())
    }

    fn stop_watching(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the watcher disconnects the channel and wakes the worker.
        self._watcher = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut state) = self.state.lock() {
            if !matches!(state.status, WatcherStatus::Disabled | WatcherStatus::Idle) {
                state.status = WatcherStatus::Stopped;
            }
            state.dirty_roots.clear();
        }
    }
}

impl Lifecycle for FilesystemWatcher {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        if let Err(error) = self.start_watching() {
            let message = error.message().to_string();
            if let Ok(mut state) = self.state.lock() {
                state.status = WatcherStatus::Error(message.clone());
            }
            jaymi_logging::warn("watcher", format!("watcher failed to start: {message}"));
            // Boot continues; indexing still works via explicit scans.
        }
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let diagnostics = self.diagnostics();
        let healthy = self.initialized && !matches!(diagnostics.status, WatcherStatus::Error(_));
        HealthReport::new(
            NAME,
            self.initialized,
            healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            ("status".to_string(), diagnostics.status.label().to_string()),
            (
                "watched_directories".to_string(),
                diagnostics.watched_directories.len().to_string(),
            ),
            (
                "queued_updates".to_string(),
                diagnostics.queued_updates.to_string(),
            ),
            (
                "last_event".to_string(),
                diagnostics
                    .last_event
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.stop_watching();
        self.initialized = false;
        Ok(())
    }
}

fn handle_notify_event(
    event: &notify::Event,
    watched_roots: &[PathBuf],
    state: &Arc<Mutex<SharedState>>,
) {
    let summary = summarize_event(event);
    let affected: Vec<PathBuf> = event
        .paths
        .iter()
        .filter_map(|path| matching_root(path, watched_roots))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if affected.is_empty() && summary.is_none() {
        return;
    }

    if let Ok(mut guard) = state.lock() {
        if let Some(summary) = summary {
            guard.last_event = Some(summary);
            guard.last_event_at = Some(unix_now());
        }
        for root in affected {
            guard.dirty_roots.insert(root);
        }
    }
}

fn summarize_event(event: &notify::Event) -> Option<String> {
    let kind = match event.kind {
        EventKind::Create(_) => "created",
        EventKind::Remove(_) => "deleted",
        EventKind::Modify(ModifyKind::Name(RenameMode::Both))
        | EventKind::Modify(ModifyKind::Name(RenameMode::To))
        | EventKind::Modify(ModifyKind::Name(RenameMode::From))
        | EventKind::Modify(ModifyKind::Name(_)) => "renamed",
        EventKind::Modify(ModifyKind::Metadata(_)) => "metadata_changed",
        EventKind::Modify(ModifyKind::Data(_)) => "metadata_changed",
        EventKind::Modify(_) => "metadata_changed",
        EventKind::Any | EventKind::Access(_) | EventKind::Other => return None,
    };
    let path = event
        .paths
        .first()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "-".to_string());
    Some(format!("{kind} {path}"))
}

fn matching_root(path: &Path, watched_roots: &[PathBuf]) -> Option<PathBuf> {
    let candidate = canonicalize_best_effort(path);
    watched_roots
        .iter()
        .find(|root| candidate == **root || candidate.starts_with(root))
        .cloned()
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn flush_dirty_roots(
    discovery: &DiscoveryEngine,
    state: &Arc<Mutex<SharedState>>,
) -> JaymiResult<usize> {
    let dirty: Vec<PathBuf> = {
        let mut guard = state
            .lock()
            .map_err(|_| JaymiError::new("filesystem watcher state lock poisoned"))?;
        guard.dirty_roots.drain().collect()
    };
    if dirty.is_empty() {
        return Ok(0);
    }

    let mut scanned = 0usize;
    for root in dirty {
        if !root.exists() {
            jaymi_logging::warn(
                "watcher",
                format!("watched root missing during flush: {}", root.display()),
            );
            continue;
        }
        match discovery.scan(std::slice::from_ref(&root)) {
            Ok(report) => {
                scanned += 1;
                jaymi_logging::info(
                    "watcher",
                    format!(
                        "applied filesystem changes for {} added={} updated={} removed={}",
                        root.display(),
                        report.added,
                        report.updated,
                        report.removed
                    ),
                );
            }
            Err(error) => {
                jaymi_logging::warn(
                    "watcher",
                    format!(
                        "incremental scan failed for {}: {}",
                        root.display(),
                        error.message()
                    ),
                );
            }
        }
    }
    Ok(scanned)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}
