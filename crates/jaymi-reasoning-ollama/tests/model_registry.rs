//! Model Registry integration tests (Sprint B1.9).

use std::sync::Arc;

use jaymi_reasoning::{
    ModelIdentifier, ModelRegistry, ReasoningError, ReasoningHealth, ReasoningModelInfo,
    ReasoningProvider,
};
use jaymi_reasoning_ollama::{MockOllamaTransport, OllamaReasoningProvider};

fn provider_with_tags(tags_json: &str) -> Arc<OllamaReasoningProvider> {
    let transport = Arc::new(MockOllamaTransport::connected("0.9.0"));
    transport.set_tags_json(tags_json);
    Arc::new(OllamaReasoningProvider::with_transport(transport, None))
}

#[test]
fn discovery_lists_installed_ollama_models() {
    let provider = provider_with_tags(
        r#"{
          "models": [
            {
              "name": "llama3.2:latest",
              "size": 2000000000,
              "details": {
                "family": "llama",
                "parameter_size": "3.2B",
                "quantization_level": "Q4_K_M",
                "format": "gguf"
              }
            },
            {
              "name": "mistral:latest",
              "details": {
                "family": "mistral",
                "parameter_size": "7B",
                "quantization_level": "Q5_K_M"
              }
            }
          ]
        }"#,
    );
    let registry = ModelRegistry::with_provider(provider as Arc<dyn ReasoningProvider>);
    assert_eq!(registry.refresh().unwrap(), 2);
    let names: Vec<_> = registry
        .list()
        .into_iter()
        .map(|model| model.info.id.name)
        .collect();
    assert!(names.iter().any(|name| name == "llama3.2:latest"));
    assert!(names.iter().any(|name| name == "mistral:latest"));
}

#[test]
fn metadata_includes_context_params_and_quantization() {
    let provider = provider_with_tags(
        r#"{
          "models": [
            {
              "name": "llama3.2:latest",
              "details": {
                "family": "llama",
                "parameter_size": "3.2B",
                "quantization_level": "Q4_K_M"
              }
            }
          ]
        }"#,
    );
    let registry = ModelRegistry::with_provider(provider as Arc<dyn ReasoningProvider>);
    registry.refresh().unwrap();
    let model = registry.list().into_iter().next().unwrap();
    assert_eq!(model.parameter_count(), Some("3.2B"));
    assert_eq!(model.quantization(), Some("Q4_K_M"));
    assert_eq!(model.context_length(), Some(131_072));
    assert_eq!(model.info.family.as_deref(), Some("llama"));
}

#[test]
fn quantization_inferred_from_model_name_when_details_omit_it() {
    let provider = provider_with_tags(
        r#"{
          "models": [
            {
              "name": "phi3:q4_k_m",
              "details": { "family": "phi", "parameter_size": "3.8B" }
            }
          ]
        }"#,
    );
    let registry = ModelRegistry::with_provider(provider as Arc<dyn ReasoningProvider>);
    registry.refresh().unwrap();
    let model = registry.list().into_iter().next().unwrap();
    assert_eq!(model.quantization(), Some("Q4_K_M"));
    assert_eq!(model.parameter_count(), Some("3.8B"));
}

#[test]
fn health_ready_when_ollama_connected() {
    let provider = provider_with_tags(r#"{"models":[]}"#);
    let registry = ModelRegistry::with_provider(provider as Arc<dyn ReasoningProvider>);
    registry.refresh().unwrap();
    assert_eq!(
        registry.provider_health("ollama"),
        Some(ReasoningHealth::Ready)
    );
}

#[test]
fn selection_and_default_model() {
    let provider = provider_with_tags(
        r#"{
          "models": [
            { "name": "llama3.2:latest", "details": { "parameter_size": "3.2B", "quantization_level": "Q4_K_M" } },
            { "name": "mistral:latest", "details": { "parameter_size": "7B", "quantization_level": "Q5_K_M" } }
          ]
        }"#,
    );
    let registry = ModelRegistry::with_provider(provider as Arc<dyn ReasoningProvider>);
    registry.refresh().unwrap();
    assert!(registry.default_model().is_some());

    let mistral = ModelIdentifier::new("ollama", "mistral:latest");
    registry.set_default(Some(mistral.clone())).unwrap();
    let selected = registry.select_default().unwrap();
    assert_eq!(selected.info.id, mistral);
    assert!(selected.is_default);

    let by_id = registry.select(&mistral).unwrap();
    assert_eq!(by_id.info.id.name, "mistral:latest");
}

#[test]
fn unavailable_provider_blocks_selection() {
    let transport = Arc::new(MockOllamaTransport::unavailable());
    let provider = Arc::new(OllamaReasoningProvider::with_transport(transport, None));
    let registry = ModelRegistry::with_provider(provider as Arc<dyn ReasoningProvider>);
    assert_eq!(registry.refresh().unwrap(), 0);
    assert_eq!(
        registry.provider_health("ollama").map(|health| health.as_str()),
        Some("unavailable")
    );
    assert!(registry.list().is_empty());
    let err = registry
        .select(&ModelIdentifier::new("ollama", "llama3.2:latest"))
        .unwrap_err();
    assert!(matches!(err, ReasoningError::ModelNotFound { .. }));
    let err = registry.select_default().unwrap_err();
    assert!(matches!(err, ReasoningError::Unavailable { .. }));
}

#[test]
fn registry_contract_accepts_future_provider_ids() {
    // Contract smoke: metadata can describe non-Ollama backends without
    // changing RegisteredModel / ModelRegistry APIs.
    let info = ReasoningModelInfo::new(ModelIdentifier::new("openai", "gpt-4.1"), "GPT-4.1")
        .with_context_tokens(128_000)
        .with_parameter_count("unknown")
        .with_quantization("n/a");
    assert_eq!(info.id.provider, "openai");
    assert_eq!(info.context_tokens, Some(128_000));
}
