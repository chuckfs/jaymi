# Reasoning

**Status: Partial** — Sprint B1.1–B1.11 + **B1.13.1**–**B1.13.8**: Prompt → Provider
through Planner-owned conversation runtime and dual delivery clarity.

Provider-independent contracts and the orchestration engine live in
`jaymi-reasoning`. The first concrete backend is
[`OllamaReasoningProvider`](ollama.md).

## Pipeline

```text
LlmContext
  → PromptBuilder
  → ReasoningRequest.prompt
  → ReasoningProvider (transport)
  → StreamingResponse / ConversationStream
  → ReasoningResponse
```

Planner path (conversational / unknown only):

```text
Application::prepare_context_session
  → Planner → ContextBundle → LlmContext → PromptBuilder → ConversationStream
  → Conversation Response
```

Host preparation is identical for tool-backed and conversational requests
(Sprint B1.13.4). Never bypass Planner. Never bypass Context. Never bypass
PromptBuilder. Never call providers directly.

## Multi-turn history (B1.13.3)

```text
Experience (durable transcript)
  → Application snapshot (to_reasoning_history)
  → Planner (prepare_reasoning_history)
  → ReasoningRequest.history
  → PromptBuilder Conversation section
  → ReasoningProvider
  → Model
```

* Experience owns the transcript — Planner does **not** keep a parallel copy
* Prior turns include user, assistant, system, and relevant execution summaries
* Current goal stays on `ReasoningRequest.goal` (not duplicated in history)
* PromptBuilder budgets / truncates the Conversation section under model limits
* Long conversations stay continuous via retained Experience turns

## Context session preparation (B1.13.4)

```text
prepare_context_session
  → ContextEngine (session inputs)
  → assemble_with → ContextBundle
  → LlmContext → PromptBuilder → Reasoning
```

Conversational `begin_generation` / streaming use the **same** host preparation
as `Application::handle`. No alternate session builder. Workspace Intelligence
enrichments that land in `prepare_context_session` automatically apply to chat,
including the Sprint B2.1 / B2.2 [`WorkspaceSnapshot`](workspace-snapshot.md),
Sprint B2.3 [`EditorSnapshot`](editor-snapshot.md), Sprint B2.4
[`ProjectSnapshot`](project-snapshot.md), Sprint B2.5
[`GitSnapshot`](git-snapshot.md), and Sprint B2.6
[`RuntimeSnapshot`](runtime-snapshot.md) (ambient-maintained; Context providers
consume; Reasoning never talks to LSP, scans projects, runs git, or re-runs
cargo / tests).

## Delivered prompt diagnostics (B1.13.5)

```text
PromptBuilder → Prompt::seal_for_delivery(to_chat_messages)
  → ReasoningRequest.prompt
  → Provider wire messages
```

`PromptDiagnostics` size, budget, sections, conversation turns, and final token
estimate describe the delivered chat messages. Unused sections stay listed with
zero characters only — never as sent content.

## Model Registry loop (B1.13.6)

```text
ModelRegistry (default / preferred)
  → Planner::prepare_reasoning_model (± fallback)
  → ReasoningRequest.model
  → ReasoningProvider
```

Diagnostics expose Configured / Actual / Provider / Loaded model. Missing or
unavailable selections fall back to the next available registry model.

Persisted defaults restore at boot from `Settings.reasoning` through the
Application facade (Settings Workspace). See [settings.md](settings.md).

## Conversation state (B1.7 / B1.13.7)

User-visible phase owned **only** by the Planner (`jaymi_planner::ConversationState`):

```text
Idle → PreparingContext → Reasoning | Streaming | WaitingForReview | Executing
     → Completed | Cancelled | Failed
```

`StreamingLifecycle` is the generation sub-machine under Reasoning / Streaming.
Experience / UI mirror `PlannerResponse.conversation_state` / live Planner state
only — they never invent Preparing / Thinking / Streaming / terminal transitions
(Sprint **B1.13.7**). Retry returns to Reasoning via
`Planner::resume_reasoning_after_retry`.

## Dual delivery (B1.13.8)

Streaming (pumpable UI) and blocking (observer collect) are **delivery modes**
over one pipeline — not duplicated generation engines.

```text
Shared: assemble + build_reasoning_request + ConversationStream + terminal map
Blocking: handle_conversational_with_observer → run_with_observer
Pumpable: start_conversation_stream → pump → complete_conversation_stream
```

Intentional differences (do not collapse):

* **Pumpable** keeps the UI thread free: Enter acks Thinking immediately, then
  `pump_generation` per frame (background start + chunk reader + `try_pump`).
* **Blocking** soft-fails when no backend is wired; pumpable hard-errors on
  stream open and the Application bridges to the observer path on a **background**
  worker (installed via `PumpGeneration::Finished`).
* **User-turn recording**: blocking records before Planner entry; pumpable
  records on UI-thread ack (before background prep / assemble / stream-open).

Extracted shared pieces live in `jaymi_planner::conversational`. See
[conversation-ux.md](conversation-ux.md#dual-delivery-b1138) and
[UI thread ownership](conversation-ux.md#ui-thread-ownership).

## Streaming conversation (B1.6)

Lifecycle:

```text
Idle → Thinking → Streaming → Cancelled | Completed | Failed
```

[`ConversationStream`] owns incremental events (`Lifecycle`, `Thought`, `Token`,
terminal), cancel with reason, retry / reconnect after disconnect, and partial
completion. Experience updates assistant turns token-by-token via
`begin_streaming_assistant` / `apply_stream_event`.

**Time To First Token:** Provider reads run on a background worker inside
`StreamingResponse`. The UI calls `ConversationStream::try_pump` (via
`Application::pump_generation`) and never blocks on provider I/O. Visible
`Token` events are forwarded to the conversation as soon as the provider emits
them — they do **not** wait for diagnostics, execution summaries, metrics, or
the final `ReasoningResponse`. Developer diagnostics / metrics continue
collecting in the background and attach on terminal events only.

Diagnostics on [`ReasoningMetrics`]:

| Field | Meaning |
|-------|---------|
| `latency_ms` | Wall-clock request → terminal |
| `ttft_ms` | Time to first token from provider transport start |
| `provider_latency_ms` | Provider-reported duration, else TTFT (compat) |
| `generation_duration_ms` | First token → terminal |
| `tokens_per_sec_milli` | Approx tokens/sec × 1000 |
| `cancel_reason` | `user` / `timeout` / `provider_disconnect` / `engine` / `error` |
| `partial` | Terminal content is incomplete |
| `pipeline` | Stage timings (diagnostics only; see below) |

### Pipeline stage timings

Lightweight `PipelineTiming` records elapsed milliseconds per stage without
changing generation behavior. Stages include request received, Planner,
Context assembly, per-provider contribute, PromptBuilder, ReasoningEngine,
provider transport, TTFT, total generation, and request→done total.

Timings surface **only** in Developer Diagnostics — the **Performance**
dashboard and `ReasoningDiagnosticsReport` labeled values under **Pipeline
Timing**. They are never shown in the normal conversation UI.

## When Reasoning runs

| Request kind | Invokes Reasoning? |
|--------------|--------------------|
| Conversational / unknown | Yes (streaming after Context assemble) |
| Tool-backed | No |
| PlanWork | No |
| Execution / Review | No |

## Ownership

| Owner | Responsibilities |
|-------|------------------|
| **Planner** | ConversationState transitions; routing; conversational path after assemble |
| **ConversationStream** | Generation lifecycle, incremental events, retry/reconnect, partial |
| **ReasoningEngine** | Timeouts, cancellation, metrics, provider selection, retry, stream lifecycle |
| **PromptBuilder** | Prompt construction; sole source of generation text |
| **ReasoningProvider** | Transport: complete / stream / health / models from assembled Prompt |
| **Context Engine** | Context assembly (`ContextBundle` / `LlmContext`) |
| **Experience** | Durable conversation transcript; supplies prior turns per request |
| **UI** | Mirror ConversationState; never invent transitions |

## Engine API

`jaymi_reasoning::ReasoningEngine` (re-exported from `jaymi_planner::reasoning`):

* `build_prompt` — delegates to `PromptBuilder`
* `select_provider` — request model provider → preferred → Ready → Degraded
* `complete` — build Prompt → attach to request → select → timeout/retry → response
* `stream` / `stream_with_retry` — same handoff → `StreamingResponse`
* `reason` — stream collect with complete fallback

Providers require `ReasoningRequest.prompt` (`assembled_prompt` capability).
Calling a provider without going through the engine (or manually attaching a
Prompt) returns `InvalidRequest`.

`ConversationStream::start` → `pump` / `run_with_observer` / `collect` / `cancel` / `retry`.

`PlannerResponse` exposes `reasoning_used`, `reasoning_provider_id`,
`stream_lifecycle`, `reasoning_metrics`, and `conversation_state`.

Prompt construction adapts to model limits (`context_window − reserved_completion`)
via `ReasoningEngine::build_prompt` — see [docs/prompt.md](prompt.md).

## Backends

| Id | Crate | Notes |
|----|-------|-------|
| `ollama` | `jaymi-reasoning-ollama` | Local HTTP; see [docs/ollama.md](ollama.md) |

Discovered models are catalogued by the [Model Registry](models.md) (Sprint B1.9)
— tracking only, never download / marketplace.

Developer Diagnostics for the conversational path are documented in
[reasoning-diagnostics.md](reasoning-diagnostics.md) (Sprint B1.10).

Without a wired backend, conversational requests return a soft message — not “Unsupported”.

## Not in this slice

* No tool calling through the model
* Engine does not assemble context or execute tools
* Frame-by-frame egui repaint during generation is cooperative via
  `Application::pump_generation` (Sprint B1.11) — see [conversation-ux.md](conversation-ux.md)
