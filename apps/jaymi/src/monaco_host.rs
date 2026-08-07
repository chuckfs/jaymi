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
    /// Zero-based cursor line.
    pub cursor_line: u32,
    /// Zero-based cursor column.
    pub cursor_column: u32,
    /// Collapsed fold ranges (zero-based inclusive).
    pub folded_regions: Vec<(u32, u32)>,
    /// Whether the minimap is enabled.
    pub minimap: bool,
    /// Whether word wrap is enabled.
    pub word_wrap: bool,
    /// Font size in pixels.
    pub font_size: u32,
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
    /// Cursor position changed.
    Cursor {
        path: String,
        line: u32,
        column: u32,
    },
    /// Text selection changed (range + optional text).
    Selection {
        path: String,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
        text: Option<String>,
    },
    /// Folded regions changed.
    Folds {
        path: String,
        regions: Vec<(u32, u32)>,
    },
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
    column: Option<u32>,
    #[serde(rename = "startLine")]
    start_line: Option<u32>,
    #[serde(rename = "startColumn")]
    start_column: Option<u32>,
    #[serde(rename = "endLine")]
    end_line: Option<u32>,
    #[serde(rename = "endColumn")]
    end_column: Option<u32>,
    text: Option<String>,
    #[serde(rename = "newName")]
    new_name: Option<String>,
    #[serde(default)]
    folds: Option<Vec<IpcFold>>,
}

#[derive(Debug, Deserialize)]
struct IpcFold {
    #[serde(rename = "startLine")]
    start_line: u32,
    #[serde(rename = "endLine")]
    end_line: u32,
}

/// Owns the wry WebView and keeps it synced with CodingState.
pub struct MonacoHost {
    webview: WebView,
    rx: Receiver<MonacoIpcMessage>,
    ready: bool,
    last_pushed: Option<MonacoDocument>,
    /// Last Monaco theme id pushed (`jaymi-light` / `jaymi-dark`).
    last_theme_id: Option<String>,
    /// Last Logical bounds applied (x, y from bottom, w, h) — skip redundant set_bounds.
    last_bounds: Option<(f64, f64, f64, f64)>,
    assets_dir: PathBuf,
    /// True after we forced the WebView to resign keyboard for egui chrome.
    keyboard_released: bool,
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
            last_theme_id: None,
            last_bounds: None,
            assets_dir,
            keyboard_released: false,
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

    /// Release keyboard so egui TextEdit (terminal, search, …) can receive keys.
    ///
    /// Child WKWebView stays first-responder after Monaco clicks. Blur the JS
    /// editor and briefly toggle visibility (no `unsafe`) so the parent window
    /// can deliver keys to egui. Call at most once per focus episode.
    pub fn release_keyboard(&mut self) -> Result<(), String> {
        if self.keyboard_released {
            return Ok(());
        }
        let _ = self.webview.evaluate_script(
            "(function(){try{if(window.editor&&editor.blur)editor.blur();var a=document.activeElement;if(a&&a.blur)a.blur();}catch(e){}})();",
        );
        if self.ready && self.last_bounds.is_some() {
            let _ = self.webview.set_visible(false);
            let _ = self.webview.set_visible(true);
        }
        self.keyboard_released = true;
        Ok(())
    }

    /// Allow Monaco to take keyboard again after egui chrome is done.
    pub fn clear_keyboard_release(&mut self) {
        self.keyboard_released = false;
    }

    /// Show or hide the overlay and update its bounds to match the egui rect.
    ///
    /// `viewport.rect` and `screen_height` are egui **points**. wry Logical
    /// coordinates are also points — do not multiply by `pixels_per_point` or
    /// Retina displays place the WebView off-screen (double-scaled).
    pub fn set_viewport(
        &mut self,
        viewport: Option<MonacoViewport>,
        screen_height: f32,
        _zoom: f32,
    ) -> Result<(), String> {
        match viewport {
            None => {
                self.last_bounds = None;
                self.webview
                    .set_visible(false)
                    .map_err(|error| format!("hide monaco: {error}"))
            }
            Some(viewport) => {
                // Keep the child WebView hidden until Monaco finishes loading so
                // a blank overlay cannot steal clicks from Project Explorer.
                if !self.ready {
                    self.last_bounds = None;
                    return self
                        .webview
                        .set_visible(false)
                        .map_err(|error| format!("hide monaco: {error}"));
                }
                let rect = viewport.rect;
                if !rect.min.x.is_finite()
                    || !rect.min.y.is_finite()
                    || !rect.max.x.is_finite()
                    || !rect.max.y.is_finite()
                    || rect.width() < 2.0
                    || rect.height() < 2.0
                {
                    self.last_bounds = None;
                    return self
                        .webview
                        .set_visible(false)
                        .map_err(|error| format!("hide monaco: {error}"));
                }
                let x = f64::from(rect.min.x);
                let y = f64::from(screen_height - rect.max.y);
                let w = f64::from(rect.width());
                let h = f64::from(rect.height());
                let next = (x, y, w, h);
                let bounds_changed = self.last_bounds.is_none_or(|prev| {
                    (prev.0 - next.0).abs() > 0.5
                        || (prev.1 - next.1).abs() > 0.5
                        || (prev.2 - next.2).abs() > 0.5
                        || (prev.3 - next.3).abs() > 0.5
                });
                if bounds_changed {
                    self.webview
                        .set_bounds(Rect {
                            position: Position::Logical(LogicalPosition::new(x, y)),
                            size: Size::Logical(LogicalSize::new(w, h)),
                        })
                        .map_err(|error| format!("bounds monaco: {error}"))?;
                    self.last_bounds = Some(next);
                }
                self.webview
                    .set_visible(true)
                    .map_err(|error| format!("show monaco: {error}"))
            }
        }
    }

    /// Release keyboard so egui TextEdit (terminal, search, …) can receive keys.
    ///
    /// Child WKWebView stays first-responder after Monaco clicks. Blur the JS
    /// editor and briefly toggle visibility (no `unsafe`) so the parent window
    /// can deliver keys to egui. Call at most once per focus episode.
    pub fn release_keyboard(&mut self) -> Result<(), String> {
        if self.keyboard_released {
            return Ok(());
        }
        let _ = self.webview.evaluate_script(
            "(function(){try{if(window.editor&&editor.blur)editor.blur();var a=document.activeElement;if(a&&a.blur)a.blur();}catch(e){}})();",
        );
        if self.ready && self.last_bounds.is_some() {
            let _ = self.webview.set_visible(false);
            let _ = self.webview.set_visible(true);
        }
        self.keyboard_released = true;
        Ok(())
    }

    /// Allow Monaco to take keyboard again after egui chrome is done.
    pub fn clear_keyboard_release(&mut self) {
        self.keyboard_released = false;
    }

    /// Whether Monaco finished loading and can accept documents.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Push CodingState buffer into Monaco when the document identity/content changes.
    ///
    /// Scroll and cursor are owned by Monaco while a tab stays open — re-pushing them
    /// on every IPC echo calls `revealPosition` and snaps the viewport back to the
    /// cursor (often the top of the file). View-only diffs update `last_pushed` only.
    pub fn sync_document(&mut self, document: &MonacoDocument) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        if let Some(previous) = &self.last_pushed {
            let same_buffer = previous.path == document.path
                && previous.content == document.content
                && previous.language == document.language
                && previous.minimap == document.minimap
                && previous.word_wrap == document.word_wrap
                && previous.font_size == document.font_size
                && previous.folded_regions == document.folded_regions;
            if same_buffer {
                // Scroll / cursor drifted via Monaco IPC — do not echo back.
                self.last_pushed = Some(document.clone());
                return Ok(());
            }
        }

        let folds_json = serde_json::to_string(
            &document
                .folded_regions
                .iter()
                .map(|(start, end)| serde_json::json!({ "startLine": start, "endLine": end }))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());
        let script = format!(
            "window.__jaymiSetDocument && window.__jaymiSetDocument({}, {}, {}, {}, {}, {}, {}, {}, {}, {});",
            json_string(&document.path),
            json_string(&document.content),
            json_string(&document.language),
            if document.minimap { "true" } else { "false" },
            if document.word_wrap { "true" } else { "false" },
            document.font_size,
            document.scroll_top as i64,
            document.cursor_line,
            document.cursor_column,
            folds_json
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco document sync: {error}"))?;
        self.last_pushed = Some(document.clone());
        Ok(())
    }

    /// Update minimap without rewriting the buffer.
    pub fn set_minimap(&self, enabled: bool) -> Result<(), String> {
        self.set_editor_options(Some(enabled), None, None)
    }

    /// Push workspace-owned editor chrome options into Monaco.
    pub fn set_editor_options(
        &self,
        minimap: Option<bool>,
        word_wrap: Option<bool>,
        font_size: Option<u32>,
    ) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        let minimap_js = minimap
            .map(|value| if value { "true" } else { "false" })
            .unwrap_or("null");
        let wrap_js = word_wrap
            .map(|value| if value { "true" } else { "false" })
            .unwrap_or("null");
        let font_js = font_size
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let script = format!(
            "window.__jaymiSetOptions && window.__jaymiSetOptions({{ minimap: {minimap_js}, wordWrap: {wrap_js}, fontSize: {font_js} }});"
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco options: {error}"))
    }

    /// Define and activate a Jaymi Monaco theme generated from the app [`crate::Theme`].
    ///
    /// `definition_json` is a Monaco `IStandaloneThemeData` object (already JSON).
    /// No-ops when the same theme id is already active.
    pub fn set_theme(&mut self, theme_id: &str, definition_json: &str) -> Result<(), String> {
        if !self.ready {
            return Ok(());
        }
        if self.last_theme_id.as_deref() == Some(theme_id) {
            return Ok(());
        }
        let script = format!(
            "window.__jaymiSetTheme && window.__jaymiSetTheme({}, {});",
            json_string(theme_id),
            definition_json
        );
        self.webview
            .evaluate_script(&script)
            .map_err(|error| format!("monaco theme: {error}"))?;
        self.last_theme_id = Some(theme_id.to_string());
        Ok(())
    }

    /// Force the next [`Self::set_theme`] call to re-push (e.g. after WebView remount).
    pub fn clear_theme_cache(&mut self) {
        self.last_theme_id = None;
    }

    /// Mark that CodingState accepted an edit originating from Monaco (echo suppression).
    pub fn note_external_edit(&mut self, path: &str, content: &str) {
        if let Some(previous) = &mut self.last_pushed {
            if previous.path == path {
                previous.content = content.to_string();
            }
        }
    }

    /// Record Monaco-owned scroll so the next sync does not echo it back.
    pub fn note_external_scroll(&mut self, path: &str, scroll_top: f32) {
        if let Some(previous) = &mut self.last_pushed {
            if previous.path == path {
                previous.scroll_top = scroll_top;
            }
        }
    }

    /// Record Monaco-owned cursor so the next sync does not echo it back.
    pub fn note_external_cursor(&mut self, path: &str, line: u32, column: u32) {
        if let Some(previous) = &mut self.last_pushed {
            if previous.path == path {
                previous.cursor_line = line;
                previous.cursor_column = column;
            }
        }
    }

    /// Record Monaco-owned folds so the next sync does not echo them back.
    pub fn note_external_folds(&mut self, path: &str, regions: &[(u32, u32)]) {
        if let Some(previous) = &mut self.last_pushed {
            if previous.path == path {
                previous.folded_regions = regions.to_vec();
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
        "cursor" => Some(MonacoIpcMessage::Cursor {
            path: payload.path.unwrap_or_default(),
            line: payload.line.unwrap_or(0),
            column: payload.column.or(payload.character).unwrap_or(0),
        }),
        "selection" => {
            let start_line = payload.start_line.unwrap_or(0);
            let start_column = payload.start_column.unwrap_or(0);
            let end_line = payload.end_line.unwrap_or(start_line);
            let end_column = payload.end_column.unwrap_or(start_column);
            let text = payload
                .text
                .map(|t| t.trim_end_matches('\0').to_string())
                .filter(|t| !t.is_empty());
            Some(MonacoIpcMessage::Selection {
                path: payload.path.unwrap_or_default(),
                start_line,
                start_column,
                end_line,
                end_column,
                text,
            })
        }
        "folds" => Some(MonacoIpcMessage::Folds {
            path: payload.path.unwrap_or_default(),
            regions: payload
                .folds
                .unwrap_or_default()
                .into_iter()
                .map(|fold| (fold.start_line, fold.end_line))
                .collect(),
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
    let canonical_assets = assets_dir
        .canonicalize()
        .unwrap_or_else(|_| assets_dir.to_path_buf());
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
            parse_ipc(r#"{"type":"cursor","path":"/a.rs","line":3,"column":7}"#),
            Some(MonacoIpcMessage::Cursor {
                path: "/a.rs".into(),
                line: 3,
                column: 7,
            })
        );
        assert_eq!(
            parse_ipc(
                r#"{"type":"selection","path":"/a.rs","startLine":1,"startColumn":0,"endLine":1,"endColumn":8,"text":"fn hello"}"#
            ),
            Some(MonacoIpcMessage::Selection {
                path: "/a.rs".into(),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 8,
                text: Some("fn hello".into()),
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
