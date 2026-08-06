//! Transport abstraction over Ollama's local HTTP API.

use std::io::{BufRead, Cursor};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

/// Default Ollama listen address.
pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";

/// Transport-level failures (mapped into [`jaymi_reasoning::ReasoningError`] by the provider).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Could not reach the backend.
    Unavailable(String),
    /// HTTP status indicating failure.
    HttpStatus {
        /// Status code.
        status: u16,
        /// Response body excerpt when available.
        body: String,
    },
    /// I/O or protocol failure while reading.
    Io(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "unavailable: {reason}"),
            Self::HttpStatus { status, body } => {
                write!(f, "http {status}: {body}")
            }
            Self::Io(reason) => write!(f, "io: {reason}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Minimal HTTP surface used by the Ollama provider (sync).
pub trait OllamaTransport: Send + Sync {
    /// GET `path` and return response text.
    fn get_text(&self, path: &str) -> Result<String, TransportError>;

    /// POST JSON to `path` and return the full response body.
    fn post_json(&self, path: &str, body: &Value) -> Result<String, TransportError>;

    /// POST JSON and return a line-oriented reader (NDJSON streams).
    fn post_json_stream(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<Box<dyn BufRead + Send>, TransportError>;
}

/// Live ureq-backed transport.
#[derive(Debug, Clone)]
pub struct HttpOllamaTransport {
    base_url: String,
    timeout: Duration,
}

impl HttpOllamaTransport {
    /// Create a transport for `base_url` (e.g. `http://127.0.0.1:11434`).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout: Duration::from_secs(120),
        }
    }

    /// Override request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn map_ureq(err: ureq::Error) -> TransportError {
        match err {
            ureq::Error::Status(code, response) => {
                let body = response
                    .into_string()
                    .unwrap_or_default()
                    .chars()
                    .take(512)
                    .collect();
                TransportError::HttpStatus {
                    status: code,
                    body,
                }
            }
            ureq::Error::Transport(transport) => {
                TransportError::Unavailable(transport.to_string())
            }
        }
    }
}

impl Default for HttpOllamaTransport {
    fn default() -> Self {
        Self::new(DEFAULT_OLLAMA_BASE_URL)
    }
}

impl OllamaTransport for HttpOllamaTransport {
    fn get_text(&self, path: &str) -> Result<String, TransportError> {
        let response = ureq::get(&self.url(path))
            .timeout(self.timeout)
            .call()
            .map_err(Self::map_ureq)?;
        response.into_string().map_err(|err| TransportError::Io(err.to_string()))
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<String, TransportError> {
        let response = ureq::post(&self.url(path))
            .timeout(self.timeout)
            .set("Content-Type", "application/json")
            .send_json(body.clone())
            .map_err(Self::map_ureq)?;
        response.into_string().map_err(|err| TransportError::Io(err.to_string()))
    }

    fn post_json_stream(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<Box<dyn BufRead + Send>, TransportError> {
        let response = ureq::post(&self.url(path))
            .timeout(self.timeout)
            .set("Content-Type", "application/json")
            .send_json(body.clone())
            .map_err(Self::map_ureq)?;
        Ok(Box::new(std::io::BufReader::new(response.into_reader())))
    }
}

/// In-memory transport for unit tests.
#[derive(Debug, Default)]
pub struct MockOllamaTransport {
    inner: Mutex<MockOllamaState>,
}

#[derive(Debug, Default)]
struct MockOllamaState {
    version: Option<String>,
    tags_json: Option<String>,
    ps_json: Option<String>,
    chat_response: Option<String>,
    chat_stream_lines: Vec<String>,
    unavailable: bool,
    fail_chat_status: Option<(u16, String)>,
    get_calls: Vec<String>,
    post_calls: Vec<String>,
    last_chat_body: Option<Value>,
    last_stream_body: Option<Value>,
}

impl MockOllamaTransport {
    /// Connected mock with version + empty model list.
    pub fn connected(version: impl Into<String>) -> Self {
        let transport = Self::default();
        let version = version.into();
        transport.set_version_json(format!(r#"{{"version":"{version}"}}"#));
        transport.set_tags_json(r#"{"models":[]}"#);
        transport.set_ps_json(r#"{"models":[]}"#);
        transport
    }

    /// Force all calls to fail as unavailable.
    pub fn unavailable() -> Self {
        let transport = Self::default();
        {
            let mut state = transport.inner.lock().expect("lock");
            state.unavailable = true;
        }
        transport
    }

    /// Set `/api/version` JSON body.
    pub fn set_version_json(&self, json: impl Into<String>) {
        self.inner.lock().expect("lock").version = Some(json.into());
    }

    /// Set `/api/tags` JSON body.
    pub fn set_tags_json(&self, json: impl Into<String>) {
        self.inner.lock().expect("lock").tags_json = Some(json.into());
    }

    /// Set `/api/ps` JSON body.
    pub fn set_ps_json(&self, json: impl Into<String>) {
        self.inner.lock().expect("lock").ps_json = Some(json.into());
    }

    /// Set non-streaming chat response body.
    pub fn set_chat_response(&self, json: impl Into<String>) {
        self.inner.lock().expect("lock").chat_response = Some(json.into());
    }

    /// Set NDJSON lines for streaming chat.
    pub fn set_chat_stream_lines(&self, lines: Vec<String>) {
        self.inner.lock().expect("lock").chat_stream_lines = lines;
    }

    /// Fail chat with an HTTP status.
    pub fn fail_chat(&self, status: u16, body: impl Into<String>) {
        self.inner.lock().expect("lock").fail_chat_status = Some((status, body.into()));
    }

    /// Recorded GET paths.
    pub fn get_calls(&self) -> Vec<String> {
        self.inner.lock().expect("lock").get_calls.clone()
    }

    /// Recorded POST paths.
    pub fn post_calls(&self) -> Vec<String> {
        self.inner.lock().expect("lock").post_calls.clone()
    }

    /// Last non-streaming `/api/chat` request body.
    pub fn last_chat_body(&self) -> Option<Value> {
        self.inner.lock().expect("lock").last_chat_body.clone()
    }

    /// Last streaming `/api/chat` request body.
    pub fn last_stream_body(&self) -> Option<Value> {
        self.inner.lock().expect("lock").last_stream_body.clone()
    }
}

impl OllamaTransport for MockOllamaTransport {
    fn get_text(&self, path: &str) -> Result<String, TransportError> {
        let mut state = self.inner.lock().expect("lock");
        state.get_calls.push(path.to_string());
        if state.unavailable {
            return Err(TransportError::Unavailable("mock offline".into()));
        }
        match path {
            "/api/version" => state
                .version
                .clone()
                .ok_or_else(|| TransportError::Unavailable("no version".into())),
            "/api/tags" => state
                .tags_json
                .clone()
                .ok_or_else(|| TransportError::Unavailable("no tags".into())),
            "/api/ps" => state
                .ps_json
                .clone()
                .ok_or_else(|| TransportError::Io("no ps".into())),
            other => Err(TransportError::Io(format!("unexpected GET {other}"))),
        }
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<String, TransportError> {
        let mut state = self.inner.lock().expect("lock");
        state.post_calls.push(path.to_string());
        if path == "/api/chat" {
            state.last_chat_body = Some(body.clone());
        }
        if state.unavailable {
            return Err(TransportError::Unavailable("mock offline".into()));
        }
        if let Some((status, body)) = &state.fail_chat_status {
            return Err(TransportError::HttpStatus {
                status: *status,
                body: body.clone(),
            });
        }
        match path {
            "/api/chat" => state
                .chat_response
                .clone()
                .ok_or_else(|| TransportError::Io("no chat response".into())),
            other => Err(TransportError::Io(format!("unexpected POST {other}"))),
        }
    }

    fn post_json_stream(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<Box<dyn BufRead + Send>, TransportError> {
        let mut state = self.inner.lock().expect("lock");
        state.post_calls.push(format!("{path}?stream"));
        if path == "/api/chat" {
            state.last_stream_body = Some(body.clone());
        }
        if state.unavailable {
            return Err(TransportError::Unavailable("mock offline".into()));
        }
        if let Some((status, body)) = &state.fail_chat_status {
            return Err(TransportError::HttpStatus {
                status: *status,
                body: body.clone(),
            });
        }
        if path != "/api/chat" {
            return Err(TransportError::Io(format!("unexpected stream POST {path}")));
        }
        let mut blob = state.chat_stream_lines.join("\n");
        if !blob.is_empty() && !blob.ends_with('\n') {
            blob.push('\n');
        }
        Ok(Box::new(Cursor::new(blob.into_bytes())))
    }
}
