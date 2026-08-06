# Conversational Reasoning Diagnostics

**Status: Partial** — Sprint B1.10 + **B1.13.1** / **B1.13.5** / **B1.13.6**
(delivered-prompt diagnostics + Model Registry loop).

Developer Diagnostics explain the complete conversational reasoning lifecycle.
No hidden state: every field is explicit on `ReasoningDiagnosticsReport`.

```text
Idle → Preparing Context → Reasoning / Streaming → Completed | Cancelled | Failed
```

## Surfaces

| Surface | Where |
|---------|--------|
| Subsystem row **Reasoning Status** | One-line summary (`summary_line`) |
| **Conversational Reasoning** inspector | Developer Diagnostics + dashboard text |
| Coding Diagnostics section | Same labeled values |

## Fields

| Field | Meaning |
|-------|---------|
| Reasoning Provider | Logical backend id (`ollama`, …) |
| Current Model | Last / default `provider/name` (compat alias) |
| Configured Model | Registry default or preferred selection |
| Actual Model | Provider-reported model from last-turn metrics |
| Provider Model | Model id attached onto `ReasoningRequest` |
| Loaded Model | Backend-loaded model (e.g. Ollama `/api/ps`) |
| Prompt Tokens | Provider input tokens or delivered prompt estimate |
| Completion Tokens | Provider output tokens |
| Context Size | Model context window (tokens) |
| Latency | Wall-clock ms (+ provider/TTFT when known) |
| Streaming | `StreamingLifecycle` label |
| Cancellation | `none` or cancel reason |
| Reasoning Health | `ready` / `degraded` / `unavailable` |
| Prompt Size | Delivered chat-message characters (`to_chat_messages`) |
| Prompt Budget | used / remaining / reserved / window / efficiency — from delivered size |
| Prompt Sections | Per-section chars / tokens / disposition (`included` / `excluded` / `truncated` / `filtered` / `budgeted`) |
| Truncated Sections | Section ids shortened or budget-omitted |
| Excluded Sections | Section ids not present in the delivered prompt |
| Conversation Turns | Prior history turns folded into Conversation |
| Final Token Estimate | Delivered prompt token estimate |
| Conversation Runtime State | Planner `ConversationState` |
| Provider Status | id, health detail, model count, capabilities (`assembled_prompt`, …) |

## Retention

Live health and registry come from the wired `ReasoningProvider` / `ModelRegistry`.
Last-turn metrics and prompt diagnostics are retained from `PlannerResponse` after
conversational turns (`prompt_diagnostics` + `reasoning_metrics`). After B1.13.5
those prompt fields describe the chat messages that reached the provider — never
unused PromptBuilder framing or discarded pre-stream builds.

## Tests

`crates/jaymi-reasoning` (`prompt_delivery_diagnostics`, unit diagnostics),
`crates/jaymi-reasoning-ollama` provider/streaming inspection, and
`apps/jaymi/tests/reasoning_diagnostics.rs` cover every diagnostic value,
unavailable provider, streaming/cancellation, prompt inspection, equality,
integrity, and health reporting.
