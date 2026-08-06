# Session Cache

**Status: Current Implementation** — Application-owned, session-scoped.

Jaymi caches inexpensive **immutable** snapshots for the life of an Application
session so Settings, Theme, Developer Diagnostics, and Model Registry reads do
not re-discover or re-lock every paint.

```text
Application (sole owner)
    │
    ├─ SessionCache
    │     ├─ Model Registry snapshot  (installed models + provider health + default)
    │     ├─ Capability availability  (CapabilityDiscoveryReport)
    │     └─ Settings                 (includes theme preference)
    │
    └─ Experience / conversation      ← never cached here
```

## What is cached

| Slot | Contents | Source of truth (on miss / invalidate) |
|------|----------|----------------------------------------|
| Model Registry | `ModelRegistrySnapshot` — installed models, default, provider health | `ModelRegistry::snapshot()` after `refresh` when rediscovering |
| Capability availability | `CapabilityDiscoveryReport` | `Planner::discover_capability_status` |
| Settings | `jaymi_config::Settings` | `Config::settings()` |
| Theme | preference inside Settings | same settings slot |

## What must not be cached

* Conversation turns / transcript
* Active generation / stream state
* Experience workspace / Coding editor buffers
* ContextBundle / Context Inspector (request-scoped)
* Planner responses, permission decisions, approval history

Those remain live Application / Planner / Context Engine state.

## Ownership

| Layer | Owns | Must not |
|-------|------|----------|
| **Application** | `SessionCache` lifecycle, seed at boot, invalidate + re-warm | Cache conversation or ContextBundle |
| **Model Registry** | Live catalog + `refresh` / `snapshot` | UI paint loops; session cache slots |
| **Configuration** | Persisted settings on disk | Session cache invalidation (Application does) |
| **Capability Engine / Planner** | Discovery against live inventory | Session cache |
| **Settings / Theme UI** | Paint snapshots from Application APIs | Call `ModelRegistry::refresh` or lock Config every frame |

## Invalidation

| Event | API | Slots cleared |
|-------|-----|---------------|
| Refresh Models / Test Connection | `invalidate_session_cache_models` then refresh + store | Model Registry (models + health) |
| Settings persist / preference change | `notify_settings_changed` | Settings (+ theme) |
| Provider registration change | `notify_providers_changed` / `register_reasoning_provider` | Model Registry + capability availability |

Generation counter (`session_cache_generation`) bumps on every invalidation for
diagnostics and tests.

## Read path

* `theme_preference` / `settings_snapshot` — Settings UI + shell theme sync
* `reasoning_settings_snapshot` — uses cached Model Registry snapshot
* `reasoning_diagnostics` — uses cached registry; **does not** refresh providers on paint
* `diagnostics` — uses cached settings + capability availability; surfaces a
  **Session Cache** subsystem row

Rediscovery remains intentional: Refresh Models and Test Connection.

## Related

* [models.md](models.md) — Model Registry contract
* [settings.md](settings.md) — Settings Workspace ownership
* [capabilities.md](capabilities.md) — capability availability
* [experience.md](experience.md) — conversation / workspace (not cached here)
