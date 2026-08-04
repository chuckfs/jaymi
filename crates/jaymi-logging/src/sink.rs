//! Local rotating file sink — no network, no telemetry.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{JaymiError, JaymiResult};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ROTATED_FILES: usize = 3;

/// Severity for a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Informational messages.
    Info,
    /// Recoverable problems.
    Warn,
    /// Failures that blocked an operation.
    Error,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
        }
    }
}

/// Size-rotated append-only log file writer.
pub struct RotatingFileSink {
    path: PathBuf,
    state: Mutex<SinkState>,
}

struct SinkState {
    file: Option<File>,
    len: u64,
}

impl RotatingFileSink {
    /// Open (or create) the primary log file for appending.
    pub fn open(path: &Path) -> JaymiResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                JaymiError::new(format!(
                    "failed to create log directory {}: {error}",
                    parent.display()
                ))
            })?;
        }

        let file = open_append(path)?;
        let len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);

        Ok(Self {
            path: path.to_path_buf(),
            state: Mutex::new(SinkState {
                file: Some(file),
                len,
            }),
        })
    }

    /// Append one formatted log line, rotating when the size limit is exceeded.
    pub fn write(&self, level: LogLevel, target: &str, message: &str) -> JaymiResult<()> {
        let line = format!(
            "{} {} [{}] {}\n",
            timestamp(),
            level.label(),
            target,
            message
        );
        let bytes = line.as_bytes();

        let mut state = self
            .state
            .lock()
            .map_err(|_| JaymiError::new("log sink lock poisoned"))?;

        if state.len > 0 && state.len.saturating_add(bytes.len() as u64) > MAX_LOG_BYTES {
            // Closing the file handle before rename avoids platform lock issues.
            state.file = None;
            rotate_files(&self.path)?;
            state.file = Some(open_append(&self.path)?);
            state.len = 0;
        }

        let file = state.file.as_mut().ok_or_else(|| {
            JaymiError::new(format!("log file is not open: {}", self.path.display()))
        })?;
        file.write_all(bytes).map_err(|error| {
            JaymiError::new(format!(
                "failed to write log {}: {error}",
                self.path.display()
            ))
        })?;
        file.flush().map_err(|error| {
            JaymiError::new(format!(
                "failed to flush log {}: {error}",
                self.path.display()
            ))
        })?;
        state.len = state.len.saturating_add(bytes.len() as u64);
        Ok(())
    }
}

fn open_append(path: &Path) -> JaymiResult<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            JaymiError::new(format!(
                "failed to open log file {}: {error}",
                path.display()
            ))
        })
}

fn rotate_files(path: &Path) -> JaymiResult<()> {
    let oldest = rotated_path(path, MAX_ROTATED_FILES);
    if oldest.exists() {
        fs::remove_file(&oldest).map_err(|error| {
            JaymiError::new(format!(
                "failed to remove old log {}: {error}",
                oldest.display()
            ))
        })?;
    }

    for index in (1..MAX_ROTATED_FILES).rev() {
        let from = rotated_path(path, index);
        let to = rotated_path(path, index + 1);
        if from.exists() {
            fs::rename(&from, &to).map_err(|error| {
                JaymiError::new(format!(
                    "failed to rotate log {} → {}: {error}",
                    from.display(),
                    to.display()
                ))
            })?;
        }
    }

    if path.exists() {
        let first = rotated_path(path, 1);
        fs::rename(path, &first).map_err(|error| {
            JaymiError::new(format!(
                "failed to rotate log {} → {}: {error}",
                path.display(),
                first.display()
            ))
        })?;
    }

    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), index))
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_creates_and_appends() {
        let dir = temp_dir("sink");
        let path = dir.join("jaymi.log");
        let sink = RotatingFileSink::open(&path).unwrap();
        sink.write(LogLevel::Info, "test", "one").unwrap();
        sink.write(LogLevel::Warn, "test", "two").unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("INFO  [test] one"));
        assert!(contents.contains("WARN  [test] two"));
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-sink-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
