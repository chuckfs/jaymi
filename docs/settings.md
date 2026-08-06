# Settings Workspace

**Status: Partial** — Settings shell + Reasoning page Current; other categories Coming Soon.

Settings is a **product workspace** for preferences only. It is not a system owner.

```text
User
  ↓
Settings Workspace (preferences UI)
  ↓
Configuration (persist)
  ↓
Application / Planner (coordinate)
  ↓
Reasoning Engine → Model Registry → ReasoningProvider → Ollama
```

## Ownership

| Layer | Owns | Must not |
|-------|------|----------|
| Settings Workspace | Category navigation; paint snapshots; emit intents | Discovery, registry, provider I/O |
| Configuration | Persisted preferences (`reasoning`, theme, …) | Live model catalog |
| Application | Snapshots + intents → Config / Planner / Registry | Bypass Planner for generation |
| Model Registry | Installed models, default, health | UI concerns |
| ReasoningProvider | Transport, list_models, health | Prompt assembly |

## Navigation

`NavTab::Settings` opens the Settings surface (replaces conversation chrome while open). Command `jaymi.workbench.openSettings` and Coding “Open Settings” both enter this workspace.

Categories: General, Appearance, **Reasoning**, Privacy, Projects, Coding, Providers, Diagnostics, About.

Only Reasoning is fully wired; others show Coming Soon.

## Reasoning preferences

Persisted in `config.json` as:

```json
"reasoning": {
  "preferred_provider_id": "ollama",
  "preferred_model": "llama3.2:latest"
}
```

Boot restores the preference into `ModelRegistry::set_default` and `Planner::set_preferred_model` when the model still exists.

Application APIs (Settings may call only these):

* `reasoning_settings_snapshot`
* `refresh_reasoning_models`
* `set_default_reasoning_model`
* `test_reasoning_connection`
* `theme_preference` / `settings_snapshot` (session-cached; see [session-cache.md](session-cache.md))

The UI groups models by `provider_id` so future backends (llama.cpp, MLX, OpenAI, Anthropic, Gemini) can appear without redesign. Those providers are **not** implemented in this slice.

## Related

* [models.md](models.md) — Model Registry contract
* [reasoning.md](reasoning.md) — conversational pipeline
* [experience.md](experience.md) — shell destinations
