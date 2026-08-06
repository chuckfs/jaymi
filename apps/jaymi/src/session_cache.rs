//! Session-scoped cache for inexpensive immutable snapshots.
//!
//! Owned exclusively by [`crate::Application`]. Holds read-mostly data that is
//! cheap to clone and expensive to rebuild every frame (registry snapshots,
//! capability availability, settings / theme preference).
//!
//! **Must not** cache mutable conversation state (turns, active generation,
//! experience workspace, ContextBundle, Planner responses).
//!
//! Invalidation is Application-driven:
//! - Refresh Models / connection test → [`SessionCache::invalidate_models`]
//! - Settings persist / preference changes → [`SessionCache::invalidate_settings`]
//! - Provider registration changes → [`SessionCache::invalidate_providers`]

use jaymi_capabilities::CapabilityDiscoveryReport;
use jaymi_config::{Settings, Theme};
use jaymi_reasoning::ModelRegistrySnapshot;

/// Application-owned session cache slots.
#[derive(Debug, Clone, Default)]
pub struct SessionCache {
    /// Model Registry snapshot: installed models, default, provider health.
    model_registry: Option<ModelRegistrySnapshot>,
    /// Capability availability (executable vs not), from live inventory.
    capability_availability: Option<CapabilityDiscoveryReport>,
    /// Persisted settings snapshot (includes theme preference).
    settings: Option<Settings>,
    /// Bumped on every invalidation (diagnostics / tests).
    generation: u64,
}

impl SessionCache {
    /// Empty cache (all slots miss).
    pub fn new() -> Self {
        Self::default()
    }

    /// Invalidation generation (monotonic).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn bump(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }

    /// Drop Model Registry / installed-models / provider-health slot.
    pub fn invalidate_models(&mut self) {
        self.model_registry = None;
        self.bump();
    }

    /// Drop settings + theme preference slot.
    pub fn invalidate_settings(&mut self) {
        self.settings = None;
        self.bump();
    }

    /// Provider registration may change the model catalog and capability availability.
    pub fn invalidate_providers(&mut self) {
        self.model_registry = None;
        self.capability_availability = None;
        self.bump();
    }

    /// Clear every slot.
    pub fn invalidate_all(&mut self) {
        self.model_registry = None;
        self.capability_availability = None;
        self.settings = None;
        self.bump();
    }

    /// Cached Model Registry snapshot, when warm.
    pub fn model_registry(&self) -> Option<&ModelRegistrySnapshot> {
        self.model_registry.as_ref()
    }

    /// Store a Model Registry snapshot (installed models + provider health).
    pub fn set_model_registry(&mut self, snapshot: ModelRegistrySnapshot) {
        self.model_registry = Some(snapshot);
    }

    /// Cached capability availability report, when warm.
    pub fn capability_availability(&self) -> Option<&CapabilityDiscoveryReport> {
        self.capability_availability.as_ref()
    }

    /// Store capability availability.
    pub fn set_capability_availability(&mut self, report: CapabilityDiscoveryReport) {
        self.capability_availability = Some(report);
    }

    /// Cached settings snapshot, when warm.
    pub fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    /// Store settings (theme preference included).
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = Some(settings);
    }

    /// Theme preference from the cached settings slot.
    pub fn theme(&self) -> Option<Theme> {
        self.settings.as_ref().map(|settings| settings.theme)
    }

    /// True when every candidate slot is populated.
    pub fn is_warm(&self) -> bool {
        self.model_registry.is_some()
            && self.capability_availability.is_some()
            && self.settings.is_some()
    }

    /// Diagnostic summary line.
    pub fn summary(&self) -> String {
        format!(
            "generation={} · models={} · capabilities={} · settings={} · theme={}",
            self.generation,
            if self.model_registry.is_some() {
                "hit"
            } else {
                "miss"
            },
            if self.capability_availability.is_some() {
                "hit"
            } else {
                "miss"
            },
            if self.settings.is_some() {
                "hit"
            } else {
                "miss"
            },
            self.theme()
                .map(|theme| theme.as_str())
                .unwrap_or("—"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_config::Theme;

    #[test]
    fn invalidate_models_clears_only_registry_slot() {
        let mut cache = SessionCache::new();
        cache.set_settings(Settings::default());
        cache.set_model_registry(ModelRegistrySnapshot {
            models: Vec::new(),
            default_model: None,
            providers: Vec::new(),
        });
        cache.set_capability_availability(CapabilityDiscoveryReport::default());
        let gen = cache.generation();
        cache.invalidate_models();
        assert!(cache.model_registry().is_none());
        assert!(cache.settings().is_some());
        assert!(cache.capability_availability().is_some());
        assert!(cache.generation() > gen);
    }

    #[test]
    fn invalidate_settings_clears_theme() {
        let mut cache = SessionCache::new();
        let mut settings = Settings::default();
        settings.theme = Theme::Dark;
        cache.set_settings(settings);
        assert_eq!(cache.theme(), Some(Theme::Dark));
        cache.invalidate_settings();
        assert!(cache.theme().is_none());
    }

    #[test]
    fn invalidate_providers_clears_models_and_capabilities() {
        let mut cache = SessionCache::new();
        cache.set_model_registry(ModelRegistrySnapshot {
            models: Vec::new(),
            default_model: None,
            providers: Vec::new(),
        });
        cache.set_capability_availability(CapabilityDiscoveryReport::default());
        cache.set_settings(Settings::default());
        cache.invalidate_providers();
        assert!(cache.model_registry().is_none());
        assert!(cache.capability_availability().is_none());
        assert!(cache.settings().is_some());
    }
}
