//! Terminal / PTY Provider — persistent interactive shell sessions.
//!
//! Architecture path:
//! Planner → ExecuteTerminalCommands → Terminal Tool → Terminal Provider →
//! [`TerminalManager`] → [`TerminalSession`] (PTY)
//!
//! The Planner never talks to a PTY directly. Tools mediate all access.
//! Coding Workspace renders session state only — it never owns process logic.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Provider ID used for registration and tool metadata.
pub const TERMINAL_PROVIDER_ID: &str = "terminal";

/// Default Coding Workspace terminal session id.
pub const DEFAULT_TERMINAL_SESSION_ID: &str = "coding";

const DONE_MARKER: &str = "__JAYMI_DONE:";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const CARGO_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_millis(40);
const STARTUP_DRAIN: Duration = Duration::from_millis(400);

/// Public metadata for one live PTY session (no process handles exposed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSession {
    /// Stable session id for this workspace lifetime.
    pub id: String,
    /// Display title (tab label).
    pub title: String,
    /// Working directory used when the session was spawned.
    pub cwd: PathBuf,
    /// Command history (oldest first).
    pub history: Vec<String>,
    /// Whether the PTY child is still managed by the provider.
    pub alive: bool,
}

/// Result of ensuring, creating, renaming, killing, or running a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCommandResult {
    /// Session that produced the result.
    pub session_id: String,
    /// Display title for the session.
    pub title: String,
    /// Working directory used for the session.
    pub cwd: PathBuf,
    /// Command that was run, when any.
    pub command: Option<String>,
    /// Output chunk produced by this operation (may be empty on ensure/create).
    pub output: String,
    /// Full scrollback buffer for the session.
    pub scrollback: String,
    /// Command history for the session (oldest first).
    pub history: Vec<String>,
    /// Whether the session is still alive after this operation.
    pub alive: bool,
}

struct PtyHandle {
    /// Kept alive so the PTY stays open for the workspace lifetime.
    _master: Box<dyn MasterPty + Send>,
    _child: Box<dyn Child + Send>,
    writer: Box<dyn Write + Send>,
    output: Arc<Mutex<String>>,
    cwd: PathBuf,
    title: String,
    history: Vec<String>,
}

/// Owns all live PTY sessions for the Terminal Provider.
#[derive(Default)]
pub struct TerminalManager {
    sessions: HashMap<String, PtyHandle>,
    next_index: u64,
}

impl TerminalManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether any sessions are open.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// List public session metadata (no process handles).
    pub fn list(&self) -> Vec<TerminalSession> {
        let mut sessions: Vec<_> = self
            .sessions
            .iter()
            .map(|(id, handle)| TerminalSession {
                id: id.clone(),
                title: handle.title.clone(),
                cwd: handle.cwd.clone(),
                history: handle.history.clone(),
                alive: true,
            })
            .collect();
        sessions.sort_by(|left, right| left.id.cmp(&right.id));
        sessions
    }

    /// Ensure a persistent PTY session exists (spawn on first use).
    pub fn ensure(
        &mut self,
        session_id: &str,
        cwd: &Path,
        title: Option<&str>,
    ) -> JaymiResult<TerminalCommandResult> {
        if !self.sessions.contains_key(session_id) {
            let mut handle = spawn_session(cwd)?;
            handle.title = title
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| default_title(session_id));
            self.sessions.insert(session_id.to_string(), handle);
            jaymi_logging::info(
                "providers",
                format!("terminal spawn session={session_id} cwd={}", cwd.display()),
            );
        } else if let Some(title) = title.map(str::trim).filter(|value| !value.is_empty()) {
            if let Some(handle) = self.sessions.get_mut(session_id) {
                handle.title = title.to_string();
            }
        }

        let handle = self
            .sessions
            .get(session_id)
            .ok_or_else(|| JaymiError::new(format!("missing terminal session {session_id}")))?;
        Ok(snapshot_result(session_id, handle, None, String::new()))
    }

    /// Create a new session with a generated id (cwd follows the project root).
    pub fn create(
        &mut self,
        cwd: &Path,
        title: Option<&str>,
    ) -> JaymiResult<TerminalCommandResult> {
        self.next_index = self.next_index.saturating_add(1);
        let session_id =
            if self.next_index == 1 && !self.sessions.contains_key(DEFAULT_TERMINAL_SESSION_ID) {
                DEFAULT_TERMINAL_SESSION_ID.to_string()
            } else {
                format!("terminal-{}", self.next_index)
            };
        let title = title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default_title(&session_id));
        self.ensure(&session_id, cwd, Some(&title))
    }

    /// Rename a live session's display title.
    pub fn rename(&mut self, session_id: &str, title: &str) -> JaymiResult<TerminalCommandResult> {
        let title = title.trim();
        if title.is_empty() {
            return Err(JaymiError::new("terminal title must not be empty"));
        }
        let handle = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| JaymiError::new(format!("missing terminal session {session_id}")))?;
        handle.title = title.to_string();
        Ok(snapshot_result(session_id, handle, None, String::new()))
    }

    /// Kill / close one session.
    pub fn kill(&mut self, session_id: &str) -> JaymiResult<TerminalCommandResult> {
        let handle = self
            .sessions
            .remove(session_id)
            .ok_or_else(|| JaymiError::new(format!("missing terminal session {session_id}")))?;
        Ok(TerminalCommandResult {
            session_id: session_id.to_string(),
            title: handle.title,
            cwd: handle.cwd,
            command: None,
            output: String::new(),
            scrollback: String::new(),
            history: handle.history,
            alive: false,
        })
    }

    /// Run a command in a persistent session and capture stdout.
    pub fn run_command(
        &mut self,
        session_id: &str,
        cwd: &Path,
        command: &str,
    ) -> JaymiResult<TerminalCommandResult> {
        let command = command.trim();
        if command.is_empty() {
            return Err(JaymiError::new("terminal command must not be empty"));
        }

        self.ensure(session_id, cwd, None)?;

        let handle = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| JaymiError::new(format!("missing terminal session {session_id}")))?;

        let before_len = {
            let output = handle
                .output
                .lock()
                .map_err(|_| JaymiError::new("terminal output lock poisoned"))?;
            output.len()
        };

        // Echo a unique marker so we know when the shell finished the command.
        let script = format!("{command}\necho {DONE_MARKER}$?\n");
        handle
            .writer
            .write_all(script.as_bytes())
            .map_err(|error| {
                JaymiError::new(format!("failed to write to terminal session: {error}"))
            })?;
        handle.writer.flush().map_err(|error| {
            JaymiError::new(format!("failed to flush terminal session: {error}"))
        })?;

        let timeout = command_timeout(command);
        let deadline = Instant::now() + timeout;
        let marker = DONE_MARKER.to_string();
        loop {
            let output = handle
                .output
                .lock()
                .map_err(|_| JaymiError::new("terminal output lock poisoned"))?
                .clone();
            if output.len() > before_len && output[before_len..].contains(&marker) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(JaymiError::new(format!(
                    "timed out waiting for terminal command: {command}"
                )));
            }
            drop(output);
            thread::sleep(POLL_INTERVAL);
        }

        let raw_chunk = {
            let output = handle
                .output
                .lock()
                .map_err(|_| JaymiError::new("terminal output lock poisoned"))?;
            output[before_len..].to_string()
        };
        let cleaned = strip_done_marker(&raw_chunk);
        let display = strip_ansi(&cleaned);

        handle.history.push(command.to_string());
        if handle.history.len() > 200 {
            let overflow = handle.history.len() - 200;
            handle.history.drain(0..overflow);
        }

        jaymi_logging::info(
            "providers",
            format!(
                "terminal run session={session_id} command={command} bytes={}",
                display.len()
            ),
        );

        Ok(snapshot_result(
            session_id,
            handle,
            Some(command.to_string()),
            display,
        ))
    }

    /// Return whether a session is currently open.
    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.contains_key(session_id)
    }

    /// Close all sessions.
    pub fn close_all(&mut self) {
        self.sessions.clear();
    }
}

/// Local PTY provider — thin façade over [`TerminalManager`].
pub struct TerminalProvider {
    identity: ProviderIdentity,
    initialized: bool,
    manager: Mutex<TerminalManager>,
}

impl std::fmt::Debug for TerminalProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let session_count = self.manager.lock().map(|guard| guard.len()).unwrap_or(0);
        f.debug_struct("TerminalProvider")
            .field("id", &self.identity.id)
            .field("initialized", &self.initialized)
            .field("session_count", &session_count)
            .finish()
    }
}

impl TerminalProvider {
    /// Create an uninitialized terminal provider.
    pub fn new() -> Self {
        Self {
            identity: ProviderIdentity {
                id: TERMINAL_PROVIDER_ID.to_string(),
                name: "Terminal".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Local PTY shell sessions for Coding Workspace".to_string(),
                category: ProviderCategory::Local,
                author: "jaymi".to_string(),
                capabilities: vec![Capability::ExecuteTerminalCommands, Capability::Code],
            },
            initialized: false,
            manager: Mutex::new(TerminalManager::new()),
        }
    }

    /// Returns true after [`Provider::initialize`] succeeds.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn with_manager_mut<T>(
        &self,
        f: impl FnOnce(&mut TerminalManager) -> JaymiResult<T>,
    ) -> JaymiResult<T> {
        self.require_initialized()?;
        let mut manager = self
            .manager
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;
        f(&mut manager)
    }

    /// Ensure a persistent PTY session exists (spawn on first use).
    pub fn ensure_session(
        &self,
        session_id: &str,
        cwd: &Path,
    ) -> JaymiResult<TerminalCommandResult> {
        self.with_manager_mut(|manager| manager.ensure(session_id, cwd, None))
    }

    /// Create a new PTY session (cwd should be the current project root).
    pub fn create_session(
        &self,
        cwd: &Path,
        title: Option<&str>,
    ) -> JaymiResult<TerminalCommandResult> {
        self.with_manager_mut(|manager| manager.create(cwd, title))
    }

    /// Rename a session's display title.
    pub fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> JaymiResult<TerminalCommandResult> {
        self.with_manager_mut(|manager| manager.rename(session_id, title))
    }

    /// Kill one session.
    pub fn kill_session(&self, session_id: &str) -> JaymiResult<TerminalCommandResult> {
        self.with_manager_mut(|manager| manager.kill(session_id))
    }

    /// List live sessions (metadata only).
    pub fn list_sessions(&self) -> JaymiResult<Vec<TerminalSession>> {
        self.with_manager_mut(|manager| Ok(manager.list()))
    }

    /// Run a command in a persistent session and capture stdout.
    pub fn run_command(
        &self,
        session_id: &str,
        cwd: &Path,
        command: &str,
    ) -> JaymiResult<TerminalCommandResult> {
        self.with_manager_mut(|manager| manager.run_command(session_id, cwd, command))
    }

    /// Return whether a session is currently open.
    pub fn has_session(&self, session_id: &str) -> JaymiResult<bool> {
        self.with_manager_mut(|manager| Ok(manager.has_session(session_id)))
    }

    /// Close one session.
    pub fn close_session(&self, session_id: &str) -> JaymiResult<()> {
        let _ = self.kill_session(session_id)?;
        Ok(())
    }

    /// Close all sessions (e.g. when Coding Workspace closes).
    pub fn close_all_sessions(&self) -> JaymiResult<()> {
        if !self.initialized {
            return Ok(());
        }
        let mut manager = self
            .manager
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;
        manager.close_all();
        Ok(())
    }

    fn require_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("terminal provider is not initialized"))
        }
    }
}

impl Default for TerminalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for TerminalProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> JaymiResult<()> {
        self.require_initialized()
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        let _ = self.close_all_sessions();
        self.initialized = false;
        Ok(())
    }
}

fn default_title(session_id: &str) -> String {
    if session_id == DEFAULT_TERMINAL_SESSION_ID {
        "Terminal".to_string()
    } else {
        format!("Terminal ({session_id})")
    }
}

fn spawn_session(cwd: &Path) -> JaymiResult<PtyHandle> {
    let cwd = normalize_cwd(cwd)?;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 32,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| JaymiError::new(format!("failed to open pty: {error}")))?;

    let shell = default_shell();
    let mut cmd = CommandBuilder::new(&shell);
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    // Keep prompts simple so markers are easier to detect.
    cmd.env("PS1", "$ ");
    cmd.env("PROMPT", "$ ");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| JaymiError::new(format!("failed to spawn shell: {error}")))?;

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| JaymiError::new(format!("failed to clone pty reader: {error}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| JaymiError::new(format!("failed to take pty writer: {error}")))?;

    let output = Arc::new(Mutex::new(String::new()));
    let reader_buf = Arc::clone(&output);
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    if let Ok(mut guard) = reader_buf.lock() {
                        guard.push_str(&chunk);
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Drain shell startup noise so the first command starts cleanly.
    thread::sleep(STARTUP_DRAIN);

    Ok(PtyHandle {
        _master: pair.master,
        _child: child,
        writer,
        output,
        cwd,
        title: "Terminal".into(),
        history: Vec::new(),
    })
}

fn snapshot_result(
    session_id: &str,
    handle: &PtyHandle,
    command: Option<String>,
    output: String,
) -> TerminalCommandResult {
    let scrollback = handle
        .output
        .lock()
        .map(|guard| strip_ansi(&strip_done_marker(&guard)))
        .unwrap_or_default();
    TerminalCommandResult {
        session_id: session_id.to_string(),
        title: handle.title.clone(),
        cwd: handle.cwd.clone(),
        command,
        output,
        scrollback,
        history: handle.history.clone(),
        alive: true,
    }
}

fn normalize_cwd(path: &Path) -> JaymiResult<PathBuf> {
    let path = if path.as_os_str().is_empty() {
        std::env::current_dir().map_err(|error| {
            JaymiError::new(format!("cannot resolve current directory: {error}"))
        })?
    } else {
        path.to_path_buf()
    };
    let meta = std::fs::metadata(&path).map_err(|error| {
        JaymiError::new(format!(
            "cannot access terminal cwd {}: {error}",
            path.display()
        ))
    })?;
    if !meta.is_dir() {
        return Err(JaymiError::new(format!(
            "terminal cwd is not a directory: {}",
            path.display()
        )));
    }
    std::fs::canonicalize(&path).or(Ok(path))
}

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "powershell.exe".to_string()
        } else {
            "/bin/bash".to_string()
        }
    })
}

fn command_timeout(command: &str) -> Duration {
    let lower = command.to_ascii_lowercase();
    if lower.contains("cargo ") || lower.starts_with("cargo") {
        CARGO_TIMEOUT
    } else {
        DEFAULT_TIMEOUT
    }
}

fn strip_done_marker(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.contains(DONE_MARKER) {
            continue;
        }
        if line.trim_start().starts_with("echo ") && line.contains(DONE_MARKER) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\u{7}' {
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if ch == '\r' {
            continue;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn spawn_run_and_persist_session() {
        let dir = temp_dir("pty-persist");
        fs::write(dir.join("hello.txt"), "hi").unwrap();

        let mut provider = TerminalProvider::new();
        provider.initialize().unwrap();

        let ensured = provider
            .ensure_session(DEFAULT_TERMINAL_SESSION_ID, &dir)
            .unwrap();
        assert_eq!(ensured.session_id, DEFAULT_TERMINAL_SESSION_ID);
        assert_eq!(ensured.title, "Terminal");
        assert!(provider.has_session(DEFAULT_TERMINAL_SESSION_ID).unwrap());

        let pwd = provider
            .run_command(DEFAULT_TERMINAL_SESSION_ID, &dir, "pwd")
            .unwrap();
        assert!(
            pwd.output
                .contains(dir.file_name().unwrap().to_str().unwrap())
                || pwd.scrollback.contains(&dir.display().to_string())
                || pwd.output.contains(&dir.display().to_string()),
            "pwd output missing cwd: {}",
            pwd.output
        );

        let ls = provider
            .run_command(DEFAULT_TERMINAL_SESSION_ID, &dir, "ls")
            .unwrap();
        assert!(
            ls.output.contains("hello.txt") || ls.scrollback.contains("hello.txt"),
            "ls missing hello.txt: {}",
            ls.output
        );

        assert!(provider.has_session(DEFAULT_TERMINAL_SESSION_ID).unwrap());
        assert_eq!(ls.history.len(), 2);
        assert_eq!(ls.history[0], "pwd");
        assert_eq!(ls.history[1], "ls");
    }

    #[test]
    fn manager_creates_renames_and_kills_sessions() {
        let dir = temp_dir("pty-multi");
        let mut provider = TerminalProvider::new();
        provider.initialize().unwrap();

        let first = provider.create_session(&dir, Some("Build")).unwrap();
        assert_eq!(first.session_id, DEFAULT_TERMINAL_SESSION_ID);
        assert_eq!(first.title, "Build");
        assert!(first.alive);

        let second = provider.create_session(&dir, Some("Tests")).unwrap();
        assert_ne!(second.session_id, first.session_id);
        assert_eq!(second.title, "Tests");

        let renamed = provider
            .rename_session(&second.session_id, "Integration")
            .unwrap();
        assert_eq!(renamed.title, "Integration");

        let listed = provider.list_sessions().unwrap();
        assert_eq!(listed.len(), 2);

        provider.kill_session(&second.session_id).unwrap();
        assert!(!provider.has_session(&second.session_id).unwrap());
        assert!(provider.has_session(&first.session_id).unwrap());
        assert_eq!(provider.list_sessions().unwrap().len(), 1);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-terminal-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
