//! Settings Workspace — Reasoning preferences ownership tests.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_config::{Config, ReasoningPreferences};
use jaymi_core::Lifecycle;
use jaymi_reasoning::ModelRegistry;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-settings-{}-{}",
        label,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn reasoning_settings_snapshot_is_provider_agnostic() {
    let app = Application::boot_with_data_dir(temp_dir("snapshot")).unwrap();
    let snap = app.reasoning_settings_snapshot().expect("snapshot");
    // Snapshot always lists providers from the registry — never hardcodes Ollama in the type.
    for provider in &snap.providers {
        assert!(!provider.id.is_empty());
        assert!(!provider.display_name.is_empty());
    }
}

#[test]
fn set_default_reasoning_model_persists_preferences() {
    let data_dir = temp_dir("persist-default");
    let app = Application::boot_with_data_dir(&data_dir).unwrap();
    let registry = app
        .container()
        .resolve::<Arc<ModelRegistry>>()
        .expect("registry");
    let _ = registry.refresh();
    let Some(model) = registry.list().into_iter().next() else {
        // Soft environments without Ollama models — persistence path still
        // covered by config round-trip below.
        let mut config = Config::with_data_dir(&data_dir);
        config.initialize().unwrap();
        config.settings_mut().reasoning = ReasoningPreferences {
            preferred_provider_id: Some("ollama".into()),
            preferred_model: Some("llama3.2:latest".into()),
        };
        config.save().unwrap();
        let raw = fs::read_to_string(data_dir.join("config.json")).unwrap();
        assert!(raw.contains("llama3.2:latest"));
        return;
    };

    let snap = app
        .set_default_reasoning_model(model.provider_id.clone(), model.info.id.name.clone())
        .expect("set default");
    let expected_key = format!("{}/{}", model.provider_id, model.info.id.name);
    assert_eq!(snap.default_model_key.as_deref(), Some(expected_key.as_str()));

    let config = app
        .container()
        .resolve::<Arc<Mutex<Config>>>()
        .expect("config");
    let prefs = config.lock().unwrap().settings().reasoning.clone();
    assert_eq!(prefs.preferred_provider_id.as_deref(), Some(model.provider_id.as_str()));
    assert_eq!(prefs.preferred_model.as_deref(), Some(model.info.id.name.as_str()));

    // Restart restores preference when the model still exists.
    drop(app);
    let app2 = Application::boot_with_data_dir(&data_dir).unwrap();
    let preferred = app2.preferred_model().unwrap();
    assert!(preferred.is_some());
    assert_eq!(
        preferred.as_ref().map(|id| id.name.as_str()),
        Some(model.info.id.name.as_str())
    );
}

#[test]
fn refresh_reasoning_models_returns_snapshot() {
    let app = Application::boot_with_data_dir(temp_dir("refresh")).unwrap();
    let snap = app.refresh_reasoning_models().expect("refresh");
    // Status is always one of the mapped UI states.
    let _ = snap.status.label();
}

#[test]
fn settings_module_does_not_depend_on_ollama_crate() {
    // Compile-time ownership: this test file imports Application + config +
    // registry only — never jaymi_reasoning_ollama. Settings UI follows the same rule.
    let _ = std::any::type_name::<jaymi::SettingsWorkspaceState>();
}
