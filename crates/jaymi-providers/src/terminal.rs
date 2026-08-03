//! Terminal / PTY Provider — persistent interactive shell sessions.
//!
//! Architecture path:
//! Planner → ExecuteTerminalCommands → Terminal Tool → Terminal Provider → PTY
//!
//! The Planner never talks to a PTY directly. Tools mediate all access.

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

/// Result of ensuring or running a command in a PTY session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCommandResult {
    /// Session that produced the result.
    pub session_id: String,
    /// Working directory used for the session.
    pub cwd: PathBuf,
    /// Command that was run, when any.
    pub command: Option<String>,
    /// Output chunk produced by this operation (may be empty on ensure).
    pub output: String,
    /// Full scrollback buffer for the session.
    pub scrollback: String,
    /// Command history for the session (oldest first).
    pub history: Vec<String>,
}

struct PtySession {
    /// Kept alive so the PTY stays open for the workspace lifetime.
    _master: Box<dyn MasterPty + Send>,
    _child: Box<dyn Child + Send>,
    writer: Box<dyn Write + Send>,
    output: Arc<Mutex<String>>,
    cwd: PathBuf,
    history: Vec<String>,
}

/// Local PTY provider with persistent shell sessions.
pub struct TerminalProvider {
    identity: ProviderIdentity,
    initialized: bool,
    sessions: Mutex<HashMap<String, PtySession>>,
}

impl std::fmt::Debug for TerminalProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let session_count = self.sessions.lock().map(|guard| guard.len()).unwrap_or(0);
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
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true after [`Provider::initialize`] succeeds.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Ensure a persistent PTY session exists (spawn on first use).
    pub fn ensure_session(
        &self,
        session_id: &str,
        cwd: &Path,
    ) -> JaymiResult<TerminalCommandResult> {
        self.require_initialized()?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;

        if !sessions.contains_key(session_id) {
            let session = spawn_session(cwd)?;
            sessions.insert(session_id.to_string(), session);
            jaymi_logging::info(
                "providers",
                format!(
                    "terminal spawn session={session_id} cwd={}",
                    cwd.display()
                ),
            );
        }

        let session = sessions
            .get(session_id)
            .ok_or_else(|| JaymiError::new(format!("missing terminal session {session_id}")))?;
        Ok(snapshot_result(session_id, session, None, String::new()))
    }

    /// Run a command in a persistent session and capture stdout.
    pub fn run_command(
        &self,
        session_id: &str,
        cwd: &Path,
        command: &str,
    ) -> JaymiResult<TerminalCommandResult> {
        self.require_initialized()?;
        let command = command.trim();
        if command.is_empty() {
            return Err(JaymiError::new("terminal command must not be empty"));
        }

        self.ensure_session(session_id, cwd)?;

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| JaymiError::new(format!("missing terminal session {session_id}")))?;

        let before_len = {
            let output = session
                .output
                .lock()
                .map_err(|_| JaymiError::new("terminal output lock poisoned"))?;
            output.len()
        };

        // Echo a unique marker so we know when the shell finished the command.
        let script = format!("{command}\necho {DONE_MARKER}$?\n");
        session.writer.write_all(script.as_bytes()).map_err(|error| {
            JaymiError::new(format!("failed to write to terminal session: {error}"))
        })?;
        session.writer.flush().map_err(|error| {
            JaymiError::new(format!("failed to flush terminal session: {error}"))
        })?;

        let timeout = command_timeout(command);
        let deadline = Instant::now() + timeout;
        let marker = DONE_MARKER.to_string();
        loop {
            let output = session
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
            let output = session
                .output
                .lock()
                .map_err(|_| JaymiError::new("terminal output lock poisoned"))?;
            output[before_len..].to_string()
        };
        let cleaned = strip_done_marker(&raw_chunk);
        let display = strip_ansi(&cleaned);

        session.history.push(command.to_string());
        // Keep history bounded for UI navigation.
        if session.history.len() > 200 {
            let overflow = session.history.len() - 200;
            session.history.drain(0..overflow);
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
            session,
            Some(command.to_string()),
            display,
        ))
    }

    /// Return whether a session is currently open.
    pub fn has_session(&self, session_id: &str) -> JaymiResult<bool> {
        self.require_initialized()?;
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;
        Ok(sessions.contains_key(session_id))
    }

    /// Close one session.
    pub fn close_session(&self, session_id: &str) -> JaymiResult<()> {
        self.require_initialized()?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;
        sessions.remove(session_id);
        Ok(())
    }

    /// Close all sessions (e.g. when Coding Workspace closes).
    pub fn close_all_sessions(&self) -> JaymiResult<()> {
        if !self.initialized {
            return Ok(());
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("terminal session lock poisoned"))?;
        sessions.clear();
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

fn spawn_session(cwd: &Path) -> JaymiResult<PtySession> {
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

    Ok(PtySession {
        _master: pair.master,
        _child: child,
        writer,
        output,
        cwd,
        history: Vec::new(),
    })
}

fn snapshot_result(
    session_id: &str,
    session: &PtySession,
    command: Option<String>,
    output: String,
) -> TerminalCommandResult {
    let scrollback = session
        .output
        .lock()
        .map(|guard| strip_ansi(&strip_done_marker(&guard)))
        .unwrap_or_default();
    TerminalCommandResult {
        session_id: session_id.to_string(),
        cwd: session.cwd.clone(),
        command,
        output,
        scrollback,
        history: session.history.clone(),
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
        JaymiError::new(format!("cannot access terminal cwd {}: {error}", path.display()))
    })?;
    if !meta.is_dir() {
        return Err(JaymiError::new(format!(
            "terminal cwd is not a directory: {}",
            path.display()
        )));
    }
    std::fs::canonicalize(&path).or_else(|_| Ok(path))
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
        // Also drop the echoed script line that re-prints the marker command.
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
        assert!(provider.has_session(DEFAULT_TERMINAL_SESSION_ID).unwrap());

        let pwd = provider
            .run_command(DEFAULT_TERMINAL_SESSION_ID, &dir, "pwd")
            .unwrap();
        assert!(
            pwd.output.contains(dir.file_name().unwrap().to_str().unwrap())
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
