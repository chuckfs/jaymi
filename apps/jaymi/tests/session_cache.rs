//! Session-scoped cache for inexpensive immutable data.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_config::{Config, Theme};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-session-cache-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn boot_seeds_session_cache() {
    let app = Application::boot_with_data_dir(temp_dir("seed")).unwrap();
    let summary = app.session_cache_summary();
    assert!(
        summary.contains("models=hit"),
        "expected models warm: {summary}"
    );
    assert!(
        summary.contains("capabilities=hit"),
        "expected capabilities warm: {summary}"
    );
    assert!(
        summary.contains("settings=hit"),
        "expected settings warm: {summary}"
    );
    assert!(app.theme_preference().is_ok());
    assert!(app.settings_snapshot().is_ok());
}

#[test]
fn refresh_models_invalidates_and_rewarm_models_slot() {
    let app = Application::boot_with_data_dir(temp_dir("refresh")).unwrap();
    let before = app.session_cache_generation();
    let _ = app.refresh_reasoning_models().expect("refresh");
    assert!(
        app.session_cache_generation() > before,
        "refresh must bump generation"
    );
    assert!(app.session_cache_summary().contains("models=hit"));
}

#[test]
fn settings_change_invalidates_settings_and_theme() {
    let data_dir = temp_dir("settings");
    let app = Application::boot_with_data_dir(&data_dir).unwrap();
    let before = app.session_cache_generation();

    {
        let config = app
            .container()
            .resolve::<Arc<Mutex<Config>>>()
            .expect("config");
        let mut config = config.lock().unwrap();
        config.settings_mut().theme = Theme::Dark;
        config.save().unwrap();
    }
    app.notify_settings_changed();

    assert!(app.session_cache_generation() > before);
    assert_eq!(app.theme_preference().unwrap(), Theme::Dark);
    assert!(app.session_cache_summary().contains("settings=hit"));
}

#[test]
fn provider_registration_invalidates_models_and_capabilities() {
    let app = Application::boot_with_data_dir(temp_dir("providers")).unwrap();
    let before = app.session_cache_generation();
    app.notify_providers_changed();
    assert!(app.session_cache_generation() > before);
    let summary = app.session_cache_summary();
    assert!(
        summary.contains("models=miss") || summary.contains("capabilities=miss"),
        "provider notify should clear registry/capability slots: {summary}"
    );
}

#[test]
fn session_cache_never_holds_conversation_state() {
    let app = Application::boot_with_data_dir(temp_dir("no-conversation")).unwrap();
    let _ = app
        .handle(jaymi_core::UserRequest::new("hello session cache"))
        .expect("handle");
    // Cache summary stays about immutable slots only — no turn / transcript fields.
    let summary = app.session_cache_summary();
    assert!(summary.contains("generation="));
    assert!(!summary.to_lowercase().contains("conversation"));
    assert!(!summary.to_lowercase().contains("turn"));
}

#[test]
fn diagnostics_surfaces_session_cache_row() {
    let app = Application::boot_with_data_dir(temp_dir("diag")).unwrap();
    let snapshot = app.diagnostics().expect("diagnostics");
    let row = snapshot
        .subsystem("Session Cache")
        .expect("Session Cache subsystem");
    assert!(row.detail.contains("generation="));
    assert!(row.detail.contains("models="));
}
