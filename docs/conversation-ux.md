# Conversational Reasoning UX

**Status: Partial** — Sprint B1.11 polish + **B1.13.3**–**B1.13.8**
(Planner-owned conversation runtime + dual delivery clarity).

Conversation stays visually primary. Coding workspace behavior is unchanged.

## Affordances

| Affordance | Behavior |
|------------|----------|
| Typing indicator | Spinner + Planner `ConversationState::status_label()` while Preparing / Reasoning |
| Streaming cursor | Blinking caret on the in-progress assistant turn |
| Cancel | Composer **Stop** + `Esc` → `ConversationStream::cancel` |
| Retry | On cancelled / failed turns → stream retry or regenerate |
| Copy | Copies assistant text via egui clipboard |
| Regenerate | Replays the last user turn through a new generation |
| Smooth streaming | Per-frame `try_pump` / `pump_generation` (non-blocking UI) |
| Time To First Token | Tokens forward as soon as the provider emits them — never wait on diagnostics, metrics, or final response objects |
| Loading transitions | Ease-out opacity on the typing indicator |
| Multi-turn continuity | Prior Experience turns flow into `ReasoningRequest.history` |
| Context preparation | `prepare_context_session` before conversational assemble (same as `handle`) |

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
  begin_generation → try_pump / pump_generation → complete_conversation_stream
  Provider I/O runs on a background worker; UI frames never block on read.
  Token events are applied to Experience immediately; metrics / prompt diagnostics
  / pipeline timing continue collecting and attach on terminal completion only.
```

| Concern | Shared? | Notes |
|---------|---------|-------|
| Context session prep | Yes | `Application::prepare_conversational_host` |
| Assemble prelude | Yes | `Planner::begin_conversational_assemble` |
| Terminal → response map | Yes | `jaymi_planner::conversational` |
| Soft-fail no backend | Blocking only | Pumpable hard-errors; host bridges to observer |
| User-turn recording | Intentionally different | Blocking records before Planner; pumpable after stream open |
| UI non-blocking / TTFT | Pumpable only | Background chunk reader + `try_pump`; tokens before diagnostics |

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
* Retry returns to Reasoning through `Planner::resume_reasoning_after_retry`

## APIs

* `Application::begin_generation` / `pump_generation` / `cancel_generation`
* `Application::retry_generation` / `regenerate_response` / `assistant_turn_text`
* `Application::handle_with_workspace` / `handle_streaming_with_workspace`
* `ExperienceSession::to_reasoning_history`
* Helpers in `apps/jaymi/src/conversation_ux.rs`

## Tests

`apps/jaymi/tests/conversation_ux.rs`, `conversation_history.rs`,
`conversation_context_prep.rs`, `conversation_runtime.rs`,
`conversation_dual_delivery.rs`, and unit tests cover interaction helpers,
streaming, cancellation, retry/regenerate, clipboard text, accessibility labels,
multi-turn history, shared context prep, Planner-only runtime ownership, and
pumpable ↔ blocking delivery integrity.
