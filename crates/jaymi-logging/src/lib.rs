//! Logging for the Jaymi personal AI environment.
//!
//! Second subsystem in the deterministic boot sequence. Writes local-only
//! rotating log files under the configured data directory. No cloud logging
//! or telemetry.

#![forbid(unsafe_code)]

mod sink;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
use sink::RotatingFileSink;

pub use sink::LogLevel;

const NAME: &str = "logging";
const DEPENDENCIES: &[&str] = &["configuration"];
const LOGS_SUBDIR: &str = "logs";
const LOG_FILE_NAME: &str = "jaymi.log";

static ACTIVE: OnceLock<Mutex<Option<ActiveLogger>>> = OnceLock::new();

fn active_slot() -> &'static Mutex<Option<ActiveLogger>> {
    ACTIVE.get_or_init(|| Mutex::new(None))
}

#[derive(Clone)]
struct ActiveLogger {
    sink: Arc<RotatingFileSink>,
    min_level: LogLevel,
}

/// Write an informational message to the active local logger, if any.
pub fn info(target: &str, message: impl AsRef<str>) {
    emit(LogLevel::Info, target, message.as_ref());
}

/// Write a warning to the active local logger, if any.
pub fn warn(target: &str, message: impl AsRef<str>) {
    emit(LogLevel::Warn, target, message.as_ref());
}

/// Write an error to the active local logger, if any.
pub fn error(target: &str, message: impl AsRef<str>) {
    emit(LogLevel::Error, target, message.as_ref());
}

fn emit(level: LogLevel, target: &str, message: &str) {
    let Ok(guard) = active_slot().lock() else {
        return;
    };
    if let Some(active) = guard.as_ref() {
        if level < active.min_level {
            return;
        }
        let _ = active.sink.write(level, target, message);
    }
}

/// Logging subsystem responsible for process-wide diagnostic output.
pub struct Logger {
    initialized: bool,
    log_dir: PathBuf,
    log_path: PathBuf,
    min_level: LogLevel,
    sink: Option<Arc<RotatingFileSink>>,
}

impl Logger {
    /// Create an uninitialized logger using the default data directory.
    pub fn new() -> Self {
        Self::with_data_dir(default_data_dir())
    }

    /// Create an uninitialized logger that writes under `data_dir/logs/`.
    pub fn with_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::with_data_dir_and_level(data_dir, LogLevel::Info)
    }

    /// Create an uninitialized logger with an explicit minimum severity.
    pub fn with_data_dir_and_level(data_dir: impl AsRef<Path>, min_level: LogLevel) -> Self {
        let log_dir = data_dir.as_ref().join(LOGS_SUBDIR);
        let log_path = log_dir.join(LOG_FILE_NAME);
        Self {
            initialized: false,
            log_dir,
            log_path,
            min_level,
            sink: None,
        }
    }

    /// Returns true when logging has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Directory containing rotating log files.
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Path to the active log file (`jaymi.log`).
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Configured minimum severity.
    pub fn min_level(&self) -> LogLevel {
        self.min_level
    }

    /// Write an informational line through this logger instance.
    pub fn info(&self, target: &str, message: impl AsRef<str>) {
        self.write(LogLevel::Info, target, message.as_ref());
    }

    /// Write a warning through this logger instance.
    pub fn warn(&self, target: &str, message: impl AsRef<str>) {
        self.write(LogLevel::Warn, target, message.as_ref());
    }

    /// Write an error through this logger instance.
    pub fn error(&self, target: &str, message: impl AsRef<str>) {
        self.write(LogLevel::Error, target, message.as_ref());
    }

    fn write(&self, level: LogLevel, target: &str, message: &str) {
        if level < self.min_level {
            return;
        }
        if let Some(sink) = &self.sink {
            let _ = sink.write(level, target, message);
        }
    }

    fn install_active(&self) {
        if let Some(sink) = &self.sink {
            if let Ok(mut guard) = active_slot().lock() {
                *guard = Some(ActiveLogger {
                    sink: Arc::clone(sink),
                    min_level: self.min_level,
                });
            }
        }
    }

    fn clear_active_if_ours(&self) {
        let Some(sink) = &self.sink else {
            return;
        };
        if let Ok(mut guard) = active_slot().lock() {
            let is_ours = guard
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(&active.sink, sink));
            if is_ours {
                *guard = None;
            }
        }
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Logger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Logger")
            .field("initialized", &self.initialized)
            .field("log_dir", &self.log_dir)
            .field("log_path", &self.log_path)
            .field("min_level", &self.min_level)
            .finish()
    }
}

impl Lifecycle for Logger {
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
        std::fs::create_dir_all(&self.log_dir).map_err(|error| {
            JaymiError::new(format!(
                "failed to create log directory {}: {error}",
                self.log_dir.display()
            ))
        })?;

        let sink = RotatingFileSink::open(&self.log_path).map_err(|error| {
            JaymiError::new(format!(
                "failed to open log file {}: {error}",
                self.log_path.display()
            ))
        })?;
        let sink = Arc::new(sink);
        self.sink = Some(Arc::clone(&sink));
        self.install_active();
        self.initialized = true;

        self.info("logging", "local file logging initialized");
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let writable = self.initialized
            && self.sink.is_some()
            && self.log_path.exists()
            && self
                .log_path
                .parent()
                .map(|parent| parent.is_dir())
                .unwrap_or(false);
        let healthy = self.initialized && writable;

        HealthReport::new(
            NAME,
            self.initialized,
            healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            ("log_path".to_string(), self.log_path.display().to_string()),
            ("log_dir".to_string(), self.log_dir.display().to_string()),
            ("writable".to_string(), writable.to_string()),
            (
                "min_level".to_string(),
                format!("{:?}", self.min_level).to_lowercase(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        if self.initialized {
            self.info("logging", "local file logging shutting down");
        }
        self.clear_active_if_ours();
        self.sink = None;
        self.initialized = false;
        Ok(())
    }
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("jaymi")
        })
        .unwrap_or_else(|| PathBuf::from("./data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn initialize_creates_log_file() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = temp_dir("init");
        let mut logger = Logger::with_data_dir(&dir);
        assert!(!logger.log_path().exists());

        logger.initialize().unwrap();
        assert!(logger.is_initialized());
        assert!(logger.log_dir().is_dir());
        assert!(logger.log_path().exists());

        let health = logger.health_check();
        assert!(health.healthy);
        assert_eq!(
            detail(&health, "log_path"),
            logger.log_path().display().to_string()
        );
        assert_eq!(detail(&health, "writable"), "true");

        logger.info("test", "hello from unit test");
        let contents = std::fs::read_to_string(logger.log_path()).unwrap();
        assert!(contents.contains("local file logging initialized"));
        assert!(contents.contains("hello from unit test"));
        assert!(contents.contains("INFO"));

        logger.shutdown().unwrap();
        assert!(!logger.is_initialized());
        assert!(!logger.health_check().healthy);
    }

    #[test]
    fn free_functions_write_while_active() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = temp_dir("active");
        let mut logger = Logger::with_data_dir(&dir);
        logger.initialize().unwrap();

        info("planner", "request received");
        warn("tools", "slow tool");
        error("providers", "provider failed");

        let contents = std::fs::read_to_string(logger.log_path()).unwrap();
        assert!(contents.contains("[planner] request received"));
        assert!(contents.contains("WARN"));
        assert!(contents.contains("[tools] slow tool"));
        assert!(contents.contains("ERROR"));
        assert!(contents.contains("[providers] provider failed"));

        logger.shutdown().unwrap();
    }

    #[test]
    fn levels_and_targets_are_recorded() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = temp_dir("levels");
        let mut logger = Logger::with_data_dir(&dir);
        logger.initialize().unwrap();
        logger.warn("boot", "startup warning");
        logger.error("boot", "startup error");

        let contents = std::fs::read_to_string(logger.log_path()).unwrap();
        assert!(contents.contains("WARN  [boot] startup warning"));
        assert!(contents.contains("ERROR [boot] startup error"));
        logger.shutdown().unwrap();
    }

    fn detail(report: &HealthReport, key: &str) -> String {
        report
            .details
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-log-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
