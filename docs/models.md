# Model Registry

**Status: Partial** — Sprint B1.9 catalog + **B1.13.6** Reasoning loop + **Settings**
persisted default (Preferences → Config → Application → Registry / Planner).

Jaymi’s Model Registry tracks **available reasoning models**. It is **not** a
marketplace and does **not** download, install, or pull weights.

```text
Settings Workspace (intents only)
        ↓
Application facade
        ↓
Configuration (preferred provider / model)
        ↓
ModelRegistry (default / preferred) ← ReasoningProvider::list_models / health
        ↓
Planner::prepare_reasoning_model
        ↓
ReasoningRequest.model
        ↓
ReasoningProvider (respects selection)
```

Settings never talks to the registry or providers directly — see
[settings.md](settings.md). Application holds a session-scoped cache of the
registry snapshot (installed models + provider health); Refresh Models and
Test Connection invalidate it — see [session-cache.md](session-cache.md).

## Contract

Lives in `jaymi-reasoning` (`ModelRegistry`, `RegisteredModel`).

| Surface | Purpose |
|---------|---------|
| `refresh` | Re-discover models from every registered `ReasoningProvider` |
| `list` / `get` | Installed / discovered catalog |
| `default_model` / `set_default` | Preferred model selection |
| `select` / `select_default` | Resolve a usable model (fails if provider unhealthy) |
| `provider_health` / `snapshot` | Health + diagnostics |

Planner resolves via `prepare_reasoning_model` (preferred → default → fallback):

* **Missing / unavailable** preferred or default → first available model
* **No models** → soft-fail (request may omit model; diagnostics show `-`)

`Application::set_default_reasoning_model` persists into `config.json`
(`Settings.reasoning`) and updates registry default + Planner preferred.
`Application::set_preferred_model` remains the in-session Planner override API.

`ReasoningModelInfo` carries vendor-neutral `ModelCapabilityFlags`
(completion / thinking / tools / vision / embeddings) populated by providers
during discovery (Ollama uses `/api/tags` + `/api/show`).

## Diagnostics (B1.13.6)

| Field | Meaning |
|-------|---------|
| Configured Model | Registry default or preferred |
| Actual Model | Provider-reported model on last-turn metrics |
| Provider Model | Id attached onto `ReasoningRequest` |
| Loaded Model | Backend-loaded model (e.g. Ollama `/api/ps`) |

## Ollama (first backend)

Boot registers `OllamaReasoningProvider` into `ModelRegistry` and refreshes
once. Tags from `/api/tags` (enriched by `/api/show` when available) fill:

* installed model names
* `parameter_size` → parameter count
* `quantization_level` (or inferred from the tag name)
* context length (show `model_info` or heuristic — see [ollama.md](ollama.md))
* capability flags

When Planner sets `ReasoningRequest.model`, Ollama uses that name and rejects
unknown models with `ModelNotFound`.

## Not in this slice

* Model download / pull / install UI
* Marketplace or ranking
* Additional backends (llama.cpp, MLX, cloud) — registry + Settings UI seams only
* Changing tool `ProviderRegistry` (that catalog stays separate)
