//! Language Server Provider — Rust Analyzer (and mock) over LSP JSON-RPC.
//!
//! Architecture path:
//! Planner → Code → language_server Tool → LSP Provider → rust-analyzer
//!
//! The Planner never talks to a language server directly. Tools mediate all access.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jaymi_capabilities::Capability;
use jaymi_core::{
    JaymiError, JaymiResult, LspCompletionItem, LspDiagnostic, LspHover, LspLocation, LspOperation,
    LspPosition, LspRange, LspRequest, LspTextEdit,
};
use serde_json::{json, Value};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Provider ID used for registration and tool metadata.
pub const LSP_PROVIDER_ID: &str = "lsp";

/// Default language server binary when `JAYMI_LSP_COMMAND` is unset.
pub const DEFAULT_LSP_COMMAND: &str = "rust-analyzer";

/// Sentinel command that selects the in-process mock language server.
pub const MOCK_LSP_COMMAND: &str = "mock";

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const DIAGNOSTIC_WAIT: Duration = Duration::from_secs(2);

/// Aggregated result of an LSP provider operation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LspOperationResult {
    /// Operation that produced this result.
    pub operation: Option<LspOperation>,
    /// Hover payload when requested.
    pub hover: Option<LspHover>,
    /// Completion candidates when requested.
    pub completions: Vec<LspCompletionItem>,
    /// Diagnostics (cached publishDiagnostics and/or explicit query).
    pub diagnostics: Vec<LspDiagnostic>,
    /// Go-to-definition locations.
    pub definitions: Vec<LspLocation>,
    /// Find-references locations.
    pub references: Vec<LspLocation>,
    /// Rename / workspace text edits.
    pub edits: Vec<LspTextEdit>,
    /// Human-readable summary.
    pub message: String,
}

#[derive(Debug, Clone)]
struct OpenDocument {
    text: String,
    version: i32,
    #[allow(dead_code)]
    language: String,
}

enum SessionBackend {
    Mock(MockSession),
    Process(ProcessSession),
}

struct WorkspaceSession {
    #[allow(dead_code)]
    root: PathBuf,
    backend: SessionBackend,
    documents: HashMap<PathBuf, OpenDocument>,
    diagnostics: HashMap<PathBuf, Vec<LspDiagnostic>>,
}

/// Local Language Server provider (Rust Analyzer by default).
pub struct LspProvider {
    identity: ProviderIdentity,
    initialized: bool,
    force_mock: bool,
    sessions: Mutex<HashMap<PathBuf, WorkspaceSession>>,
    next_id: AtomicI64,
}

impl std::fmt::Debug for LspProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let session_count = self.sessions.lock().map(|guard| guard.len()).unwrap_or(0);
        f.debug_struct("LspProvider")
            .field("id", &self.identity.id)
            .field("initialized", &self.initialized)
            .field("session_count", &session_count)
            .finish()
    }
}

impl LspProvider {
    /// Create an uninitialized LSP provider (command resolved on first use).
    pub fn new() -> Self {
        Self {
            identity: ProviderIdentity {
                id: LSP_PROVIDER_ID.to_string(),
                name: "Language Server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Rust Analyzer language server for Coding Workspace".to_string(),
                category: ProviderCategory::Local,
                author: "jaymi".to_string(),
                capabilities: vec![Capability::Code],
            },
            initialized: false,
            force_mock: false,
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
        }
    }

    /// Create a provider forced into mock mode (tests).
    pub fn mock() -> Self {
        let mut provider = Self::new();
        provider.force_mock = true;
        provider
    }

    /// Returns true after [`Provider::initialize`] succeeds.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Execute a structured LSP request.
    pub fn execute(&self, request: &LspRequest) -> JaymiResult<LspOperationResult> {
        self.require_initialized()?;
        if request.workspace_root.as_os_str().is_empty() {
            return Err(JaymiError::new("lsp workspace root must not be empty"));
        }
        let root = request
            .workspace_root
            .canonicalize()
            .unwrap_or_else(|_| request.workspace_root.clone());

        match request.operation {
            LspOperation::Ensure => {
                self.ensure_session(&root)?;
                Ok(LspOperationResult {
                    operation: Some(LspOperation::Ensure),
                    message: format!("Ensured language server for {}", root.display()),
                    ..LspOperationResult::default()
                })
            }
            LspOperation::DidOpen => {
                let path = require_path(request)?;
                let content = request.content.clone().unwrap_or_default();
                let language = request
                    .language
                    .clone()
                    .unwrap_or_else(|| language_for_path(&path));
                let version = request.version.unwrap_or(1);
                self.did_open(&root, &path, &language, &content, version)
            }
            LspOperation::DidChange => {
                let path = require_path(request)?;
                let content = request
                    .content
                    .clone()
                    .ok_or_else(|| JaymiError::new("lsp did_change requires content"))?;
                let version = request.version.unwrap_or(1);
                self.did_change(&root, &path, &content, version)
            }
            LspOperation::DidClose => {
                let path = require_path(request)?;
                self.did_close(&root, &path)
            }
            LspOperation::Hover => {
                let path = require_path(request)?;
                let (line, character) = require_position(request)?;
                self.hover(&root, &path, line, character)
            }
            LspOperation::Completion => {
                let path = require_path(request)?;
                let (line, character) = require_position(request)?;
                self.completion(&root, &path, line, character)
            }
            LspOperation::Diagnostics => self.diagnostics(&root, request.path.as_deref()),
            LspOperation::Definition => {
                let path = require_path(request)?;
                let (line, character) = require_position(request)?;
                self.definition(&root, &path, line, character)
            }
            LspOperation::Rename => {
                let path = require_path(request)?;
                let (line, character) = require_position(request)?;
                let new_name = request
                    .new_name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| JaymiError::new("lsp rename requires new_name"))?;
                self.rename(&root, &path, line, character, new_name)
            }
            LspOperation::References => {
                let path = require_path(request)?;
                let (line, character) = require_position(request)?;
                self.references(&root, &path, line, character)
            }
        }
    }

    fn ensure_session(&self, root: &Path) -> JaymiResult<()> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("lsp session lock poisoned"))?;
        if sessions.contains_key(root) {
            return Ok(());
        }
        let backend = spawn_backend(root, &self.next_id, self.force_mock)?;
        sessions.insert(
            root.to_path_buf(),
            WorkspaceSession {
                root: root.to_path_buf(),
                backend,
                documents: HashMap::new(),
                diagnostics: HashMap::new(),
            },
        );
        jaymi_logging::info(
            "providers",
            format!("lsp spawn workspace={}", root.display()),
        );
        Ok(())
    }

    fn with_session_mut<R>(
        &self,
        root: &Path,
        f: impl FnOnce(&mut WorkspaceSession) -> JaymiResult<R>,
    ) -> JaymiResult<R> {
        self.ensure_session(root)?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("lsp session lock poisoned"))?;
        let session = sessions
            .get_mut(root)
            .ok_or_else(|| JaymiError::new("lsp session missing after ensure"))?;
        f(session)
    }

    fn did_open(
        &self,
        root: &Path,
        path: &Path,
        language: &str,
        content: &str,
        version: i32,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    mock.did_open(path, content);
                    session.diagnostics.insert(
                        path.to_path_buf(),
                        mock.diagnostics_for(path, content),
                    );
                }
                SessionBackend::Process(process) => {
                    process.notify(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": path_to_uri(path),
                                "languageId": language,
                                "version": version,
                                "text": content,
                            }
                        }),
                    )?;
                    // Give the server a moment to publish diagnostics.
                    let published = process.wait_diagnostics(DIAGNOSTIC_WAIT);
                    for (uri, diags) in published {
                        if let Some(diag_path) = uri_to_path(&uri) {
                            session.diagnostics.insert(diag_path, diags);
                        }
                    }
                }
            }
            session.documents.insert(
                path.to_path_buf(),
                OpenDocument {
                    text: content.to_string(),
                    version,
                    language: language.to_string(),
                },
            );
            let diagnostics = session
                .diagnostics
                .get(path)
                .cloned()
                .unwrap_or_default();
            Ok(LspOperationResult {
                operation: Some(LspOperation::DidOpen),
                diagnostics,
                message: format!("Opened {} in language server", path.display()),
                ..LspOperationResult::default()
            })
        })
    }

    fn did_change(
        &self,
        root: &Path,
        path: &Path,
        content: &str,
        version: i32,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    mock.did_change(path, content);
                    session.diagnostics.insert(
                        path.to_path_buf(),
                        mock.diagnostics_for(path, content),
                    );
                }
                SessionBackend::Process(process) => {
                    process.notify(
                        "textDocument/didChange",
                        json!({
                            "textDocument": {
                                "uri": path_to_uri(path),
                                "version": version,
                            },
                            "contentChanges": [{ "text": content }],
                        }),
                    )?;
                    let published = process.wait_diagnostics(DIAGNOSTIC_WAIT);
                    for (uri, diags) in published {
                        if let Some(diag_path) = uri_to_path(&uri) {
                            session.diagnostics.insert(diag_path, diags);
                        }
                    }
                }
            }
            if let Some(doc) = session.documents.get_mut(path) {
                doc.text = content.to_string();
                doc.version = version;
            } else {
                session.documents.insert(
                    path.to_path_buf(),
                    OpenDocument {
                        text: content.to_string(),
                        version,
                        language: language_for_path(path),
                    },
                );
            }
            let diagnostics = session
                .diagnostics
                .get(path)
                .cloned()
                .unwrap_or_default();
            Ok(LspOperationResult {
                operation: Some(LspOperation::DidChange),
                diagnostics,
                message: format!("Synced {} with language server", path.display()),
                ..LspOperationResult::default()
            })
        })
    }

    fn did_close(&self, root: &Path, path: &Path) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            match &mut session.backend {
                SessionBackend::Mock(mock) => mock.did_close(path),
                SessionBackend::Process(process) => {
                    process.notify(
                        "textDocument/didClose",
                        json!({
                            "textDocument": { "uri": path_to_uri(path) }
                        }),
                    )?;
                }
            }
            session.documents.remove(path);
            session.diagnostics.remove(path);
            Ok(LspOperationResult {
                operation: Some(LspOperation::DidClose),
                message: format!("Closed {} in language server", path.display()),
                ..LspOperationResult::default()
            })
        })
    }

    fn hover(
        &self,
        root: &Path,
        path: &Path,
        line: u32,
        character: u32,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            let hover = match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    let text = session
                        .documents
                        .get(path)
                        .map(|doc| doc.text.as_str())
                        .unwrap_or("");
                    mock.hover(path, text, line, character)
                }
                SessionBackend::Process(process) => {
                    let result = process.request(
                        "textDocument/hover",
                        json!({
                            "textDocument": { "uri": path_to_uri(path) },
                            "position": { "line": line, "character": character },
                        }),
                    )?;
                    parse_hover(result)
                }
            };
            let message = hover
                .as_ref()
                .map(|item| format!("Hover ({} chars)", item.contents.len()))
                .unwrap_or_else(|| "No hover information".into());
            Ok(LspOperationResult {
                operation: Some(LspOperation::Hover),
                hover,
                message,
                ..LspOperationResult::default()
            })
        })
    }

    fn completion(
        &self,
        root: &Path,
        path: &Path,
        line: u32,
        character: u32,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            let completions = match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    let text = session
                        .documents
                        .get(path)
                        .map(|doc| doc.text.as_str())
                        .unwrap_or("");
                    mock.completion(path, text, line, character)
                }
                SessionBackend::Process(process) => {
                    let result = process.request(
                        "textDocument/completion",
                        json!({
                            "textDocument": { "uri": path_to_uri(path) },
                            "position": { "line": line, "character": character },
                        }),
                    )?;
                    parse_completions(result)
                }
            };
            Ok(LspOperationResult {
                operation: Some(LspOperation::Completion),
                message: format!("{} completion(s)", completions.len()),
                completions,
                ..LspOperationResult::default()
            })
        })
    }

    fn diagnostics(
        &self,
        root: &Path,
        path: Option<&Path>,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            // Drain any pending publishDiagnostics from the process backend.
            if let SessionBackend::Process(process) = &mut session.backend {
                let published = process.drain_diagnostics();
                for (uri, diags) in published {
                    if let Some(diag_path) = uri_to_path(&uri) {
                        session.diagnostics.insert(diag_path, diags);
                    }
                }
            }
            let diagnostics = match path {
                Some(path) => {
                    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                    session
                        .diagnostics
                        .get(&key)
                        .cloned()
                        .or_else(|| session.diagnostics.get(path).cloned())
                        .unwrap_or_default()
                }
                None => session
                    .diagnostics
                    .values()
                    .flat_map(|items| items.iter().cloned())
                    .collect(),
            };
            Ok(LspOperationResult {
                operation: Some(LspOperation::Diagnostics),
                message: format!("{} diagnostic(s)", diagnostics.len()),
                diagnostics,
                ..LspOperationResult::default()
            })
        })
    }

    fn definition(
        &self,
        root: &Path,
        path: &Path,
        line: u32,
        character: u32,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            let definitions = match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    let text = session
                        .documents
                        .get(path)
                        .map(|doc| doc.text.as_str())
                        .unwrap_or("");
                    mock.definition(path, text, line, character)
                }
                SessionBackend::Process(process) => {
                    let result = process.request(
                        "textDocument/definition",
                        json!({
                            "textDocument": { "uri": path_to_uri(path) },
                            "position": { "line": line, "character": character },
                        }),
                    )?;
                    parse_locations(result)
                }
            };
            Ok(LspOperationResult {
                operation: Some(LspOperation::Definition),
                message: format!("{} definition(s)", definitions.len()),
                definitions,
                ..LspOperationResult::default()
            })
        })
    }

    fn rename(
        &self,
        root: &Path,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            let edits = match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    let text = session
                        .documents
                        .get(path)
                        .map(|doc| doc.text.as_str())
                        .unwrap_or("");
                    mock.rename(path, text, line, character, new_name)
                }
                SessionBackend::Process(process) => {
                    let result = process.request(
                        "textDocument/rename",
                        json!({
                            "textDocument": { "uri": path_to_uri(path) },
                            "position": { "line": line, "character": character },
                            "newName": new_name,
                        }),
                    )?;
                    parse_workspace_edits(result)
                }
            };
            Ok(LspOperationResult {
                operation: Some(LspOperation::Rename),
                message: format!("{} edit(s) for rename to `{new_name}`", edits.len()),
                edits,
                ..LspOperationResult::default()
            })
        })
    }

    fn references(
        &self,
        root: &Path,
        path: &Path,
        line: u32,
        character: u32,
    ) -> JaymiResult<LspOperationResult> {
        self.with_session_mut(root, |session| {
            let references = match &mut session.backend {
                SessionBackend::Mock(mock) => {
                    let text = session
                        .documents
                        .get(path)
                        .map(|doc| doc.text.as_str())
                        .unwrap_or("");
                    mock.references(path, text, line, character)
                }
                SessionBackend::Process(process) => {
                    let result = process.request(
                        "textDocument/references",
                        json!({
                            "textDocument": { "uri": path_to_uri(path) },
                            "position": { "line": line, "character": character },
                            "context": { "includeDeclaration": true },
                        }),
                    )?;
                    parse_locations(result)
                }
            };
            Ok(LspOperationResult {
                operation: Some(LspOperation::References),
                message: format!("{} reference(s)", references.len()),
                references,
                ..LspOperationResult::default()
            })
        })
    }

    fn require_initialized(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("lsp provider is not initialized"))
        }
    }
}

impl Default for LspProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for LspProvider {
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
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| JaymiError::new("lsp session lock poisoned"))?;
        for (_, session) in sessions.drain() {
            if let SessionBackend::Process(mut process) = session.backend {
                let _ = process.shutdown();
            }
        }
        self.initialized = false;
        Ok(())
    }
}

// —— Mock backend ——————————————————————————————————————————————————————————

struct MockSession;

impl MockSession {
    fn new() -> Self {
        Self
    }

    fn did_open(&mut self, _path: &Path, _content: &str) {}
    fn did_change(&mut self, _path: &Path, _content: &str) {}
    fn did_close(&mut self, _path: &Path) {}

    fn diagnostics_for(&self, path: &Path, content: &str) -> Vec<LspDiagnostic> {
        let mut out = Vec::new();
        for (index, line) in content.lines().enumerate() {
            if let Some(col) = line.find("BAD_IDENT") {
                out.push(LspDiagnostic {
                    path: path.display().to_string(),
                    message: "cannot find value `BAD_IDENT` in this scope".into(),
                    severity: "error".into(),
                    range: LspRange {
                        start: LspPosition {
                            line: index as u32,
                            character: col as u32,
                        },
                        end: LspPosition {
                            line: index as u32,
                            character: (col + "BAD_IDENT".len()) as u32,
                        },
                    },
                    source: Some("mock-lsp".into()),
                });
            }
        }
        out
    }

    fn hover(&self, path: &Path, text: &str, line: u32, character: u32) -> Option<LspHover> {
        let word = word_at(text, line, character)?;
        Some(LspHover {
            contents: format!("```rust\n(mock) {word}: _\n```\n\nHover from mock LSP for `{}`.", path.display()),
            range: Some(word_range(text, line, character, &word)?),
        })
    }

    fn completion(
        &self,
        _path: &Path,
        text: &str,
        line: u32,
        character: u32,
    ) -> Vec<LspCompletionItem> {
        let prefix = word_at(text, line, character).unwrap_or_default();
        let mut items = vec![
            LspCompletionItem {
                label: "println!".into(),
                kind: Some("snippet".into()),
                detail: Some("Print to stdout".into()),
                insert_text: Some("println!(\"$1\")".into()),
            },
            LspCompletionItem {
                label: "Vec".into(),
                kind: Some("struct".into()),
                detail: Some("std::vec::Vec".into()),
                insert_text: Some("Vec".into()),
            },
            LspCompletionItem {
                label: "String".into(),
                kind: Some("struct".into()),
                detail: Some("std::string::String".into()),
                insert_text: Some("String".into()),
            },
            LspCompletionItem {
                label: "Result".into(),
                kind: Some("enum".into()),
                detail: Some("std::result::Result".into()),
                insert_text: Some("Result".into()),
            },
        ];
        if !prefix.is_empty() {
            items.retain(|item| item.label.to_lowercase().starts_with(&prefix.to_lowercase()));
            if items.is_empty() {
                items.push(LspCompletionItem {
                    label: prefix.clone(),
                    kind: Some("text".into()),
                    detail: Some("current token".into()),
                    insert_text: Some(prefix),
                });
            }
        }
        items
    }

    fn definition(
        &self,
        path: &Path,
        text: &str,
        line: u32,
        character: u32,
    ) -> Vec<LspLocation> {
        let Some(word) = word_at(text, line, character) else {
            return Vec::new();
        };
        // Prefer the first occurrence of the word in the file as "definition".
        if let Some((def_line, def_col)) = find_word(text, &word) {
            return vec![LspLocation {
                path: path.display().to_string(),
                range: LspRange {
                    start: LspPosition {
                        line: def_line,
                        character: def_col,
                    },
                    end: LspPosition {
                        line: def_line,
                        character: def_col + word.chars().count() as u32,
                    },
                },
            }];
        }
        Vec::new()
    }

    fn rename(
        &self,
        path: &Path,
        text: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Vec<LspTextEdit> {
        let Some(word) = word_at(text, line, character) else {
            return Vec::new();
        };
        let mut edits = Vec::new();
        for (index, line_text) in text.lines().enumerate() {
            let mut start = 0usize;
            while let Some(rel) = line_text[start..].find(&word) {
                let col = start + rel;
                let before_ok = col == 0
                    || !line_text
                        .chars()
                        .nth(col.saturating_sub(1))
                        .is_some_and(is_ident_char);
                let after = col + word.len();
                let after_ok = !line_text
                    .chars()
                    .nth(after)
                    .is_some_and(is_ident_char);
                if before_ok && after_ok {
                    edits.push(LspTextEdit {
                        path: path.display().to_string(),
                        range: LspRange {
                            start: LspPosition {
                                line: index as u32,
                                character: col as u32,
                            },
                            end: LspPosition {
                                line: index as u32,
                                character: after as u32,
                            },
                        },
                        new_text: new_name.to_string(),
                    });
                }
                start = after;
            }
        }
        edits
    }

    fn references(
        &self,
        path: &Path,
        text: &str,
        line: u32,
        character: u32,
    ) -> Vec<LspLocation> {
        self.rename(path, text, line, character, "")
            .into_iter()
            .map(|edit| LspLocation {
                path: edit.path,
                range: edit.range,
            })
            .collect()
    }
}

// —— Process (rust-analyzer) backend ———————————————————————————————————————

struct ProcessSession {
    child: Child,
    stdin: ChildStdin,
    pending: Arc<Mutex<HashMap<i64, Option<Value>>>>,
    diagnostics: Arc<Mutex<Vec<(String, Vec<LspDiagnostic>)>>>,
    next_id: Arc<AtomicI64>,
}

impl ProcessSession {
    fn spawn(root: &Path, command: &[String], next_id: Arc<AtomicI64>) -> JaymiResult<Self> {
        let program = command
            .first()
            .ok_or_else(|| JaymiError::new("lsp command is empty"))?;
        let mut child = Command::new(program)
            .args(&command[1..])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                JaymiError::new(format!("failed to spawn language server `{program}`: {error}"))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| JaymiError::new("lsp stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| JaymiError::new("lsp stdout missing"))?;

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let pending_reader = Arc::clone(&pending);
        let diagnostics_reader = Arc::clone(&diagnostics);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(message) = read_message(&mut reader) {
                if let Some(id) = message.get("id").and_then(Value::as_i64) {
                    if let Ok(mut guard) = pending_reader.lock() {
                        guard.insert(id, message.get("result").cloned());
                    }
                } else if message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics")
                {
                    if let Some(params) = message.get("params") {
                        if let Some((uri, diags)) = parse_publish_diagnostics(params) {
                            if let Ok(mut guard) = diagnostics_reader.lock() {
                                guard.push((uri, diags));
                            }
                        }
                    }
                }
            }
        });

        let mut session = Self {
            child,
            stdin,
            pending,
            diagnostics,
            next_id,
        };

        let init = session.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": path_to_uri(root),
                "capabilities": {
                    "textDocument": {
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "completion": { "completionItem": { "snippetSupport": true } },
                        "publishDiagnostics": {},
                        "definition": { "linkSupport": true },
                        "references": {},
                        "rename": { "prepareSupport": false },
                    },
                    "workspace": {
                        "workspaceEdit": { "documentChanges": true },
                    }
                },
            }),
        )?;
        if init.is_null() {
            return Err(JaymiError::new("language server initialize returned null"));
        }
        session.notify("initialized", json!({}))?;
        Ok(session)
    }

    fn request(&mut self, method: &str, params: Value) -> JaymiResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| JaymiError::new("lsp pending lock poisoned"))?;
            pending.insert(id, None);
        }
        write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }),
        )?;
        let deadline = Instant::now() + RPC_TIMEOUT;
        loop {
            {
                let pending = self
                    .pending
                    .lock()
                    .map_err(|_| JaymiError::new("lsp pending lock poisoned"))?;
                if let Some(Some(result)) = pending.get(&id).cloned() {
                    drop(pending);
                    let mut pending = self
                        .pending
                        .lock()
                        .map_err(|_| JaymiError::new("lsp pending lock poisoned"))?;
                    pending.remove(&id);
                    return Ok(result);
                }
            }
            if Instant::now() > deadline {
                return Err(JaymiError::new(format!(
                    "lsp request `{method}` timed out"
                )));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> JaymiResult<()> {
        write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }),
        )
    }

    fn wait_diagnostics(&self, timeout: Duration) -> Vec<(String, Vec<LspDiagnostic>)> {
        let deadline = Instant::now() + timeout;
        loop {
            let drained = self.drain_diagnostics();
            if !drained.is_empty() {
                return drained;
            }
            if Instant::now() > deadline {
                return Vec::new();
            }
            thread::sleep(Duration::from_millis(40));
        }
    }

    fn drain_diagnostics(&self) -> Vec<(String, Vec<LspDiagnostic>)> {
        self.diagnostics
            .lock()
            .map(|mut guard| std::mem::take(&mut *guard))
            .unwrap_or_default()
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

fn spawn_backend(
    root: &Path,
    next_id: &AtomicI64,
    force_mock: bool,
) -> JaymiResult<SessionBackend> {
    let command = resolve_lsp_command();
    if force_mock || command.first().map(String::as_str) == Some(MOCK_LSP_COMMAND) {
        return Ok(SessionBackend::Mock(MockSession::new()));
    }
    let shared = Arc::new(AtomicI64::new(next_id.load(Ordering::SeqCst)));
    match ProcessSession::spawn(root, &command, Arc::clone(&shared)) {
        Ok(process) => {
            next_id.store(shared.load(Ordering::SeqCst), Ordering::SeqCst);
            Ok(SessionBackend::Process(process))
        }
        Err(error) => {
            jaymi_logging::warn(
                "providers",
                format!(
                    "lsp process unavailable ({error}); falling back to mock language server"
                ),
            );
            Ok(SessionBackend::Mock(MockSession::new()))
        }
    }
}

/// Resolve the language server command line.
pub fn resolve_lsp_command() -> Vec<String> {
    if let Ok(raw) = std::env::var("JAYMI_LSP_COMMAND") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed
                .split_whitespace()
                .map(str::to_string)
                .collect();
        }
    }
    vec![DEFAULT_LSP_COMMAND.to_string()]
}

fn require_path(request: &LspRequest) -> JaymiResult<PathBuf> {
    let path = request
        .path
        .as_ref()
        .ok_or_else(|| JaymiError::new(format!("lsp {} requires a path", request.operation.as_str())))?;
    if path.as_os_str().is_empty() {
        return Err(JaymiError::new("lsp path must not be empty"));
    }
    Ok(path.canonicalize().unwrap_or_else(|_| path.clone()))
}

fn require_position(request: &LspRequest) -> JaymiResult<(u32, u32)> {
    let line = request
        .line
        .ok_or_else(|| JaymiError::new(format!("lsp {} requires line", request.operation.as_str())))?;
    let character = request.character.ok_or_else(|| {
        JaymiError::new(format!(
            "lsp {} requires character",
            request.operation.as_str()
        ))
    })?;
    Ok((line, character))
}

fn language_for_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust".into(),
        "toml" => "toml".into(),
        "md" => "markdown".into(),
        "json" => "json".into(),
        _ => "plaintext".into(),
    }
}

fn path_to_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", abs.display())
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    Some(PathBuf::from(path))
}

fn write_message(writer: &mut impl Write, message: &Value) -> JaymiResult<()> {
    let body = serde_json::to_vec(message)
        .map_err(|error| JaymiError::new(format!("lsp encode failed: {error}")))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())
        .map_err(|error| JaymiError::new(format!("lsp write header failed: {error}")))?;
    writer
        .write_all(&body)
        .map_err(|error| JaymiError::new(format!("lsp write body failed: {error}")))?;
    writer
        .flush()
        .map_err(|error| JaymiError::new(format!("lsp flush failed: {error}")))
}

fn read_message(reader: &mut impl BufRead) -> JaymiResult<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| JaymiError::new(format!("lsp read header failed: {error}")))?;
        if read == 0 {
            return Err(JaymiError::new("lsp server closed stdout"));
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| JaymiError::new(format!("bad Content-Length: {error}")))?,
            );
        }
    }
    let length = content_length.ok_or_else(|| JaymiError::new("lsp missing Content-Length"))?;
    let mut buf = vec![0u8; length];
    reader
        .read_exact(&mut buf)
        .map_err(|error| JaymiError::new(format!("lsp read body failed: {error}")))?;
    serde_json::from_slice(&buf)
        .map_err(|error| JaymiError::new(format!("lsp decode failed: {error}")))
}

fn parse_hover(value: Value) -> Option<LspHover> {
    if value.is_null() {
        return None;
    }
    let contents = value.get("contents")?;
    let text = match contents {
        Value::String(s) => s.clone(),
        Value::Object(map) => map
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(s.clone()),
                Value::Object(map) => map.get("value").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => return None,
    };
    let range = value.get("range").and_then(parse_range);
    Some(LspHover {
        contents: text,
        range,
    })
}

fn parse_completions(value: Value) -> Vec<LspCompletionItem> {
    let items = match value {
        Value::Array(items) => items,
        Value::Object(map) => map
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            Some(LspCompletionItem {
                label: label.clone(),
                kind: item.get("kind").map(|kind| format!("{kind}")),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                insert_text: item
                    .get("insertText")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(Some(label)),
            })
        })
        .collect()
}

fn parse_locations(value: Value) -> Vec<LspLocation> {
    match value {
        Value::Null => Vec::new(),
        Value::Object(_) => parse_location(&value).into_iter().collect(),
        Value::Array(items) => items.iter().filter_map(parse_location).collect(),
        _ => Vec::new(),
    }
}

fn parse_location(value: &Value) -> Option<LspLocation> {
    let uri = value
        .get("uri")
        .or_else(|| value.pointer("/targetUri"))
        .and_then(Value::as_str)?;
    let range = value
        .get("range")
        .or_else(|| value.pointer("/targetRange"))
        .and_then(parse_range)?;
    Some(LspLocation {
        path: uri_to_path(uri)?.display().to_string(),
        range,
    })
}

fn parse_workspace_edits(value: Value) -> Vec<LspTextEdit> {
    let mut edits = Vec::new();
    if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri, list) in changes {
            let Some(path) = uri_to_path(uri) else {
                continue;
            };
            if let Some(items) = list.as_array() {
                for item in items {
                    if let (Some(range), Some(new_text)) = (
                        item.get("range").and_then(parse_range),
                        item.get("newText").and_then(Value::as_str),
                    ) {
                        edits.push(LspTextEdit {
                            path: path.display().to_string(),
                            range,
                            new_text: new_text.to_string(),
                        });
                    }
                }
            }
        }
    }
    if let Some(document_changes) = value.get("documentChanges").and_then(Value::as_array) {
        for change in document_changes {
            let uri = change
                .pointer("/textDocument/uri")
                .and_then(Value::as_str);
            let Some(uri) = uri else {
                continue;
            };
            let Some(path) = uri_to_path(uri) else {
                continue;
            };
            if let Some(items) = change.get("edits").and_then(Value::as_array) {
                for item in items {
                    if let (Some(range), Some(new_text)) = (
                        item.get("range").and_then(parse_range),
                        item.get("newText").and_then(Value::as_str),
                    ) {
                        edits.push(LspTextEdit {
                            path: path.display().to_string(),
                            range,
                            new_text: new_text.to_string(),
                        });
                    }
                }
            }
        }
    }
    edits
}

fn parse_publish_diagnostics(params: &Value) -> Option<(String, Vec<LspDiagnostic>)> {
    let uri = params.get("uri")?.as_str()?.to_string();
    let path = uri_to_path(&uri)?.display().to_string();
    let items = params.get("diagnostics")?.as_array()?;
    let diagnostics = items
        .iter()
        .filter_map(|item| {
            let message = item.get("message")?.as_str()?.to_string();
            let range = item.get("range").and_then(parse_range)?;
            let severity = match item.get("severity").and_then(Value::as_u64).unwrap_or(1) {
                1 => "error",
                2 => "warning",
                3 => "info",
                _ => "hint",
            };
            Some(LspDiagnostic {
                path: path.clone(),
                message,
                severity: severity.into(),
                range,
                source: item
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect();
    Some((uri, diagnostics))
}

fn parse_range(value: &Value) -> Option<LspRange> {
    let start = value.get("start")?;
    let end = value.get("end")?;
    Some(LspRange {
        start: LspPosition {
            line: start.get("line")?.as_u64()? as u32,
            character: start.get("character")?.as_u64()? as u32,
        },
        end: LspPosition {
            line: end.get("line")?.as_u64()? as u32,
            character: end.get("character")?.as_u64()? as u32,
        },
    })
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn word_at(text: &str, line: u32, character: u32) -> Option<String> {
    let line_text = text.lines().nth(line as usize)?;
    let chars: Vec<char> = line_text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let mut index = (character as usize).min(chars.len().saturating_sub(1));
    if index < chars.len() && !is_ident_char(chars[index]) && index > 0 {
        index -= 1;
    }
    if index >= chars.len() || !is_ident_char(chars[index]) {
        return None;
    }
    let mut start = index;
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && is_ident_char(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

fn word_range(text: &str, line: u32, character: u32, word: &str) -> Option<LspRange> {
    let line_text = text.lines().nth(line as usize)?;
    let chars: Vec<char> = line_text.chars().collect();
    let mut index = (character as usize).min(chars.len().saturating_sub(1));
    if index < chars.len() && !is_ident_char(chars[index]) && index > 0 {
        index -= 1;
    }
    let mut start = index;
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }
    Some(LspRange {
        start: LspPosition {
            line,
            character: start as u32,
        },
        end: LspPosition {
            line,
            character: start as u32 + word.chars().count() as u32,
        },
    })
}

fn find_word(text: &str, word: &str) -> Option<(u32, u32)> {
    for (index, line) in text.lines().enumerate() {
        let mut start = 0usize;
        while let Some(rel) = line[start..].find(word) {
            let col = start + rel;
            let before_ok = col == 0
                || !line
                    .chars()
                    .nth(col.saturating_sub(1))
                    .is_some_and(is_ident_char);
            let after = col + word.len();
            let after_ok = !line.chars().nth(after).is_some_and(is_ident_char);
            if before_ok && after_ok {
                return Some((index as u32, col as u32));
            }
            start = after;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mock_hover_completion_and_diagnostics() {
        let dir = temp_dir();
        let file = dir.join("lib.rs");
        let content = "fn main() {\n    let x = BAD_IDENT;\n}\n";
        fs::write(&file, content).unwrap();

        let mut provider = LspProvider::mock();
        provider.initialize().unwrap();

        let open = provider
            .execute(&LspRequest {
                workspace_root: dir.clone(),
                operation: LspOperation::DidOpen,
                path: Some(file.clone()),
                content: Some(content.into()),
                language: Some("rust".into()),
                version: Some(1),
                line: None,
                character: None,
                new_name: None,
            })
            .unwrap();
        assert_eq!(open.diagnostics.len(), 1);
        assert!(open.diagnostics[0].message.contains("BAD_IDENT"));

        let hover = provider
            .execute(&LspRequest {
                workspace_root: dir.clone(),
                operation: LspOperation::Hover,
                path: Some(file.clone()),
                content: None,
                language: None,
                version: None,
                line: Some(1),
                character: Some(12),
                new_name: None,
            })
            .unwrap();
        assert!(hover
            .hover
            .as_ref()
            .map(|item| item.contents.contains("BAD_IDENT"))
            .unwrap_or(false));

        let completion = provider
            .execute(&LspRequest {
                workspace_root: dir,
                operation: LspOperation::Completion,
                path: Some(file),
                content: None,
                language: None,
                version: None,
                line: Some(1),
                character: Some(8),
                new_name: None,
            })
            .unwrap();
        assert!(!completion.completions.is_empty());
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-lsp-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
