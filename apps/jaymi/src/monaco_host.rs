//! Monaco editor host — wry child WebView overlay for the Coding Workspace.
//!
//! Architecture:
//! - Rust [`CodingState`] remains the source of truth for buffers (survives remounts).
//! - Monaco is a view: rehydrated from CodingState whenever the WebView is (re)created.
//! - Edits flow Monaco → IPC → [`CodingShellEvent::EditContent`] → Application (Planner unchanged).

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;
use serde::Deserialize;
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::http::{header, Response, StatusCode};
use wry::raw_window_handle::HasWindowHandle;
use wry::{Rect, WebView, WebViewBuilder};

/// Document projected into Monaco for the active editor tab.
#[derive(Debug, Clone, PartialEq)]
pub struct MonacoDocument {
    /// Absolute file path (also used as model identity).
    pub path: String,
    /// Buffer contents from [`CodingState`].
    pub content: String,
    /// Monaco language id.
    pub language: String,
    /// Vertical scroll in pixels.
    pub scroll_top: f32,
    /// Whether the minimap is enabled.
    pub minimap: bool,
}

/// Screen-space viewport reserved for the Monaco overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonacoViewport {
    /// egui screen rect for the editor body.
    pub rect: egui::Rect,
}

/// Messages from the Monaco WebView to Rust.
#[derive(Debug, Clone, PartialEq)]
pub enum MonacoIpcMessage {
    /// Monaco finished loading.
    Ready,
    /// Buffer edited in Monaco.
    Change { path: String, content: String },
    /// Scroll position changed.
    Scroll { path: String, offset: f32 },
    /// ⌘S / Ctrl+S inside Monaco.
    Save { path: String },
    /// Language Server request from Monaco providers.
    Lsp {
        id: u64,
        method: String,
        path: String,
        line: u32,
        character: u32,
        new_name: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct IpcPayload {
    #[serde(rename = "type")]
    kind: String,
    path: Option<String>,
    content: Option<String>,
    offset: Option<f64>,
    id: Option<u64>,
    method: Option<String>,
    line: Option<u32>,
    character: Option<u32>,
    #[serde(rename = "newName")]
    new_name: Option<String>,
}

/// Owns the wry WebView and keeps it synced with CodingState.
pub struct MonacoHost {
    webview: WebView,
    rx: Receiver<MonacoIpcMessage>,
    ready: bool,
    last_pushed: Option<MonacoDocument>,
    assets_dir: PathBuf,
}

impl MonacoHost {
    /// Create a child WebView hosted in the eframe window.
    pub fn new(window: &impl HasWindowHandle, assets_dir: PathBuf) -> Result<Self, String> {
        if !assets_dir.join("index.html").is_file() {
            return Err(format!(
                "monaco assets missing index.html under {}",
                assets_dir.display()
            ));
        }
        if !assets_dir.join("vs/loader.js").is_file() {
            return Err(format!(
                "monaco assets missing vs/loader.js under {} (run app build to fetch)",
                assets_dir.display()
            ));
        }

        let (tx, rx) = mpsc::channel();
        let assets_for_protocol = assets_dir.clone();

        let webview = WebViewBuilder::new_as_child(window)
            .with_visible(false)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |request| {
                if let Some(parsed) = parse_ipc(request.body()) {
                    let _ = tx.send(parsed);
                }
            })
            .with_custom_protocol("jaymi".into(), move |_request| {
                // wry 0.45: handler receives Request<Vec<u8>>; path is in the URI.
                serve_asset(&assets_for_protocol, _request.uri().path())
            })
            .with_url("jaymi://localhost/index.html")
            .build()
            .map_err(|error| format!("failed to create monaco webview: {error}"))?;

        Ok(Self {
            webview,
            rx,
            ready: false,
            last_pushed: None,
            assets_dir,
        })
    }

    /// Assets directory used by this host.
    pub fn assets_dir(&self) -> &Path {
        &self.assets_dir
    }

    /// Drain IPC messages from Monaco.
    pub fn poll(&mut self) -> Vec<MonacoIpcMessage> {
        let mut messages = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(MonacoIpcMessage::Ready) => {
                    self.ready = true;
                    // Force a re-push of the current document after reload.
                    self.last_pushed = None;
                    messages.push(MonacoIpcMessage::Ready);
                }
                Ok(message) => messages.push(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        messages
    }

    /// Show or hide the overlay and update its bounds to match the egui rect.
    pub fn set_viewport(
        &self,
        viewport: Option<MonacoViewport>,
        screen_height: f32,
        zoom: f32,
    ) -> Result<(), String> {
        match viewport {
            None => self
                .webview
                .set_visible(false)
                .map_err(|error| format!("hide monaco: {error}")),
            Some(viewport) => {
                let rect = viewport.rect * zoom;
                let height = screen_height * zoom;
                self.webview
                    .set_bounds(Rect {
                        position: Position::Logical(LogicalPosition::new(
                            f64::from(rect.min.x),
                            f64::from(height - rect.max.y),
                        )),
                        size: Size::Logical(LogicalSize::new(
                            f64::from(rect.width().max(1.0)),
                            f64::from(rect.height().max(1.0)),
                        )),
                    })
                    .map_err(|error| format!("bounds monaco: {error}"))?;
                self.webview
                    .set_visible(true)
                    .map_err(|error| format!("show monaco: {error}"))
            }
        }
    }

    /// Push CodingState buffer into Monaco when the document identity/content changes.
    pub fn sync_document(&mut self, document: &MonacoDocument) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        if self.last_pushed.as_ref() == Some(document) {
            return Ok(());
        }

        let script = format!(
            "window.__jaymiSetDocument && window.__jaymiSetDocument({}, {}, {}, {}, {});",
            json_string(&document.path),
            json_string(&document.content),
            json_string(&document.language),
            if document.minimap { "true" } else { "false" },
            document.scroll_top as i64
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco document sync: {error}"))?;
        self.last_pushed = Some(document.clone());
        Ok(())
    }

    /// Update minimap without rewriting the buffer.
    pub fn set_minimap(&self, enabled: bool) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        let script = format!(
            "window.__jaymiSetMinimap && window.__jaymiSetMinimap({});",
            if enabled { "true" } else { "false" }
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco minimap: {error}"))
    }

    /// Mark that CodingState accepted an edit originating from Monaco (echo suppression).
    pub fn note_external_edit(&mut self, path: &str, content: &str) {
        if let Some(previous) = &mut self.last_pushed {
            if previous.path == path {
                previous.content = content.to_string();
            }
        }
    }

    /// Resolve a Monaco LSP request promise.
    pub fn resolve_lsp(&self, id: u64, payload_json: &str) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        let script = format!(
            "window.__jaymiLspResult && window.__jaymiLspResult({}, {});",
            id, payload_json
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco lsp resolve: {error}"))
    }

    /// Push diagnostics markers into Monaco for the active model.
    pub fn set_diagnostics(&self, markers_json: &str) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        let script = format!(
            "window.__jaymiSetDiagnostics && window.__jaymiSetDiagnostics({});",
            markers_json
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco diagnostics: {error}"))
    }
}

/// Map a file path to a Monaco language id.
pub fn language_for_path(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "toml" => "ini",
        "md" | "markdown" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "jsx" => "javascript",
        "py" => "python",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" | "zsh" => "shell",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "xml" => "xml",
        "sql" => "sql",
        "txt" | "log" => "plaintext",
        _ => "plaintext",
    }
}

/// Resolve the Monaco assets directory (dev checkout or next to the executable).
pub fn resolve_monaco_assets() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/monaco");
    if manifest.join("index.html").is_file() && manifest.join("vs/loader.js").is_file() {
        return manifest;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("assets/monaco");
            if candidate.join("index.html").is_file() {
                return candidate;
            }
        }
    }
    manifest
}

fn parse_ipc(body: &str) -> Option<MonacoIpcMessage> {
    let payload: IpcPayload = serde_json::from_str(body).ok()?;
    match payload.kind.as_str() {
        "ready" => Some(MonacoIpcMessage::Ready),
        "change" => Some(MonacoIpcMessage::Change {
            path: payload.path.unwrap_or_default(),
            content: payload.content.unwrap_or_default(),
        }),
        "scroll" => Some(MonacoIpcMessage::Scroll {
            path: payload.path.unwrap_or_default(),
            offset: payload.offset.unwrap_or(0.0) as f32,
        }),
        "save" => Some(MonacoIpcMessage::Save {
            path: payload.path.unwrap_or_default(),
        }),
        "lsp" => Some(MonacoIpcMessage::Lsp {
            id: payload.id.unwrap_or(0),
            method: payload.method.unwrap_or_default(),
            path: payload.path.unwrap_or_default(),
            line: payload.line.unwrap_or(0),
            character: payload.character.unwrap_or(0),
            new_name: payload.new_name,
        }),
        _ => None,
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn serve_asset(assets_dir: &Path, request_path: &str) -> Response<Cow<'static, [u8]>> {
    let relative = request_path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let path = assets_dir.join(relative);
    let canonical_assets = assets_dir.canonicalize().unwrap_or_else(|_| assets_dir.to_path_buf());
    let Ok(canonical_file) = path.canonicalize() else {
        return not_found();
    };
    if !canonical_file.starts_with(&canonical_assets) {
        return not_found();
    }
    let Ok(bytes) = std::fs::read(&canonical_file) else {
        return not_found();
    };
    let mime = mime_for_path(&canonical_file);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Cow::Owned(bytes))
        .unwrap_or_else(|_| not_found())
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Cow::Borrowed(b"not found".as_slice()))
        .expect("static not-found response")
}

fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_mapping_covers_coding_extensions() {
        assert_eq!(language_for_path("src/main.rs"), "rust");
        assert_eq!(language_for_path("Cargo.toml"), "ini");
        assert_eq!(language_for_path("README.md"), "markdown");
        assert_eq!(language_for_path("data.json"), "json");
        assert_eq!(language_for_path("config.yaml"), "yaml");
        assert_eq!(language_for_path("notes.txt"), "plaintext");
    }

    #[test]
    fn parses_monaco_ipc_payloads() {
        assert_eq!(
            parse_ipc(r#"{"type":"ready"}"#),
            Some(MonacoIpcMessage::Ready)
        );
        assert_eq!(
            parse_ipc(r#"{"type":"change","path":"/a.rs","content":"fn main(){}"}"#),
            Some(MonacoIpcMessage::Change {
                path: "/a.rs".into(),
                content: "fn main(){}".into(),
            })
        );
        assert_eq!(
            parse_ipc(r#"{"type":"scroll","path":"/a.rs","offset":12.5}"#),
            Some(MonacoIpcMessage::Scroll {
                path: "/a.rs".into(),
                offset: 12.5,
            })
        );
        assert_eq!(
            parse_ipc(r#"{"type":"save","path":"/a.rs"}"#),
            Some(MonacoIpcMessage::Save {
                path: "/a.rs".into(),
            })
        );
    }

    #[test]
    fn serve_asset_reads_index_html() {
        let assets = resolve_monaco_assets();
        if !assets.join("index.html").is_file() {
            return;
        }
        let response = serve_asset(&assets, "/index.html");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.body().is_empty());
    }
}
