//! Model Registry — catalog of available reasoning models.
//!
//! This is **not** a marketplace and does **not** download models. It tracks
//! what reasoning backends currently expose via [`ReasoningProvider::list_models`].
//!
//! The contract is provider-independent. Today Ollama fills it; future
//! llama.cpp / MLX / GGUF / OpenAI / Anthropic / Gemini backends register the
//! same way without changing this API.
//!
//! **Sprint B1.9**

use std::sync::{Arc, RwLock};

use crate::error::{ReasoningError, ReasoningResult};
use crate::model::{ModelIdentifier, ReasoningModelInfo};
use crate::provider::{ReasoningHealth, ReasoningProvider};

/// One model entry as seen by the registry (metadata + provider health).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegisteredModel {
    /// Provider-independent model metadata.
    pub info: ReasoningModelInfo,
    /// Logical provider id that reported this model.
    pub provider_id: String,
    /// Provider health at last refresh.
    pub provider_health: ReasoningHealth,
    /// True when this model is the registry default.
    pub is_default: bool,
    /// True when the owning provider is currently usable.
    pub available: bool,
}

impl RegisteredModel {
    /// Stable model id.
    pub fn id(&self) -> &ModelIdentifier {
        &self.info.id
    }

    /// Context window when known.
    pub fn context_length(&self) -> Option<u64> {
        self.info.context_tokens
    }

    /// Parameter count label when known.
    pub fn parameter_count(&self) -> Option<&str> {
        self.info.parameter_count.as_deref()
    }

    /// Quantization label when known.
    pub fn quantization(&self) -> Option<&str> {
        self.info.quantization.as_deref()
    }
}

/// Health snapshot for one registered reasoning provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderHealthEntry {
    /// Logical provider id.
    pub provider_id: String,
    /// Display name.
    pub display_name: String,
    /// Live health.
    pub health: ReasoningHealth,
    /// Models discovered from this provider at last refresh.
    pub model_count: usize,
}

/// Immutable diagnostics snapshot of the registry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelRegistrySnapshot {
    /// Installed / discovered models.
    pub models: Vec<RegisteredModel>,
    /// Default model id when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<ModelIdentifier>,
    /// Per-provider health.
    pub providers: Vec<ProviderHealthEntry>,
}

/// Catalog of available reasoning models across registered backends.
#[derive(Default)]
pub struct ModelRegistry {
    providers: RwLock<Vec<Arc<dyn ReasoningProvider>>>,
    models: RwLock<Vec<RegisteredModel>>,
    default_model: RwLock<Option<ModelIdentifier>>,
    provider_health: RwLock<Vec<ProviderHealthEntry>>,
}

impl std::fmt::Debug for ModelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let provider_ids: Vec<String> = self
            .providers
            .read()
            .map(|guard| {
                guard
                    .iter()
                    .map(|provider| provider.id().to_string())
                    .collect()
            })
            .unwrap_or_default();
        f.debug_struct("ModelRegistry")
            .field("providers", &provider_ids)
            .field(
                "model_count",
                &self.models.read().map(|g| g.len()).unwrap_or(0),
            )
            .field(
                "default_model",
                &self.default_model.read().ok().and_then(|g| g.clone()),
            )
            .finish()
    }
}

impl ModelRegistry {
    /// Empty registry (no providers).
    pub fn new() -> Self {
        Self::default()
    }

    /// Registry with a single provider (typical boot: Ollama).
    pub fn with_provider(provider: Arc<dyn ReasoningProvider>) -> Self {
        let mut registry = Self::new();
        registry.register_provider(provider);
        registry
    }

    /// Register an additional reasoning backend.
    pub fn register_provider(&mut self, provider: Arc<dyn ReasoningProvider>) {
        if let Ok(mut guard) = self.providers.write() {
            if guard.iter().any(|existing| existing.id() == provider.id()) {
                return;
            }
            guard.push(provider);
        }
    }

    /// Builder-style register.
    pub fn with_additional_provider(mut self, provider: Arc<dyn ReasoningProvider>) -> Self {
        self.register_provider(provider);
        self
    }

    /// Refresh discovery from every registered provider.
    ///
    /// Returns the number of models discovered. Unavailable providers contribute
    /// health entries but may contribute zero models.
    pub fn refresh(&self) -> ReasoningResult<usize> {
        let providers = self
            .providers
            .read()
            .map_err(|_| ReasoningError::Unavailable {
                reason: "model registry lock poisoned".into(),
            })?
            .clone();
        let default_id = self
            .default_model
            .read()
            .map_err(|_| ReasoningError::Unavailable {
                reason: "model registry lock poisoned".into(),
            })?
            .clone();

        let mut models = Vec::new();
        let mut health_entries = Vec::new();

        for provider in providers {
            let health = provider.health();
            let available = health.is_usable();
            // Always attempt discovery; failures become an empty list for that
            // provider (e.g. offline Ollama). Models found while health is
            // unavailable stay listed but `available` is false.
            let listed = provider.list_models().unwrap_or_default();
            health_entries.push(ProviderHealthEntry {
                provider_id: provider.id().to_string(),
                display_name: provider.display_name().to_string(),
                health: health.clone(),
                model_count: listed.len(),
            });
            for info in listed {
                let is_default = default_id
                    .as_ref()
                    .map(|id| id.provider == info.id.provider && id.name == info.id.name)
                    .unwrap_or(false);
                models.push(RegisteredModel {
                    provider_id: provider.id().to_string(),
                    provider_health: health.clone(),
                    is_default,
                    available,
                    info,
                });
            }
        }

        models.sort_by(|left, right| {
            left.info
                .id
                .display()
                .cmp(&right.info.id.display())
        });

        // If no default is set, prefer the first available model.
        let mut default_id = default_id;
        if default_id.is_none() {
            if let Some(first) = models.iter().find(|model| model.available) {
                default_id = Some(first.info.id.clone());
            }
        }
        if let Some(default) = &default_id {
            for model in &mut models {
                model.is_default =
                    model.info.id.provider == default.provider && model.info.id.name == default.name;
            }
        }

        *self
            .models
            .write()
            .map_err(|_| ReasoningError::Unavailable {
                reason: "model registry lock poisoned".into(),
            })? = models;
        *self
            .provider_health
            .write()
            .map_err(|_| ReasoningError::Unavailable {
                reason: "model registry lock poisoned".into(),
            })? = health_entries;
        *self
            .default_model
            .write()
            .map_err(|_| ReasoningError::Unavailable {
                reason: "model registry lock poisoned".into(),
            })? = default_id;

        Ok(self.len())
    }

    /// Number of discovered models after the last refresh.
    pub fn len(&self) -> usize {
        self.models.read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// True when no models are catalogued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Installed / discovered models (last refresh).
    pub fn list(&self) -> Vec<RegisteredModel> {
        self.models
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Look up a model by id.
    pub fn get(&self, id: &ModelIdentifier) -> Option<RegisteredModel> {
        self.list().into_iter().find(|model| {
            model.info.id.provider == id.provider && model.info.id.name == id.name
        })
    }

    /// Current default model id, when any.
    pub fn default_model(&self) -> Option<ModelIdentifier> {
        self.default_model
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Set or clear the default model.
    ///
    /// When `Some`, the model must already be present from a refresh (or will
    /// be validated on the next refresh). Setting an unknown id returns
    /// [`ReasoningError::ModelNotFound`] after refresh has populated the catalog.
    pub fn set_default(&self, id: Option<ModelIdentifier>) -> ReasoningResult<()> {
        if let Some(wanted) = &id {
            let models = self.list();
            if !models.is_empty()
                && !models.iter().any(|model| {
                    model.info.id.provider == wanted.provider && model.info.id.name == wanted.name
                })
            {
                return Err(ReasoningError::ModelNotFound {
                    model: wanted.display(),
                });
            }
        }
        let mut default = self
            .default_model
            .write()
            .map_err(|_| ReasoningError::Unavailable {
                reason: "model registry lock poisoned".into(),
            })?;
        *default = id.clone();
        drop(default);
        // Refresh default flags on cached entries without re-querying providers.
        if let Ok(mut models) = self.models.write() {
            for model in models.iter_mut() {
                model.is_default = id
                    .as_ref()
                    .map(|wanted| {
                        model.info.id.provider == wanted.provider
                            && model.info.id.name == wanted.name
                    })
                    .unwrap_or(false);
            }
        }
        Ok(())
    }

    /// Select a model for use — must exist and its provider must be usable.
    pub fn select(&self, id: &ModelIdentifier) -> ReasoningResult<RegisteredModel> {
        let model = self.get(id).ok_or_else(|| ReasoningError::ModelNotFound {
            model: id.display(),
        })?;
        if !model.available {
            return Err(ReasoningError::Unavailable {
                reason: format!(
                    "provider `{}` is not usable ({})",
                    model.provider_id,
                    model.provider_health.as_str()
                ),
            });
        }
        Ok(model)
    }

    /// Select the default model (or first available if unset).
    pub fn select_default(&self) -> ReasoningResult<RegisteredModel> {
        if let Some(id) = self.default_model() {
            return self.select(&id);
        }
        self.list()
            .into_iter()
            .find(|model| model.available)
            .ok_or(ReasoningError::Unavailable {
                reason: "no available reasoning model".into(),
            })
    }

    /// Health of a registered provider.
    pub fn provider_health(&self, provider_id: &str) -> Option<ReasoningHealth> {
        self.provider_health
            .read()
            .ok()?
            .iter()
            .find(|entry| entry.provider_id == provider_id)
            .map(|entry| entry.health.clone())
    }

    /// All provider health entries from the last refresh.
    pub fn providers(&self) -> Vec<ProviderHealthEntry> {
        self.provider_health
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Diagnostics snapshot.
    pub fn snapshot(&self) -> ModelRegistrySnapshot {
        ModelRegistrySnapshot {
            models: self.list(),
            default_model: self.default_model(),
            providers: self.providers(),
        }
    }

    /// Compact single-line summary for Developer Diagnostics.
    pub fn summary_line(&self) -> String {
        let snapshot = self.snapshot();
        let health = snapshot
            .providers
            .first()
            .map(|entry| entry.health.as_str())
            .unwrap_or("none");
        let default = snapshot
            .default_model
            .as_ref()
            .map(ModelIdentifier::display)
            .unwrap_or_else(|| "-".into());
        let default_meta = snapshot
            .models
            .iter()
            .find(|model| model.is_default)
            .map(|model| {
                format!(
                    "context={} · params={} · quant={}",
                    model
                        .context_length()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".into()),
                    model.parameter_count().unwrap_or("-"),
                    model.quantization().unwrap_or("-"),
                )
            })
            .unwrap_or_else(|| "context=- · params=- · quant=-".into());
        format!(
            "providers={} · health={} · models={} · default={} · {default_meta}",
            snapshot.providers.len(),
            health,
            snapshot.models.len(),
            default,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelIdentifier;
    use crate::provider::{ReasoningCapabilities, ReasoningHealth, ReasoningStream};
    use crate::request::ReasoningRequest;
    use crate::response::ReasoningResponse;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    struct MockProvider {
        id: String,
        health: Mutex<ReasoningHealth>,
        models: Mutex<Vec<ReasoningModelInfo>>,
        list_calls: AtomicU32,
    }

    impl MockProvider {
        fn new(id: &str, models: Vec<ReasoningModelInfo>) -> Self {
            Self {
                id: id.into(),
                health: Mutex::new(ReasoningHealth::Ready),
                models: Mutex::new(models),
                list_calls: AtomicU32::new(0),
            }
        }

        fn set_health(&self, health: ReasoningHealth) {
            *self.health.lock().expect("lock") = health;
        }
    }

    impl ReasoningProvider for MockProvider {
        fn id(&self) -> &str {
            &self.id
        }

        fn display_name(&self) -> &str {
            &self.id
        }

        fn capabilities(&self) -> ReasoningCapabilities {
            ReasoningCapabilities::full()
        }

        fn health(&self) -> ReasoningHealth {
            self.health.lock().expect("lock").clone()
        }

        fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            // Catalog can still surface models when the provider is down; the
            // registry marks them unavailable via health.
            Ok(self.models.lock().expect("lock").clone())
        }

        fn complete(&self, request: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
            Ok(ReasoningResponse::completed(request.goal))
        }

        fn stream(
            &self,
            _request: ReasoningRequest,
        ) -> ReasoningResult<Box<dyn ReasoningStream>> {
            Err(ReasoningError::Unavailable {
                reason: "not used".into(),
            })
        }
    }

    fn sample_model(provider: &str, name: &str) -> ReasoningModelInfo {
        ReasoningModelInfo::new(ModelIdentifier::new(provider, name), name)
            .with_context_tokens(8_192)
            .with_parameter_count("7B")
            .with_quantization("Q4_K_M")
    }

    #[test]
    fn discovery_lists_installed_models() {
        let provider = Arc::new(MockProvider::new(
            "ollama",
            vec![
                sample_model("ollama", "llama3.2"),
                sample_model("ollama", "mistral"),
            ],
        ));
        let registry = ModelRegistry::with_provider(provider);
        assert_eq!(registry.refresh().unwrap(), 2);
        let models = registry.list();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].info.id.name, "llama3.2");
        assert!(models.iter().any(|m| m.info.id.name == "mistral"));
    }

    #[test]
    fn metadata_exposes_context_params_quant() {
        let provider = Arc::new(MockProvider::new(
            "ollama",
            vec![sample_model("ollama", "llama3.2")],
        ));
        let registry = ModelRegistry::with_provider(provider);
        registry.refresh().unwrap();
        let model = registry.list().into_iter().next().unwrap();
        assert_eq!(model.context_length(), Some(8_192));
        assert_eq!(model.parameter_count(), Some("7B"));
        assert_eq!(model.quantization(), Some("Q4_K_M"));
    }

    #[test]
    fn health_and_unavailable_provider() {
        let provider = Arc::new(MockProvider::new(
            "ollama",
            vec![sample_model("ollama", "llama3.2")],
        ));
        let registry = ModelRegistry::with_provider(provider.clone());
        registry.refresh().unwrap();
        assert_eq!(
            registry.provider_health("ollama"),
            Some(ReasoningHealth::Ready)
        );

        provider.set_health(ReasoningHealth::Unavailable {
            reason: "offline".into(),
        });
        assert_eq!(registry.refresh().unwrap(), 1);
        assert_eq!(
            registry.provider_health("ollama").map(|h| h.as_str()),
            Some("unavailable")
        );
        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].available);
        let err = registry
            .select(&ModelIdentifier::new("ollama", "llama3.2"))
            .unwrap_err();
        assert!(matches!(err, ReasoningError::Unavailable { .. }));
    }

    #[test]
    fn selection_and_default_model() {
        let provider = Arc::new(MockProvider::new(
            "ollama",
            vec![
                sample_model("ollama", "llama3.2"),
                sample_model("ollama", "mistral"),
            ],
        ));
        let registry = ModelRegistry::with_provider(provider);
        registry.refresh().unwrap();
        // Auto-default to first available after refresh.
        assert_eq!(
            registry.default_model().map(|id| id.name),
            Some("llama3.2".into())
        );
        registry
            .set_default(Some(ModelIdentifier::new("ollama", "mistral")))
            .unwrap();
        assert_eq!(
            registry.default_model().map(|id| id.name),
            Some("mistral".into())
        );
        let selected = registry.select_default().unwrap();
        assert_eq!(selected.info.id.name, "mistral");
        assert!(selected.is_default);

        let err = registry
            .set_default(Some(ModelIdentifier::new("ollama", "missing")))
            .unwrap_err();
        assert!(matches!(err, ReasoningError::ModelNotFound { .. }));
    }
}
