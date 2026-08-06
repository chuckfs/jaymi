//! Resolve a reasoning model from the Model Registry for Planner requests.
//!
//! Sprint **B1.13.6** — closes the loop between Model Registry and Reasoning:
//! Planner populates [`ReasoningRequest::model`] from the registry (preferred /
//! default / fallback). Providers must respect that selection.

use jaymi_reasoning::{
    ModelIdentifier, ModelRegistry, ReasoningError, ReasoningResult, RegisteredModel,
};

/// How the Planner chose the model for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelectionKind {
    /// Explicit preferred model (Planner / host selection).
    Explicit,
    /// Registry default model (or first available when unset).
    Default,
    /// Preferred or default was missing / unavailable — used another available model.
    Fallback {
        /// Model that could not be used.
        unavailable: ModelIdentifier,
        /// Why it was skipped.
        reason: String,
    },
}

/// Result of resolving a model for a Reasoning request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    /// Selected catalog entry.
    pub model: RegisteredModel,
    /// How it was chosen.
    pub kind: ModelSelectionKind,
}

impl ModelSelection {
    /// Identifier to attach onto [`jaymi_reasoning::ReasoningRequest`].
    pub fn id(&self) -> &ModelIdentifier {
        self.model.id()
    }

    /// True when a fallback was required.
    pub fn used_fallback(&self) -> bool {
        matches!(self.kind, ModelSelectionKind::Fallback { .. })
    }
}

/// Resolve the model the Planner should put on `ReasoningRequest.model`.
///
/// 1. Prefer `preferred` when set (explicit selection)
/// 2. Else registry default
/// 3. On missing / unavailable → first available model (fallback)
/// 4. If nothing was configured, first available is treated as Default
/// 5. If nothing available → [`ReasoningError::Unavailable`]
pub fn prepare_reasoning_model(
    registry: &ModelRegistry,
    preferred: Option<&ModelIdentifier>,
) -> ReasoningResult<ModelSelection> {
    let attempted = preferred.cloned().or_else(|| registry.default_model());
    let explicit = preferred.is_some();

    if let Some(id) = attempted {
        match registry.select(&id) {
            Ok(model) => {
                let kind = if explicit {
                    ModelSelectionKind::Explicit
                } else {
                    ModelSelectionKind::Default
                };
                return Ok(ModelSelection { model, kind });
            }
            Err(ReasoningError::ModelNotFound { model }) => {
                return first_available_fallback(
                    registry,
                    Some(id),
                    format!("model not found: {model}"),
                );
            }
            Err(ReasoningError::Unavailable { reason }) => {
                return first_available_fallback(registry, Some(id), reason);
            }
            Err(other) => return Err(other),
        }
    }

    first_available_fallback(registry, None, "no default model configured".into())
}

fn first_available_fallback(
    registry: &ModelRegistry,
    unavailable: Option<ModelIdentifier>,
    reason: String,
) -> ReasoningResult<ModelSelection> {
    let model = registry
        .list()
        .into_iter()
        .find(|entry| entry.available)
        .ok_or_else(|| {
            if registry.is_empty() {
                ReasoningError::Unavailable {
                    reason: "no reasoning models discovered in the registry".into(),
                }
            } else if let Some(id) = &unavailable {
                ReasoningError::Unavailable {
                    reason: format!(
                        "no available reasoning model (failed {}: {reason})",
                        id.display()
                    ),
                }
            } else {
                ReasoningError::Unavailable {
                    reason: "no available reasoning model".into(),
                }
            }
        })?;

    let kind = match unavailable {
        Some(unavailable)
            if unavailable.provider != model.info.id.provider
                || unavailable.name != model.info.id.name =>
        {
            ModelSelectionKind::Fallback {
                unavailable,
                reason,
            }
        }
        Some(unavailable) => {
            return Err(ReasoningError::Unavailable {
                reason: format!("{} ({reason})", unavailable.display()),
            });
        }
        None => ModelSelectionKind::Default,
    };

    Ok(ModelSelection { model, kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_reasoning::{
        ReasoningCapabilities, ReasoningHealth, ReasoningModelInfo, ReasoningProvider,
        ReasoningRequest, ReasoningResponse, ReasoningStream,
    };
    use std::sync::Arc;

    struct FakeProvider {
        id: String,
        models: Vec<&'static str>,
        usable: bool,
    }

    impl ReasoningProvider for FakeProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn capabilities(&self) -> ReasoningCapabilities {
            ReasoningCapabilities {
                list_models: true,
                health: true,
                ..ReasoningCapabilities::default()
            }
        }
        fn health(&self) -> ReasoningHealth {
            if self.usable {
                ReasoningHealth::Ready
            } else {
                ReasoningHealth::Unavailable {
                    reason: "offline".into(),
                }
            }
        }
        fn list_models(&self) -> ReasoningResult<Vec<ReasoningModelInfo>> {
            Ok(self
                .models
                .iter()
                .map(|name| {
                    ReasoningModelInfo::new(ModelIdentifier::new(&self.id, *name), *name)
                        .with_context_tokens(8_192)
                })
                .collect())
        }
        fn complete(&self, _: ReasoningRequest) -> ReasoningResult<ReasoningResponse> {
            Err(ReasoningError::NotImplemented)
        }
        fn stream(&self, _: ReasoningRequest) -> ReasoningResult<Box<dyn ReasoningStream>> {
            Err(ReasoningError::NotImplemented)
        }
    }

    fn registry(models: &[&'static str], usable: bool) -> ModelRegistry {
        let provider = Arc::new(FakeProvider {
            id: "ollama".into(),
            models: models.to_vec(),
            usable,
        });
        let registry = ModelRegistry::with_provider(provider);
        registry.refresh().unwrap();
        registry
    }

    #[test]
    fn default_model_is_selected() {
        let registry = registry(&["llama", "mistral"], true);
        registry
            .set_default(Some(ModelIdentifier::new("ollama", "mistral")))
            .unwrap();
        let selection = prepare_reasoning_model(&registry, None).unwrap();
        assert_eq!(selection.id().name, "mistral");
        assert_eq!(selection.kind, ModelSelectionKind::Default);
    }

    #[test]
    fn explicit_model_is_selected() {
        let registry = registry(&["llama", "mistral"], true);
        let preferred = ModelIdentifier::new("ollama", "mistral");
        let selection = prepare_reasoning_model(&registry, Some(&preferred)).unwrap();
        assert_eq!(selection.id().name, "mistral");
        assert_eq!(selection.kind, ModelSelectionKind::Explicit);
    }

    #[test]
    fn unavailable_preferred_falls_back() {
        let provider_a = Arc::new(FakeProvider {
            id: "ollama".into(),
            models: vec!["down"],
            usable: false,
        });
        let provider_b = Arc::new(FakeProvider {
            id: "other".into(),
            models: vec!["up"],
            usable: true,
        });
        let mut registry = ModelRegistry::with_provider(provider_a);
        registry.register_provider(provider_b);
        registry.refresh().unwrap();
        registry
            .set_default(Some(ModelIdentifier::new("ollama", "down")))
            .unwrap();
        let selection = prepare_reasoning_model(&registry, None).unwrap();
        assert_eq!(selection.id().display(), "other/up");
        assert!(selection.used_fallback());
    }

    #[test]
    fn missing_model_falls_back() {
        let registry = registry(&["llama"], true);
        let preferred = ModelIdentifier::new("ollama", "missing");
        let selection = prepare_reasoning_model(&registry, Some(&preferred)).unwrap();
        assert_eq!(selection.id().name, "llama");
        assert!(selection.used_fallback());
    }

    #[test]
    fn no_models_errors() {
        let registry = registry(&[], true);
        let err = prepare_reasoning_model(&registry, None).unwrap_err();
        assert!(matches!(err, ReasoningError::Unavailable { .. }));
    }
}
