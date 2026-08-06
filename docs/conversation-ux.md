# Conversational Reasoning UX

**Status: Partial** — Sprint B1.11 polish + **B1.13.3**–**B1.13.8**
(Planner-owned conversation runtime + dual delivery clarity) + **UI-thread
ownership** (Enter returns before prep / assemble / stream-open).

Conversation stays visually primary. Coding workspace behavior is unchanged.

## Affordances

| Affordance | Behavior |
|------------|----------|
| Typing indicator | Spinner + Planner `ConversationState::status_label()` while Preparing / Reasoning |
| Streaming cursor | Blinking caret on the in-progress assistant turn |
| Cancel | Composer **Stop** + `Esc` → cancel Starting or `ConversationStream::cancel` |
| Retry | On cancelled / failed turns → stream retry or regenerate |
| Copy | Copies assistant text via egui clipboard |
| Regenerate | Replays the last user turn through a new generation |
| Smooth streaming | Per-frame `try_pump` / `pump_generation` (non-blocking UI) |
| Time To First Token | Tokens forward as soon as the provider emits them — never wait on diagnostics, metrics, or final response objects |
| Loading transitions | Ease-out opacity on the typing indicator |
| Multi-turn continuity | Prior Experience turns flow into `ReasoningRequest.history` |
| Context preparation | `prepare_context_session` on the **background** start task (same as `handle`) |

## UI thread ownership

Enter must acknowledge the send, show Thinking, and return to the egui event
loop **before** any planning, Context assemble, network, or inference.

| Stage | Thread | Notes |
|-------|--------|-------|
| Record user turn + empty Thinking assistant | UI | Experience |
| `Planner::acknowledge_conversational_send` (`PreparingContext → Reasoning`) | UI | Planner-owned only |
| `prepare_context_session` / history snapshot | Background | Was blocking Enter |
| Intent → Capability → Complexity → `assemble_with` | Background | Planner prelude |
| PromptBuilder + `provider.stream()` / retry sleep | Background | `ConversationStream::start` |
| Soft-fail / tool-backed fallback | Background | Surfaces via `pump_generation` → `Finished` |
| Token pump (`try_pump`) | UI (non-blocking) | Unchanged |

```text
Enter (UI)
  → record user + Planner ack Thinking + Started
  → return to event loop
Background
  → prepare_conversational_host → start_conversation_stream
  → Ready | Completed | Failed
pump_generation (UI)
  → promote Starting → Active | Finished
  → try_pump tokens …
```

## Dual delivery (B1.13.8)

One generation pipeline (`ConversationStream`); two host delivery modes — not two
engines. Do not merge them solely to reduce LOC.

```text
Shared
  prepare_context_session → Intent → AssembleHints → ContextBundle
  → build_reasoning_request → ConversationStream
  → conversational terminal → PlannerResponse

Blocking (observer collect)
  handle_with_workspace / handle_streaming_with_workspace
  → handle_conversational_with_observer → run_with_observer

Pumpable (UI)
  begin_generation (UI ack) → background start → pump_generation
  → try_pump / complete_conversation_stream
  Provider I/O and stream-open run off the UI thread; frames never block on read
  or start. Soft/tool completions finish on the worker and install via pump.
```

| Concern | Shared? | Notes |
|---------|---------|-------|
| Context session prep | Yes | `Application::prepare_conversational_host` (background on pumpable) |
| Assemble prelude | Yes | `Planner::begin_conversational_assemble` (skips re-entry after ack) |
| Terminal → response map | Yes | `jaymi_planner::conversational` |
| Soft-fail no backend | Both | Blocking inline; pumpable via background → `PumpGeneration::Finished` |
| User-turn recording | Intentionally different | Blocking before Planner; pumpable on UI ack (before background start) |
| UI non-blocking / TTFT | Pumpable only | Background start + chunk reader + `try_pump` |

Shared helpers live in `crates/jaymi-planner/src/conversational.rs`.

## State ownership (B1.13.7)

```text
Planner
  → Conversation Runtime (ConversationState)
  → UI / Experience (mirror only)
```

Planner owns: Preparing Context · Reasoning (Thinking) · Streaming · Cancelled ·
Completed · Failed (plus Waiting For Review / Executing for tool plans).

* Experience mirrors via `mirror_conversation_state` / `apply_planner_response`
* Turn `StreamingLifecycle` is nested under Reasoning / Streaming (generation only)
* UI never invents Preparing / Failed / Streaming transitions
* Immediate Thinking uses `Planner::acknowledge_conversational_send`
* Retry returns to Reasoning through `Planner::resume_reasoning_after_retry`

## APIs

* `Application::begin_generation` / `pump_generation` / `cancel_generation`
* `Application::retry_generation` / `regenerate_response` / `assistant_turn_text`
* `Application::handle_with_workspace` / `handle_streaming_with_workspace`
* `Planner::acknowledge_conversational_send`
* `ExperienceSession::to_reasoning_history`
* Helpers in `apps/jaymi/src/conversation_ux.rs`

## Tests

`apps/jaymi/tests/conversation_ux.rs`, `conversation_history.rs`,
`conversation_context_prep.rs`, `conversation_runtime.rs`,
`conversation_dual_delivery.rs`, and unit tests cover interaction helpers,
streaming, cancellation, retry/regenerate, clipboard text, accessibility labels,
multi-turn history, shared context prep, Planner-only runtime ownership,
non-blocking Enter ack, and pumpable ↔ blocking delivery integrity.
