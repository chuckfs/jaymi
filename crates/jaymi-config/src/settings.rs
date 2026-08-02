//! Versioned application settings persisted as JSON.
//!
//! Unknown fields are preserved in [`Settings::extensions`] so newer config
//! keys survive round-trips on older binaries, and missing fields fall back to
//! defaults when loading older files.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Current settings schema version written by this build.
pub const CURRENT_SETTINGS_VERSION: u32 = 1;

const CONFIG_FILE_NAME: &str = "config.json";

/// Minimum severity written to local log files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Warnings and errors.
    Warn,
    /// Informational messages and above.
    #[default]
    Info,
}

impl LogLevel {
    /// Stable label for diagnostics and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
        }
    }
}

/// UI theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the operating system appearance when possible.
    #[default]
    System,
    /// Prefer a light appearance.
    Light,
    /// Prefer a dark appearance.
    Dark,
}

impl Theme {
    /// Stable label for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// Default preferences used when selecting providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPreferences {
    /// Prefer local providers when multiple options exist.
    #[serde(default = "default_true")]
    pub prefer_local: bool,
    /// Ordered list of preferred provider ids.
    #[serde(default)]
    pub preferred_provider_ids: Vec<String>,
}

impl Default for ProviderPreferences {
    fn default() -> Self {
        Self {
            prefer_local: true,
            preferred_provider_ids: Vec::new(),
        }
    }
}

/// Persisted Jaymi settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Settings schema version.
    #[serde(default = "current_settings_version")]
    pub version: u32,
    /// Local data directory for database, logs, and caches.
    #[serde(default = "default_data_dir_string")]
    pub data_dir: String,
    /// Minimum log severity to persist.
    #[serde(default)]
    pub log_level: LogLevel,
    /// Preferred UI theme.
    #[serde(default)]
    pub theme: Theme,
    /// Default provider selection preferences.
    #[serde(default)]
    pub default_provider_preferences: ProviderPreferences,
    /// Whether background indexing is enabled when the knowledge engine exists.
    #[serde(default = "default_true")]
    pub indexing_enabled: bool,
    /// Absolute directories recursively scanned by discovery when no path is given.
    #[serde(default)]
    pub discovery_roots: Vec<String>,
    /// Forward-compatible extension values preserved across load/save.
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_SETTINGS_VERSION,
            data_dir: default_data_dir_string(),
            log_level: LogLevel::Info,
            theme: Theme::System,
            default_provider_preferences: ProviderPreferences::default(),
            indexing_enabled: true,
            discovery_roots: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Build defaults rooted at an explicit data directory.
    pub fn with_data_dir(data_dir: impl Into<String>) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Self::default()
        }
    }

    /// Path to the JSON file for this settings' data directory.
    pub fn config_path_for(data_dir: impl AsRef<std::path::Path>) -> PathBuf {
        data_dir.as_ref().join(CONFIG_FILE_NAME)
    }
}

fn current_settings_version() -> u32 {
    CURRENT_SETTINGS_VERSION
}

fn default_true() -> bool {
    true
}

pub(crate) fn default_data_dir_string() -> String {
    std::env::var_os("HOME")
        .map(|home| {
            std::path::Path::new(&home)
                .join(".local")
                .join("share")
                .join("jaymi")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "./data".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_expected_product_values() {
        let settings = Settings::default();
        assert_eq!(settings.version, CURRENT_SETTINGS_VERSION);
        assert_eq!(settings.log_level, LogLevel::Info);
        assert_eq!(settings.theme, Theme::System);
        assert!(settings.indexing_enabled);
        assert!(settings.discovery_roots.is_empty());
        assert!(settings.default_provider_preferences.prefer_local);
        assert!(settings
            .default_provider_preferences
            .preferred_provider_ids
            .is_empty());
        assert!(!settings.data_dir.is_empty());
    }

    #[test]
    fn missing_fields_deserialize_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"data_dir":"/tmp/jaymi"}"#).unwrap();
        assert_eq!(settings.data_dir, "/tmp/jaymi");
        assert_eq!(settings.version, CURRENT_SETTINGS_VERSION);
        assert_eq!(settings.log_level, LogLevel::Info);
        assert_eq!(settings.theme, Theme::System);
        assert!(settings.indexing_enabled);
        assert!(settings.default_provider_preferences.prefer_local);
    }

    #[test]
    fn unknown_fields_round_trip_via_extensions() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "data_dir":"/tmp/jaymi",
                "future_flag":true,
                "future_map":{"a":1}
            }"#,
        )
        .unwrap();
        assert_eq!(settings.extensions.get("future_flag"), Some(&serde_json::json!(true)));
        assert!(settings.extensions.contains_key("future_map"));

        let encoded = serde_json::to_value(&settings).unwrap();
        assert_eq!(encoded["future_flag"], serde_json::json!(true));
        assert_eq!(encoded["data_dir"], "/tmp/jaymi");
    }
}
