//! Capability composition — independent capabilities cooperating in one plan.
//!
//! Composition never merges capabilities. Each capability remains its own
//! plan step with its own tools, providers, and permissions. The Planner
//! orders steps; the Capability Engine does not execute them.

use jaymi_core::{JaymiError, JaymiResult};

use crate::Capability;

/// Ordered composition of independent capabilities for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityComposition {
    /// Capabilities in cooperation order (never merged).
    pub capabilities: Vec<Capability>,
    /// Optional natural-language goal that produced this composition.
    pub goal: Option<String>,
}

impl CapabilityComposition {
    /// Build a composition from an ordered capability list.
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> JaymiResult<Self> {
        let capabilities = compose_capabilities(
            &capabilities.into_iter().collect::<Vec<_>>(),
        )?;
        Ok(Self {
            capabilities,
            goal: None,
        })
    }

    /// Attach a goal without changing the capability sequence.
    pub fn with_goal(mut self, goal: impl Into<String>) -> Self {
        self.goal = Some(goal.into());
        self
    }

    /// Primary capability (first step) — used for response metadata / workspace.
    pub fn primary(&self) -> Capability {
        self.capabilities[0]
    }

    /// Number of independent capability steps.
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// True when the composition has no capabilities (should not occur after `new`).
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Capability slice in plan order.
    pub fn as_slice(&self) -> &[Capability] {
        &self.capabilities
    }

    /// Short summary for logs and responses.
    pub fn summary(&self) -> String {
        let ids: Vec<_> = self.capabilities.iter().map(Capability::id).collect();
        format!("compose [{}]", ids.join(" → "))
    }
}

/// Deduplicate while preserving order. Rejects an empty list.
///
/// Capabilities stay independent — duplicates are dropped so a capability is
/// never "merged" with itself into a single hybrid step.
pub fn compose_capabilities(capabilities: &[Capability]) -> JaymiResult<Vec<Capability>> {
    if capabilities.is_empty() {
        return Err(JaymiError::new(
            "composition requires at least one capability",
        ));
    }
    let mut ordered = Vec::with_capacity(capabilities.len());
    for capability in capabilities {
        if !ordered.contains(capability) {
            ordered.push(*capability);
        }
    }
    Ok(ordered)
}

/// Classic Research → Coding → Creation cooperation sequence.
///
/// Maps to registered abstract abilities: Search (research), Code (coding),
/// GenerateImages (creation). Each remains an independent plan step.
pub fn research_coding_creation() -> Vec<Capability> {
    vec![
        Capability::Search,
        Capability::Code,
        Capability::GenerateImages,
    ]
}

/// True when the slice is a multi-capability composition (more than one step).
pub fn is_multi_capability(capabilities: &[Capability]) -> bool {
    capabilities.len() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_dedupes_without_merging() {
        let composed = compose_capabilities(&[
            Capability::Search,
            Capability::Code,
            Capability::Search,
            Capability::GenerateImages,
        ])
        .unwrap();
        assert_eq!(
            composed,
            vec![
                Capability::Search,
                Capability::Code,
                Capability::GenerateImages
            ]
        );
    }

    #[test]
    fn research_coding_creation_pipeline_is_stable() {
        assert_eq!(
            research_coding_creation(),
            vec![
                Capability::Search,
                Capability::Code,
                Capability::GenerateImages
            ]
        );
        let composition = CapabilityComposition::new(research_coding_creation())
            .unwrap()
            .with_goal("research then build then illustrate");
        assert_eq!(composition.primary(), Capability::Search);
        assert_eq!(composition.len(), 3);
        assert!(composition.summary().contains("search → code → generate_images"));
    }

    #[test]
    fn empty_composition_is_rejected() {
        assert!(compose_capabilities(&[]).is_err());
        assert!(CapabilityComposition::new(Vec::<Capability>::new()).is_err());
    }
}
