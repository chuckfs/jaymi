# Model Registry

**Status: Partial** — Sprint B1.9 catalog + **B1.13.6** Reasoning loop.

Jaymi’s Model Registry tracks **available reasoning models**. It is **not** a
marketplace and does **not** download, install, or pull weights.

```text
ReasoningProvider::list_models / health
        ↓
   ModelRegistry (default / preferred)
        ↓
 Planner::prepare_reasoning_model
        ↓
 ReasoningRequest.model
        ↓
 ReasoningProvider (respects selection)
```

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

`Planner::set_preferred_model` / `Application::set_preferred_model` set an
explicit override for the next conversational turns.

## Diagnostics (B1.13.6)

| Field | Meaning |
|-------|---------|
| Configured Model | Registry default or preferred |
| Actual Model | Provider-reported model on last-turn metrics |
| Provider Model | Id attached onto `ReasoningRequest` |
| Loaded Model | Backend-loaded model (e.g. Ollama `/api/ps`) |

## Ollama (first backend)

Boot registers `OllamaReasoningProvider` into `ModelRegistry` and refreshes
once. Tags from `/api/tags` fill:

* installed model names
* `parameter_size` → parameter count
* `quantization_level` (or inferred from the tag name)
* heuristic context length (see [ollama.md](ollama.md) / B1.8)

When Planner sets `ReasoningRequest.model`, Ollama uses that name and rejects
unknown models with `ModelNotFound`.

## Not in this slice

* Model download / pull / install UI
* Marketplace or ranking
* Multi-backend routing beyond the shared registry contract
* Changing tool `ProviderRegistry` (that catalog stays separate)
