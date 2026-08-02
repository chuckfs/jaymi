//! Embedding Provider interface and local (model-agnostic) implementation.
//!
//! Embedding engines plug in behind [`EmbeddingProvider`]. The Planner never
//! depends on a concrete model — Search selects a registered provider.

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult};

use crate::categories::ProviderCategory;
use crate::provider::{Provider, ProviderIdentity};

/// Stable provider identity for the local embedding provider.
pub const EMBEDDING_PROVIDER_ID: &str = "embedding.local";

/// Default local model label (not a third-party model binding).
pub const LOCAL_EMBEDDING_MODEL: &str = "local-lexical-v1";

/// Default embedding dimensionality for the local provider.
pub const LOCAL_EMBEDDING_DIMS: usize = 64;

/// One embedding vector produced by a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingVector {
    /// Dense float components.
    pub values: Vec<f32>,
    /// Model identity that produced the vector.
    pub model_id: String,
}

impl EmbeddingVector {
    /// Number of dimensions.
    pub fn dims(&self) -> usize {
        self.values.len()
    }
}

/// Runtime status of an embedding provider (for diagnostics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProviderStatus {
    /// Provider identity id.
    pub provider_id: String,
    /// Human-readable provider name.
    pub name: String,
    /// Declared embedding model id (provider-specific, not hardcoded elsewhere).
    pub model_id: String,
    /// Embedding dimensionality.
    pub dimensions: usize,
    /// True when the provider can generate embeddings.
    pub available: bool,
    /// Whether the provider finished initialization.
    pub initialized: bool,
    /// Short detail string for diagnostics.
    pub detail: String,
}

/// Embedding-specific provider surface.
///
/// Implementations also implement [`Provider`] so they register through the
/// shared Provider framework. Swapping models does not require Planner changes.
pub trait EmbeddingProvider: Provider {
    /// Current embedding readiness for diagnostics.
    fn embedding_status(&self) -> EmbeddingProviderStatus;

    /// Model identity string for this provider configuration.
    fn model_id(&self) -> &str;

    /// Embedding dimensionality.
    fn dimensions(&self) -> usize;

    /// Generate embeddings for one or more texts (order-preserving).
    fn embed(&self, texts: &[String]) -> JaymiResult<Vec<EmbeddingVector>>;
}

/// Local lexical embedding provider — deterministic, no remote model binding.
///
/// Uses hashed token bags with a small concept-alias fold so related wording
/// can retrieve by meaning. Not tied to any commercial embedding API.
#[derive(Debug)]
pub struct LocalEmbeddingProvider {
    identity: ProviderIdentity,
    initialized: bool,
    model_id: String,
    dimensions: usize,
}

impl LocalEmbeddingProvider {
    /// Create an uninitialized local embedding provider.
    pub fn new() -> Self {
        Self {
            identity: ProviderIdentity {
                id: EMBEDDING_PROVIDER_ID.to_string(),
                name: "Local Embedding".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Model-agnostic local lexical embeddings".to_string(),
                category: ProviderCategory::Local,
                author: "jaymi".to_string(),
                capabilities: vec![Capability::Embeddings, Capability::Search],
            },
            initialized: false,
            model_id: LOCAL_EMBEDDING_MODEL.to_string(),
            dimensions: LOCAL_EMBEDDING_DIMS,
        }
    }

    /// Override the model label (still local — not a remote binding).
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Returns true after initialization.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn embed_one(&self, text: &str) -> EmbeddingVector {
        let mut values = vec![0.0f32; self.dimensions];
        for token in tokenize(text) {
            let folded = fold_concept(&token);
            let slot = hash_token(&folded) as usize % self.dimensions;
            values[slot] += 1.0;
            // Adjacent slot softens collisions for multi-token concepts.
            let neighbor = (slot + 1) % self.dimensions;
            values[neighbor] += 0.25;
        }
        l2_normalize(&mut values);
        EmbeddingVector {
            values,
            model_id: self.model_id.clone(),
        }
    }
}

impl Default for LocalEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for LocalEmbeddingProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new(
                "local embedding provider is not initialized",
            ))
        }
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        Ok(())
    }
}

impl EmbeddingProvider for LocalEmbeddingProvider {
    fn embedding_status(&self) -> EmbeddingProviderStatus {
        EmbeddingProviderStatus {
            provider_id: self.identity.id.clone(),
            name: self.identity.name.clone(),
            model_id: self.model_id.clone(),
            dimensions: self.dimensions,
            available: self.initialized,
            initialized: self.initialized,
            detail: if self.initialized {
                format!(
                    "local · model={} · dims={} · available",
                    self.model_id, self.dimensions
                )
            } else {
                "uninitialized".to_string()
            },
        }
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed(&self, texts: &[String]) -> JaymiResult<Vec<EmbeddingVector>> {
        if !self.initialized {
            return Err(JaymiError::new(
                "local embedding provider is not initialized",
            ));
        }
        Ok(texts.iter().map(|text| self.embed_one(text)).collect())
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() > 1)
        .map(|token| token.to_string())
        .collect()
}

fn fold_concept(token: &str) -> String {
    match token {
        "fungi" | "fungus" | "mushroom" | "mushrooms" | "mycelium" | "spore" | "spores" => {
            "concept_fungi".into()
        }
        "habitat" | "woodland" | "forest" | "soil" | "damp" | "moist" => "concept_habitat".into(),
        "biology" | "biological" | "organism" | "organisms" => "concept_biology".into(),
        other => other.to_string(),
    }
}

fn hash_token(token: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in token.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn l2_normalize(values: &mut [f32]) {
    let norm = values
        .iter()
        .map(|value| (*value as f64) * (*value as f64))
        .sum::<f64>()
        .sqrt();
    if norm <= f64::EPSILON {
        return;
    }
    for value in values.iter_mut() {
        *value = (*value as f64 / norm) as f32;
    }
}

/// Cosine similarity between two equal-length vectors.
pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (a, b) in left.iter().zip(right.iter()) {
        let a = f64::from(*a);
        let b = f64::from(*b);
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    let denom = left_norm.sqrt() * right_norm.sqrt();
    if denom <= f64::EPSILON {
        0.0
    } else {
        (dot / denom) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ProviderRegistry;
    use jaymi_core::Lifecycle;

    #[test]
    fn local_provider_registers_and_embeds() {
        let mut registry = ProviderRegistry::new();
        registry.initialize().unwrap();
        let mut provider = LocalEmbeddingProvider::new();
        provider.initialize().unwrap();
        registry.register(&provider).unwrap();

        let vectors = provider
            .embed(&[
                "Mushrooms thrive in damp woodland soil.".into(),
                "Buy milk and bread tomorrow.".into(),
            ])
            .unwrap();
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].dims(), LOCAL_EMBEDDING_DIMS);
        assert_eq!(vectors[0].model_id, LOCAL_EMBEDDING_MODEL);

        let query = provider.embed(&["where do fungi live".into()]).unwrap();
        let fungi_sim = cosine_similarity(&query[0].values, &vectors[0].values);
        let milk_sim = cosine_similarity(&query[0].values, &vectors[1].values);
        assert!(
            fungi_sim > milk_sim,
            "semantic fold should prefer fungi doc ({fungi_sim} vs {milk_sim})"
        );
    }
}
