//! Configuration for the Jaymi personal AI environment.
//!
//! First subsystem in the deterministic boot sequence. Loads and saves a
//! versioned JSON settings file under the data directory, creating defaults
//! automatically when no file exists.

#![forbid(unsafe_code)]

mod settings;

use std::fs;
use std::path::{Path, PathBuf};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

pub use settings::{LogLevel, ProviderPreferences, Settings, Theme, CURRENT_SETTINGS_VERSION};

const NAME: &str = "configuration";
const DEPENDENCIES: &[&str] = &[];

/// Application configuration loaded before every other subsystem.
#[derive(Debug, Clone)]
pub struct Config {
    initialized: bool,
    /// Local data directory used by later subsystems.
    pub data_dir: String,
    settings: Settings,
    config_path: PathBuf,
    /// When set before initialize, forces the data directory (used by tests).
    data_dir_override: Option<String>,
}

impl Config {
    /// Create an uninitialized configuration service with default paths.
    pub fn new() -> Self {
        let data_dir = settings::default_data_dir_string();
        Self {
            initialized: false,
            config_path: Settings::config_path_for(&data_dir),
            settings: Settings::with_data_dir(data_dir.clone()),
            data_dir,
            data_dir_override: None,
        }
    }

    /// Create an uninitialized configuration rooted at an explicit data directory.
    pub fn with_data_dir(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_string_lossy().into_owned();
        Self {
            initialized: false,
            config_path: Settings::config_path_for(&data_dir),
            settings: Settings::with_data_dir(data_dir.clone()),
            data_dir_override: Some(data_dir.clone()),
            data_dir,
        }
    }

    /// Returns true when configuration has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Path to the persisted JSON configuration file.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Immutable view of the loaded settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Mutable view of the loaded settings.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Persist the current settings to disk.
    pub fn save(&self) -> JaymiResult<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                JaymiError::new(format!(
                    "failed to create config directory {}: {error}",
                    parent.display()
                ))
            })?;
        }

        let encoded = serde_json::to_string_pretty(&self.settings).map_err(|error| {
            JaymiError::new(format!("failed to serialize configuration: {error}"))
        })?;
        fs::write(&self.config_path, format!("{encoded}\n")).map_err(|error| {
            JaymiError::new(format!(
                "failed to write configuration {}: {error}",
                self.config_path.display()
            ))
        })?;
        Ok(())
    }

    /// Reload settings from disk, replacing the in-memory copy.
    pub fn reload(&mut self) -> JaymiResult<()> {
        self.settings = self.load_from_disk_or_defaults()?;
        self.sync_public_fields();
        Ok(())
    }

    fn load_from_disk_or_defaults(&self) -> JaymiResult<Settings> {
        if !self.config_path.exists() {
            return Ok(Settings::with_data_dir(self.data_dir.clone()));
        }

        let raw = fs::read_to_string(&self.config_path).map_err(|error| {
            JaymiError::new(format!(
                "failed to read configuration {}: {error}",
                self.config_path.display()
            ))
        })?;
        let mut settings: Settings = serde_json::from_str(&raw).map_err(|error| {
            JaymiError::new(format!(
                "failed to parse configuration {}: {error}",
                self.config_path.display()
            ))
        })?;

        // The directory that contains the file is authoritative for this run.
        settings.data_dir = self.data_dir.clone();
        if settings.version == 0 {
            settings.version = CURRENT_SETTINGS_VERSION;
        }
        Ok(settings)
    }

    fn sync_public_fields(&mut self) {
        self.data_dir = self.settings.data_dir.clone();
        self.config_path = Settings::config_path_for(&self.data_dir);
    }

    fn resolve_data_dir(&mut self) {
        if let Some(override_dir) = &self.data_dir_override {
            self.data_dir = override_dir.clone();
        } else if self.data_dir.is_empty() {
            self.data_dir = settings::default_data_dir_string();
        }
        self.config_path = Settings::config_path_for(&self.data_dir);
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle for Config {
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
        self.resolve_data_dir();

        fs::create_dir_all(&self.data_dir).map_err(|error| {
            JaymiError::new(format!(
                "failed to create data directory {}: {error}",
                self.data_dir
            ))
        })?;

        let created_defaults = !self.config_path.exists();
        self.settings = self.load_from_disk_or_defaults()?;
        self.sync_public_fields();

        // Always write on first boot so defaults are durable and inspectable.
        // Also rewrite when an override forced a different data_dir.
        if created_defaults || self.data_dir_override.is_some() {
            self.settings.data_dir = self.data_dir.clone();
            self.settings.version = CURRENT_SETTINGS_VERSION.max(self.settings.version);
            self.save()?;
        }

        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let file_ok = !self.initialized || self.config_path.exists();
        let healthy = self.initialized && file_ok && !self.data_dir.is_empty();

        HealthReport::new(
            NAME,
            self.initialized,
            healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            (
                "config_path".to_string(),
                self.config_path.display().to_string(),
            ),
            ("data_dir".to_string(), self.data_dir.clone()),
            (
                "settings_version".to_string(),
                self.settings.version.to_string(),
            ),
            (
                "log_level".to_string(),
                self.settings.log_level.as_str().to_string(),
            ),
            (
                "theme".to_string(),
                self.settings.theme.as_str().to_string(),
            ),
            (
                "indexing_enabled".to_string(),
                self.settings.indexing_enabled.to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn initialize_creates_defaults_on_disk() {
        let dir = temp_dir("defaults");
        let mut config = Config::with_data_dir(&dir);
        config.initialize().unwrap();

        assert!(config.is_initialized());
        assert!(config.config_path().exists());
        assert_eq!(config.data_dir, dir.to_string_lossy());
        assert_eq!(config.settings().log_level, LogLevel::Info);
        assert_eq!(config.settings().theme, Theme::System);
        assert!(config.settings().indexing_enabled);
        assert!(config.settings().default_provider_preferences.prefer_local);

        let raw = fs::read_to_string(config.config_path()).unwrap();
        assert!(raw.contains("\"log_level\": \"info\""));
        assert!(raw.contains("\"theme\": \"system\""));
        assert!(raw.contains("\"indexing_enabled\": true"));
    }

    #[test]
    fn save_and_reload_round_trip() {
        let dir = temp_dir("roundtrip");
        let mut config = Config::with_data_dir(&dir);
        config.initialize().unwrap();

        config.settings_mut().log_level = LogLevel::Warn;
        config.settings_mut().theme = Theme::Dark;
        config.settings_mut().indexing_enabled = false;
        config
            .settings_mut()
            .default_provider_preferences
            .preferred_provider_ids
            .push("filesystem".to_string());
        config.save().unwrap();

        let mut reloaded = Config::with_data_dir(&dir);
        reloaded.initialize().unwrap();
        assert_eq!(reloaded.settings().log_level, LogLevel::Warn);
        assert_eq!(reloaded.settings().theme, Theme::Dark);
        assert!(!reloaded.settings().indexing_enabled);
        assert_eq!(
            reloaded
                .settings()
                .default_provider_preferences
                .preferred_provider_ids,
            vec!["filesystem".to_string()]
        );
    }

    #[test]
    fn reload_reads_external_edits() {
        let dir = temp_dir("reload");
        let mut config = Config::with_data_dir(&dir);
        config.initialize().unwrap();

        let mut edited = config.settings().clone();
        edited.theme = Theme::Light;
        edited.log_level = LogLevel::Error;
        fs::write(
            config.config_path(),
            serde_json::to_string_pretty(&edited).unwrap(),
        )
        .unwrap();

        config.reload().unwrap();
        assert_eq!(config.settings().theme, Theme::Light);
        assert_eq!(config.settings().log_level, LogLevel::Error);
    }

    #[test]
    fn lifecycle_marks_ready() {
        let dir = temp_dir("lifecycle");
        let mut config = Config::with_data_dir(&dir);
        assert!(!config.health_check().initialized);
        config.initialize().unwrap();
        let health = config.health_check();
        assert!(health.initialized);
        assert!(health.healthy);
        assert!(detail(&health, "config_path").ends_with("config.json"));
        config.shutdown().unwrap();
        assert!(!config.health_check().initialized);
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
            "jaymi-config-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
