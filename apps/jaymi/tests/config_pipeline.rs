//! Integration tests for Slice 0.3 — configuration load/save/defaults.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_config::{Config, LogLevel, Theme};
use jaymi_core::Lifecycle;

#[test]
fn boot_creates_default_config_file() {
    let data_dir = temp_dir("config-boot");
    let config_path = data_dir.join("config.json");
    assert!(!config_path.exists());

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    assert!(config_path.exists());

    let config = app.container().resolve::<Config>().expect("config");
    assert!(config.is_initialized());
    assert_eq!(config.settings().log_level, LogLevel::Info);
    assert_eq!(config.settings().theme, Theme::System);
    assert!(config.settings().indexing_enabled);
    assert!(config.settings().default_provider_preferences.prefer_local);

    let snapshot = app.diagnostics().expect("diagnostics");
    assert_eq!(
        snapshot.config_path.as_ref().map(PathBuf::from),
        Some(config_path)
    );
    assert_eq!(snapshot.config_log_level.as_deref(), Some("info"));
    assert_eq!(snapshot.config_theme.as_deref(), Some("system"));
    assert_eq!(snapshot.config_indexing_enabled, Some(true));
}

#[test]
fn boot_loads_saved_configuration() {
    let data_dir = temp_dir("config-load");
    let mut config = Config::with_data_dir(&data_dir);
    config.initialize().unwrap();
    config.settings_mut().log_level = LogLevel::Warn;
    config.settings_mut().theme = Theme::Dark;
    config.settings_mut().indexing_enabled = false;
    config
        .settings_mut()
        .default_provider_preferences
        .preferred_provider_ids
        .push("filesystem".into());
    config.save().unwrap();

    let app = Application::boot_with_data_dir(&data_dir).expect("boot");
    let loaded = app.container().resolve::<Config>().expect("config");
    assert_eq!(loaded.settings().log_level, LogLevel::Warn);
    assert_eq!(loaded.settings().theme, Theme::Dark);
    assert!(!loaded.settings().indexing_enabled);
    assert_eq!(
        loaded
            .settings()
            .default_provider_preferences
            .preferred_provider_ids,
        vec!["filesystem".to_string()]
    );

    let snapshot = app.diagnostics().expect("diagnostics");
    assert_eq!(snapshot.config_log_level.as_deref(), Some("warn"));
    assert_eq!(snapshot.config_theme.as_deref(), Some("dark"));
    assert_eq!(snapshot.config_indexing_enabled, Some(false));
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-config-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
