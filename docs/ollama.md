# Ollama Reasoning Provider

**Status: Partial** — Sprint B1.3 first backend; conversational Planner path
(B1.5–B1.6); Model Registry (B1.9); **B1.13.1** consumes PromptBuilder output
only (no parallel prompt construction).

`OllamaReasoningProvider` is Jaymi's first concrete [`ReasoningProvider`].

It speaks Ollama's local HTTP API and satisfies the Reasoning contract:

* health check (`/api/version`, tags, loaded models)
* list models (`/api/tags`) with metadata (family, parameter size, quantization, context heuristics)
* complete chat (`/api/chat`, `stream: false`) from assembled `Prompt`
* stream chat (`/api/chat`, NDJSON) from assembled `Prompt`
* cancel generation (cooperative token + drop stream)
* model metadata

## Prompt delivery (B1.13.1)

```text
PromptBuilder → ReasoningRequest.prompt → messages_from_prompt → /api/chat
```

Ollama maps `Prompt::to_chat_messages()` onto wire `ChatMessage`s. It does **not**
rebuild prompts from `goal`, `history`, or `LlmContext`. Missing
`request.prompt` → `InvalidRequest`.

Capabilities: `assembled_prompt=true`, `structured_context=true` (context reaches
the model through PromptBuilder).

## What it does not do

* Tool calling
* Planning / execution
* Memory
* Context assembly
* Prompt construction (PromptBuilder / Reasoning Engine only)

## Networking

Default endpoint: `http://127.0.0.1:11434`

Transport is injectable (`OllamaTransport`) so unit tests use `MockOllamaTransport`
without a live server. Live traffic uses `HttpOllamaTransport` (`ureq`).

Errors map into provider-independent `ReasoningError`:

| Condition | Error |
|-----------|--------|
| Server unreachable | `Unavailable` |
| Model missing | `ModelNotFound` |
| Bad NDJSON / body | `StreamFailed` / `GenerationFailed` |
| Caller cancel | `Cancelled` |
| Missing assembled prompt | `InvalidRequest` |

## Diagnostics

`OllamaDiagnostics` / `summary_line()` expose:

* connected
* provider version
* installed models
* loaded model
* latency
* streaming status (`idle` / `streaming` / `completed` / `cancelled` / `failed`)

Developer Diagnostics show these under **Reasoning Status** when the provider is registered at boot.

## Wiring

Boot constructs `OllamaReasoningProvider::local()`, registers it in the service
container, wraps it in `ModelRegistry`, refreshes discovery, and passes the
provider into `PlannerDeps.reasoning`. Conversational / unknown
Planner requests invoke Reasoning after Context assemble (Sprint B1.5).
Tool-backed, PlanWork, Review, and execution paths do not.

## Tests

`crates/jaymi-reasoning-ollama/tests/ollama_provider.rs` covers mock transport,
unavailable provider, streaming, cancellation, health, malformed streams, and
PromptBuilder → model handoff (complete + stream via ReasoningEngine).

`crates/jaymi-reasoning-ollama/tests/model_registry.rs` covers registry discovery,
metadata, health, selection, default model, and unavailable provider.
